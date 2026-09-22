//! dsh 会话统计与在线统计。
//!
//! 数据源是 `$DSH_HOME/sessions/<cwd编码>/<会话目录>/session(.v3).jsonl.zstd`
//! （会话日志本身就是持久层，本模块只做只读扫描，统计缓存写在启动器自己的目录里，
//! 可随时删除重建）。聚合口径与生态插件 token-meter（`stats/online.ts` + `fold.ts`）对齐：
//!  - 单请求 Token = inputTokens + cacheReadTokens + cacheWriteTokens + outputTokens
//!    （reasoningTokens 已含在 outputTokens，不重复计）；
//!  - 同一 (turn, step) 的 usage chunk 为早期采样、assistant/message usage 为终值，
//!    后到者覆盖前者（pending/flush 语义），不重复累计；
//!  - fork/resume 会话跳过其继承的父会话前缀事件，切分口径与插件 logread.ts 一致：
//!    优先最后一个 `session/end-seed {inherited:true}` 标记（含标记本身），其次按
//!    header.createdAt 位置切（第一个 time ≥ createdAt 的事件起算），都不可得时
//!    退化用父会话 seq+data 前缀比对兜底。绝不因父会话缺失而全量计入（那会重复统计）。

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

// ── 会话日志读取 ─────────────────────────────────────────────

/// 事件（header 行以外的 JSONL 行）的精简投影，只保留聚合要用的字段
#[derive(Clone, Debug)]
struct Event {
    seq: i64,
    kind: String,
    time_ms: u64,
    data: serde_json::Value,
}

/// 会话 header（JSONL 首行，type=session）——只保留统计用到的字段
#[derive(Clone, Debug, Default)]
struct SessionHeader {
    is_seeded: bool,
    parent_session: Option<String>,
    /// 会话创建时刻（毫秒）；fork 日志里没有它时退化用父前缀比对
    created_at: u64,
}

#[derive(Clone, Debug)]
struct SessionLog {
    header: SessionHeader,
    events: Vec<Event>,
}

impl SessionLog {
    /// 读取并解压一个会话日志文件（新旧两代命名）
    fn read(path: &Path) -> Result<SessionLog, String> {
        let bytes = fs::read(path).map_err(|e| format!("读取失败: {e}"))?;
        let text = zstd::decode_all(bytes.as_slice())
            .map_err(|e| format!("zstd 解压失败: {e}"))?
            .into_iter()
            .collect::<Vec<u8>>();
        let text = String::from_utf8_lossy(&text).into_owned();
        let mut lines = text.lines().filter(|l| !l.trim().is_empty());
        let first = lines.next().ok_or_else(|| "会话日志为空".to_string())?;
        let header = parse_header(first);
        let mut events = Vec::new();
        for line in lines {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
                continue;
            };
            let Some(obj) = v.as_object() else { continue };
            let kind = obj.get("type").and_then(|t| t.as_str()).unwrap_or("").to_string();
            let seq = obj.get("seq").and_then(|s| s.as_i64()).unwrap_or(-1);
            let time_ms = obj.get("time").and_then(|s| s.as_u64()).unwrap_or(0);
            let data = obj.get("data").cloned().unwrap_or(serde_json::Value::Null);
            events.push(Event { seq, kind, time_ms, data });
        }
        Ok(SessionLog { header, events })
    }
}

fn parse_header(line: &str) -> SessionHeader {
    let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
        return SessionHeader::default();
    };
    let Some(obj) = v.as_object() else { return SessionHeader::default() };
    SessionHeader {
        is_seeded: obj.get("isSeeded").and_then(|s| s.as_bool()).unwrap_or(false),
        parent_session: obj.get("parentSession").and_then(|s| s.as_str()).map(|s| s.to_string()),
        created_at: obj.get("createdAt").and_then(|s| s.as_u64()).unwrap_or(0),
    }
}

// ── fold 聚合（与 dsh-token-stats lib/fold.js 同口径）───────────

