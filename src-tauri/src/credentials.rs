use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::profiles;

/// dsh 的凭据文件：$DSH_HOME/.credentials.yaml（缺省 ~/.dsh/.credentials.yaml，0600）
pub fn credentials_path() -> PathBuf {
    profiles::dsh_native_home().join(".credentials.yaml")
}

/// refs 里的一条命名凭据（如 DEEPSEEK_API_KEY）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRef {
    pub name: String,
    pub value: String,
    /// 键正上方那一行注释（`# …` 去掉 `#` 后的内容）；没有注释就是 None
    pub note: Option<String>,
}

/// 写入载荷（Tauri command 入参）
#[derive(Deserialize, Debug)]
pub struct CredentialRefInput {
    pub name: String,
    pub value: String,
    /// 注释：Some 时写成键上方的一行 `# 注释`，None 时若原有注释行则删掉它
    #[serde(default)]
    pub note: Option<String>,
}

/// records 里的一条内部凭据记录（dsh 自管理，启动器只读展示）
#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CredentialRecord {
    pub key: String,
    pub kind: Option<String>,
    /// payload.secret 的字符长度（值本身不回传前端）
    pub secret_length: Option<usize>,
    pub payload_keys: Vec<String>,
}

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CredentialFile {
    pub path: String,
    pub exists: bool,
    pub version: Option<i64>,
    pub refs: Vec<CredentialRef>,
    pub records: Vec<CredentialRecord>,
    /// 读取时刻的文件指纹（`size:mtime 秒`，与 sessions.rs 同形态）；文件不存在为 None。
    /// 保存时原样带回，用于拒绝「读取之后文件被 dsh 等外部程序改过」的静默覆盖。
    pub fingerprint: Option<String>,
}

/// 凭据文件指纹：`size:mtime 秒`；读不到元数据/时间戳就是 None（不阻塞展示）。
fn fingerprint_of(path: &std::path::Path) -> Option<String> {
    let md = std::fs::metadata(path).ok()?;
    let mtime = md
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    Some(format!("{}:{}", md.len(), mtime.as_secs()))
}

