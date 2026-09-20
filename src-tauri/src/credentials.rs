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
}

/// 写入载荷（Tauri command 入参）
#[derive(Deserialize, Debug)]
pub struct CredentialRefInput {
    pub name: String,
    pub value: String,
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
}

/// 读取凭据文件。文件不存在时返回 exists=false 而非报错（dsh 首次运行后才创建）。
pub fn read() -> Result<CredentialFile, String> {
    let path = credentials_path();
    let mut out = CredentialFile {
        path: path.to_string_lossy().into_owned(),
        exists: path.is_file(),
        version: None,
        refs: Vec::new(),
        records: Vec::new(),
    };
    if !out.exists {
        return Ok(out);
    }
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("读取失败: {e}"))?;
    if raw.trim().is_empty() {
        return Ok(out);
    }
    let doc: serde_yaml::Value =
        serde_yaml::from_str(&raw).map_err(|e| format!("凭据文件解析失败: {e}"))?;
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

/// 整表保存 refs：结构化重写整个文件，其余顶层键（records / version 等）原样保留。
/// refs 保留原文件的键序（存留条目按原位、新增条目按传入顺序追加）；写前备份，
/// 写后收紧权限（去除 group/other 位）。
pub fn write_refs(items: &[CredentialRefInput]) -> Result<(), String> {
    let mut seen = std::collections::BTreeSet::new();
    for it in items {
        let name = it.name.trim();
        if name.is_empty() {
            return Err("凭据名称不能为空".into());
        }
        if name.len() > 128 {
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
    }

    let path = credentials_path();
    // 只有「文件不存在」才当作空文档从头写；权限 / IO 等其它读取失败必须拒绝写入，
    // 否则会用新内容覆盖掉磁盘上已经存在的凭据（静默丢数据）。
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("读取凭据文件失败，已拒绝写入以免覆盖现有凭据：{e}")),
    };
    let mut doc: serde_yaml::Value = if raw.trim().is_empty() {
        serde_yaml::Value::Mapping(serde_yaml::Mapping::new())
    } else {
        serde_yaml::from_str(&raw).map_err(|e| format!("凭据文件解析失败，拒绝写入: {e}"))?
    };
    let map = doc
        .as_mapping_mut()
        .ok_or("凭据文件顶层不是映射，拒绝写入")?;

    let mut refs = serde_yaml::Mapping::new();
    if let Some(old) = map.get("refs").and_then(|v| v.as_mapping()) {
        for (k, v) in old {
            match k.as_str() {
                // 存留条目按原顺序写入新值；未出现的 = 已删除，跳过
                Some(name) => {
                    if let Some(it) = items.iter().find(|it| it.name.trim() == name) {
                        refs.insert(k.clone(), serde_yaml::Value::String(it.value.clone()));
                    }
                }
                // 非字符串键不属于 refs 管理，原样保留避免静默丢数据
                None => {
                    refs.insert(k.clone(), v.clone());
                }
            }
        }
    }
    for it in items {
        let key = serde_yaml::Value::String(it.name.trim().to_string());
        if refs.get(&key).is_none() {
            refs.insert(key, serde_yaml::Value::String(it.value.clone()));
        }
    }
    map.insert(
        serde_yaml::Value::String("refs".into()),
        serde_yaml::Value::Mapping(refs),
    );
    if map.get("version").is_none() {
        map.insert(
            serde_yaml::Value::String("version".into()),
            serde_yaml::Value::Number(serde_yaml::Number::from(1)),
        );
    }

    let out = serde_yaml::to_string(&doc).map_err(|e| format!("序列化失败: {e}"))?;
    crate::profile_cfg::backup(&path)?;
    std::fs::create_dir_all(
        path.parent()
            .ok_or_else(|| "凭据文件路径异常：无父目录".to_string())?,
    )
    .map_err(|e| format!("创建目录失败: {e}"))?;
    std::fs::write(&path, out).map_err(|e| format!("写入失败: {e}"))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = std::fs::metadata(&path) {
            let mode = meta.permissions().mode() & 0o777;
            if mode & 0o077 != 0 {
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
        }
    }
    Ok(())
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
            })
            .collect()
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
        assert!(!credentials_path().with_file_name(".credentials.launcher-bak").exists());
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
        let bak = std::fs::read_to_string(dir.with_file_name(".credentials.launcher-bak")).unwrap();
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

        std::fs::remove_dir_all(&tmp).ok();
        std::env::remove_var("DSH_HOME");
    }
}