/// 会话内单条 day|hour|model 桶
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct Bucket {
    d: String,
    h: u32,
    m: String,
    i: u64,
    o: u64,
    cr: u64,
    cw: u64,
    n: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct SessionAgg {
    records: Vec<Bucket>,
    peak: Option<(u64, String, String)>,
    first: Option<String>,
    last: Option<String>,
    used: bool,
    /// 活动区间：本会话事件时刻按 BASE_GAP_MS 合并（已跳过继承前缀），供在线时长估算
    active: Vec<[u64; 2]>,
    /// 对话进行中区间：turn/start → turn/end（已跳过继承前缀）
    turns: Vec<[u64; 2]>,
    /// 模型生成墙钟：Σ(step/start → assistant/message)，与官方 sessionStats 投影同口径
    llm_ms: u64,
    /// 工具执行墙钟：Σ(tool/call → tool/result 按 callId 配对)
    tool_ms: u64,
    /// 每日引擎时长：(day, llm_ms, tool_ms)
    day_engine: Vec<(String, u64, u64)>,
}

fn model_key(provider: Option<&str>, model: Option<&str>) -> String {
    format!("{}/{}", provider.unwrap_or("unknown"), model.unwrap_or("unknown"))
}

/// 本地时区 day 键（YYYY-MM-DD）与小时；毫秒时间戳越界时退化为空串（该 usage 被跳过）
fn local_day_hour(ms: u64) -> (String, u32) {
    use chrono::{Local, TimeZone};
    match Local.timestamp_millis_opt(ms as i64).single() {
        Some(dt) => (dt.format("%Y-%m-%d").to_string(), dt.format("%H").to_string().parse().unwrap_or(0)),
        None => (String::new(), 0),
    }
}

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

// ── 在线时长区间算术（对齐 token-meter stats/online.ts）──────────
//
// 两级合并（缓存友好）：fold 阶段按 BASE_GAP_MS（1 分钟）把事件时刻压成区间存进缓存；
// 快照阶段再用更大的 gap 合并这些区间。单链聚类可结合：先按小阈值合并再按大阈值合并
// 等价于直接按大阈值合并 —— 所以换阈值不必重扫日志。
// 口径：「在线」= 有事件且相邻事件间隔 ≤ 阈值的墙钟时间；每段只算到最后一个事件，
// 任何档位都是**下界**（窗口开着但没事件的时间在数据里不存在）。

/// fold 阶段的基础合并阈值（毫秒）：再小的阈值不保证准确（1 分钟精度）
const BASE_GAP_MS: u64 = 60_000;

/// 客户端可切换的预设阈值（分钟）；1 分钟即基础阈值
pub const PRESET_GAPS_MIN: [u32; 5] = [1, 5, 15, 30, 60];

/// 推荐默认阈值：干活时日志持续有事件，5 分钟以下会切断「读回答想需求」的静默期，
/// 60 分钟会把开会/吃饭整段算成在线
pub const DEFAULT_GAP_MIN: u32 = 15;

/// 配置阈值归一：钳到 [1,60] 并吸附到最近预设值；非法值兜底推荐档 15
pub fn norm_gap_min(raw: u32) -> u32 {
    PRESET_GAPS_MIN.into_iter().min_by_key(|g| (*g as i64 - raw as i64).abs()).unwrap_or(DEFAULT_GAP_MIN)
}

/// 事件时刻 → 区间：相邻时刻间隔 ≤ gap_ms 视为同一段活动
fn merge_points(times: &[u64], gap_ms: u64) -> Vec<[u64; 2]> {
    if times.is_empty() { return Vec::new(); }
    let mut s = times.to_vec();
    s.sort_unstable();
    let mut out = Vec::new();
    let (mut start, mut end) = (s[0], s[0]);
    for &t in &s[1..] {
        if t.saturating_sub(end) <= gap_ms {
            if t > end { end = t; }
        } else {
            out.push([start, end]);
            start = t;
            end = t;
        }
    }
    out.push([start, end]);
    out
}

/// 区间 → 区间：把已按更小阈值合并过的区间再按更大阈值合并（gap_ms=0 即纯并集）
fn merge_intervals(input: &[[u64; 2]], gap_ms: u64) -> Vec<[u64; 2]> {
    if input.is_empty() { return Vec::new(); }
    let mut s = input.to_vec();
    s.sort_unstable_by_key(|iv| (iv[0], iv[1]));
    let mut out = Vec::new();
    let (mut start, mut end) = (s[0][0], s[0][1]);
    for iv in &s[1..] {
        if iv[0].saturating_sub(end) <= gap_ms {
            if iv[1] > end { end = iv[1]; }
        } else {
            out.push([start, end]);
            start = iv[0];
            end = iv[1];
        }
    }
    out.push([start, end]);
    out
}

/// 区间总时长（毫秒）
fn total_ms(iv: &[[u64; 2]]) -> u64 {
    iv.iter().filter(|x| x[1] > x[0]).map(|x| x[1] - x[0]).sum()
}

fn local_day(ms: u64) -> Option<String> {
    use chrono::{Local, TimeZone};
    Local.timestamp_millis_opt(ms as i64).single().map(|dt| dt.format("%Y-%m-%d").to_string())
}

/// 本地次日 00:00 的毫秒时间戳（DST 歧义等异常时区返回 None，该段被截断）
fn next_midnight_ms(ms: u64) -> Option<u64> {
    use chrono::{Local, TimeZone};
    let dt = Local.timestamp_millis_opt(ms as i64).single()?;
    let next = (dt.date_naive() + chrono::Duration::days(1)).and_hms_opt(0, 0, 0)?;
    Some(Local.from_local_datetime(&next).single()?.timestamp_millis() as u64)
}

/// 按本地日切分区间 → 每日毫秒（跨零点的区间会落到两天）
fn split_by_day(iv: &[[u64; 2]]) -> BTreeMap<String, u64> {
    let mut by_day = BTreeMap::new();
    for &[a, b] in iv {
        if b <= a { continue; }
        let mut cur = a;
        while cur < b {
            // 每轮按当前 cur 重新求当日结束点：跨零点的区间连续落进多天
            let Some(day_end) = next_midnight_ms(cur) else { break };
            let seg_end = b.min(day_end);
            if seg_end <= cur { break; } // 防异常时区（DST 等）导致的死循环
            if let Some(k) = local_day(cur) {
                *by_day.entry(k).or_insert(0) += seg_end - cur;
            }
            cur = seg_end;
        }
    }
    by_day
}

/// 按区间起点所在本地日统计段数（跨零点的段归入起点那天）
fn count_by_day(iv: &[[u64; 2]]) -> BTreeMap<String, u64> {
    let mut out = BTreeMap::new();
    for &[a, _] in iv {
        if let Some(k) = local_day(a) { *out.entry(k).or_insert(0) += 1; }
    }
    out
}

/// 折叠一个会话的事件（跳过前 skip 条种子事件）为聚合结果。
/// 规则逐行对齐 fold.js：路由 provider/model 分开半更新；pending 采样被终值覆盖；
/// tokens<=0 不记。
fn fold_session(events: &[Event], skip: usize) -> SessionAgg {
    let (mut rp, mut rm) = (String::from("unknown"), String::from("unknown"));
    let set_route = |rp: &mut String, rm: &mut String, p: Option<&str>, m: Option<&str>| {
        if let Some(v) = p.filter(|s| !s.is_empty()) { *rp = v.to_string(); }
        if let Some(v) = m.filter(|s| !s.is_empty()) { *rm = v.to_string(); }
    };
    struct Pending { key: String, usage: serde_json::Value, time_ms: u64, model: String }
    let mut pending: Option<Pending> = None;
    let mut records: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut peak: Option<(u64, String, String)> = None;
    let mut first: Option<String> = None;
    let mut last: Option<String> = None;
    let mut used = false;

    let commit = |usage: &serde_json::Value, time_ms: u64, model: &str,
                  records: &mut BTreeMap<String, Bucket>,
                  peak: &mut Option<(u64, String, String)>,
                  first: &mut Option<String>, last: &mut Option<String>, used: &mut bool| {
        let g = |k: &str| usage.get(k).and_then(|v| v.as_u64()).unwrap_or(0);
        let (i, o, cr, cw) = (g("inputTokens"), g("outputTokens"), g("cacheReadTokens"), g("cacheWriteTokens"));
        let tokens = i + o + cr + cw;
        if tokens == 0 { return; }
        *used = true;
        let (d, h) = local_day_hour(time_ms);
        if d.is_empty() { return; }
        let k = format!("{d}|{h}|{model}");
        let r = records.entry(k).or_insert_with(|| Bucket { d: d.clone(), h, m: model.to_string(), ..Default::default() });
        r.i += i; r.o += o; r.cr += cr; r.cw += cw; r.n += 1;
        if peak.as_ref().map_or(true, |p| tokens > p.0) { *peak = Some((tokens, d.clone(), model.to_string())); }
        if first.as_ref().map_or(true, |f| &d < f) { *first = Some(d.clone()); }
        if last.as_ref().map_or(true, |l| &d > l) { *last = Some(d); }
    };
    let flush = |pending: &mut Option<Pending>,
                 records: &mut BTreeMap<String, Bucket>,
                 peak: &mut Option<(u64, String, String)>,
                 first: &mut Option<String>, last: &mut Option<String>, used: &mut bool| {
        if let Some(p) = pending.take() { commit(&p.usage, p.time_ms, &p.model, records, peak, first, last, used); }
    };

    // 在线时长原料：所有事件时刻 + turn 边界（都只统计跳过继承前缀之后的部分）；
    // 引擎时长原料（与官方 sessionStats 投影同口径）：llm = step/start → assistant/message，
    // tool = tool/call → tool/result 按 callId 配对
    let mut times: Vec<u64> = Vec::new();
    let mut turn_ivs: Vec<[u64; 2]> = Vec::new();
    let mut open_turn: Option<u64> = None;
    let mut llm_ms: u64 = 0;
    let mut tool_ms: u64 = 0;
    let mut day_llm: BTreeMap<String, u64> = BTreeMap::new();
    let mut day_tool: BTreeMap<String, u64> = BTreeMap::new();
    let mut open_step: Option<(Option<serde_json::Value>, Option<serde_json::Value>, u64)> = None;
    let mut pending_calls: BTreeMap<String, u64> = BTreeMap::new();

    for (idx, ev) in events.iter().enumerate() {
        if idx < skip { continue; }
        let has_time = ev.time_ms > 0;
        if has_time { times.push(ev.time_ms); }
        match ev.kind.as_str() {
            "turn/start" if has_time => open_turn = Some(ev.time_ms),
            "turn/end" if has_time => {
                if let Some(s) = open_turn {
                    if ev.time_ms > s { turn_ivs.push([s, ev.time_ms]); }
                }
                open_turn = None;
                // 结果没落地的调用属于被取消/失败的轮次，丢弃以免污染 toolMs
                pending_calls.clear();
            }
            _ => {}
        }
        let Some(data) = ev.data.as_object() else { continue };
        let sget = |keys: &[&str]| -> Option<&str> {
            let mut cur = &ev.data;
            for k in keys { cur = cur.get(k)?; }
            cur.as_str()
        };
        match ev.kind.as_str() {
            "step/start" => {
                open_step = if has_time {
                    Some((data.get("turn").cloned(), data.get("step").cloned(), ev.time_ms))
                } else {
                    None
                };
            }
            "tool/call" => {
                if has_time {
                    if let Some(id) = data.get("callId").and_then(|v| v.as_str()) {
                        pending_calls.insert(id.to_string(), ev.time_ms);
                    }
                }
            }
            "tool/result" => {
                if !has_time { continue; }
                let call_id = data
                    .get("message")
                    .and_then(|m| m.get("source"))
                    .and_then(|s| s.get("callId"))
                    .and_then(|v| v.as_str());
                let Some(id) = call_id else { continue };
                let Some(dispatched) = pending_calls.remove(id) else { continue };
                let span = ev.time_ms.saturating_sub(dispatched);
                tool_ms += span;
                if let Some(d) = local_day(ev.time_ms) { *day_tool.entry(d).or_insert(0) += span; }
            }
            "request/header" => {
                if let Some(cfg) = data.get("header").and_then(|h| h.get("config")) {
                    let p = cfg.get("provider").and_then(|v| v.as_str());
                    let m = cfg.get("model").and_then(|v| v.as_str());
                    set_route(&mut rp, &mut rm, p, m);
                }
            }
            "request/context" => {
                let p = data.get("provider").and_then(|v| v.as_str());
                let m = data.get("model").and_then(|v| v.as_str());
                set_route(&mut rp, &mut rm, p, m);
            }
            "assistant/chunk" => {
                if sget(&["chunk", "type"]) == Some("usage") {
                    if let Some(usage) = data.get("chunk").and_then(|c| c.get("usage")) {
                        let key = format!("{}:{}",
                            data.get("turn").map(|v| v.to_string()).unwrap_or_default(),
                            data.get("step").map(|v| v.to_string()).unwrap_or_default());
                        if pending.as_ref().map_or(false, |p| p.key != key) {
                            flush(&mut pending, &mut records, &mut peak, &mut first, &mut last, &mut used);
                        }
                        pending = Some(Pending { key, usage: usage.clone(), time_ms: ev.time_ms, model: model_key(Some(&rp), Some(&rm)) });
                    }
                }
            }
            "assistant/message" => {
                let key = format!("{}:{}",
                    data.get("turn").map(|v| v.to_string()).unwrap_or_default(),
                    data.get("step").map(|v| v.to_string()).unwrap_or_default());
                // 该步骤的模型墙钟到此结束（只认组装出消息的步骤，与官方投影一致）
                if let Some((s_turn, s_step, start)) = open_step.take() {
                    if has_time && s_turn.as_ref() == data.get("turn") && s_step.as_ref() == data.get("step") {
                        let span = ev.time_ms.saturating_sub(start);
                        llm_ms += span;
                        if let Some(d) = local_day(ev.time_ms) { *day_llm.entry(d).or_insert(0) += span; }
                    }
                }
                let src_model = match data.get("message").and_then(|m| m.get("source")) {
                    Some(src) if src.get("kind").and_then(|k| k.as_str()) == Some("model") => Some(model_key(
                        src.get("provider").and_then(|v| v.as_str()),
                        src.get("model").and_then(|v| v.as_str()),
                    )),
                    _ => None,
                };
                if let Some(usage) = data.get("usage").filter(|u| !u.is_null()) {
                    if pending.as_ref().map_or(false, |p| p.key != key) {
                        flush(&mut pending, &mut records, &mut peak, &mut first, &mut last, &mut used);
                    }
                    pending = Some(Pending {
                        key,
                        usage: usage.clone(),
                        time_ms: ev.time_ms,
                        model: src_model.clone().unwrap_or_else(|| model_key(Some(&rp), Some(&rm))),
                    });
                } else if let Some(p) = pending.as_mut() {
                    if p.key == key {
                        if let Some(sm) = src_model { p.model = sm; }
                    }
                }
            }
            _ => {}
        }
    }
    flush(&mut pending, &mut records, &mut peak, &mut first, &mut last, &mut used);
    let active = merge_points(&times, BASE_GAP_MS);
    let turns = merge_intervals(&turn_ivs, 0);
    let day_engine: Vec<(String, u64, u64)> = day_llm
        .keys()
        .chain(day_tool.keys())
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|d| (d.clone(), day_llm.get(&d).copied().unwrap_or(0), day_tool.get(&d).copied().unwrap_or(0)))
        .collect();
    SessionAgg {
        records: records.into_values().collect(),
        peak,
        first,
        last,
        used,
        active,
        turns,
        llm_ms,
        tool_ms,
        day_engine,
    }
}

