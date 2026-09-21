//! 模型配置：~/.dsh/settings.yaml 中的 `llm-pi-ai.providers`（模型服务提供方与可用模型）
//! 与 `agent-default-model`（Agent 默认模型 / 推理档位）两节的结构化编辑。
//!
//! 保存策略：把这两节按当前文本「整节替换」（行级外科手术），文件其余内容与节外
//! 注释逐字节保留；节内注释随重写丢失。写前自动备份，写后整体 YAML 校验。

use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use serde_yaml::{Mapping, Value as Yaml};

use crate::profile_cfg::{backup, global_config_path};

const SECTION_PROVIDERS: &str = "llm-pi-ai";
const SECTION_DEFAULT: &str = "agent-default-model";
const KEY_PROVIDERS: &str = "providers";

const KNOWN_PROVIDER_KEYS: &[&str] = &[
    "displayName",
    "apiKeyEnv",
    "api",
    "baseURL",
    "headers",
    "compat",
    "models",
];
const KNOWN_MODEL_KEYS: &[&str] = &[
    "id",
    "name",
    "contextWindow",
    "maxTokens",
    "input",
    "reasoningEfforts",
];
const KNOWN_DEFAULT_KEYS: &[&str] = &["provider", "model", "reasoningEffort"];

// ── 读取（后端 → 前端） ─────────────────────────

/// 一个可用模型（llm-pi-ai.providers.<id>.models[] 条目）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntry {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    /// 输入模态（如 text / image）
    pub input: Vec<String>,
    /// 推理档位映射：逻辑档位名 → API 参数值（值可为 null），原样透传
    pub reasoning_efforts: Option<Json>,
    /// 未识别字段原样透传（保存时原位恢复，避免丢数据）
    pub extra: Option<Json>,
}

/// 一个模型服务提供方（llm-pi-ai.providers 的一个键值对）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderEntry {
    pub id: String,
    pub display_name: Option<String>,
    /// openai-completions | openai-responses | anthropic-messages（自定义值原样透传）
    pub api: Option<String>,
    // camelCase 会得到 baseUrl，与 YAML 键 / 前端契约的 baseURL 不一致，显式指定
    #[serde(rename = "baseURL")]
    pub base_url: Option<String>,
    /// API Key 的环境变量名（值可在环境变量或凭据 refs 中维护）
    pub api_key_env: Option<String>,
    pub headers: Option<Json>,
    pub compat: Option<Json>,
    pub models: Vec<ModelEntry>,
    pub extra: Option<Json>,
}

/// agent-default-model：Agent 默认使用的模型与推理档位
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModel {
    pub provider: String,
    pub model: String,
    /// 取所选模型 reasoningEfforts 的键名（如 low / high / xhigh）
    pub reasoning_effort: Option<String>,
    pub extra: Option<Json>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfig {
    pub path: String,
    pub exists: bool,
    pub providers: Vec<ProviderEntry>,
    pub default_model: Option<DefaultModel>,
    /// 文件存在但解析失败时置位：前端禁止结构化保存（避免覆盖坏文件前的未知内容）
    pub parse_error: Option<String>,
}

fn yaml_to_json(v: &Yaml) -> Option<Json> {
    serde_json::to_value(v).ok()
}

fn get<'a>(m: Option<&'a Mapping>, key: &str) -> Option<&'a Yaml> {
    m.and_then(|mm| mm.get(Yaml::String(key.into())))
}

fn as_string(v: Option<&Yaml>) -> Option<String> {
    v.and_then(|x| x.as_str()).map(String::from)
}

fn as_u64(v: Option<&Yaml>) -> Option<u64> {
    v.and_then(|x| {
        x.as_u64()
            .or_else(|| x.as_i64().and_then(|i| u64::try_from(i).ok()))
    })
}

/// 提取映射中除 known 之外的字符串键条目（原样透传给前端，保存时原位恢复）
fn extra_of(m: Option<&Mapping>, known: &[&str]) -> Option<Json> {
    let m = m?;
    let mut out = serde_json::Map::new();
    for (k, v) in m {
        let Some(key) = k.as_str() else { continue };
        if known.contains(&key) {
            continue;
        }
        if let Some(j) = yaml_to_json(v) {
            out.insert(key.to_string(), j);
        }
    }
    (!out.is_empty()).then_some(Json::Object(out))
}

fn provider_from_yaml(id: &str, v: &Yaml) -> ProviderEntry {
    let m = v.as_mapping();
    ProviderEntry {
        id: id.to_string(),
        display_name: as_string(get(m, "displayName")),
        api: as_string(get(m, "api")),
        base_url: as_string(get(m, "baseURL")),
        api_key_env: as_string(get(m, "apiKeyEnv")),
        headers: get(m, "headers").filter(|x| !x.is_null()).and_then(yaml_to_json),
        compat: get(m, "compat").filter(|x| !x.is_null()).and_then(yaml_to_json),
        models: get(m, "models")
            .and_then(|x| x.as_sequence())
            .map(|seq| seq.iter().filter_map(model_from_yaml).collect())
            .unwrap_or_default(),
        extra: extra_of(m, KNOWN_PROVIDER_KEYS),
    }
}

