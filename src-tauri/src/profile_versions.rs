//! Profile 的「绑定启动版本」：记住每个 profile 上次是用哪个 dsh 版本跑起来的。
//!
//! 界面上的「当前版本」是全局的，切版本（升级或降级）后点启动会直接用新版本拉起 dsh，
//! 而 profile 的插件很可能不兼容，表现为「起不来」却看不出原因。这里记录每个 profile
//! 上次真正跑起来的版本，`start_instance` 据此判断是否要先让用户确认风险。
//!
//! 落在 `~/.dsh-starter/profile-versions.json`：profile 目录由 dsh 自己管理，
//! 启动器不写它的 package.json（见 AGENTS.md），所以绑定关系存在启动器自己的数据目录里。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

/// 落盘的单个 profile 绑定记录
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct BoundVersion {
    pub version: String,
    /// 记录写入时间（毫秒时间戳），用于诊断「这条绑定是什么时候形成的」
    pub updated_at: u64,
}

impl Default for BoundVersion {
    fn default() -> Self {
        Self {
            version: String::new(),
            updated_at: 0,
        }
    }
}

/// 给前端的展示结构
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileVersionInfo {
    pub profile: String,
    pub version: String,
    pub updated_at: u64,
}

const FILE: &str = "profile-versions.json";

fn path() -> std::path::PathBuf {
    crate::settings::starter_home().join(FILE)
}