/// fork/resume 去重（对齐插件 logread.ts readLogDirect 的切分优先级）：
/// 返回该会话日志开头属于继承前缀的事件条数。
///  a) 最后一个 `session/end-seed {inherited:true}` 标记（含标记本身）→ 最精确；
///  b) 标记不可得且有 createdAt → 切到「第一个 time ≥ createdAt」之前（位置法，
///     不能按条数：v0 fork 的 seq 空间与父文件条数不一致）；整份都在 createdAt 之前
///     视为时钟异常，不切（与插件 `kept ≥ len → skip 0` 行为一致）；
///  c) 都没有 → 0（调用方再决定退化策略）。
fn fork_cut(events: &[Event], created_at: u64) -> usize {
    for (i, ev) in events.iter().enumerate().rev() {
        if ev.kind == "session/end-seed"
            && ev.data.get("inherited").and_then(|v| v.as_bool()) == Some(true)
        {
            return i + 1;
        }
    }
    if created_at > 0 {
        if let Some(i) = events.iter().position(|e| e.time_ms >= created_at) {
            return i;
        }
    }
    0
}

/// fork/resume 兜底去重：返回该会话继承自父会话的事件数（最长前缀，按 seq+data 比对）。
fn inherited_count(child: &[Event], parent: &[Event]) -> usize {
    let mut n = 0;
    for (c, p) in child.iter().zip(parent.iter()) {
        if c.seq == p.seq && c.kind == p.kind && c.data == p.data { n += 1; } else { break; }
    }
    n
}

// ── 磁盘扫描 + 指纹缓存（进程内 + 持久化到 ~/.dsh-starter）──────

/// 一个会话日志文件的缓存条目：fp 命中即整体复用聚合（含已 baked 的 fork skip，
/// 因为 seeded 会话继承的父前缀在 fork 时就固定、不随父会话后续增长而变）。
#[derive(Clone, Serialize, Deserialize)]
struct CacheEntry {
    fp: String,
    agg: SessionAgg,
}

#[derive(Serialize, Deserialize)]
struct CacheFile {
    /// 口径版本：聚合规则变了就 +1，旧缓存整体作废
    version: u32,
    entries: BTreeMap<String, CacheEntry>,
}

/// v2：SessionAgg 增加活动/turn 区间与引擎时长（在线时长统计）
/// v3：fork 去重改用插件的 marker/createdAt 切分（旧口径漏切的 fork 会重复计 token）
const CACHE_VERSION: u32 = 3;
const CACHE_FILE: &str = "session-stats-cache.json";

/// 统计缓存落在启动器自己的数据目录（与会话日志、dsh 数据隔离），可随时删除重建
pub fn cache_path() -> PathBuf {
    crate::settings::starter_home().join(CACHE_FILE)
}

/// 缓存读写失败不致命：退化为每次全量重算
static CACHE: std::sync::OnceLock<std::sync::Mutex<HashMap<PathBuf, CacheEntry>>> =
    std::sync::OnceLock::new();

fn cache() -> &'static std::sync::Mutex<HashMap<PathBuf, CacheEntry>> {
    CACHE.get_or_init(|| std::sync::Mutex::new(load_cache_from_disk()))
}

fn load_cache_from_disk() -> HashMap<PathBuf, CacheEntry> {
    fs::read_to_string(cache_path())
        .ok()
        .and_then(|t| serde_json::from_str::<CacheFile>(&t).ok())
        .filter(|f| f.version == CACHE_VERSION)
        .map(|f| f.entries.into_iter().map(|(k, e)| (PathBuf::from(k), e)).collect())
        .unwrap_or_default()
}

/// 原子写（tmp + rename），与 settings.rs 同一套路；失败只影响下次扫描速度
fn save_cache_to_disk() {
    let Ok(guard) = cache().lock() else { return };
    let mut file = CacheFile { version: CACHE_VERSION, entries: BTreeMap::new() };
    for (path, e) in guard.iter() {
        file.entries.insert(path.to_string_lossy().into_owned(), e.clone());
    }
    drop(guard);
    let Ok(text) = serde_json::to_string(&file) else { return };
    let path = cache_path();
    let Some(parent) = path.parent() else { return };
    if fs::create_dir_all(parent).is_err() { return; }
    let tmp = parent.join(format!("{CACHE_FILE}.tmp"));
    if fs::write(&tmp, &text).is_err() { return; }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600));
    }
    if fs::rename(&tmp, &path).is_err() {
        let _ = fs::remove_file(&tmp);
    }
}

