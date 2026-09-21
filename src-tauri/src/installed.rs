use serde::Serialize;
use std::path::{Path, PathBuf};

use crate::settings::{self, Settings};
use crate::util;

pub const PKG_DIR_IN_NODE_MODULES: &str = "node_modules/@deepseek-ai/dsh";

#[derive(Clone, Serialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct InstalledVersion {
    pub version: String,
    /// managed（本启动器安装）| global（npm 全局）| path（PATH 中的 dsh）
    pub source: String,
    /// 安装根位置（managed 为版本目录，其余为包目录或可执行文件路径）
    pub location: String,
    /// dsh 的 bin.js 绝对路径（配合 node 运行）
    pub bin_js: Option<String>,
    pub node_path: Option<String>,
}

/// 从包目录的 package.json 里解析 bin.dsh 指向的 js 文件
pub fn resolve_bin_js(pkg_dir: &Path) -> Option<PathBuf> {
    let manifest = pkg_dir.join("package.json");
    let txt = std::fs::read_to_string(manifest).ok()?;
    let j: serde_json::Value = serde_json::from_str(&txt).ok()?;
    let bin_rel = match j.get("bin") {
        Some(serde_json::Value::String(s)) => s.clone(),
        Some(serde_json::Value::Object(m)) => m
            .get("dsh")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())?,
        _ => return None,
    };
    let bin = pkg_dir.join(bin_rel);
    if bin.is_file() {
        Some(bin)
    } else {
        None
    }
}

fn read_pkg_version(pkg_dir: &Path) -> Option<String> {
    let manifest = pkg_dir.join("package.json");
    let txt = std::fs::read_to_string(manifest).ok()?;
    let j: serde_json::Value = serde_json::from_str(&txt).ok()?;
    j.get("version")?.as_str().map(|s| s.to_string())
}

/// 扫描启动器管理的 ~/.dsh-starter/versions/*
pub fn scan_managed(node: Option<&Path>) -> Vec<InstalledVersion> {
    let mut out = Vec::new();
    let vdir = settings::versions_dir();
    let Ok(rd) = std::fs::read_dir(&vdir) else {
        return out;
    };
    for entry in rd.flatten() {
        let pkg_dir = entry.path().join(PKG_DIR_IN_NODE_MODULES);
        if !pkg_dir.is_dir() {
            continue;
        }
        let Some(version) = read_pkg_version(&pkg_dir) else {
            continue;
        };
        out.push(InstalledVersion {
            version,
            source: "managed".into(),
            location: entry.path().to_string_lossy().into_owned(),
            bin_js: resolve_bin_js(&pkg_dir).map(|p| p.to_string_lossy().into_owned()),
            node_path: node.map(|p| p.to_string_lossy().into_owned()),
        });
    }
    out
}

/// 通过 `npm root -g` 找全局安装的 @deepseek-ai/dsh（阻塞，调用方放线程里）
pub fn scan_global_sync(settings: &Settings) -> Option<InstalledVersion> {
    let npm = util::find_npm(settings)?;
    let mut cmd = util::spawn_command(&npm.program, &npm.args);
    cmd.arg("root").arg("-g");
    let out = match cmd.output() {
        Ok(o) => o,
        Err(e) => {
            crate::diag::warn(
                "install",
                &format!(
                    "npm root -g 执行失败：{}（{e}）",
                    util::cmd_line(&cmd)
                ),
            );
            return None;
        }
    };
    if !out.status.success() {
        crate::diag::debug("install", || {
            format!(
                "npm root -g 退出码 {:?}：{}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            )
        });
        return None;
    }
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if root.is_empty() {
        return None;
    }
    let pkg_dir = PathBuf::from(root).join("@deepseek-ai").join("dsh");
    if !pkg_dir.is_dir() {
        return None;
    }
    let version = read_pkg_version(&pkg_dir)?;
    Some(InstalledVersion {
        version,
        source: "global".into(),
        location: pkg_dir.to_string_lossy().into_owned(),
        bin_js: resolve_bin_js(&pkg_dir).map(|p| p.to_string_lossy().into_owned()),
        node_path: util::find_node(settings).map(|p| p.to_string_lossy().into_owned()),
    })
}

/// PATH 中是否有 dsh；若是 npm 安装的符号链接则尝试解析出版本
pub fn scan_path(node: Option<&Path>) -> Vec<InstalledVersion> {
    let mut out = Vec::new();
    let Some(bin) = util::which("dsh") else {
        return out;
    };
    // 尝试顺着符号链接找到 node_modules 里的包目录
    let mut probe: PathBuf = bin.clone();
    for _ in 0..8 {
        let target = match std::fs::read_link(&probe) {
            Ok(t) => t,
            Err(_) => break,
        };
        probe = if target.is_absolute() {
            target
        } else {
            probe.parent().unwrap_or(Path::new("/")).join(target)
        };
        if let Some(idx) = probe
            .components()
            .position(|c| c.as_os_str().to_string_lossy() == "node_modules")
        {
            let prefix: PathBuf = probe.components().take(idx + 1).collect();
            let pkg_dir = prefix.join(PKG_DIR_IN_NODE_MODULES);
            if let Some(version) = read_pkg_version(&pkg_dir) {
                out.push(InstalledVersion {
                    version,
                    source: "path".into(),
                    location: pkg_dir.to_string_lossy().into_owned(),
                    bin_js: resolve_bin_js(&pkg_dir).map(|p| p.to_string_lossy().into_owned()),
                    node_path: node.map(|p| p.to_string_lossy().into_owned()),
                });
                return out;
            }
        }
    }
    // 无法解析的独立安装：只记录可执行文件位置
    out.push(InstalledVersion {
        version: "unknown".into(),
        source: "path".into(),
        location: bin.to_string_lossy().into_owned(),
        bin_js: None,
        node_path: None,
    });
    out
}

/// 汇总所有已安装来源（阻塞；managed + global + path，按位置去重）
pub fn collect_installed(settings: &Settings) -> Vec<InstalledVersion> {
    let node = util::find_node(settings);
    let mut all = scan_managed(node.as_deref());
    if let Some(g) = scan_global_sync(settings) {
        all.push(g);
    }
    for p in scan_path(node.as_deref()) {
        if !all
            .iter()
            .any(|i| i.location == p.location || (i.version == p.version && p.version != "unknown"))
        {
            all.push(p);
        }
    }
    all.sort_by(|a, b| crate::semver::compare(&b.version, &a.version));
    all
}

/// 选出用于“启动最新”的版本
pub fn pick_latest(installed: &[InstalledVersion]) -> Option<InstalledVersion> {
    let mut candidates: Vec<&InstalledVersion> = installed
        .iter()
        .filter(|i| i.version != "unknown")
        .collect();
    if candidates.is_empty() {
        return None;
    }
    candidates.sort_by(|a, b| {
        let ord = crate::semver::compare(&b.version, &a.version);
        if ord == std::cmp::Ordering::Equal {
            // managed 优先于 global 优先于 path
            let rank = |s: &str| match s {
                "managed" => 0,
                "global" => 1,
                _ => 2,
            };
            rank(&a.source).cmp(&rank(&b.source))
        } else {
            ord
        }
    });
    candidates.first().map(|c| (*c).clone())
}