fn model_from_yaml(v: &Yaml) -> Option<ModelEntry> {
    let m = v.as_mapping()?;
    Some(ModelEntry {
        id: as_string(get(Some(m), "id")).unwrap_or_default(),
        name: as_string(get(Some(m), "name")),
        context_window: as_u64(get(Some(m), "contextWindow")),
        max_tokens: as_u64(get(Some(m), "maxTokens")),
        input: get(Some(m), "input")
            .and_then(|x| x.as_sequence())
            .map(|s| {
                s.iter()
                    .filter_map(|i| i.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        reasoning_efforts: get(Some(m), "reasoningEfforts")
            .filter(|x| !x.is_null())
            .and_then(yaml_to_json),
        extra: extra_of(Some(m), KNOWN_MODEL_KEYS),
    })
}

/// 读取模型配置。文件不存在时 exists=false 而非报错（dsh 首次运行后才创建）。
pub fn read() -> Result<ModelConfig, String> {
    let path = global_config_path();
    let exists = path.is_file();
    let mut out = ModelConfig {
        path: path.to_string_lossy().into_owned(),
        exists,
        providers: Vec::new(),
        default_model: None,
        parse_error: None,
    };
    if !exists {
        return Ok(out);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(out);
    }
    let doc: Yaml = match serde_yaml::from_str(&raw) {
        Ok(d) => d,
        Err(e) => {
            out.parse_error = Some(e.to_string());
            return Ok(out);
        }
    };
    let Some(root) = doc.as_mapping() else {
        out.parse_error = Some("顶层不是映射".into());
        return Ok(out);
    };
    let llm = get(Some(root), SECTION_PROVIDERS).and_then(|v| v.as_mapping());
    if let Some(provs) = get(llm, KEY_PROVIDERS).and_then(|p| p.as_mapping()) {
        for (k, v) in provs {
            let Some(id) = k.as_str() else { continue };
            out.providers.push(provider_from_yaml(id, v));
        }
    }
    if let Some(dm) = get(Some(root), SECTION_DEFAULT).and_then(|d| d.as_mapping()) {
        // provider / model 任一缺失即视为未设置默认（节内可能只剩透传的未知键）
        let provider = as_string(get(Some(dm), "provider")).unwrap_or_default();
        let model = as_string(get(Some(dm), "model")).unwrap_or_default();
        if !provider.is_empty() && !model.is_empty() {
            out.default_model = Some(DefaultModel {
                provider,
                model,
                reasoning_effort: as_string(get(Some(dm), "reasoningEffort")),
                extra: extra_of(Some(dm), KNOWN_DEFAULT_KEYS),
            });
        }
    }
    Ok(out)
}

// ── 保存（前端 → 后端） ─────────────────────────

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelEntryInput {
    pub id: String,
    pub name: Option<String>,
    pub context_window: Option<u64>,
    pub max_tokens: Option<u64>,
    pub input: Option<Vec<String>>,
    pub reasoning_efforts: Option<Json>,
    pub extra: Option<Json>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInput {
    pub id: String,
    pub display_name: Option<String>,
    pub api: Option<String>,
    #[serde(rename = "baseURL")]
    pub base_url: Option<String>,
    pub api_key_env: Option<String>,
    pub headers: Option<Json>,
    pub compat: Option<Json>,
    pub models: Vec<ModelEntryInput>,
    pub extra: Option<Json>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct DefaultModelInput {
    pub provider: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub extra: Option<Json>,
}

#[derive(Clone, Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ModelConfigInput {
    pub providers: Vec<ProviderInput>,
    pub default_model: Option<DefaultModelInput>,
}

fn json_to_yaml(v: &Json) -> Result<Yaml, String> {
    serde_yaml::to_value(v).map_err(|e| format!("字段值转换失败: {e}"))
}

fn norm_str(s: &Option<String>) -> Option<String> {
    s.as_ref()
        .map(|x| x.trim())
        .filter(|x| !x.is_empty())
        .map(String::from)
}

fn validate_id(id: &str, what: &str) -> Result<String, String> {
    let id = id.trim();
    if id.is_empty() {
        return Err(format!("{what}不能为空"));
    }
    if id.len() > 128 {
        return Err(format!("{what}过长（> 128 字符）: {id}"));
    }
    if id.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(format!("{what}不能包含空白或控制字符: {id}"));
    }
    Ok(id.to_string())
}

fn validate(input: &ModelConfigInput) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for p in &input.providers {
        let id = validate_id(&p.id, "Provider ID")?;
        if !seen.insert(id.clone()) {
            return Err(format!("Provider ID 重复: {id}"));
        }
        if let Some(b) = norm_str(&p.base_url) {
            if !(b.starts_with("http://") || b.starts_with("https://")) {
                return Err(format!(
                    "Provider「{id}」的 baseURL 需以 http:// 或 https:// 开头: {b}"
                ));
            }
        }
        if let Some(e) = norm_str(&p.api_key_env) {
            validate_id(&e, "apiKeyEnv")?;
        }
        let mut seen_model = std::collections::BTreeSet::new();
        for mo in &p.models {
            let mid = validate_id(&mo.id, "模型 ID")?;
            if !seen_model.insert(mid.clone()) {
                return Err(format!("Provider「{id}」内模型 ID 重复: {mid}"));
            }
        }
    }
    if let Some(d) = &input.default_model {
        let pid = d.provider.trim();
        let Some(p) = input.providers.iter().find(|p| p.id.trim() == pid) else {
            return Err(format!("默认模型指向的 Provider「{pid}」不存在"));
        };
        let mid = d.model.trim();
        if !p.models.iter().any(|mo| mo.id.trim() == mid) {
            return Err(format!(
                "默认模型指向的模型「{mid}」在 Provider「{pid}」中不存在"
            ));
        }
    }
    Ok(())
}

/// extra 透传字段先铺底（保存原样保留的未知字段），已知键随后按规范顺序覆盖
fn spread_extra(m: &mut Mapping, extra: &Option<Json>) -> Result<(), String> {
    if let Some(Json::Object(obj)) = extra {
        for (k, v) in obj {
            m.insert(Yaml::String(k.clone()), json_to_yaml(v)?);
        }
    }
    Ok(())
}

fn remove_known(m: &mut Mapping, known: &[&str]) {
    for k in known {
        m.remove(&Yaml::String((*k).into()));
    }
}

fn insert_str(m: &mut Mapping, key: &str, v: &Option<String>) {
    if let Some(s) = v {
        m.insert(Yaml::String(key.into()), Yaml::String(s.clone()));
    }
}

fn model_to_yaml(mo: &ModelEntryInput) -> Result<Yaml, String> {
    let mut m = Mapping::new();
    spread_extra(&mut m, &mo.extra)?;
    remove_known(&mut m, KNOWN_MODEL_KEYS);
    m.insert(Yaml::String("id".into()), Yaml::String(mo.id.clone()));
    insert_str(&mut m, "name", &mo.name);
    if let Some(v) = mo.context_window {
        m.insert(
            Yaml::String("contextWindow".into()),
            Yaml::Number(v.into()),
        );
    }
    if let Some(v) = mo.max_tokens {
        m.insert(Yaml::String("maxTokens".into()), Yaml::Number(v.into()));
    }
    if let Some(list) = &mo.input {
        if !list.is_empty() {
            m.insert(
                Yaml::String("input".into()),
                Yaml::Sequence(
                    list.iter()
                        .map(|s| Yaml::String(s.clone()))
                        .collect(),
                ),
            );
        }
    }
    if let Some(re) = &mo.reasoning_efforts {
        let y = json_to_yaml(re)?;
        let empty = y.as_mapping().is_some_and(|mm| mm.is_empty());
        if !empty {
            m.insert(Yaml::String("reasoningEfforts".into()), y);
        }
    }
    Ok(Yaml::Mapping(m))
}

/// agent-default-model 的映射：extra 透传字段 + 三个已知键（effort 为空则不写）
fn default_to_yaml(d: &DefaultModelInput) -> Result<Yaml, String> {
    let mut m = Mapping::new();
    spread_extra(&mut m, &d.extra)?;
    remove_known(&mut m, KNOWN_DEFAULT_KEYS);
    m.insert(Yaml::String("provider".into()), Yaml::String(d.provider.clone()));
    m.insert(Yaml::String("model".into()), Yaml::String(d.model.clone()));
    insert_str(&mut m, "reasoningEffort", &d.reasoning_effort);
    Ok(Yaml::Mapping(m))
}

fn provider_to_yaml(p: &ProviderInput) -> Result<Yaml, String> {    let mut m = Mapping::new();
    spread_extra(&mut m, &p.extra)?;
    remove_known(&mut m, KNOWN_PROVIDER_KEYS);
    insert_str(&mut m, "displayName", &p.display_name);
    insert_str(&mut m, "apiKeyEnv", &p.api_key_env);
    insert_str(&mut m, "api", &p.api);
    insert_str(&mut m, "baseURL", &p.base_url);
    if let Some(h) = &p.headers {
        let y = json_to_yaml(h)?;
        if y.as_mapping().is_some_and(|mm| !mm.is_empty()) {
            m.insert(Yaml::String("headers".into()), y);
        }
    }
    if let Some(c) = &p.compat {
        let y = json_to_yaml(c)?;
        if y.as_mapping().is_some_and(|mm| !mm.is_empty()) {
            m.insert(Yaml::String("compat".into()), y);
        }
    }
    let models: Vec<Yaml> = p
        .models
        .iter()
        .map(model_to_yaml)
        .collect::<Result<_, _>>()?;
    m.insert(Yaml::String("models".into()), Yaml::Sequence(models));
    Ok(Yaml::Mapping(m))
}

/// 在顶层行中定位 `key:` 节的行区间 [start, end)；end 已排除尾随空行 / 注释行
///（它们视觉上多属于下一节，保留原样）。
fn find_section(lines: &[&str], key: &str) -> Option<(usize, usize)> {
    let prefix = format!("{key}:");
    let mut start = None;
    for (i, line) in lines.iter().enumerate() {
        if line.starts_with(' ') || line.starts_with('\t') {
            continue;
        }
        let t = line.trim_start();
        if t.is_empty() || t.starts_with('#') {
            continue;
        }
        let is_key = line.trim_end() == prefix
            || (line.starts_with(&prefix) && line[prefix.len()..].starts_with(' '));
        if is_key {
            start = Some(i);
            break;
        }
    }
    let s = start?;
    let mut e = s + 1;
    while e < lines.len() {
        let l = lines[e];
        let t = l.trim_start();
        let top_level =
            !l.starts_with(' ') && !l.starts_with('\t') && !t.is_empty() && !t.starts_with('#');
        if top_level {
            break;
        }
        e += 1;
    }
    while e > s + 1 {
        let t = lines[e - 1].trim();
        if t.is_empty() || t.starts_with('#') {
            e -= 1;
        } else {
            break;
        }
    }
    Some((s, e))
}

/// 节级操作：(key, Some(新节文本)) = 整节替换（缺失则追加到文件末尾）；
/// (key, None) = 整节删除（本来就不存在则无操作）。区间按原文本定位、倒序应用。
fn apply_section_ops(raw: &str, ops: &[(String, Option<String>)]) -> Result<String, String> {
    let lines: Vec<&str> = raw.lines().collect();
    let mut ranges: Vec<(usize, usize, &Option<String>)> = Vec::new();
    let mut appends: Vec<&String> = Vec::new();
    for (key, text) in ops {
        match find_section(&lines, key) {
            Some((s, e)) => ranges.push((s, e, text)),
            None => {
                if let Some(t) = text {
                    appends.push(t);
                }
            }
        }
    }
    ranges.sort_by_key(|(s, _, _)| std::cmp::Reverse(*s));
    let mut out: Vec<String> = lines.into_iter().map(String::from).collect();
    for (s, e, text) in ranges {
        match text {
            Some(t) => {
                let repl: Vec<String> = t.lines().map(String::from).collect();
                out.splice(s..e, repl);
            }
            None => {
                out.drain(s..e);
            }
        }
    }
    let mut text = out.join("\n");
    for t in appends {
        if !text.trim().is_empty() {
            if !text.ends_with('\n') {
                text.push('\n');
            }
            text.push('\n');
        }
        text.push_str(t);
    }
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    Ok(text)
}

/// 保存模型配置：只重写 `llm-pi-ai`（providers 整体替换，该节其余子键保留）与
/// `agent-default-model` 两节，文件其余内容与节外注释逐字节保留。写前备份 + 写后校验。
pub fn write(input: &ModelConfigInput) -> Result<(), String> {
    validate(input)?;

    let providers: Vec<ProviderInput> = input
        .providers
        .iter()
        .map(|p| ProviderInput {
            id: p.id.trim().to_string(),
            display_name: norm_str(&p.display_name),
            api: norm_str(&p.api),
            base_url: norm_str(&p.base_url),
            api_key_env: norm_str(&p.api_key_env),
            headers: p.headers.clone(),
            compat: p.compat.clone(),
            models: p
                .models
                .iter()
                .map(|mo| ModelEntryInput {
                    id: mo.id.trim().to_string(),
                    name: norm_str(&mo.name),
                    context_window: mo.context_window,
                    max_tokens: mo.max_tokens,
                    input: mo.input.clone().filter(|l| !l.is_empty()),
                    reasoning_efforts: mo.reasoning_efforts.clone(),
                    extra: mo.extra.clone(),
                })
                .collect(),
            extra: p.extra.clone(),
        })
        .collect();
    let default_model: Option<DefaultModelInput> = input.default_model.as_ref().map(|d| {
        DefaultModelInput {
            provider: d.provider.trim().to_string(),
            model: d.model.trim().to_string(),
            reasoning_effort: norm_str(&d.reasoning_effort),
            extra: d.extra.clone(),
        }
    });

    // 读原文件：存在但解析失败 / 顶层非映射 → 拒绝（防止覆盖未知内容）
    let path = global_config_path();
    // 只有「文件不存在」才当作空文档；权限 / IO 等其它读取失败必须拒绝写入，
    // 否则会用新内容覆盖掉磁盘上已有的 settings.yaml（静默丢配置）。
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取 settings.yaml 失败，已拒绝写入以免覆盖现有配置：{e}")),
    };
    let existing_root: Option<Mapping> = if raw.trim().is_empty() {
        None
    } else {
        let doc: Yaml = serde_yaml::from_str(&raw).map_err(|e| {
            format!("现有 settings.yaml 不是合法 YAML，拒绝写入（请先在「配置文件」页修复）: {e}")
        })?;
        Some(
            doc.as_mapping()
                .ok_or("settings.yaml 顶层不是映射，拒绝写入")?
                .clone(),
        )
    };

    // llm-pi-ai 节：providers 整体替换，其余子键原样保留
    let mut llm_section: Mapping = existing_root
        .as_ref()
        .and_then(|r| r.get(Yaml::String(SECTION_PROVIDERS.into())))
        .and_then(|v| v.as_mapping())
        .cloned()
        .unwrap_or_default();
    llm_section.remove(&Yaml::String(KEY_PROVIDERS.into()));
    let mut provs = Mapping::new();
    for p in &providers {
        provs.insert(Yaml::String(p.id.clone()), provider_to_yaml(p)?);
    }
    llm_section.insert(Yaml::String(KEY_PROVIDERS.into()), Yaml::Mapping(provs));
    let mut llm_wrap = Mapping::new();
    llm_wrap.insert(
        Yaml::String(SECTION_PROVIDERS.into()),
        Yaml::Mapping(llm_section),
    );
    let llm_text = serde_yaml::to_string(&Yaml::Mapping(llm_wrap))
        .map_err(|e| format!("序列化失败: {e}"))?;

    // agent-default-model 节
    let default_op: Option<String> = match &default_model {
        Some(d) => {
            let mut m: Mapping = existing_root
                .as_ref()
                .and_then(|r| r.get(Yaml::String(SECTION_DEFAULT.into())))
                .and_then(|v| v.as_mapping())
                .cloned()
                .unwrap_or_default();
            remove_known(&mut m, KNOWN_DEFAULT_KEYS);
            if let Yaml::Mapping(nm) = default_to_yaml(d)? {
                for (k, v) in nm {
                    m.insert(k, v);
                }
            }
            let mut wrap = Mapping::new();
            wrap.insert(Yaml::String(SECTION_DEFAULT.into()), Yaml::Mapping(m));
            Some(
                serde_yaml::to_string(&Yaml::Mapping(wrap))
                    .map_err(|e| format!("序列化失败: {e}"))?,
            )
        }
        None => {
            // 清除默认：删除三个键；节空则整节移除，否则保留剩余键
            match existing_root
                .as_ref()
                .and_then(|r| r.get(Yaml::String(SECTION_DEFAULT.into())))
                .and_then(|v| v.as_mapping())
                .cloned()
            {
                None => None,
                Some(mut m) => {
                    remove_known(&mut m, KNOWN_DEFAULT_KEYS);
                    if m.is_empty() {
                        None
                    } else {
                        let mut wrap = Mapping::new();
                        wrap.insert(Yaml::String(SECTION_DEFAULT.into()), Yaml::Mapping(m));
                        Some(
                            serde_yaml::to_string(&Yaml::Mapping(wrap))
                                .map_err(|e| format!("序列化失败: {e}"))?,
                        )
                    }
                }
            }
        }
    };

    let out = apply_section_ops(
        &raw,
        &[
            (SECTION_PROVIDERS.to_string(), Some(llm_text)),
            (SECTION_DEFAULT.to_string(), default_op),
        ],
    )?;

    serde_yaml::from_str::<Yaml>(&out).map_err(|e| format!("生成的 YAML 不合法，已中止: {e}"))?;
    backup(&path)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
    }
    std::fs::write(&path, out).map_err(|e| format!("写入失败: {e}"))?;
    Ok(())
}