/// 会话日志文件指纹：size:mtime 秒。live（mtime 距今 <2min）的会话返回 None（永不缓存）。
fn fingerprint_of(md: &fs::Metadata, now: u64) -> Option<String> {
    let mtime = md.modified().ok()?.duration_since(UNIX_EPOCH).ok()?.as_secs();
    if now.saturating_sub(mtime * 1000) < LIVE_WINDOW_MS {
        return None;
    }
    Some(format!("{}:{}", md.len(), mtime))
}

/// 活跃/在线判定窗口：日志文件 2 分钟内被写过即视为仍在活动
const LIVE_WINDOW_MS: u64 = 120_000;

fn find_log_file(dir: &Path) -> Option<PathBuf> {
    let v3 = dir.join("session.v3.jsonl.zstd");
    if v3.is_file() { return Some(v3); }
    let v0 = dir.join("session.jsonl.zstd");
    if v0.is_file() { return Some(v0); }
    None
}

/// 扫描 sessions 根目录，产出全部会话的聚合列表与坏会话数。
///
/// 只 stat 每个文件算指纹，**指纹命中就不读盘**（这是首扫之后能秒开的核心）；
/// 只有 miss / live 会话才解压。fork/续接会话按 marker/createdAt 切掉继承前缀
/// （见 fork_cut），父前缀比对仅作兜底、父会话日志在需要时才解压。
/// 坏会话（读/解压/解析失败）计入 errors、跳过。
fn scan_sessions(root: &Path, now: u64) -> (Vec<SessionAgg>, usize) {
    // 1. 只枚举 + stat：收集 (日志路径, 目录名=session id, metadata)
    let mut files: Vec<(PathBuf, String, fs::Metadata)> = Vec::new();
    let mut dir_index: HashMap<String, PathBuf> = HashMap::new();
    if let Ok(rd) = fs::read_dir(root) {
        for cwd_entry in rd.flatten() {
            let Ok(sess_entries) = fs::read_dir(cwd_entry.path()) else { continue };
            for sess_entry in sess_entries.flatten() {
                let path = sess_entry.path();
                let Ok(ft) = sess_entry.file_type() else { continue };
                if !ft.is_dir() { continue; }
                let Some(log_file) = find_log_file(&path) else { continue };
                let Ok(md) = fs::metadata(&log_file) else { continue };
                let id = sess_entry.file_name().to_string_lossy().into_owned();
                dir_index.entry(id.clone()).or_insert_with(|| log_file.clone());
                files.push((log_file, id, md));
            }
        }
    }

    // 2. 逐个决定命中缓存还是解压。loaded 只放 miss/live 的会话（fork 去重会就地解压父会话）
    let mut loaded: HashMap<PathBuf, SessionLog> = HashMap::new();
    let mut aggs: Vec<SessionAgg> = Vec::with_capacity(files.len());
    let mut errors = 0usize;
    let mut alive: HashSet<PathBuf> = HashSet::new();

    for (log_file, _id, md) in &files {
        alive.insert(log_file.clone());
        let fp = fingerprint_of(md, now);
        // 命中：非 live 且缓存里指纹一致 → 直接复用，绝不读盘
        if let Some(f) = fp.as_ref() {
            let hit = cache().lock().ok().and_then(|g| g.get(log_file).filter(|e| &e.fp == f).map(|e| e.agg.clone()));
            if let Some(agg) = hit {
                aggs.push(agg);
                continue;
            }
        }
        // 未命中 / live：解压读取（先取出所需数据、结束 loaded 借用，再就地解压父会话）
        let pre = load_or_get(log_file, &mut loaded)
            .map(|l| (l.header.is_seeded, l.header.parent_session.clone(), l.header.created_at, l.events.clone()));
        let (seeded, parent_id, created_at, child_events) = match pre {
            Ok(v) => v,
            Err(_) => { errors += 1; continue; }
        };
        // fork 切分（插件口径）：marker → createdAt；都不可得的 seeded 会话才用父前缀比对兜底
        let mut skip = fork_cut(&child_events, created_at);
        if skip == 0 && seeded {
            if let Some(ppath) = parent_id.as_deref().and_then(|pid| dir_index.get(pid)) {
                if let Ok(parent) = load_or_get(ppath, &mut loaded) {
                    skip = inherited_count(&child_events, &parent.events);
                }
            }
        }
        let agg = fold_session(&child_events, skip);
        let _ = loaded.remove(log_file); // 子会话事件已消费，释放内存（父会话保留共享）
        if let Some(f) = fp.as_ref() {
            if let Ok(mut g) = cache().lock() {
                g.insert(log_file.clone(), CacheEntry { fp: f.clone(), agg: agg.clone() });
            }
        }
        aggs.push(agg);
    }

    // 3. 清理已删除会话的缓存条目
    if let Ok(mut g) = cache().lock() {
        g.retain(|k, _| alive.contains(k));
    }
    (aggs, errors)
}

/// 从已加载表取，未加载则读盘并放入表中（父会话被多个子会话共享时只解压一次）
fn load_or_get<'a>(path: &Path, loaded: &'a mut HashMap<PathBuf, SessionLog>) -> Result<&'a SessionLog, String> {
    if !loaded.contains_key(path) {
        let l = SessionLog::read(path)?;
        loaded.insert(path.to_path_buf(), l);
    }
    Ok(loaded.get(path).expect("just ensured"))
}