/// 读取凭据文件。文件不存在时返回 exists=false 而非报错（dsh 首次运行后才创建）。
pub fn read() -> Result<CredentialFile, String> {
    let path = credentials_path();
    let exists = path.is_file();
    let mut out = CredentialFile {
        path: path.to_string_lossy().into_owned(),
        exists,
        version: None,
        refs: Vec::new(),
        records: Vec::new(),
        fingerprint: if exists { fingerprint_of(&path) } else { None },
    };
    if !out.exists {
        return Ok(out);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(out);
    }
    let doc: serde_yaml::Value = serde_yaml::from_str(&raw).map_err(|e| {
        // 只记路径与错误文本：凭据文件里全是活跃 token，内容一个字节都不能进日志
        crate::diag::warn("profile", &format!("凭据文件解析失败：{}：{e}", path.display()));
        format!("凭据文件解析失败: {e}")
    })?;
    let notes = read_ref_notes(&raw);
    out.version = doc.get("version").and_then(|v| v.as_i64());
    if let Some(m) = doc.get("refs").and_then(|v| v.as_mapping()) {
        for (k, v) in m {
            let Some(name) = k.as_str() else { continue };
            let value = match v {
                serde_yaml::Value::String(s) => s.clone(),
                // 非字符串标量（历史脏数据）按字面量带回，保存时会被规整为字符串
                other => serde_yaml::to_string(other)
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            };
            out.refs.push(CredentialRef {
                name: name.to_string(),
                value,
                note: notes.get(name).cloned(),
            });
        }
    }
    if let Some(m) = doc.get("records").and_then(|v| v.as_mapping()) {
        for (k, v) in m {
            let Some(key) = k.as_str() else { continue };
            let payload = v.get("payload");
            let secret_length = payload
                .and_then(|p| p.get("secret"))
                .and_then(|s| s.as_str())
                .map(|s| s.chars().count());
            let payload_keys = payload
                .and_then(|p| p.as_mapping())
                .map(|m| {
                    m.keys()
                        .filter_map(|k| k.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            out.records.push(CredentialRecord {
                key: key.to_string(),
                kind: v.get("kind").and_then(|x| x.as_str()).map(String::from),
                secret_length,
                payload_keys,
            });
        }
    }
    Ok(out)
}

/// 整表保存 refs：**逐行改写**文件中 refs 块（块外的 records / version / 未知键，
/// 以及它们自己的注释，全部逐字节保留）。refs 保留原文件的键序（存留条目按原位、
/// 新增条目按传入顺序追加）；每个条目的注释写成它上方的一行 `# 注释`。
/// 生成结果先整体校验（合法 YAML 且顶层为映射）再**原子写盘**（unix：0600 临时
/// 文件 + rename，避免截断窗口与半截文件），校验失败时原文件一个字节都不动。
///
/// `expected_fingerprint` 是前端**读取时**拿到的文件指纹（`CredentialFile::fingerprint`）：
/// 磁盘指纹与之不符说明读后文件被外部（如运行中的 dsh）改过，直接拒绝写入，
/// 避免后保存者静默覆盖先写入的内容。None = 不做冲突检测（如读时文件还不存在）。
///
/// 为什么不再用 serde_yaml 整篇序列化：那会丢光文件里所有注释 —— 而注释正是这里要
/// 维护的东西（键上方那行注释就是这条凭据的说明）。
pub fn write_refs(items: &[CredentialRefInput], expected_fingerprint: Option<&str>) -> Result<(), String> {
    // 日志只允许出现键名/条目数/字节数，绝不出现值
    let names: Vec<&str> = items.iter().map(|i| i.name.trim()).collect();
    let mut seen = std::collections::BTreeSet::new();
    for it in items {
        let name = it.name.trim();
        if name.is_empty() {
            return Err("凭据名称不能为空".into());
        }
        if name.chars().count() > 128 {
            return Err(format!("凭据名称过长: {name}"));
        }
        if name.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(format!("凭据名称不能包含空白或控制字符: {name}"));
        }
        if !seen.insert(name) {
            return Err(format!("凭据名称重复: {name}"));
        }
        if it.value.len() > 64 * 1024 {
            return Err(format!("凭据 {name} 的值过长（> 64KB）"));
        }
        if let Some(note) = &it.note {
            if note.chars().count() > 500 {
                return Err(format!("凭据 {name} 的注释过长（> 500 字）"));
            }
        }
    }

    let path = credentials_path();
    if let Some(want) = expected_fingerprint {
        match fingerprint_of(&path) {
            Some(cur) if &cur != want => {
                crate::diag::warn(
                    "profile",
                    &format!(
                        "凭据保存被拦下（外部已改动）：{} 期望指纹 {want}，磁盘为 {cur}",
                        path.display()
                    ),
                );
                return Err("凭据文件在本页读取之后被其它程序（如运行中的 dsh 实例）修改，本次保存已取消。点「还原」载入最新内容后，再重新编辑保存。".into());
            }
            Some(_) => {}
            // 读时文件还在、写前不见了（被删或被换成不可读）：同样不能贸然覆盖式写入
            None if path.exists() => {
                return Err("凭据文件状态异常（无法读取文件信息），本次保存已取消。请检查文件权限后重试。".into());
            }
            None => {
                crate::diag::warn(
                    "profile",
                    &format!("凭据保存被拦下（文件已被删除）：{}", path.display()),
                );
                return Err("凭据文件在本页读取之后被其它程序删除，本次保存已取消。点「还原」确认最新状态后再编辑。".into());
            }
        }
    }
    // 只有「文件不存在」才当作空文档从头写；权限 / IO 等其它读取失败必须拒绝写入，
    // 否则会用新内容覆盖掉磁盘上已经存在的凭据（静默丢数据）。
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取凭据文件失败，已拒绝写入以免覆盖现有凭据：{e}")),
    };
    if !raw.trim().is_empty() {
        // 先整体校验一遍：顶层必须是映射、语法必须合法，否则拒绝写入
        let doc: serde_yaml::Value =
            serde_yaml::from_str(&raw).map_err(|e| format!("凭据文件解析失败，拒绝写入: {e}"))?;
        if doc.as_mapping().is_none() {
            return Err("凭据文件顶层不是映射，拒绝写入".into());
        }
    }

    let out = rewrite_refs_block(&raw, items)?;
    // 写前校验的是**生成结果**，不只是磁盘上的原文：逐行改写有自己的拼装逻辑，
    // 任何缩进/引号/特殊行分隔符层面的缺陷都会先在这里被拦下（报「内部错误」，
    // 原文件未备份未覆盖，保持原样），而不是拿坏 YAML 覆盖掉唯一的凭据文件。
    let doc: serde_yaml::Value = serde_yaml::from_str(&out)
        .map_err(|e| format!("内部错误：生成的凭据内容不是合法 YAML，已拒绝写入: {e}"))?;
    if doc.as_mapping().is_none() {
        return Err("内部错误：生成的凭据内容顶层不是映射，已拒绝写入".into());
    }
    crate::profile_cfg::backup(&path)?;
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| "凭据文件路径异常：无父目录".to_string())?,
    )
    .map_err(|e| format!("创建目录失败: {e}"))?;
    // 原子写：unix 下先落 0600 临时文件再 rename 就位 —— 既没有「截断目标后写一半」
    // 的半截凭据窗口（断电/被杀），也没有「先 0644 再 chmod」的明文可读窗口。
    #[cfg(unix)]
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt;
        let fname = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "credentials".into());
        let tmp = path.with_file_name(format!("{fname}.tmp-{}", std::process::id()));
        let wrote = (|| -> std::io::Result<()> {
            let mut f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&tmp)?;
            f.write_all(out.as_bytes())?;
            f.flush()
        })();
        if let Err(e) = wrote {
            let _ = std::fs::remove_file(&tmp);
            return Err(format!("写入失败: {e}"));
        }
        std::fs::rename(&tmp, &path).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("写入失败: {e}")
        })?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, &out).map_err(|e| format!("写入失败: {e}"))?;
    crate::diag::info(
        "profile",
        &format!(
            "凭据已保存：{} 共 {} 条（{} 字节）refs=[{}]",
            path.display(),
            items.len(),
            out.len(),
            names.join(", ")
        ),
    );
    Ok(())
}

// ── refs 块的注释读写 ───────────────────────────────────────────────
//
// 约定（与用户看到的界面一致）：**键正上方那一行 `# 注释` 就是这条凭据的注释**。
// 连续多行注释只取最后一行（更上面的常常是「这一组是什么」的小节注释），
// 那些行原样留在文件里、不参与编辑。

