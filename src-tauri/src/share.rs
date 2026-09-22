// 分享面板署名：本机 git 身份（user.name / user.email），对齐 token-meter 插件 identity.ts
//
// 取法按可靠性排序：
//  1. 环境变量覆盖 `DSH_STARTER_GIT_NAME` / `DSH_STARTER_GIT_EMAIL`（git 不可用的容器/CI）；
//  2. 跑 `git config --get user.name|user.email`（唯一能正确处理 includeIf / 仓库本地覆盖的方式）；
//  3. 直接读配置文件（$XDG_CONFIG_HOME/git/config → ~/.gitconfig，后者后读、优先生效，与插件一致）。
//
// 结果含失败一起缓存（OnceLock）：避免每次开分享面板都 fork 一次 git。

use serde::Serialize;
use std::sync::OnceLock;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareIdentity {
    pub name: String,
    pub email: String,
    /// 身份来源，前端可在面板里说明「这个身份是哪来的」
    pub via: String,
}

pub fn share_identity() -> ShareIdentity {
    static CACHE: OnceLock<ShareIdentity> = OnceLock::new();
    CACHE.get_or_init(fetch).clone()
}

fn fetch() -> ShareIdentity {
    let env_name = std::env::var("DSH_STARTER_GIT_NAME").ok().filter(|s| !s.is_empty());
    let env_email = std::env::var("DSH_STARTER_GIT_EMAIL").ok().filter(|s| !s.is_empty());
    if env_name.is_some() || env_email.is_some() {
        return ShareIdentity { name: env_name.unwrap_or_default(), email: env_email.unwrap_or_default(), via: "env".into() };
    }

    let name = git_config("user.name");
    let email = git_config("user.email");
    if !name.is_empty() || !email.is_empty() {
        return ShareIdentity { name, email, via: "git".into() };
    }

    let name = from_config_files("name");
    let email = from_config_files("email");
    let via = if name.is_empty() && email.is_empty() { "none" } else { "config-file" };
    ShareIdentity { name, email, via: via.into() }
}

fn git_config(key: &str) -> String {
    let out = crate::util::hidden_command("git")
        .args(["config", "--get", key])
        .output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => String::new(),
    }
}

/// 全局配置在前、~/.gitconfig 在后；两者都有值时后者胜出（与插件 fromConfigFiles 一致）
fn from_config_files(key: &str) -> String {
    let xdg = std::env::var("XDG_CONFIG_HOME").ok().filter(|s| !s.is_empty());
    let mut candidates: Vec<std::path::PathBuf> = match &xdg {
        Some(dir) => vec![std::path::Path::new(dir).join("git").join("config")],
        None => crate::util::home_dir().map(|h| h.join(".config").join("git").join("config")).into_iter().collect(),
    };
    if let Some(home) = crate::util::home_dir() {
        candidates.push(home.join(".gitconfig"));
    }
    let mut found = String::new();
    for file in candidates {
        if let Ok(text) = fs_read(&file) {
            let v = parse_git_config(&text, key);
            if !v.is_empty() { found = v; }
        }
    }
    found
}

fn fs_read(p: &std::path::Path) -> Result<String, std::io::Error> {
    std::fs::read_to_string(p)
}

/// 按 INI 语法取 `[user]` 段里的某个键；支持 `name = value` / `name=value` / 注释
pub fn parse_git_config(text: &str, key: &str) -> String {
    let mut section = String::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') { continue; }
        if line.starts_with('[') {
            section = line
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim()
                .to_lowercase();
            continue;
        }
        if section != "user" { continue; }
        let Some((k, v)) = line.split_once('=') else { continue };
        if !k.trim().eq_ignore_ascii_case(key) { continue; }
        let v = v.trim();
        return if let Some(rest) = v.strip_prefix('"') {
            // 引号值取到闭合引号为止（闭合引号后允许行内注释）
            rest.split('"').next().unwrap_or(rest).to_string()
        } else {
            // 裸值：`;` / `#` 起为行内注释
            v.split([';', '#']).next().unwrap_or("").trim().to_string()
        };
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::parse_git_config;

    #[test]
    fn parses_user_section() {
        let text = "[core]\n\teditor = vim\n[user]\n\tname = 张三\n  email = \"z3@example.com\" ; 注释\n[alias]\n\tst = status\n";
        assert_eq!(parse_git_config(text, "name"), "张三");
        assert_eq!(parse_git_config(text, "email"), "z3@example.com");
        assert_eq!(parse_git_config(text, "editor"), "");
    }

    #[test]
    fn missing_key_or_section_returns_empty() {
        assert_eq!(parse_git_config("[user]\nname = a\n", "email"), "");
        assert_eq!(parse_git_config("[misc]\nname = a\n", "name"), "");
        assert_eq!(parse_git_config("", "name"), "");
    }
}