// ── 对外结构（serde camelCase，与前端 types.ts 对齐）──────────

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionStats {
    /// 全部历史总览
    pub overview: Overview,
    /// 近 N 天范围（range 参数）的按日×模型序列
    pub trend: Vec<TrendPoint>,
    /// 今日 24 小时 × 模型堆叠
    pub today: TodayStats,
    /// GitHub 风格热力图：近 53 周的按日 token 与活跃会话数
    pub heatmap: Vec<DayCell>,
    /// 模型分布（全历史）
    pub models: Vec<ModelUsage>,
    /// 在线时长快照（全历史；阈值只影响快照合并，不触发重扫）
    pub online: OnlineSnapshot,
    pub range_days: u32,
    pub sessions_total: usize,
    pub sessions_with_usage: usize,
    pub errors: usize,
    pub generated_at: u64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub total_input: u64,
    pub total_output: u64,
    pub total_cache_read: u64,
    pub total_cache_write: u64,
    pub total_tokens: u64,
    pub calls: u64,
    pub active_days: usize,
    pub current_streak: usize,
    pub longest_streak: usize,
    pub peak_day: Option<PeakDay>,
    pub peak_step: Option<PeakStep>,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    pub avg_day: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakDay { pub day: String, pub tokens: u64 }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PeakStep { pub tokens: u64, pub day: String, pub model: String }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrendPoint {
    pub day: String,
    /// 按模型分的当日 token（供堆叠/折线）
    pub by_model: Vec<ModelTokens>,
    pub total: u64,
    pub calls: u64,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelTokens { pub model: String, pub tokens: u64 }

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DayCell {
    pub day: String,
    pub tokens: u64,
    pub sessions: usize,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// 当日 Token 最高的模型（悬浮明细用）
    pub top_model: Option<ModelTokens>,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ModelUsage {
    pub model: String,
    pub tokens: u64,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    pub calls: u64,
    pub share: f64,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct TodayStats {
    pub day: String,
    pub total: u64,
    pub calls: u64,
    pub yesterday_total: u64,
    pub hours: Vec<HourCell>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HourCell { pub hour: u32, pub by_model: Vec<ModelTokens>, pub total: u64 }

/// 在线时长快照：五档空闲阈值下的累计/每日在线 + 三口径（对话/模型/工具）
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineSnapshot {
    /// 客户端默认选中的阈值（吸附到预设档）
    pub default_gap_min: u32,
    pub gaps: Vec<u32>,
    /// "15" → 累计在线毫秒（键为阈值分钟数）
    pub total_ms: BTreeMap<String, u64>,
    /// "15" → 活动段数
    pub segments: BTreeMap<String, u64>,
    /// 对话进行中：turn 区间并集总毫秒
    pub turn_ms: u64,
    /// 模型生成墙钟合计（并行相加，精确）
    pub llm_ms: u64,
    /// 工具执行墙钟合计（并行相加，精确）
    pub tool_ms: u64,
    pub active_days: usize,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    pub days: Vec<OnlineDay>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OnlineDay {
    pub d: String,
    /// 当日有用量的会话数
    pub sessions: usize,
    pub tokens: u64,
    /// 当日对话进行中毫秒
    pub turn_ms: u64,
    /// "15" → 当日在线毫秒（按该阈值合并后跨零点切分）
    pub by_gap: BTreeMap<String, u64>,
    /// "15" → 当日活动段数
    pub seg_by_gap: BTreeMap<String, u64>,
    pub llm_ms: u64,
    pub tool_ms: u64,
}

// ── 聚合入口 ────────────────────────────────────────────────

/// 汇总全部会话为统计快照。range_days 控制 trend 窗口（热力图恒为近 53 周）；
/// gap_min 为在线时长空闲阈值（吸附到 1/5/15/30/60 预设档）。
pub fn session_stats(range_days: u32, gap_min: u32) -> Result<SessionStats, String> {
    let root = crate::profiles::dsh_native_home().join("sessions");
    let now = now_ms();
    let (aggs, errors) = scan_sessions(&root, now);
    save_cache_to_disk();
    let sessions_total = aggs.len();
    Ok(aggregate(&aggs, sessions_total, errors, range_days, gap_min, now))
}

fn day_add(day: &str, delta: i64) -> String {
    use chrono::{Duration, NaiveDate};
    match NaiveDate::parse_from_str(day, "%Y-%m-%d") {
        Ok(d) => (d + Duration::days(delta)).format("%Y-%m-%d").to_string(),
        Err(_) => day.to_string(),
    }
}

fn aggregate(
    aggs: &[SessionAgg],
    sessions_total: usize,
    errors: usize,
    range_days: u32,
    gap_min: u32,
    now: u64,
) -> SessionStats {
    // 全局按桶合并
    let mut global: BTreeMap<String, Bucket> = BTreeMap::new();
    let mut day_sessions: BTreeMap<String, usize> = BTreeMap::new();
    let mut peak_step: Option<(u64, String, String)> = None;
    let mut used_sessions = 0usize;
    let mut first_day: Option<String> = None;
    let mut last_day: Option<String> = None;
    // 在线时长原料：跨会话汇总的活动区间 / 对话区间 / 每日引擎时长（并集在 build_online 内做）
    let mut active_ivs: Vec<[u64; 2]> = Vec::new();
    let mut turn_ivs: Vec<[u64; 2]> = Vec::new();
    let mut day_llm: BTreeMap<String, u64> = BTreeMap::new();
    let mut day_tool: BTreeMap<String, u64> = BTreeMap::new();
    for agg in aggs {
        if agg.used { used_sessions += 1; }
        active_ivs.extend(agg.active.iter().copied());
        turn_ivs.extend(agg.turns.iter().copied());
        for (d, l, t) in &agg.day_engine {
            *day_llm.entry(d.clone()).or_default() += l;
            *day_tool.entry(d.clone()).or_default() += t;
        }
        for r in &agg.records {
            let k = format!("{}|{}|{}", r.d, r.h, r.m);
            let e = global.entry(k).or_insert_with(|| Bucket { d: r.d.clone(), h: r.h, m: r.m.clone(), ..Default::default() });
            e.i += r.i; e.o += r.o; e.cr += r.cr; e.cw += r.cw; e.n += r.n;
            day_sessions.entry(r.d.clone()).or_default();
        }
        for d in agg.records.iter().map(|r| r.d.clone()).collect::<HashSet<_>>() {
            *day_sessions.entry(d.clone()).or_default() += {
                // 每个会话每天至多计 1 次（按会话数统计）
                if agg.used { 1 } else { 0 }
            };
        }
        if let Some(p) = &agg.peak {
            if peak_step.as_ref().map_or(true, |q| p.0 > q.0) { peak_step = Some(p.clone()); }
        }
        if let Some(f) = &agg.first { if first_day.as_ref().map_or(true, |c| f < c) { first_day = Some(f.clone()); } }
        if let Some(l) = &agg.last { if last_day.as_ref().map_or(true, |c| l > c) { last_day = Some(l.clone()); } }
    }
    // 日聚合
    let mut by_day: BTreeMap<String, (u64, u64)> = BTreeMap::new(); // day → (tokens, calls)
    let mut day_io: BTreeMap<String, [u64; 4]> = BTreeMap::new(); // day → (input, output, cacheRead, cacheWrite)
    let mut day_models: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    let mut by_model: BTreeMap<String, ModelUsage> = BTreeMap::new();
    let mut total = Overview::default();
    for r in global.values() {
        let tokens = r.i + r.o + r.cr + r.cw;
        let day = by_day.entry(r.d.clone()).or_default();
        day.0 += tokens;
        day.1 += r.n;
        let io = day_io.entry(r.d.clone()).or_insert([0; 4]);
        io[0] += r.i; io[1] += r.o; io[2] += r.cr; io[3] += r.cw;
        *day_models.entry(r.d.clone()).or_default().entry(r.m.clone()).or_default() += tokens;
        let m = by_model.entry(r.m.clone()).or_insert_with(|| ModelUsage { model: r.m.clone(), ..Default::default() });
        m.tokens += tokens; m.input += r.i; m.output += r.o; m.cache_read += r.cr; m.cache_write += r.cw; m.calls += r.n;
        total.total_input += r.i;
        total.total_output += r.o;
        total.total_cache_read += r.cr;
        total.total_cache_write += r.cw;
        total.calls += r.n;
    }
    total.total_tokens = total.total_input + total.total_output + total.total_cache_read + total.total_cache_write;
    total.active_days = by_day.len();
    if !by_day.is_empty() {
        total.avg_day = total.total_tokens / by_day.len() as u64;
        let mut iter = by_day.iter();
        let (d0, t0) = iter.next().unwrap();
        total.peak_day = Some(PeakDay { day: d0.clone(), tokens: t0.0 });
        for (d, t) in iter {
            let p = total.peak_day.as_mut().unwrap();
            if t.0 > p.tokens { p.day = d.clone(); p.tokens = t.0; }
        }
    }
    if let Some(p) = peak_step {
        total.peak_step = Some(PeakStep { tokens: p.0, day: p.1, model: p.2 });
    }
    total.first_day = first_day.clone();
    total.last_day = last_day.clone();
    total.current_streak = streak_ending_at(&by_day, &local_day_hour(now).0);
    total.longest_streak = longest_streak(by_day.keys());

    // 热力图：近 53 周
    let today = local_day_hour(now).0;
    let mut heatmap = Vec::new();
    let start = day_add(&today, -((53 * 7) as i64));
    let mut d = start.clone();
    while d <= today {
        let tokens = by_day.get(&d).map(|x| x.0).unwrap_or(0);
        let sessions = day_sessions.get(&d).copied().unwrap_or(0);
        let io = day_io.get(&d).copied().unwrap_or([0; 4]);
        let top_model = day_models
            .get(&d)
            .and_then(|m| m.iter().max_by_key(|(_, v)| *v))
            .map(|(model, tokens)| ModelTokens { model: model.clone(), tokens: *tokens });
        heatmap.push(DayCell {
            day: d.clone(),
            tokens,
            sessions,
            input: io[0],
            output: io[1],
            cache_read: io[2],
            cache_write: io[3],
            top_model,
        });
        d = day_add(&d, 1);
    }

    // trend：窗口内逐日（含 0 值日，前端画连续折线）
    let cutoff = day_add(&today, -(range_days.saturating_sub(1) as i64));
    let mut trend = Vec::new();
    let mut d = cutoff.clone();
    while d <= today {
        let mut per_model: BTreeMap<String, u64> = BTreeMap::new();
        let mut t = 0u64;
        let mut calls = 0u64;
        for r in global.values().filter(|r| r.d == d) {
            let tokens = r.i + r.o + r.cr + r.cw;
            *per_model.entry(r.m.clone()).or_default() += tokens;
            t += tokens;
            calls += r.n;
        }
        trend.push(TrendPoint {
            day: d.clone(),
            by_model: per_model.into_iter().map(|(model, tokens)| ModelTokens { model, tokens }).collect(),
            total: t,
            calls,
        });
        d = day_add(&d, 1);
    }

    // 今日：24 小时 × 模型
    let yesterday = day_add(&today, -1);
    let mut hour_map: BTreeMap<u32, BTreeMap<String, u64>> = BTreeMap::new();
    let mut today_total = 0u64;
    let mut today_calls = 0u64;
    let mut yesterday_total = 0u64;
    for r in global.values() {
        let tokens = r.i + r.o + r.cr + r.cw;
        if r.d == today {
            *hour_map.entry(r.h).or_default().entry(r.m.clone()).or_default() += tokens;
            today_total += tokens;
            today_calls += r.n;
        } else if r.d == yesterday {
            yesterday_total += tokens;
        }
    }
    let hours = (0..24u32).map(|h| {
        let bm = hour_map.get(&h);
        let by_model: Vec<ModelTokens> = bm.map(|m| m.iter().map(|(model, tokens)| ModelTokens { model: model.clone(), tokens: *tokens }).collect()).unwrap_or_default();
        let total = by_model.iter().map(|x| x.tokens).sum();
        HourCell { hour: h, by_model, total }
    }).collect();

    let grand = total.total_tokens.max(1);
    let mut models: Vec<ModelUsage> = by_model.into_values().collect();
    for m in &mut models { m.share = m.tokens as f64 / grand as f64; }
    models.sort_by(|a, b| b.tokens.cmp(&a.tokens));

    let day_tokens: BTreeMap<String, u64> = by_day.iter().map(|(d, t)| (d.clone(), t.0)).collect();
    let online = build_online(&active_ivs, &turn_ivs, &day_sessions, &day_tokens, &day_llm, &day_tool, gap_min);

    SessionStats {
        overview: total,
        trend,
        today: TodayStats { day: today, total: today_total, calls: today_calls, yesterday_total, hours },
        heatmap,
        models,
        online,
        range_days,
        sessions_total,
        sessions_with_usage: used_sessions,
        errors,
        generated_at: now,
    }
}

/// 组装在线时长快照（对齐 online.ts buildOnline）：对每个预设阈值给出累计/段数/每日切分；
/// 「对话进行中」= turn 区间并集（不做空闲合并：轮次之间的间隔就是间隔）
fn build_online(
    active: &[[u64; 2]],
    turns: &[[u64; 2]],
    day_sessions: &BTreeMap<String, usize>,
    day_tokens: &BTreeMap<String, u64>,
    day_llm: &BTreeMap<String, u64>,
    day_tool: &BTreeMap<String, u64>,
    gap_min: u32,
) -> OnlineSnapshot {
    let mut totals: BTreeMap<String, u64> = BTreeMap::new();
    let mut segments: BTreeMap<String, u64> = BTreeMap::new();
    let mut per_gap_days: Vec<BTreeMap<String, u64>> = Vec::new();
    let mut per_gap_segs: Vec<BTreeMap<String, u64>> = Vec::new();
    let mut day_set: BTreeSet<String> = BTreeSet::new();
    for g in PRESET_GAPS_MIN {
        let merged = merge_intervals(active, g as u64 * 60_000);
        totals.insert(g.to_string(), total_ms(&merged));
        segments.insert(g.to_string(), merged.len() as u64);
        let days = split_by_day(&merged);
        let segs = count_by_day(&merged);
        day_set.extend(days.keys().cloned());
        per_gap_days.push(days);
        per_gap_segs.push(segs);
    }
    day_set.extend(day_sessions.keys().cloned());
    day_set.extend(day_llm.keys().cloned());
    day_set.extend(day_tool.keys().cloned());

    let turn_merged = merge_intervals(turns, 0);
    let turn_total = total_ms(&turn_merged);
    let turn_days = split_by_day(&turn_merged);

    let days: Vec<OnlineDay> = day_set
        .into_iter()
        .map(|d| {
            let mut by_gap = BTreeMap::new();
            let mut seg_by_gap = BTreeMap::new();
            for (i, g) in PRESET_GAPS_MIN.iter().enumerate() {
                by_gap.insert(g.to_string(), per_gap_days[i].get(&d).copied().unwrap_or(0));
                seg_by_gap.insert(g.to_string(), per_gap_segs[i].get(&d).copied().unwrap_or(0));
            }
            OnlineDay {
                sessions: day_sessions.get(&d).copied().unwrap_or(0),
                tokens: day_tokens.get(&d).copied().unwrap_or(0),
                turn_ms: turn_days.get(&d).copied().unwrap_or(0),
                llm_ms: day_llm.get(&d).copied().unwrap_or(0),
                tool_ms: day_tool.get(&d).copied().unwrap_or(0),
                by_gap,
                seg_by_gap,
                d,
            }
        })
        .collect();
    OnlineSnapshot {
        default_gap_min: norm_gap_min(gap_min),
        gaps: PRESET_GAPS_MIN.to_vec(),
        total_ms: totals,
        segments,
        turn_ms: turn_total,
        llm_ms: day_llm.values().sum(),
        tool_ms: day_tool.values().sum(),
        active_days: days.len(),
        first_day: days.first().map(|x| x.d.clone()),
        last_day: days.last().map(|x| x.d.clone()),
        days,
    }
}

/// 当前连续天数：今天有用则从今天向前数，否则从昨天向前数（与插件口径一致）
fn streak_ending_at(by_day: &BTreeMap<String, (u64, u64)>, today: &str) -> usize {
    let mut start = today.to_string();
    if !by_day.contains_key(&start) {
        start = day_add(today, -1);
        if !by_day.contains_key(&start) { return 0; }
    }
    let mut n = 0usize;
    let mut d = start;
    while by_day.contains_key(&d) {
        n += 1;
        d = day_add(&d, -1);
    }
    n
}

fn longest_streak<'a, I: Iterator<Item = &'a String>>(days: I) -> usize {
    use chrono::NaiveDate;
    let mut ds: Vec<NaiveDate> = days.filter_map(|d| NaiveDate::parse_from_str(d, "%Y-%m-%d").ok()).collect();
    ds.sort();
    let mut best = 0usize;
    let mut cur = 0usize;
    let mut prev: Option<NaiveDate> = None;
    for d in ds {
        cur = match prev {
            Some(p) if (d - p).num_days() == 1 => cur + 1,
            _ => 1,
        };
        best = best.max(cur);
        prev = Some(d);
    }
    best
}

// ── 单测 ────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ev(kind: &str, time: u64, data: serde_json::Value) -> Event {
        Event { seq: 0, kind: kind.into(), time_ms: time, data }
    }

    fn usage(i: u64, o: u64, cr: u64, cw: u64) -> serde_json::Value {
        json!({ "inputTokens": i, "outputTokens": o, "cacheReadTokens": cr, "cacheWriteTokens": cw })
    }

    #[test]
    fn fold_counts_message_terminal_not_sample() {
        // 同一 (turn,step)：chunk 采样 100，message 终值 180 → 只计 180
        let events = vec![
            ev("request/context", 1, json!({ "provider": "deepseek", "model": "v4" })),
            ev("assistant/chunk", 2, json!({ "turn": 1, "step": 0, "chunk": { "type": "usage", "usage": usage(100, 0, 0, 0) } })),
            ev("assistant/message", 3, json!({ "turn": 1, "step": 0, "usage": usage(120, 60, 0, 0) })),
        ];
        let agg = fold_session(&events, 0);
        assert!(agg.used);
        assert_eq!(agg.records.len(), 1);
        let r = &agg.records[0];
        assert_eq!(r.n, 1);
        assert_eq!(r.i + r.o, 180);
        assert_eq!(r.m, "deepseek/v4");
        assert_eq!(r.d, local_day_hour(3).0);
        assert_eq!(r.h, local_day_hour(3).1);
    }

    #[test]
    fn fold_summits_samples_on_key_change() {
        // 不同 (turn,step) 各自计入；零 usage 不记
        let events = vec![
            ev("assistant/chunk", 5, json!({ "turn": 1, "step": 0, "chunk": { "type": "usage", "usage": usage(10, 5, 0, 0) } })),
            ev("assistant/chunk", 6, json!({ "turn": 2, "step": 0, "chunk": { "type": "usage", "usage": usage(7, 3, 0, 0) } })),
            ev("assistant/chunk", 7, json!({ "turn": 3, "step": 0, "chunk": { "type": "usage", "usage": usage(0, 0, 0, 0) } })),
        ];
        let agg = fold_session(&events, 0);
        assert_eq!(agg.records.iter().map(|r| r.n).sum::<u64>(), 2);
        assert_eq!(agg.records.iter().map(|r| r.i + r.o + r.cr + r.cw).sum::<u64>(), 25);
    }

    #[test]
    fn fold_route_semantic_update_keeps_known_half() {
        // request/context 只带 provider 时，不洗掉已知 model
        let events = vec![
            ev("request/header", 1, json!({ "header": { "config": { "provider": "p1", "model": "m1" } } })),
            ev("request/context", 2, json!({ "provider": "p2" })),
            ev("assistant/chunk", 3, json!({ "turn": 1, "step": 0, "chunk": { "type": "usage", "usage": usage(1, 0, 0, 0) } })),
        ];
        let agg = fold_session(&events, 0);
        assert_eq!(agg.records[0].m, "p2/m1");
    }

    #[test]
    fn fold_skip_ignores_seeded_prefix() {
        let events = vec![
            ev("assistant/message", 1, json!({ "turn": 0, "step": 0, "usage": usage(50, 0, 0, 0) })),
            ev("assistant/message", 2, json!({ "turn": 1, "step": 0, "usage": usage(7, 0, 0, 0) })),
        ];
        let agg = fold_session(&events, 1);
        assert_eq!(agg.records.iter().map(|r| r.i).sum::<u64>(), 7);
    }

    #[test]
    fn inherited_count_matches_prefix_by_seq_and_data() {
        let mk = |n: usize, off: i64| -> SessionLog {
            SessionLog {
                header: SessionHeader::default(),
                events: (0..n).map(|i| Event {
                    seq: i as i64 + off,
                    kind: "x".into(),
                    time_ms: 0,
                    data: json!({ "v": i }),
                }).collect(),
            }
        };
        assert_eq!(inherited_count(&mk(3, 0).events, &mk(5, 0).events), 3);
        assert_eq!(inherited_count(&mk(3, 2).events, &mk(5, 0).events), 0);
    }

    #[test]
    fn fork_cut_prefers_inherited_marker() {
        let events = vec![
            ev("session/start", 100, json!({})),
            ev("assistant/message", 200, json!({ "turn": 0, "step": 0, "usage": usage(50, 0, 0, 0) })),
            ev("session/end-seed", 250, json!({ "inherited": true })),
            ev("assistant/message", 400, json!({ "turn": 1, "step": 0, "usage": usage(7, 0, 0, 0) })),
        ];
        // 标记（含本身）优先于 createdAt：createdAt 给个会切错的值验证优先级
        assert_eq!(fork_cut(&events, 150), 3);
        // 多个标记取最后一个
        let mut e2 = events.clone();
        e2.push(ev("session/end-seed", 450, json!({ "inherited": true })));
        e2.push(ev("turn/start", 500, json!({})));
        assert_eq!(fork_cut(&e2, 0), 5);
        // inherited 非 true 的标记不算切点
        let e3 = vec![ev("session/end-seed", 100, json!({ "inherited": false }))];
        assert_eq!(fork_cut(&e3, 0), 0);
    }

    #[test]
    fn fork_cut_falls_back_to_created_at_position() {
        let events = vec![
            ev("assistant/message", 1_000, json!({ "turn": 0, "step": 0, "usage": usage(50, 0, 0, 0) })),
            ev("assistant/message", 2_000, json!({ "turn": 1, "step": 0, "usage": usage(7, 0, 0, 0) })),
            ev("assistant/message", 5_000, json!({ "turn": 2, "step": 0, "usage": usage(3, 0, 0, 0) })),
        ];
        assert_eq!(fork_cut(&events, 2_000), 1);
        assert_eq!(fork_cut(&events, 1_000), 0); // 普通会话：首事件即 ≥ createdAt → 不切
        assert_eq!(fork_cut(&events, 9_000), 0); // 整份都在 createdAt 前 → 视为时钟异常不切（与插件一致）
        assert_eq!(fork_cut(&events, 0), 0); // 无 createdAt 且无标记 → 交给调用方兜底
    }

    #[test]
    fn forked_log_with_renumbered_parent_is_not_double_counted() {
        // 真实事故回归：fork 继承了父前缀但 seq 与父文件不再逐条相等（旧前缀比对在第 8 条断裂），
        // 只要带 inherited 标记就必须切干净
        let events = vec![
            ev("assistant/message", 1, json!({ "turn": 9, "step": 0, "usage": usage(130_836_526, 0, 0, 0) })),
            Event { seq: 7777, kind: "session/end-seed".into(), time_ms: 2, data: json!({ "inherited": true }) },
            ev("assistant/message", 3, json!({ "turn": 1, "step": 0, "usage": usage(7, 0, 0, 0) })),
        ];
        let skip = fork_cut(&events, 1_700_000_000_000);
        let agg = fold_session(&events, skip);
        assert_eq!(agg.records.iter().map(|r| r.i).sum::<u64>(), 7);
    }

    #[test]
    fn streak_counts_back_from_yesterday_when_today_idle() {
        let mut by_day = BTreeMap::new();
        let today = local_day_hour(now_ms()).0;
        let y = day_add(&today, -1);
        let yy = day_add(&today, -2);
        by_day.insert(y.clone(), (1, 1));
        by_day.insert(yy.clone(), (1, 1));
        assert_eq!(streak_ending_at(&by_day, &today), 2);
        by_day.insert(today.clone(), (1, 1));
        assert_eq!(streak_ending_at(&by_day, &today), 3);
    }

    #[test]
    fn longest_streak_handles_gaps() {
        let days: Vec<String> = ["2026-01-01", "2026-01-02", "2026-01-05", "2026-01-06", "2026-01-07"]
            .iter().map(|s| s.to_string()).collect();
        assert_eq!(longest_streak(days.iter()), 3);
    }

    #[test]
    fn aggregate_produces_consistent_totals() {
        let t = now_ms();
        let log = SessionLog {
            header: SessionHeader::default(),
            events: vec![ev("assistant/message", t, json!({ "turn": 1, "step": 0, "usage": usage(10, 5, 3, 2) }))],
        };
        let agg = fold_session(&log.events, 0);
        let stats = aggregate(&[agg], 1, 0, 7, DEFAULT_GAP_MIN, t);
        assert_eq!(stats.overview.total_tokens, 20);
        assert_eq!(stats.overview.calls, 1);
        assert_eq!(stats.sessions_with_usage, 1);
        assert_eq!(stats.trend.len() as i64, 7);
        assert_eq!(stats.heatmap.last().unwrap().tokens, 20);
        assert_eq!(stats.today.total, 20);
        assert_eq!(stats.models[0].model, "unknown/unknown");
        // 在线快照：单点事件不构成时长，但活跃日必须出现
        assert_eq!(stats.online.gaps, PRESET_GAPS_MIN.to_vec());
        assert_eq!(stats.online.default_gap_min, DEFAULT_GAP_MIN);
        assert_eq!(stats.online.turn_ms, 0);
        assert!(stats.online.days.iter().any(|d| d.by_gap.contains_key("15")));
    }

    /// 真实数据冒烟：对本机 ~/.dsh/sessions 全量扫描，验证不 panic、口径自洽。
    /// 手动跑：cargo test --lib sessions::tests::real -- --ignored --nocapture
    #[test]
    #[ignore]
    fn real_sessions_scan_is_consistent() {
        let stats = session_stats(30, DEFAULT_GAP_MIN).expect("真实扫描失败");
        let sum_records: u64 = stats.trend.iter().map(|p| p.total).sum();
        let sum_heat: u64 = stats.heatmap.iter().map(|c| c.tokens).sum();
        let sum_models: u64 = stats.models.iter().map(|m| m.tokens).sum();
        eprintln!(
            "会话 {}（有用量 {}，坏 {}）总 token {}，近30天 {}，热力窗口 {}，模型合计 {}，活跃 {} 天，连续 {} 天",
            stats.sessions_total, stats.sessions_with_usage, stats.errors,
            stats.overview.total_tokens, sum_records, sum_heat, sum_models,
            stats.overview.active_days, stats.overview.current_streak,
        );
        assert!(sum_models <= stats.overview.total_tokens);
        assert!(sum_records <= sum_heat);
        assert!(sum_heat <= stats.overview.total_tokens);
        assert_eq!(stats.overview.total_tokens, stats.overview.total_input + stats.overview.total_output + stats.overview.total_cache_read + stats.overview.total_cache_write);
    }

    #[test]
    #[ignore]
    fn real_online_duration_smoke() {
        let stats = session_stats(365, DEFAULT_GAP_MIN).expect("真实扫描失败");
        let on = &stats.online;
        eprintln!(
            "在线时长：默认阈值 {} 分钟，累计 {:?}",
            on.default_gap_min, on.total_ms,
        );
        eprintln!(
            "段数 {:?}，对话进行中 {} s，模型生成 {} s，工具执行 {} s，活跃 {} 天（{} → {}）",
            on.segments,
            on.turn_ms / 1000, on.llm_ms / 1000, on.tool_ms / 1000,
            on.active_days,
            on.first_day.clone().unwrap_or_default(),
            on.last_day.clone().unwrap_or_default(),
        );
        for d in on.days.iter().rev().take(5) {
            eprintln!("{}: 在线 {} min，对话 {} min，llm {} s，tool {} s",
                d.d, d.by_gap.get("15").unwrap_or(&0) / 60_000, d.turn_ms / 60_000, d.llm_ms / 1000, d.tool_ms / 1000);
        }
        // 口径自洽：阈值越大合并越激进，累计在线只增不减
        let mut prev = 0u64;
        for g in PRESET_GAPS_MIN {
            let t = on.total_ms.get(&g.to_string()).copied().unwrap_or(0);
            assert!(t >= prev, "阈值 {g} 分钟反而变短：{t} < {prev}");
            prev = t;
        }
        assert!(on.llm_ms + on.tool_ms > 0 || on.turn_ms == 0);
    }

    #[test]
    fn merge_points_splits_on_gap() {
        let iv = merge_points(&[0, 30_000, 120_000], 60_000);
        assert_eq!(iv, vec![[0, 30_000], [120_000, 120_000]]);
        // 恰好等于阈值的间隔仍算同一段（≤）
        assert_eq!(merge_points(&[0, 60_000], 60_000), vec![[0, 60_000]]);
        assert!(merge_points(&[], 60_000).is_empty());
    }

    #[test]
    fn merge_intervals_union_and_remerge() {
        assert_eq!(merge_intervals(&[[0, 10], [5, 20], [30, 40]], 0), vec![[0, 20], [30, 40]]);
        // 两级合并等价：1 分钟粗合并后再按 15 分钟合并 == 直接按 15 分钟合并
        let raw = vec![[0u64, 40_000], [50_000, 60_000], [800_000, 900_000]];
        let one = merge_intervals(&merge_intervals(&raw, 60_000), 15 * 60_000);
        assert_eq!(one, merge_intervals(&raw, 15 * 60_000));
        assert_eq!(one, vec![[0, 900_000]]);
    }

    #[test]
    fn two_level_merge_matches_direct_points_merge() {
        // 事件时刻先按 BASE_GAP 合并（缓存级），再按大阈值重并 == 原始时刻直接大阈值合并
        let times: Vec<u64> = (0..20).map(|i| i as u64 * 45_000).collect();
        let cached = merge_points(&times, BASE_GAP_MS);
        assert_eq!(
            total_ms(&merge_intervals(&cached, 15 * 60_000)),
            total_ms(&merge_points(&times, 15 * 60_000)),
        );
    }

    #[test]
    fn split_by_day_crosses_midnight() {
        use chrono::{Local, NaiveDate, TimeZone};
        let t = |day: u32, h: u32| -> u64 {
            Local.from_local_datetime(
                &NaiveDate::from_ymd_opt(2026, 3, day).unwrap().and_hms_opt(h, 0, 0).unwrap(),
            )
            .single()
            .unwrap()
            .timestamp_millis() as u64
        };
        let m = split_by_day(&[[t(10, 23), t(11, 3)]]);
        assert_eq!(m.get("2026-03-10"), Some(&(3_600_000)));
        assert_eq!(m.get("2026-03-11"), Some(&(3 * 3_600_000)));
    }

    #[test]
    fn norm_gap_min_snaps_to_preset() {
        assert_eq!(norm_gap_min(0), 1);
        assert_eq!(norm_gap_min(7), 5);
        assert_eq!(norm_gap_min(20), 15);
        assert_eq!(norm_gap_min(999), 60);
    }

    #[test]
    fn fold_collects_turn_llm_tool() {
        let events = vec![
            ev("turn/start", 1_000, json!({})),
            ev("step/start", 2_000, json!({ "turn": 1, "step": 0 })),
            ev("tool/call", 3_000, json!({ "callId": "c1" })),
            ev("tool/result", 8_000, json!({ "message": { "source": { "callId": "c1" } } })),
            ev("assistant/message", 15_000, json!({ "turn": 1, "step": 0, "usage": usage(1, 0, 0, 0) })),
            ev("turn/end", 20_000, json!({})),
        ];
        let agg = fold_session(&events, 0);
        assert_eq!(agg.turns, vec![[1_000, 20_000]]);
        assert_eq!(agg.llm_ms, 13_000); // step/start 2s → message 15s
        assert_eq!(agg.tool_ms, 5_000); // call 3s → result 8s
        assert_eq!(agg.day_engine.len(), 1);
    }

    #[test]
    fn fold_turn_end_drops_unresolved_tool_calls() {
        // 轮次取消：调用没有落地结果 → 不计入 toolMs
        let events = vec![
            ev("turn/start", 1_000, json!({})),
            ev("tool/call", 2_000, json!({ "callId": "c1" })),
            ev("turn/end", 9_000, json!({})),
            ev("tool/result", 60_000, json!({ "message": { "source": { "callId": "c1" } } })),
        ];
        let agg = fold_session(&events, 0);
        assert_eq!(agg.tool_ms, 0);
    }

    #[test]
    fn build_online_totals_are_monotonic_by_gap() {
        let active = vec![[0u64, 40_000], [50_000, 60_000]];
        let turns = vec![[1_000u64, 20_000]];
        let mut day_sessions = BTreeMap::new();
        day_sessions.insert("2026-01-01".to_string(), 2usize);
        let mut day_tokens = BTreeMap::new();
        day_tokens.insert("2026-01-01".to_string(), 999u64);
        let mut day_llm = BTreeMap::new();
        day_llm.insert("2026-01-01".to_string(), 5_000u64);
        let mut day_tool = BTreeMap::new();
        day_tool.insert("2026-01-01".to_string(), 3_000u64);
        let on = build_online(&active, &turns, &day_sessions, &day_tokens, &day_llm, &day_tool, 15);
        assert_eq!(on.default_gap_min, 15);
        // 阈值 ≥ 10s 时两段并成一段
        assert_eq!(on.total_ms.get("1"), Some(&60_000));
        assert_eq!(on.segments.get("1"), Some(&1));
        assert_eq!(on.turn_ms, 19_000);
        assert_eq!(on.llm_ms, 5_000);
        assert_eq!(on.tool_ms, 3_000);
        // 天维度：活跃区间落进 1970-01-01（本地），会话/token 落进注入日
        let d1 = on.days.iter().find(|x| x.d == "2026-01-01").expect("缺少注入日");
        assert_eq!(d1.sessions, 2);
        assert_eq!(d1.tokens, 999);
        assert_eq!(d1.llm_ms, 5_000);
        assert_eq!(d1.tool_ms, 3_000);
        assert_eq!(on.active_days, on.days.len());
        assert!(on.days.len() >= 2);
    }
}