/// 顶层键名（`refs:` / `"refs":` → refs）；缩进行、注释行、非映射行返回 None
fn top_level_key(line: &str) -> Option<String> {
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let t = line.trim_end();
    if t.is_empty() || t.starts_with('#') {
        return None;
    }
    let (k, _) = t.split_once(':')?;
    let k = k.trim().trim_matches(|c| c == '"' || c == '\'');
    if k.is_empty() {
        None
    } else {
        Some(k.to_string())
    }
}

/// 缩进宽度（tab 记 1，够用：只用来比较层级）
fn indent_of(line: &str) -> usize {
    line.len() - line.trim_start_matches([' ', '\t']).len()
}

/// 一行是不是注释
fn is_comment(line: &str) -> bool {
    line.trim_start().starts_with('#')
}

/// 取注释内容（去掉 `#` 与前导空格）
fn comment_text(line: &str) -> String {
    line.trim_start().trim_start_matches('#').trim().to_string()
}

/// 块内一个条目的键：优先用 YAML 解析（能处理带引号的键），失败再退回「第一个冒号前」
fn entry_key(line: &str) -> Option<String> {
    let t = line.trim_end();
    if t.trim().is_empty() || is_comment(t) {
        return None;
    }
    if let Ok(serde_yaml::Value::Mapping(m)) = serde_yaml::from_str::<serde_yaml::Value>(t) {
        if m.len() == 1 {
            if let Some(k) = m.keys().next().and_then(|k| k.as_str()) {
                return Some(k.to_string());
            }
            return None; // 非字符串键：交给调用方按「不管理」处理
        }
    }
    let (k, _) = t.split_once(':')?;
    Some(k.trim().trim_matches(|c| c == '"' || c == '\'').to_string())
}

/// 定位顶层 `refs:` 块：[块首行号（含 refs: 那一行）, 块尾行号（不含）]。
/// 块尾 = 下一个顶层键之前（顶层注释会先探一眼：后面跟的是顶层键就说明它属于块外）。
fn refs_block_bounds(lines: &[&str]) -> Option<(usize, usize)> {
    let start = lines
        .iter()
        .position(|l| top_level_key(l).as_deref() == Some("refs"))?;
    let mut end = start + 1;
    while end < lines.len() {
        let l = lines[end];
        if l.trim().is_empty() {
            // 空行：只有当它后面还是块内内容时才算块内，否则留给块外（别把分隔空行吃掉）
            let mut j = end;
            while j < lines.len() && lines[j].trim().is_empty() {
                j += 1;
            }
            match lines.get(j) {
                Some(next) if indent_of(next) > 0 || is_comment(next) => end = j,
                _ => break,
            }
            continue;
        }
        if indent_of(l) > 0 {
            end += 1;
            continue;
        }
        if is_comment(l) {
            // 顶层注释：往后看第一个非注释非空行 —— 是顶层键就属于块外
            let mut j = end + 1;
            while j < lines.len() && (lines[j].trim().is_empty() || is_comment(lines[j])) {
                j += 1;
            }
            if j < lines.len() && indent_of(lines[j]) == 0 && !is_comment(lines[j]) {
                break;
            }
            end = j;
            continue;
        }
        break;
    }
    Some((start, end))
}

/// 读出每个 ref 键正上方那一行注释（键 → 注释文本）
fn read_ref_notes(raw: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    let lines: Vec<&str> = raw.lines().collect();
    let Some((start, end)) = refs_block_bounds(&lines) else {
        return out;
    };
    // 条目缩进 = 块内第一个非空非注释行的缩进；只有这个缩进的行才算条目，
    // 更深的是上一项的多行值（如证书块），不参与
    let entry_indent = lines[start + 1..end]
        .iter()
        .find(|l| !l.trim().is_empty() && !is_comment(l))
        .map(|l| indent_of(l));
    let Some(entry_indent) = entry_indent else {
        return out;
    };
    for i in start + 1..end {
        let line = lines[i];
        if line.trim().is_empty() || is_comment(line) || indent_of(line) != entry_indent {
            continue;
        }
        let Some(key) = entry_key(line) else { continue };
        // 「上一行是注释」才是注释：空行、更深缩进的续行都不算
        let prev = i.checked_sub(1).map(|p| lines[p]).filter(|p| is_comment(p));
        if let Some(p) = prev {
            let text = comment_text(p);
            if !text.is_empty() {
                out.insert(key, text);
            }
        }
    }
    out
}

