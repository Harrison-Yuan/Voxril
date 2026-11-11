//! 进程生命周期管理 — 统一的进程/kill/清理工具
//!
//! 所有子进程（训练、推理、UVR、下载）都应通过此模块管理，
//! 确保关闭时不留僵尸进程。

use std::process::Command;

/// 立即杀死指定 PID 列表（SIGKILL）
pub fn kill_pids(pids: &[u32]) {
    for pid in pids {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
}

/// 杀死当前进程的所有直接子进程（通过 pgrep -P）
/// 用于兜底清理未被跟踪的孤儿进程
pub fn kill_orphan_children() {
    let own_pid = std::process::id();
    let Ok(out) = Command::new("pgrep")
        .args(["-P", &own_pid.to_string()])
        .output()
    else { return };
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Ok(pid) = line.trim().parse::<u32>() {
            let _ = Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
    }
}

/// 杀死已知 PID + 兜底孤儿 + Python multiprocessing 残留
/// 这是应用退出时的最终清理，不应被拦截
pub fn kill_all(pids: &[u32]) {
    kill_pids(pids);
    kill_orphan_children();
    kill_multiprocessing_spawn();
}

/// 使用 SIGTERM（礼貌）杀死匹配模式的进程
/// 返回是否所有进程都已退出
pub fn kill_by_pattern(pattern: &str, sigkill_after_secs: u64) -> bool {
    // 先 SIGTERM
    let _ = Command::new("pkill")
        .args(["-SIGTERM", "-f", pattern])
        .output();

    if sigkill_after_secs == 0 {
        return true;
    }

    // 轮询等待退出
    for _ in 0..(sigkill_after_secs * 2) {
        std::thread::sleep(std::time::Duration::from_millis(500));
        let alive = Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false);
        if !alive {
            return true;
        }
    }

    // 超时后 SIGKILL
    let _ = Command::new("pkill")
        .args(["-9", "-f", pattern])
        .output();
    false
}

/// 清理 Python multiprocessing 残留（fork 后变成孤儿进程）
pub fn kill_multiprocessing_spawn() {
    for pattern in &[
        "from multiprocessing.spawn import spawn_main",
        "from multiprocessing.resource_tracker import main",
    ] {
        let _ = Command::new("pkill")
            .args(["-9", "-f", pattern])
            .output();
    }
}
