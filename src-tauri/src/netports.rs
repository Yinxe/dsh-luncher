//! 端口占用探测：判断某个 TCP 端口上是否真有 web 实例在监听，以及是谁占的。
//!
//! 为什么用端口而不只靠 PID：PID 会被复用、`detached.json` 会随启动器重启丢失，
//! 而「profile 的 web 端口」来自配置（cordis.patch.yml 的 webserver.config.port），
//! 是稳定身份——只要端口还在监听，重启后就能重新发现终端/独立启动的 dsh 实例。
//!
//! 两条通道分工，不要混用：
//! - [`is_listening`]：std `TcpStream::connect_timeout`，实时、零权限，判「活着没」；
//! - [`listener_pid`]：`listeners` crate（Linux 读 /proc、macOS libproc、Windows
//!   IP Helper）取「谁占的」，用于展示与停止。
//!
//! 注意：端口是**补充**不是替代。dsh 启动后要过一会儿才 bind 端口，这段「已拉起、
//! 还没监听」的窗口只有进程句柄/进程表知道，端口探测会说「没运行」。

use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::time::Duration;

/// 连接探测超时：本机回环上 connect 立即返回（成功或 ECONNREFUSED），这里只兜底异常网络栈
const PROBE_TIMEOUT: Duration = Duration::from_millis(300);

/// 配置里的 host 归一化成可连接的探测地址：0.0.0.0 / :: / 空 / 非法 → 回环
fn probe_addr(host: &str, port: u16) -> SocketAddr {
    let ip: IpAddr = host
        .trim()
        .parse()
        .unwrap_or(IpAddr::V4(Ipv4Addr::LOCALHOST));
    let ip = if ip.is_unspecified() {
        IpAddr::V4(Ipv4Addr::LOCALHOST)
    } else {
        ip
    };
    SocketAddr::new(ip, port)
}

/// 该端口上是否有进程在监听（= 对应的 web 实例活着）。
///
/// 用 connect 而不是 bind：bind 会被 TIME_WAIT / SO_REUSEADDR 语义左右，
/// 会把刚停掉、尚在 TIME_WAIT 的端口误判成还占着。
pub fn is_listening(host: &str, port: u16) -> bool {
    port != 0 && TcpStream::connect_timeout(&probe_addr(host, port), PROBE_TIMEOUT).is_ok()
}

/// 占用该 TCP **监听**端口（LISTEN）的进程 PID。
///
/// 只认 LISTEN：同一端口上还会有 ESTABLISHED / TIME_WAIT 条目，它们不代表
/// 「谁在提供服务」。拿不到（权限不足、进程刚退出、平台不支持）返回 `None`——
/// 调用方不得据此认定进程不存在。
pub fn listener_pid(port: u16) -> Option<u32> {
    use listeners::{Protocol, SocketState};
    if port == 0 || !listeners::IS_OS_SUPPORTED {
        return None;
    }
    listeners::get_all()
        .ok()?
        .into_iter()
        .find(|l| {
            l.protocol == Protocol::TCP && l.state == SocketState::Listen && l.socket.port() == port
        })
        .map(|l| l.process.pid)
}

/// 本机所有处于 LISTEN 的 TCP 端口及其占用进程 PID（一个端口可能对应多个 PID）。
///
/// 与 [`listener_pid`] 的区别：这个不预设端口，用来**反向发现**「系统里到底有哪些
/// dsh 在监听端口」，因此不依赖 profile 有没有配端口、也不依赖启动器注册表。
pub fn listening_tcp() -> Vec<(u16, u32)> {
    use listeners::{Protocol, SocketState};
    if !listeners::IS_OS_SUPPORTED {
        return Vec::new();
    }
    listeners::get_all()
        .ok()
        .map(|set| {
            set.into_iter()
                .filter(|l| l.protocol == Protocol::TCP && l.state == SocketState::Listen)
                .map(|l| (l.socket.port(), l.process.pid))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn probe_reports_bound_port_and_free_port() {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(is_listening("127.0.0.1", port), "已 bind 的端口应探测为监听中");
        // 0.0.0.0（配置面板的常见取值）应归一化成回环再探测
        assert!(is_listening("0.0.0.0", port), "0.0.0.0 应归一化为回环");
        drop(l);
        assert!(!is_listening("127.0.0.1", port), "已释放的端口应探测为空闲");
    }

    #[test]
    fn listener_pid_finds_self_for_bound_port() {
        let l = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        // 本进程就是监听者。受限环境（读不到 /proc/*/fd）或平台不支持时跳过，不误判为失败
        match listener_pid(port) {
            Some(pid) => assert_eq!(pid, std::process::id(), "监听者应是测试进程自身"),
            None => eprintln!("listener_pid 在本环境不可用，跳过断言"),
        }
    }
}