/// 生成 refs 块内容（不含 `refs:` 那一行）：注释 + 条目，块内其它内容按原样保留
fn regenerate_refs_block(lines: &[&str], items: &[CredentialRefInput]) -> Result<String, String> {
    let (start, end) = refs_block_bounds(lines).ok_or("内部错误：找不到 refs 块")?;
    let body = &lines[start + 1..end];
    let entry_indent = body
        .iter()
        .find(|l| !l.trim().is_empty() && !is_comment(l))
        .map(|l| indent_of(l))
        .unwrap_or(2);
    let indent = " ".repeat(entry_indent);

    // 把块体切成「每个条目 + 它前面那些不属于注释的原始行」
    struct Entry {
        key: Option<String>,     // None = 非字符串键 / 解析不出，原样保留
        raw: Vec<String>,        // 条目自身的行（含多行值的续行）
        pre: Vec<String>,        // 条目之前、不属于注释的原始行（空行、更上层的小节注释）
        note_line: Option<usize>, // pre 里属于「本条目注释」的那一行下标
    }
    let mut entries: Vec<Entry> = Vec::new();
    let mut pending: Vec<String> = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line = body[i];
        if line.trim().is_empty() || is_comment(line) || indent_of(line) != entry_indent {
            pending.push(line.to_string());
            i += 1;
            continue;
        }
        let key = entry_key(line);
        // 只有「紧邻的上一行是注释」才算这个条目的注释
        let note_line = pending
            .last()
            .filter(|l| is_comment(l))
            .map(|_| pending.len() - 1);
        let mut raw = vec![line.to_string()];
        i += 1;
        while i < body.len() {
            let l = body[i];
            if l.trim().is_empty() || indent_of(l) > entry_indent {
                raw.push(l.to_string());
                i += 1;
                continue;
            }
            break;
        }
        entries.push(Entry { key, raw, pre: std::mem::take(&mut pending), note_line });
    }
    // 末尾剩下的行（块尾的空行/注释）原样挂在最后一个条目后面
    let trailing = std::mem::take(&mut pending);

    let note_of = |it: &CredentialRefInput| -> Option<String> {
        it.note.as_deref().map(|n| {
            // 控制字符（\r\n\t 及其它）一律压成空格再折叠空白：注释在文件里必须是一行，
            // 与其让非法字符走到「生成的内容不是合法 YAML」这条用户看不懂的内部错误，
            // 不如写入前就地净化（与前端 normalizeNote 行为一致）。
            let spaced: String = n
                .chars()
                .map(|c| if c.is_control() { ' ' } else { c })
                .collect();
            spaced.split_whitespace().collect::<Vec<_>>().join(" ")
        })
        .map(|n| n.trim_start_matches('#').trim().to_string())
        .filter(|n| !n.is_empty())
    };

    let mut out = String::new();
    let mut emitted: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for e in &entries {
        let managed = e.key.as_ref().and_then(|k| items.iter().find(|it| it.name.trim() == k));
        let Some(it) = managed else {
            // 已删除的条目：只丢掉它自己（连同它的注释行），上方的小节注释 / 空行留给后面
            if e.key.is_some() {
                for (idx, l) in e.pre.iter().enumerate() {
                    if Some(idx) != e.note_line {
                        out.push_str(l);
                        out.push('\n');
                    }
                }
                continue;
            }
            // 非字符串键等无法管理的行：原样保留（连注释一起）
            for l in e.pre.iter().chain(e.raw.iter()) {
                out.push_str(l);
                out.push('\n');
            }
            continue;
        };
        let _ = managed;
        emitted.insert(it.name.trim().to_string());
        for (idx, l) in e.pre.iter().enumerate() {
            if Some(idx) != e.note_line {
                out.push_str(l);
                out.push('\n');
            }
        }
        if let Some(n) = note_of(it) {
            out.push_str(&indent);
            out.push_str("# ");
            out.push_str(&n);
            out.push('\n');
        }
        out.push_str(&indent);
        out.push_str(&yaml_key(it.name.trim()));
        out.push_str(": ");
        push_scalar(&mut out, &indent, &it.value);
        out.push('\n');
    }
    // 新增条目：按传入顺序追加
    for it in items {
        let name = it.name.trim().to_string();
        if emitted.contains(&name) {
            continue;
        }
        emitted.insert(name.clone());
        if let Some(n) = note_of(it) {
            out.push_str(&indent);
            out.push_str("# ");
            out.push_str(&n);
            out.push('\n');
        }
        out.push_str(&indent);
        out.push_str(&yaml_key(&name));
        out.push_str(": ");
        push_scalar(&mut out, &indent, &it.value);
        out.push('\n');
    }
    for l in &trailing {
        out.push_str(l);
        out.push('\n');
    }
    Ok(out)
}

/// 键的 YAML 写法：安全字符裸写，其余加单引号
fn yaml_key(name: &str) -> String {
    if !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./@+".contains(c))
    {
        name.to_string()
    } else {
        format!("'{}'", name.replace('\'', "''"))
    }
}

/// 值的 YAML 写法（交给 serde_yaml 处理引号 / 转义；多行值会得到块标量）
fn yaml_scalar(value: &str) -> String {
    let s = serde_yaml::to_string(&serde_yaml::Value::String(value.to_string())).unwrap_or_default();
    s.trim_end_matches('\n').to_string()
}

/// 把标量写进 out：多行值（serde 给的是 `|-` 块标量，续行自带 2 空格）
/// 的续行还要再套一层条目缩进，否则缩进深度不超过键、YAML 直接解析失败
fn push_scalar(out: &mut String, indent: &str, value: &str) {
    let scalar = yaml_scalar(value);
    let mut lines = scalar.split('\n');
    out.push_str(lines.next().unwrap_or(""));
    for l in lines {
        out.push('\n');
        out.push_str(indent);
        out.push_str(l);
    }
}