// ── 获取远端可用模型（GET {baseURL}/models） ─────────────

/// 远端可用模型条目（/models 响应解析结果）
#[derive(Clone, Serialize, Debug)]
pub struct RemoteModel {
    pub id: String,
    /// anthropic 的 display_name / openai 风格的 name；与 id 相同或缺省时为 None
    pub name: Option<String>,
}

/// 密钥解析：手动传入优先，其次凭据 refs（按名字），最后环境变量
fn resolve_api_key(api_key_env: &str) -> Option<String> {
    let name = api_key_env.trim();
    if name.is_empty() {
        return None;
    }
    if let Ok(creds) = crate::credentials::read() {
        if let Some(r) = creds.refs.iter().find(|r| r.name == name) {
            if !r.value.trim().is_empty() {
                return Some(r.value.clone());
            }
        }
    }
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// 解析 /models 响应：openai 风格 { data: [{ id, name? }] } 或
/// anthropic 风格 { data: [{ id, display_name }] }，按 id 去重保序
fn parse_models_response(text: &str) -> Result<Vec<RemoteModel>, String> {
    let v: Json = serde_json::from_str(text).map_err(|e| format!("响应不是合法 JSON: {e}"))?;
    let Some(items) = v.get("data").and_then(|d| d.as_array()) else {
        return Err("响应缺少 data 数组（不是 OpenAI / Anthropic 风格的 /models 输出）".into());
    };
    let mut out: Vec<RemoteModel> = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for it in items {
        let Some(id) = it
            .get("id")
            .and_then(|x| x.as_str())
            .map(|s| s.trim().to_string())
        else {
            continue;
        };
        if id.is_empty() || !seen.insert(id.clone()) {
            continue;
        }
        let name = it
            .get("display_name")
            .or_else(|| it.get("name"))
            .and_then(|x| x.as_str())
            .map(String::from)
            .filter(|n| !n.trim().is_empty() && n.trim() != id);
        out.push(RemoteModel { id, name });
    }
    Ok(out)
}

/// 拉取服务方的可用模型列表：GET {baseURL}/models。
/// 密钥解析顺序：手动传入 > 凭据 refs（按 apiKeyEnv 名字）> 环境变量；
/// 无密钥时也发起请求（本地网关可免密钥），401 由 HTTP 状态错误呈现。
pub async fn fetch_provider_models(
    base_url: &str,
    api: &str,
    api_key_env: &str,
    api_key: Option<String>,
) -> Result<Vec<RemoteModel>, String> {
    let base = base_url.trim().trim_end_matches('/');
    if base.is_empty() {
        return Err("baseURL 不能为空，请先填写 API 地址".into());
    }
    if !(base.starts_with("http://") || base.starts_with("https://")) {
        return Err("baseURL 需以 http:// 或 https:// 开头".into());
    }
    let url = format!("{base}/models");
    let key = api_key
        .map(|k| k.trim().to_string())
        .filter(|k| !k.is_empty())
        .or_else(|| resolve_api_key(api_key_env));
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .build()
        .map_err(|e| format!("创建 HTTP 客户端失败: {e}"))?;
    let mut req = client.get(&url);
    if api.trim() == "anthropic-messages" {
        let key = key.ok_or("anthropic 协议需要 API 密钥：请先在上方配置密钥")?;
        req = req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01");
    } else if let Some(k) = key {
        req = req.header("Authorization", format!("Bearer {k}"));
    }
    let resp = req.send().await.map_err(|e| format!("请求失败: {e}"))?;
    let status = resp.status();
    let text = resp.text().await.map_err(|e| format!("读取响应失败: {e}"))?;
    if !status.is_success() {
        let brief: String = text.chars().take(200).collect();
        return Err(format!("HTTP {status}：{brief}"));
    }
    parse_models_response(&text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// DSH_HOME 是进程级环境变量，用到它的测试必须串行执行
    use crate::util::DSH_ENV_LOCK;

    fn tmp_home(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dsh-model-{tag}-{}", std::process::id()))
    }

    fn cfg_path() -> PathBuf {
        global_config_path()
    }

    const SAMPLE: &str = r#"# 顶部注释（应保留）
ui-onboarding:
  welcomeNoticeVersion: v1
llm-pi-ai:
  providers:
    alpha:
      apiKeyEnv: ALPHA_KEY
      api: openai-completions
      baseURL: https://a.example/v1
      compat:
        supportsStore: false
      models:
        - id: m1
          name: Model One
          contextWindow: 100000
          reasoningEfforts:
            low: low
            off: null
        - id: m2
          name: Model Two
          input:
            - text
            - image
    beta:
      displayName: Beta
      api: anthropic-messages
      baseURL: https://b.example/v1
      models:
        - id: b1
agent-default-model:
  provider: alpha
  model: m1
  reasoningEffort: low
  customExtra: keep-me
ui-theme:
  preference: dark
  # ui-theme 节内注释（应保留）
dshp-token-meter:
  # 手动迁移注释（应保留）
  activeVendor: "alpha"
"#;

    fn setup(tag: &str, content: Option<&str>) -> PathBuf {
        let tmp = tmp_home(tag);
        std::env::set_var("DSH_HOME", &tmp);
        match content {
            Some(c) => {
                std::fs::create_dir_all(tmp_home(tag)).unwrap();
                std::fs::write(cfg_path(), c).unwrap();
            }
            None => std::fs::create_dir_all(tmp_home(tag)).unwrap(),
        }
        tmp
    }

    fn prov<'a>(input: &'a ModelConfigInput, id: &str) -> &'a ProviderInput {
        input.providers.iter().find(|p| p.id == id).unwrap()
    }

    fn simple_input() -> ModelConfigInput {
        ModelConfigInput {
            providers: vec![ProviderInput {
                id: "alpha".into(),
                display_name: Some("Alpha".into()),
                api: Some("openai-completions".into()),
                base_url: Some("https://a.example/v1".into()),
                api_key_env: Some("ALPHA_KEY".into()),
                headers: None,
                compat: None,
                models: vec![ModelEntryInput {
                    id: "m1".into(),
                    name: Some("Model One".into()),
                    context_window: Some(100000),
                    max_tokens: None,
                    input: Some(vec!["text".into(), "image".into()]),
                    reasoning_efforts: Some(serde_json::json!({"low": "low", "off": null})),
                    extra: None,
                }],
                extra: None,
            }],
            default_model: Some(DefaultModelInput {
                provider: "alpha".into(),
                model: "m1".into(),
                reasoning_effort: Some("low".into()),
                extra: None,
            }),
        }
    }

    #[test]
    fn read_parses_providers_default_and_extras() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("read", Some(SAMPLE));
        let cfg = read().unwrap();
        assert!(cfg.exists);
        assert_eq!(cfg.providers.len(), 2);
        let alpha = cfg.providers.iter().find(|p| p.id == "alpha").unwrap();
        assert_eq!(alpha.api_key_env.as_deref(), Some("ALPHA_KEY"));
        assert_eq!(alpha.compat.as_ref().unwrap()["supportsStore"], false);
        assert_eq!(alpha.models.len(), 2);
        let m1 = &alpha.models[0];
        assert_eq!(m1.id, "m1");
        assert_eq!(m1.context_window, Some(100000));
        assert_eq!(
            m1.reasoning_efforts.as_ref().unwrap()["off"],
            serde_json::Value::Null
        );
        let m2 = &alpha.models[1];
        assert_eq!(m2.input, vec!["text".to_string(), "image".to_string()]);
        let beta = cfg.providers.iter().find(|p| p.id == "beta").unwrap();
        assert_eq!(beta.display_name.as_deref(), Some("Beta"));
        let dm = cfg.default_model.unwrap();
        assert_eq!(dm.provider, "alpha");
        assert_eq!(dm.reasoning_effort.as_deref(), Some("low"));
        assert_eq!(
            dm.extra.as_ref().unwrap()["customExtra"],
            "keep-me"
        );
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn read_missing_and_parse_error() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("missing", None);
        let cfg = read().unwrap();
        assert!(!cfg.exists);
        assert!(cfg.providers.is_empty() && cfg.parse_error.is_none());

        std::fs::write(cfg_path(), "providers: [unclosed\n").unwrap();
        let cfg = read().unwrap();
        assert!(cfg.exists && cfg.parse_error.is_some());
        assert!(cfg.providers.is_empty());

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_rewrites_sections_and_preserves_rest() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("write", Some(SAMPLE));
        let mut input = simple_input();
        // 修改 alpha baseURL + 给模型加 maxTokens，另加一个 gamma provider，默认切到 gamma
        let alpha = prov_mut(&mut input, "alpha");
        alpha.base_url = Some("https://new.example/v1".into());
        alpha.models[0].max_tokens = Some(32000);
        input.providers.push(ProviderInput {
            id: "gamma".into(),
            display_name: None,
            api: Some("openai-responses".into()),
            base_url: Some("https://g.example/v1".into()),
            api_key_env: None,
            headers: Some(serde_json::json!({"x-session": "abc"})),
            compat: None,
            models: vec![ModelEntryInput {
                id: "g1".into(),
                name: None,
                context_window: None,
                max_tokens: None,
                input: None,
                reasoning_efforts: None,
                extra: None,
            }],
            extra: None,
        });
        input.default_model = Some(DefaultModelInput {
            provider: "gamma".into(),
            model: "g1".into(),
            reasoning_effort: None,
            extra: None,
        });
        write(&input).unwrap();

        let out = std::fs::read_to_string(cfg_path()).unwrap();
        // 其余内容逐字节保留
        assert!(out.starts_with("# 顶部注释（应保留）\nui-onboarding:\n"));
        assert!(out.contains("# ui-theme 节内注释（应保留）"));
        assert!(out.contains("# 手动迁移注释（应保留）"));
        assert!(out.contains("activeVendor: \"alpha\""));
        // 修改生效
        assert!(out.contains("baseURL: https://new.example/v1"));
        assert!(!out.contains("https://a.example"), "旧值应被替换:\n{out}");
        assert!(out.contains("maxTokens: 32000"));
        assert!(out.contains("gamma:") && out.contains("x-session: abc"));
        // 默认模型：切到 gamma/g1；effort 未设置 → 键不写；原 customExtra 之类透传字段……
        // 默认节被整体重写时 extra 为 None（本次输入未带），不保留旧键
        assert!(out.contains("provider: gamma"));
        assert!(out.contains("model: g1"));
        assert!(!out.contains("reasoningEffort:"));
        serde_yaml::from_str::<Yaml>(&out).unwrap();

        // 回读一致
        let cfg = read().unwrap();
        assert_eq!(cfg.providers.len(), 2);
        assert_eq!(
            cfg.providers[0].models[0].max_tokens,
            Some(32000)
        );
        assert_eq!(cfg.default_model.unwrap().provider, "gamma");
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    fn prov_mut<'a>(input: &'a mut ModelConfigInput, id: &str) -> &'a mut ProviderInput {
        input.providers.iter_mut().find(|p| p.id == id).unwrap()
    }

    #[test]
    fn write_preserves_extra_fields_roundtrip() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("extra", Some(SAMPLE));
        let cfg = read().unwrap();
        // 原样回写：extra（customExtra）与全部字段都应保留
        let input = ModelConfigInput {
            providers: cfg.providers.iter().map(|p| ProviderInput {
                id: p.id.clone(),
                display_name: p.display_name.clone(),
                api: p.api.clone(),
                base_url: p.base_url.clone(),
                api_key_env: p.api_key_env.clone(),
                headers: p.headers.clone(),
                compat: p.compat.clone(),
                models: p.models.iter().map(|mo| ModelEntryInput {
                    id: mo.id.clone(),
                    name: mo.name.clone(),
                    context_window: mo.context_window,
                    max_tokens: mo.max_tokens,
                    input: if mo.input.is_empty() { None } else { Some(mo.input.clone()) },
                    reasoning_efforts: mo.reasoning_efforts.clone(),
                    extra: mo.extra.clone(),
                }).collect(),
                extra: p.extra.clone(),
            }).collect(),
            default_model: cfg.default_model.clone().map(|d| DefaultModelInput {
                provider: d.provider,
                model: d.model,
                reasoning_effort: d.reasoning_effort,
                extra: d.extra,
            }),
        };
        write(&input).unwrap();
        let again = read().unwrap();
        assert_eq!(again.providers.len(), cfg.providers.len());
        assert_eq!(
            again.default_model.as_ref().unwrap().extra.as_ref().unwrap()["customExtra"],
            "keep-me"
        );
        let out = std::fs::read_to_string(cfg_path()).unwrap();
        assert!(out.contains("customExtra: keep-me"));
        serde_yaml::from_str::<Yaml>(&out).unwrap();
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_creates_missing_file() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("create", None);
        write(&simple_input()).unwrap();
        let out = std::fs::read_to_string(cfg_path()).unwrap();
        assert!(out.contains("llm-pi-ai:") && out.contains("agent-default-model:"));
        serde_yaml::from_str::<Yaml>(&out).unwrap();
        let cfg = read().unwrap();
        assert!(cfg.exists && cfg.providers.len() == 1);
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 提取顶层 key 节的文本（find_section 的文本版，断言用）
    fn section_text(text: &str, key: &str) -> String {
        let lines: Vec<&str> = text.lines().collect();
        match find_section(&lines, key) {
            Some((s, e)) => lines[s..e].join("\n"),
            None => String::new(),
        }
    }

    #[test]
    fn write_clears_default() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        // 情形一：节内有未知键（customExtra）→ 只删三个已知键，节保留
        let tmp = setup("clear", Some(SAMPLE));
        let mut input = simple_input();
        input.default_model = None;
        write(&input).unwrap();
        let out = std::fs::read_to_string(cfg_path()).unwrap();
        let sec = section_text(&out, SECTION_DEFAULT);
        assert!(!sec.contains("\n  provider:"), "provider 应被删除:\n{sec}");
        assert!(!sec.contains("\n  model:"), "model 应被删除:\n{sec}");
        assert!(!sec.contains("reasoningEffort"), "effort 应被删除:\n{sec}");
        assert!(sec.contains("\n  customExtra: keep-me"), "未知键应保留:\n{sec}");
        serde_yaml::from_str::<Yaml>(&out).unwrap();
        assert!(read().unwrap().default_model.is_none());
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");

        // 情形二：节内只有已知键 → 整节移除
        let only_known = "llm-pi-ai:\n  providers:\n    alpha:\n      models:\n      - id: m1\nagent-default-model:\n  provider: alpha\n  model: m1\n  reasoningEffort: low\nui-theme:\n  preference: dark\n";
        let tmp = setup("clear2", Some(only_known));
        let cfg = read().unwrap();
        let input = ModelConfigInput {
            providers: cfg
                .providers
                .iter()
                .map(|p| ProviderInput {
                    id: p.id.clone(),
                    display_name: p.display_name.clone(),
                    api: p.api.clone(),
                    base_url: p.base_url.clone(),
                    api_key_env: p.api_key_env.clone(),
                    headers: p.headers.clone(),
                    compat: p.compat.clone(),
                    models: p
                        .models
                        .iter()
                        .map(|mo| ModelEntryInput {
                            id: mo.id.clone(),
                            name: mo.name.clone(),
                            context_window: mo.context_window,
                            max_tokens: mo.max_tokens,
                            input: if mo.input.is_empty() { None } else { Some(mo.input.clone()) },
                            reasoning_efforts: mo.reasoning_efforts.clone(),
                            extra: mo.extra.clone(),
                        })
                        .collect(),
                    extra: p.extra.clone(),
                })
                .collect(),
            default_model: None,
        };
        write(&input).unwrap();
        let out = std::fs::read_to_string(cfg_path()).unwrap();
        assert!(
            !out.contains("agent-default-model"),
            "只剩已知键时默认节应整节移除:\n{out}"
        );
        assert!(out.contains("ui-theme:"), "其余内容保留");
        serde_yaml::from_str::<Yaml>(&out).unwrap();
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_rejects_invalid_and_keeps_file() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("invalid", Some(SAMPLE));
        let before = std::fs::read_to_string(cfg_path()).unwrap();

        let mut input = simple_input();
        input.providers.push(prov(&input, "alpha").clone());
        assert!(write(&input).is_err(), "重复 Provider ID 应被拒绝");

        let mut input = simple_input();
        input.default_model = Some(DefaultModelInput {
            provider: "nope".into(),
            model: "m1".into(),
            reasoning_effort: None,
            extra: None,
        });
        assert!(write(&input).is_err(), "悬空默认 Provider 应被拒绝");

        let mut input = simple_input();
        prov_mut(&mut input, "alpha").base_url = Some("ftp://x".into());
        assert!(write(&input).is_err(), "非法 baseURL 应被拒绝");

        let mut input = simple_input();
        input.default_model = Some(DefaultModelInput {
            provider: "alpha".into(),
            model: "ghost".into(),
            reasoning_effort: None,
            extra: None,
        });
        assert!(write(&input).is_err(), "悬空默认模型应被拒绝");

        assert_eq!(std::fs::read_to_string(cfg_path()).unwrap(), before);
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_refuses_broken_yaml() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = setup("broken", Some("- a\n- b\n"));
        assert!(write(&simple_input()).is_err());
        assert_eq!(std::fs::read_to_string(cfg_path()).unwrap(), "- a\n- b\n");
        std::fs::write(cfg_path(), "llm-pi-ai: [unclosed\n").unwrap();
        assert!(write(&simple_input()).is_err());
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn section_ops_keep_edge_comments() {
        // 节前注释 / 节后注释（属于下一节）都必须原样保留
        let raw = "# describes llm\nllm-pi-ai:\n  providers: {}\n# describes ui-theme\nui-theme:\n  a: 1\n";
        let llm_text = "llm-pi-ai:\n  providers:\n    x:\n      models: []\n";
        let out = apply_section_ops(
            raw,
            &[(SECTION_PROVIDERS.to_string(), Some(llm_text.to_string()))],
        )
        .unwrap();
        assert!(out.starts_with("# describes llm\n"));
        assert!(out.contains("# describes ui-theme"));
        assert!(out.contains("    x:"));
        serde_yaml::from_str::<Yaml>(&out).unwrap();
        let out_lines: Vec<&str> = out.lines().collect();
        let (s, e) = find_section(&out_lines, SECTION_PROVIDERS).unwrap();
        assert_eq!(out_lines[s], "llm-pi-ai:");
        assert_eq!(e, 5, "节区间应为 [1, 5): s={s} e={e}");
    }

    /// JSON 契约回归：base_url 必须序列化/反序列化为 baseURL（camelCase 会得到
    /// baseUrl，曾导致前端读不到 API 地址、保存时清空全部 baseURL）
    #[test]
    fn json_contract_uses_baseurl_key() {
        let e = ProviderEntry {
            id: "x".into(),
            display_name: None,
            api: None,
            base_url: Some("https://a.example/v1".into()),
            api_key_env: Some("X_KEY".into()),
            headers: None,
            compat: None,
            models: vec![],
            extra: None,
        };
        let j = serde_json::to_value(&e).unwrap();
        assert!(j.get("baseURL").is_some(), "序列化键必须是 baseURL: {j}");
        assert!(j.get("baseUrl").is_none());

        let i: ProviderInput = serde_json::from_str(
            r#"{"id":"x","baseURL":"https://a.example/v1","models":[]}"#,
        )
        .unwrap();
        assert_eq!(i.base_url.as_deref(), Some("https://a.example/v1"));
        // 兼容 baseUrl 也不应反序列化成功（防止悄悄回退）
        let bad: ProviderInput =
            serde_json::from_str(r#"{"id":"x","baseUrl":"https://a","models":[]}"#).unwrap();
        assert!(bad.base_url.is_none());
    }

    #[test]
    fn parse_models_response_variants() {        // openai 风格：name 字段 + id 去重 + 空 id 丢弃
        let openai = r#"{"object":"list","data":[
            {"id":"m1","name":"Model 1"},{"id":"m1"},{"id":""},{"id":"m2"},{"id":"m1-copy","name":"m1-copy"}]}"#;
        let out = parse_models_response(openai).unwrap();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].name.as_deref(), Some("Model 1"));
        assert_eq!(out[1].name, None);
        assert_eq!(out[2].name, None, "name == id 时置 None");

        // anthropic 风格：display_name
        let anthropic = r#"{"data":[{"id":"claude-x","display_name":"Claude X"}]}"#;
        let out = parse_models_response(anthropic).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "claude-x");
        assert_eq!(out[0].name.as_deref(), Some("Claude X"));

        assert!(parse_models_response("{\"nope\":1}").is_err());
        assert!(parse_models_response("not json").is_err());
        assert!(parse_models_response("[]").is_err());
    }

    #[test]
    fn resolve_api_key_prefers_credentials_then_env() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("apikey");
        std::env::set_var("DSH_HOME", &tmp);
        std::env::remove_var("TEST_MODEL_KEY_ENV");
        assert_eq!(resolve_api_key("TEST_MODEL_KEY_ENV"), None);
        // 环境变量兜底
        std::env::set_var("TEST_MODEL_KEY_ENV", "from-env");
        assert_eq!(
            resolve_api_key("TEST_MODEL_KEY_ENV").as_deref(),
            Some("from-env")
        );
        // 凭据 refs 优先于环境变量
        crate::credentials::write_refs(&[crate::credentials::CredentialRefInput {
            name: "TEST_MODEL_KEY_ENV".into(),
            value: "from-cred".into(),
            note: Some("模型密钥".into()),
        }])
        .unwrap();
        assert_eq!(
            resolve_api_key("TEST_MODEL_KEY_ENV").as_deref(),
            Some("from-cred")
        );
        // 空名字直接 None
        assert_eq!(resolve_api_key("  "), None);

        std::env::remove_var("TEST_MODEL_KEY_ENV");
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 真实数据冒烟：把 ~/.dsh/settings.yaml 拷进临时 DSH_HOME 后「原样回写」，
    /// 断言两节之外的行逐字节不变、providers / 模型清单不丢。只读真实文件。
    #[test]
    fn real_home_smoke() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let real = PathBuf::from(home).join(".dsh/settings.yaml");
        let Ok(real_raw) = std::fs::read_to_string(&real) else {
            return;
        };
        let tmp = tmp_home("real");
        std::env::set_var("DSH_HOME", &tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(cfg_path(), &real_raw).unwrap();

        let before = read().unwrap();
        assert!(!before.providers.is_empty(), "真实文件应解析出 providers");
        let input = ModelConfigInput {
            providers: before
                .providers
                .iter()
                .map(|p| ProviderInput {
                    id: p.id.clone(),
                    display_name: p.display_name.clone(),
                    api: p.api.clone(),
                    base_url: p.base_url.clone(),
                    api_key_env: p.api_key_env.clone(),
                    headers: p.headers.clone(),
                    compat: p.compat.clone(),
                    models: p
                        .models
                        .iter()
                        .map(|mo| ModelEntryInput {
                            id: mo.id.clone(),
                            name: mo.name.clone(),
                            context_window: mo.context_window,
                            max_tokens: mo.max_tokens,
                            input: if mo.input.is_empty() { None } else { Some(mo.input.clone()) },
                            reasoning_efforts: mo.reasoning_efforts.clone(),
                            extra: mo.extra.clone(),
                        })
                        .collect(),
                    extra: p.extra.clone(),
                })
                .collect(),
            default_model: before.default_model.clone().map(|d| DefaultModelInput {
                provider: d.provider,
                model: d.model,
                reasoning_effort: d.reasoning_effort,
                extra: d.extra,
            }),
        };
        write(&input).unwrap();
        let out = std::fs::read_to_string(cfg_path()).unwrap();
        serde_yaml::from_str::<Yaml>(&out).unwrap();

        // 两节之外的行逐字节不变
        let orig_lines: Vec<&str> = real_raw.lines().collect();
        let out_lines: Vec<&str> = out.lines().collect();
        let (os, _oe) = find_section(&orig_lines, SECTION_PROVIDERS).unwrap();
        assert_eq!(
            orig_lines[..os],
            out_lines[..os],
            "llm-pi-ai 节之前的内容应不变"
        );
        // 顶层键集合不变
        let top_keys = |lines: &[&str]| -> Vec<String> {
            lines
                .iter()
                .filter(|l| !l.starts_with(' ') && !l.starts_with('\t'))
                .filter(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with('#')
                })
                .map(|l| l.split(':').next().unwrap_or(l).to_string())
                .collect()
        };
        assert_eq!(top_keys(&orig_lines), top_keys(&out_lines), "顶层键不变");
        // providers 与模型清单不丢
        let after = read().unwrap();
        assert_eq!(after.providers.len(), before.providers.len());
        for (b, a) in before.providers.iter().zip(after.providers.iter()) {
            assert_eq!(b.id, a.id);
            let bm: Vec<&str> = b.models.iter().map(|m| m.id.as_str()).collect();
            let am: Vec<&str> = a.models.iter().map(|m| m.id.as_str()).collect();
            assert_eq!(bm, am, "provider {} 的模型清单应不变", b.id);
        }
        assert_eq!(
            after.default_model.is_some(),
            before.default_model.is_some()
        );

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }
}