/// 读-改-写必须串行：轮询与启动动作可能同时改这张表，
/// 原子 rename 只保证单条写不撕裂，挡不住两次「读-改-写」互相覆盖。
fn lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_all() -> BTreeMap<String, BoundVersion> {
    std::fs::read_to_string(path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// 原子写：先写同目录临时文件再 rename，中途崩溃不会留下半截 JSON
/// （解析失败会被当成「没有记录」，把全部绑定关系静默清掉）。
fn write_all(map: &BTreeMap<String, BoundVersion>) -> Result<(), String> {
    let r = (|| -> Result<(), String> {
        let p = path();
        let parent = p
            .parent()
            .ok_or_else(|| "绑定版本路径缺少父目录".to_string())?;
        std::fs::create_dir_all(parent).map_err(|e| format!("创建目录失败: {e}"))?;
        let text = serde_json::to_string_pretty(map).map_err(|e| e.to_string())?;
        let tmp = parent.join(format!("{FILE}.tmp"));
        std::fs::write(&tmp, &text).map_err(|e| format!("写入失败: {e}"))?;
        std::fs::rename(&tmp, &p).map_err(|e| {
            let _ = std::fs::remove_file(&tmp);
            format!("写入失败: {e}")
        })
    })();
    if let Err(e) = &r {
        // 调用方都是 `let _ =`（绑定记录丢了只影响下次启动提示，不该拦住建实例），
        // 所以失败必须留痕
        crate::diag::warn("install", &format!("profile 绑定版本写入失败：{e}"));
    }
    r
}

/// 该 profile 上次成功启动用的 dsh 版本；从未记录过（首次启动）返回 None
pub fn bound(profile: &str) -> Option<String> {
    let prof = profile.trim();
    if prof.is_empty() {
        return None;
    }
    read_all()
        .get(prof)
        .map(|b| b.version.clone())
        .filter(|v| !v.trim().is_empty())
}

/// 记录/更新绑定版本。与现值相同时直接返回，不碰磁盘（轮询每秒都会调到这里）。
pub fn record(profile: &str, version: &str) {
    let prof = profile.trim();
    let ver = version.trim();
    if prof.is_empty() || ver.is_empty() || ver == "unknown" {
        return;
    }
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_all();
    let prev = map.get(prof).map(|b| b.version.clone());
    if prev.as_deref() == Some(ver) {
        return;
    }
    map.insert(
        prof.to_string(),
        BoundVersion {
            version: ver.to_string(),
            updated_at: now_ms(),
        },
    );
    let _ = write_all(&map);
    crate::diag::info(
        "install",
        &format!(
            "profile「{prof}」绑定版本更新：{} → {ver}",
            prev.unwrap_or_else(|| "（首次记录）".into())
        ),
    );
}

/// 全部绑定记录（按 profile 名排序），供界面显示「上次 dsh x.y.z」
pub fn list() -> Vec<ProfileVersionInfo> {
    read_all()
        .into_iter()
        .map(|(profile, b)| ProfileVersionInfo {
            profile,
            version: b.version,
            updated_at: b.updated_at,
        })
        .collect()
}

/// profile 改名后跟着搬记录（不搬就留下一条旧名僵尸，新名又会当成首次启动）
pub fn rename(old: &str, new: &str) {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_all();
    if let Some(b) = map.remove(old) {
        map.insert(new.to_string(), b);
        let _ = write_all(&map);
        crate::diag::info("install", &format!("profile 绑定版本随改名搬移：{old} → {new}"));
    }
}

/// 删除 profile 时清掉它的绑定记录
pub fn drop(profile: &str) {
    let _g = lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut map = read_all();
    if map.remove(profile).is_some() {
        let _ = write_all(&map);
        crate::diag::info("install", &format!("profile「{profile}」已删除，绑定版本记录一并清除"));
    }
}

/// 实例已经活过 grace 时长 = 这个版本确实能把该 profile 跑起来，记为绑定版本。
///
/// 只认启动器自己拉起的实例（内嵌 / 独立进程）：终端里 `dsh --profile x` 的版本
/// 认不出来（`version` 为 None），不能拿它覆盖记录。
/// 秒退的实例由调用方排除在 grace 之外，因此「升级后起不来」的版本不会被记下来，
/// 下一次启动照样会提示风险。
pub fn record_running(instances: &[crate::procs::ProfileInstance], grace_ms: u64) {
    let now = now_ms();
    for i in instances {
        let running_launcher = matches!(i.source.as_deref(), Some("embedded") | Some("detached"));
        let alive = i.started_at.map(|at| now.saturating_sub(at) >= grace_ms);
        if !running_launcher || i.profile.is_empty() || !alive.unwrap_or(false) {
            continue;
        }
        if let Some(v) = &i.version {
            record(&i.profile, v);
        }
    }
}

/// 「实例要活多久才算这个版本真的能跑起来」。
///
/// dsh 在 node 拉起后还要过一遍插件加载，profile 不兼容时往往在 6~15 秒这个区间退掉；
/// 取 15s 是为了不吃掉这段，否则会把坏版本记成绑定版本，风险确认框就再也不弹了。
pub const RUN_GRACE_MS: u64 = 15_000;

#[cfg(test)]
mod tests {
    use super::*;

    /// 每个用例一份独立的 starter_home，避免并行测试互踩
    fn isolated_home(tag: &str) -> std::path::PathBuf {
        let tmp = std::env::temp_dir().join(format!(
            "dsh-pv-{}-{}-{}",
            tag,
            std::process::id(),
            now_ms()
        ));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).unwrap();
        tmp
    }

    #[test]
    fn record_round_trips_and_skips_unchanged_writes() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = isolated_home("round");
        std::env::set_var("DSH_STARTER_HOME", &home);

        assert_eq!(bound("web"), None, "没有记录时不应有绑定版本");
        record("web", "0.3.1");
        assert_eq!(bound("web").as_deref(), Some("0.3.1"));

        // 相同版本重复记录：文件不应被再次改写（轮询每秒都会进来一次）
        let before = std::fs::metadata(path()).unwrap().modified().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        record("web", "0.3.1");
        assert_eq!(
            std::fs::metadata(path()).unwrap().modified().unwrap(),
            before,
            "版本没变不该写盘"
        );

        record("web", "0.4.0");
        assert_eq!(bound("web").as_deref(), Some("0.4.0"));
        let all = list();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].profile, "web");
        assert!(all[0].updated_at > 0);

        std::env::remove_var("DSH_STARTER_HOME");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn empty_and_unknown_versions_are_never_recorded() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = isolated_home("guard");
        std::env::set_var("DSH_STARTER_HOME", &home);

        record("web", "unknown");
        record("web", "  ");
        record("  ", "0.3.1");
        assert_eq!(bound("web"), None);
        assert!(list().is_empty());

        std::env::remove_var("DSH_STARTER_HOME");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn rename_moves_and_drop_removes_the_record() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = isolated_home("rename");
        std::env::set_var("DSH_STARTER_HOME", &home);

        record("old-name", "0.3.1");
        rename("old-name", "new-name");
        assert_eq!(bound("old-name"), None);
        assert_eq!(bound("new-name").as_deref(), Some("0.3.1"));
        drop("new-name");
        assert_eq!(bound("new-name"), None);

        std::env::remove_var("DSH_STARTER_HOME");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn record_running_only_accepts_launcher_instances_past_the_grace() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = isolated_home("running");
        std::env::set_var("DSH_STARTER_HOME", &home);

        let now = now_ms();
        let mk = |profile: &str,
         source: Option<&str>,
         version: Option<&str>,
         started_at: Option<u64>| crate::procs::ProfileInstance {
            profile: profile.into(),
            running: true,
            pid: Some(1),
            source: source.map(|s| s.into()),
            version: version.map(|s| s.into()),
            port: None,
            log_file: None,
            web_url: None,
            started_at,
        };

        record_running(
            &[
                // 活过 grace 的内嵌实例 → 记录
                mk("ok", Some("embedded"), Some("0.3.1"), Some(now - 20_000)),
                // 刚起来的独立进程 → 还不能算数（秒退的坏版本不该进记录）
                mk("young", Some("detached"), Some("0.4.0"), Some(now - 1_000)),
                // 终端里跑的：版本未知，不能覆盖记录
                mk("ext", Some("external"), None, Some(now - 60_000)),
                // 版本认不出的端口探测实例同样跳过
                mk("port", Some("port"), None, Some(now - 60_000)),
            ],
            RUN_GRACE_MS,
        );

        assert_eq!(bound("ok").as_deref(), Some("0.3.1"));
        assert_eq!(bound("young"), None);
        assert_eq!(bound("ext"), None);
        assert_eq!(bound("port"), None);

        std::env::remove_var("DSH_STARTER_HOME");
        std::fs::remove_dir_all(&home).ok();
    }

    #[test]
    fn a_corrupted_file_degrades_to_no_records_instead_of_failing() {
        let _env = crate::util::DSH_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let home = isolated_home("broken");
        std::env::set_var("DSH_STARTER_HOME", &home);

        record("web", "0.3.1");
        std::fs::write(path(), "{ 这不是 JSON").unwrap();
        assert_eq!(bound("web"), None);
        // 坏文件不该把新记录卡住：下一次 record 会整体覆盖
        record("web", "0.4.0");
        assert_eq!(bound("web").as_deref(), Some("0.4.0"));
        assert!(
            !path().with_extension("json.tmp").exists(),
            "原子写不该残留临时文件"
        );

        std::env::remove_var("DSH_STARTER_HOME");
        std::fs::remove_dir_all(&home).ok();
    }
}