/// 把新的 refs 块拼回原文：块外内容逐字节保留；文件里还没有 refs 块就补一个。
fn rewrite_refs_block(raw: &str, items: &[CredentialRefInput]) -> Result<String, String> {
    let had_trailing_newline = raw.is_empty() || raw.ends_with('\n');
    let lines: Vec<&str> = raw.lines().collect();

    let Some((start, end)) = refs_block_bounds(&lines) else {
        // 没有 refs 块：追加一个（保留原有内容，并在需要时补 version）
        let mut out = raw.to_string();
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        if !out.is_empty() && !out.trim_end().is_empty() {
            out.push('\n');
        }
        out.push_str("refs:\n");
        out.push_str(&regenerate_refs_block(&["refs:"], items)?);
        if top_level_key_of_doc(&lines, "version").is_none() {
            out.push_str("version: 1\n");
        }
        return Ok(out);
    };

    let block = regenerate_refs_block(&lines, items)?;
    let mut out = String::new();
    for l in &lines[..start] {
        out.push_str(l);
        out.push('\n');
    }
    out.push_str("refs:\n");
    out.push_str(&block);
    for l in &lines[end..] {
        out.push_str(l);
        out.push('\n');
    }
    if !had_trailing_newline {
        out.pop();
    }
    Ok(out)
}

/// 文档里有没有某个顶层键（用于决定要不要补 version）
fn top_level_key_of_doc(lines: &[&str], key: &str) -> Option<usize> {
    lines
        .iter()
        .position(|l| top_level_key(l).as_deref() == Some(key))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DSH_HOME 是进程级环境变量，用到它的测试必须串行执行
    use crate::util::DSH_ENV_LOCK;

    fn tmp_home(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("dsh-cred-{tag}-{}", std::process::id()))
    }

    fn refs_of(items: &[(&str, &str)]) -> Vec<CredentialRefInput> {
        items
            .iter()
            .map(|(n, v)| CredentialRefInput {
                name: n.to_string(),
                value: v.to_string(),
                note: None,
            })
            .collect()
    }

    /// 既有测试不关心指纹冲突：同名包装固定传 None（遮蔽 `use super::*` 的同名函数）
    fn write_refs(items: &[CredentialRefInput]) -> Result<(), String> {
        super::write_refs(items, None)
    }

    #[test]
    fn write_creates_file_when_missing() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("create");
        std::env::set_var("DSH_HOME", &tmp);

        // 文件不存在：exists=false 而非报错
        let info = read().unwrap();
        assert!(!info.exists);
        assert!(info.refs.is_empty());

        write_refs(&refs_of(&[("DEEPSEEK_API_KEY", "sk-xxx"), ("QQ_APP_SECRET", "s3cret")])).unwrap();
        let info = read().unwrap();
        assert!(info.exists);
        assert_eq!(info.version, Some(1));
        assert_eq!(info.refs.len(), 2);
        assert_eq!(info.refs[0].name, "DEEPSEEK_API_KEY");
        assert_eq!(info.refs[1].value, "s3cret");
        let raw = std::fs::read_to_string(credentials_path()).unwrap();
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(credentials_path()).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "新建凭据文件应为 0600");
        }

        // 首次保存时源文件不存在，不产生备份
        assert!(!credentials_path().with_file_name(".credentials.starter-bak").exists());
        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_preserves_records_and_unknown_keys() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("preserve");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(
            &dir,
            "version: 2\n\
             refs:\n\
             \x20 AAA: old-a\n\
             \x20 BBB: old-b\n\
             \x20 CCC: old-c\n\
             records:\n\
             \x20 client-connection/browser-session:\n\
             \x20   kind: token\n\
             \x20   payload:\n\
             \x20     version: 1\n\
             \x20     secret: super-secret-value\n\
             custom_future_key:\n\
             \x20 nested: true\n",
        )
        .unwrap();

        // 更新 AAA、删除 BBB、新增 DDD
        write_refs(&refs_of(&[
            ("AAA", "new-a"),
            ("CCC", "old-c"),
            ("DDD", "new-d"),
        ]))
        .unwrap();

        let info = read().unwrap();
        let names: Vec<&str> = info.refs.iter().map(|r| r.name.as_str()).collect();
        assert_eq!(names, ["AAA", "CCC", "DDD"], "存留条目保持原序、新增在尾");
        assert_eq!(info.refs[0].value, "new-a");
        assert_eq!(info.version, Some(2), "已有 version 原样保留");

        // records 与未知顶层键原样保留（键序维持 AAA 在 refs 首位）
        let raw = std::fs::read_to_string(&dir).unwrap();
        assert!(raw.contains("client-connection/browser-session"));
        assert!(raw.contains("super-secret-value"));
        assert!(raw.contains("custom_future_key"));
        let doc: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
        assert_eq!(
            doc.get("records")
                .and_then(|r| r.get("client-connection/browser-session"))
                .and_then(|r| r.get("payload"))
                .and_then(|p| p.get("secret"))
                .and_then(|s| s.as_str()),
            Some("super-secret-value")
        );

        // records 摘要：kind / secret 长度 / payload 键
        let rec = &info.records[0];
        assert_eq!(rec.key, "client-connection/browser-session");
        assert_eq!(rec.kind.as_deref(), Some("token"));
        assert_eq!(rec.secret_length, Some("super-secret-value".len()));
        assert!(rec.payload_keys.contains(&"secret".to_string()));

        // 备份产生且内容是保存前的版本
        let bak = std::fs::read_to_string(dir.with_file_name(".credentials.starter-bak")).unwrap();
        assert!(bak.contains("old-a") && bak.contains("BBB"));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_rejects_invalid_input_untouched() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("invalid");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, "refs:\n  AAA: keep\n").unwrap();

        assert!(write_refs(&refs_of(&[("", "x")])).is_err());
        assert!(write_refs(&refs_of(&[("A B", "x")])).is_err());
        assert!(write_refs(&refs_of(&[("A", "x"), ("A", "y")])).is_err());
        // 首尾空白会被 trim（与前端一致），内部的空白/控制字符则拒绝
        assert!(write_refs(&refs_of(&[("A\tB", "x")])).is_err());
        // 全部非法时不动原文件
        assert_eq!(
            std::fs::read_to_string(&dir).unwrap(),
            "refs:\n  AAA: keep\n"
        );
        assert!(read().unwrap().refs.iter().any(|r| r.name == "AAA"));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    #[test]
    fn write_rejects_broken_doc() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("broken");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        // 顶层不是映射 / 语法坏掉：拒绝写入
        std::fs::write(&dir, "- a\n- b\n").unwrap();
        assert!(write_refs(&refs_of(&[("A", "x")])).is_err());
        std::fs::write(&dir, "refs: [unclosed\n").unwrap();
        assert!(write_refs(&refs_of(&[("A", "x")])).is_err());
        std::fs::write(&dir, "refs:\n  A: old\n").unwrap();

        // 空文件视同新建
        std::fs::write(&dir, "").unwrap();
        write_refs(&refs_of(&[("A", "x")])).unwrap();
        let info = read().unwrap();
        assert_eq!(info.refs.len(), 1);
        assert_eq!(info.version, Some(1));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 原子写的事后核对：成功写入后目录里不得残留 `.tmp-` 临时文件，
    /// 且多次保存（rename 覆盖自己）内容仍是最后一次的。
    #[cfg(unix)]
    #[test]
    fn write_leaves_no_temp_file() {
        use std::os::unix::fs::PermissionsExt;
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("atomic");
        std::env::set_var("DSH_HOME", &tmp);
        write_refs(&refs_of(&[("A", "x")])).unwrap();
        write_refs(&refs_of(&[("A", "y"), ("B", "z")])).unwrap();

        let cred = credentials_path();
        let parent = cred.parent().unwrap();
        let leftovers: Vec<_> = std::fs::read_dir(parent)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty(), "不应残留临时文件: {leftovers:?}");
        let meta = std::fs::metadata(credentials_path()).unwrap();
        let mode = meta.permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "rename 就位后权限仍是 0600，实际 {mode:#o}");
        let info = read().unwrap();
        assert_eq!(info.refs.len(), 2);
        assert_eq!(info.refs.iter().find(|r| r.name == "A").unwrap().value, "y");

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 外部修改冲突：指纹与读取时不符 → 拒绝写入、原文件一字不动、不产生备份；
    /// 指纹一致正常保存；expected 为 None 则不做检测。
    #[test]
    fn write_rejects_stale_fingerprint() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("conflict");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, "refs:\n  AAA: keep\n").unwrap();

        let fp = read().unwrap().fingerprint.expect("文件存在时应有指纹");
        // 模拟 dsh 外部修改（故意改变长度，避免 mtime 秒级粒度造成偶发）
        std::fs::write(&dir, "refs:\n  AAA: from-dsh\n").unwrap();
        let err = super::write_refs(&refs_of(&[("AAA", "from-starter")]), Some(&fp)).unwrap_err();
        assert!(err.contains("本次保存已取消"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&dir).unwrap(),
            "refs:\n  AAA: from-dsh\n",
            "冲突时不能覆盖外部修改"
        );
        assert!(
            !dir.with_file_name(".credentials.starter-bak").exists(),
            "冲突时不应产生备份"
        );

        // 拿最新指纹后可正常保存
        let fp2 = read().unwrap().fingerprint.unwrap();
        super::write_refs(&refs_of(&[("AAA", "ok")]), Some(&fp2)).unwrap();
        assert_eq!(read().unwrap().refs[0].value, "ok");

        // 文件被删同样拒绝
        let fp3 = read().unwrap().fingerprint.unwrap();
        std::fs::remove_file(&dir).unwrap();
        let err = super::write_refs(&refs_of(&[("AAA", "x")]), Some(&fp3)).unwrap_err();
        assert!(err.contains("删除"), "{err}");

        // None = 不检测（读时文件还不存在的整表新建也走这里）
        super::write_refs(&refs_of(&[("BBB", "b")]), None).unwrap();
        assert!(read().unwrap().refs.iter().any(|r| r.name == "BBB"));

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 注释含制表 / 回车换行 / 其它控制字符：写入前净化为一行，而不是落盘后
    /// 被 YAML 终检拦下报「内部错误」
    #[test]
    fn write_sanitizes_note_control_chars() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("note-sanitize");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();

        super::write_refs(
            &[CredentialRefInput {
                name: "A".into(),
                value: "v".into(),
                note: Some("  第一行\t带制表\r\n还有换行\u{7}和控制符  ".into()),
            }],
            None,
        )
        .unwrap();
        let raw = std::fs::read_to_string(&dir).unwrap();
        serde_yaml::from_str::<serde_yaml::Value>(&raw).unwrap();
        assert!(raw.contains("  # 第一行 带制表 还有换行 和控制符\n"), "{raw}");
        let info = read().unwrap();
        assert_eq!(
            info.refs[0].note.as_deref(),
            Some("第一行 带制表 还有换行 和控制符")
        );

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 真实数据冒烟：把 ~/.dsh/.credentials.yaml 拷进临时 DSH_HOME 后完整跑一遍
    /// 读取 + 整表回写，只读真实文件、写入全部发生在临时目录。
    #[test]
    fn real_home_smoke() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let Some(home) = std::env::var_os("HOME") else {
            return;
        };
        let real = PathBuf::from(home).join(".dsh/.credentials.yaml");
        let Ok(real_raw) = std::fs::read_to_string(&real) else {
            return;
        };
        let tmp = tmp_home("real");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(&dir, &real_raw).unwrap();

        let before = read().unwrap();
        assert!(before.exists && !before.refs.is_empty(), "真实文件应能解析出 refs");
        // 原值原样回写（等价于「什么都不改就保存」），不应丢任何条目
        let items: Vec<CredentialRefInput> = before
            .refs
            .iter()
            .map(|r| CredentialRefInput {
                name: r.name.clone(),
                value: r.value.clone(),
                note: r.note.clone(),
            })
            .collect();
        write_refs(&items).unwrap();
        let after = read().unwrap();
        assert_eq!(after.refs.len(), before.refs.len(), "回写后条目数不变");
        for (b, a) in before.refs.iter().zip(after.refs.iter()) {
            assert_eq!(b.name, a.name);
            assert_eq!(b.value, a.value);
        }
        assert_eq!(after.version, before.version, "version 不应被改写");
        // records 条目数量保持
        assert_eq!(after.records.len(), before.records.len());
        // 注释也要逐条对上（真实文件里就有 `#LLM PROVIDER API KEY` 这类注释）
        for (b, a) in before.refs.iter().zip(after.refs.iter()) {
            assert_eq!(b.note, a.note, "「{}」的注释回写后应保持不变", b.name);
        }
        // 块外的注释（records 上方的说明等）必须还在
        let raw_after = std::fs::read_to_string(&dir).unwrap();
        let norm = |s: &str| s.trim_start().trim_start_matches('#').trim().to_string();
        let after_comments: Vec<String> = raw_after.lines().filter(|l| is_comment(l)).map(norm).collect();
        for line in real_raw.lines().filter(|l| is_comment(l)) {
            let want = norm(line);
            assert!(
                after_comments.contains(&want),
                "注释内容丢失：「{want}」不在回写结果里"
            );
        }

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 注释读取：键正上方那一行注释才是注释；连续多行只取最后一行；
    /// 空行隔开的、以及本来就没有注释的，都是 None
    #[test]
    fn read_picks_comment_directly_above_key() {
        let raw = "\
refs:
  #LLM PROVIDER API KEY
  A: a-value
  # 与键之间隔了空行，不算注释

  B: b-value
  # 这一组是什么（小节注释）
  # B 的说明
  C: c-value
  D: d-value
records:
  # records 里的注释不该被当成 refs 的
  k:
    kind: token
";
        let notes = read_ref_notes(raw);
        assert_eq!(notes.get("A").map(String::as_str), Some("LLM PROVIDER API KEY"));
        assert_eq!(notes.get("B"), None, "空行隔开的不算注释");
        assert_eq!(notes.get("C").map(String::as_str), Some("B 的说明"));
        assert_eq!(notes.get("D"), None, "没有注释就是 None");
        assert!(!notes.contains_key("k"), "records 里的注释不属于 refs");
    }

    /// 注释写入：Some → 键上方一行 `# 注释`；None → 删掉原来那行，但不动更上面的小节注释
    #[test]
    fn write_emits_and_updates_note_comment() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("notes");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(
            &dir,
            "refs:\n\
             \x20 # 这一组是什么（小节注释）\n\
             \x20 # A 的旧说明\n\
             \x20 A: old-a\n\
             \x20 B: old-b\n\
             records:\n\
             \x20 # 记录段的注释\n\
             \x20 k:\n\
             \x20   kind: token\n",
        )
        .unwrap();

        // A 改注释、B 补注释、C 新增带注释
        write_refs(&[
            CredentialRefInput { name: "A".into(), value: "new-a".into(), note: Some("A 的新说明".into()) },
            CredentialRefInput { name: "B".into(), value: "old-b".into(), note: Some("B 的说明".into()) },
            CredentialRefInput { name: "C".into(), value: "c".into(), note: Some("C 的说明".into()) },
        ])
        .unwrap();
        let raw = std::fs::read_to_string(&dir).unwrap();
        assert!(raw.contains("  # 这一组是什么（小节注释）\n"), "小节注释要留着：\n{raw}");
        assert!(raw.contains("  # A 的新说明\n  A: new-a\n"), "{raw}");
        assert!(!raw.contains("A 的旧说明"), "旧注释被替换：\n{raw}");
        assert!(raw.contains("  # B 的说明\n  B: old-b\n"), "{raw}");
        assert!(raw.contains("  # C 的说明\n  C: c\n"), "新增条目带注释：\n{raw}");
        assert!(raw.contains("  # 记录段的注释\n"), "块外注释不能被吃掉：\n{raw}");

        // 读回来注释一致
        let info = read().unwrap();
        let get = |n: &str| info.refs.iter().find(|r| r.name == n).unwrap().note.clone();
        assert_eq!(get("A").as_deref(), Some("A 的新说明"));
        assert_eq!(get("B").as_deref(), Some("B 的说明"));
        assert_eq!(get("C").as_deref(), Some("C 的说明"));

        // 清空 B 的注释 → 那一行 comment 被删掉（A/C 不受影响）
        write_refs(&[
            CredentialRefInput { name: "A".into(), value: "new-a".into(), note: Some("A 的新说明".into()) },
            CredentialRefInput { name: "B".into(), value: "old-b".into(), note: None },
            CredentialRefInput { name: "C".into(), value: "c".into(), note: None },
        ])
        .unwrap();
        let raw = std::fs::read_to_string(&dir).unwrap();
        assert!(!raw.contains("B 的说明"), "清空后注释行应删除：\n{raw}");
        assert!(!raw.contains("C 的说明"), "{raw}");
        assert!(raw.contains("  B: old-b\n"), "{raw}");
        assert!(raw.contains("  # A 的新说明\n  A: new-a\n"), "{raw}");

        // 删除 A：它的注释跟着走，小节注释与 records 都留下
        write_refs(&[CredentialRefInput { name: "B".into(), value: "old-b".into(), note: None }]).unwrap();
        let raw = std::fs::read_to_string(&dir).unwrap();
        assert!(!raw.contains("A: new-a"), "{raw}");
        assert!(raw.contains("  # 这一组是什么（小节注释）\n"), "{raw}");
        assert!(raw.contains("  # 记录段的注释\n"), "{raw}");
        assert_eq!(read().unwrap().refs.len(), 1);

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 注释里的 `#`、值里的 `: ` / `#` / 多行内容都要能安全往返
    #[test]
    fn write_roundtrips_tricky_values_and_notes() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("tricky");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();

        let pem = "-----BEGIN KEY-----\nline-2\n-----END KEY-----";
        let items = vec![
            CredentialRefInput {
                name: "URL".into(),
                value: "https://example.com/path?a=b#frag".into(),
                note: Some("带 # 的注释：https://example.com".into()),
            },
            CredentialRefInput {
                name: "PEM".into(),
                value: pem.into(),
                note: Some("多行值（证书）".into()),
            },
            CredentialRefInput {
                name: "EMPTY".into(),
                value: String::new(),
                note: None,
            },
        ];
        write_refs(&items).unwrap();

        let parsed: serde_yaml::Value =
            serde_yaml::from_str(&std::fs::read_to_string(&dir).unwrap()).unwrap();
        assert_eq!(
            parsed.get("refs").and_then(|r| r.get("URL")).and_then(|v| v.as_str()),
            Some("https://example.com/path?a=b#frag"),
            "值里的 # 不能被当成注释"
        );
        assert_eq!(
            parsed.get("refs").and_then(|r| r.get("PEM")).and_then(|v| v.as_str()),
            Some(pem)
        );

        let info = read().unwrap();
        let url = info.refs.iter().find(|r| r.name == "URL").unwrap();
        assert_eq!(url.note.as_deref(), Some("带 # 的注释：https://example.com"));
        assert_eq!(url.value, "https://example.com/path?a=b#frag");
        let pem_ref = info.refs.iter().find(|r| r.name == "PEM").unwrap();
        assert_eq!(pem_ref.value, pem);
        assert_eq!(pem_ref.note.as_deref(), Some("多行值（证书）"));
        assert_eq!(info.refs.iter().find(|r| r.name == "EMPTY").unwrap().note, None);

        // 原样再存一次：内容不变（幂等）
        let again: Vec<CredentialRefInput> = info
            .refs
            .iter()
            .map(|r| CredentialRefInput { name: r.name.clone(), value: r.value.clone(), note: r.note.clone() })
            .collect();
        write_refs(&again).unwrap();
        let info2 = read().unwrap();
        for (a, b) in info.refs.iter().zip(info2.refs.iter()) {
            assert_eq!((&a.name, &a.value, &a.note), (&b.name, &b.value, &b.note));
        }

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }

    /// 文件里还没有 refs 块（或只有 records）时补一个，且不动既有注释
    #[test]
    fn write_appends_refs_block_when_missing() {
        let _env = DSH_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tmp_home("missing-block");
        std::env::set_var("DSH_HOME", &tmp);
        let dir = credentials_path();
        std::fs::create_dir_all(dir.parent().unwrap()).unwrap();
        std::fs::write(
            &dir,
            "# 顶部说明\nversion: 1\nrecords:\n  # 记录\n  k:\n    kind: token\n",
        )
        .unwrap();

        write_refs(&[CredentialRefInput {
            name: "NEW".into(),
            value: "v".into(),
            note: Some("新凭据".into()),
        }])
        .unwrap();
        let raw = std::fs::read_to_string(&dir).unwrap();
        assert!(raw.contains("# 顶部说明"), "{raw}");
        assert!(raw.contains("version: 1"), "{raw}");
        assert!(!raw.contains("version: 1\nversion"), "不该重复补 version：\n{raw}");
        assert!(raw.contains("# 记录"), "{raw}");
        let info = read().unwrap();
        assert_eq!(info.refs.len(), 1);
        assert_eq!(info.refs[0].note.as_deref(), Some("新凭据"));
        assert_eq!(info.records.len(), 1);

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }
}
