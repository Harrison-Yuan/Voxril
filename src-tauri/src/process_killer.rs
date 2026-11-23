//! 进程生命周期管理 — 统一的进程/kill/清理工具
//!
//! 所有子进程（训练、推理、UVR、下载）都应通过此模块管理，
//! 确保关闭时不留僵尸进程。
//!
//! macOS 使用 `kill` / `pgrep` / `pkill`；Windows 使用 `taskkill` 与
//! PowerShell 的 `Get-CimInstance Win32_Process` 按命令行匹配。

#[cfg(not(target_os = "windows"))]
use std::process::Command;

/// 立即杀死指定 PID（含子进程）
pub fn kill_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        let _ = crate::platform::quiet_command("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
}

/// 立即杀死指定 PID 列表
pub fn kill_pids(pids: &[u32]) {
    for pid in pids {
        kill_pid(*pid);
    }
}

/// 判断命令行中包含 `pattern` 的进程是否存在（大小写不敏感，字面匹配）
pub fn is_process_running(pattern: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        let lower = pattern.to_lowercase().replace('\'', "''");
        let script = format!(
            "$p = Get-CimInstance Win32_Process -Filter \"Name LIKE '%python%'\" | \
             Where-Object {{ $_.CommandLine -and $_.CommandLine.ToLower().Contains('{}') }}; \
             if ($p) {{ 'yes' }}",
            lower
        );
        crate::platform::quiet_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("yes"))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            .map(|o| o.status.success() && !o.stdout.is_empty())
            .unwrap_or(false)
    }
}

/// 杀死当前进程的所有直接子进程
/// 用于兜底清理未被跟踪的孤儿进程
pub fn kill_orphan_children() {
    let own_pid = std::process::id();
    #[cfg(target_os = "windows")]
    {
        let script = format!(
            "Get-CimInstance Win32_Process -Filter \"ParentProcessId = {}\" | \
             ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
            own_pid
        );
        let _ = crate::platform::quiet_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let Ok(out) = Command::new("pgrep")
            .args(["-P", &own_pid.to_string()])
            .output()
        else {
            return;
        };
        for line in String::from_utf8_lossy(&out.stdout).lines() {
            if let Ok(pid) = line.trim().parse::<u32>() {
                kill_pid(pid);
            }
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
    #[cfg(target_os = "windows")]
    {
        let _ = sigkill_after_secs;
        let lower = pattern.to_lowercase().replace('\'', "''");
        let script = format!(
            "Get-CimInstance Win32_Process -Filter \"Name LIKE '%python%'\" | \
             Where-Object {{ $_.CommandLine -and $_.CommandLine.ToLower().Contains('{}') }} | \
             ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
            lower
        );
        let _ = crate::platform::quiet_command("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &script])
            .output();
        !is_process_running(pattern)
    }
    #[cfg(not(target_os = "windows"))]
    {
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
            if !is_process_running(pattern) {
                return true;
            }
        }

        // 超时后 SIGKILL
        let _ = Command::new("pkill")
            .args(["-9", "-f", pattern])
            .output();
        false
    }
}

/// 清理 Python multiprocessing 残留（spawn 后变成孤儿进程）
pub fn kill_multiprocessing_spawn() {
    for pattern in &[
        "from multiprocessing.spawn import spawn_main",
        "from multiprocessing.resource_tracker import main",
        "multiprocessing.spawn",
        "multiprocessing.resource_tracker",
    ] {
        #[cfg(target_os = "windows")]
        {
            let lower = pattern.to_lowercase().replace('\'', "''");
            let script = format!(
                "Get-CimInstance Win32_Process -Filter \"Name LIKE '%python%'\" | \
                 Where-Object {{ $_.CommandLine -and $_.CommandLine.ToLower().Contains('{}') }} | \
                 ForEach-Object {{ Stop-Process -Id $_.ProcessId -Force -ErrorAction SilentlyContinue }}",
                lower
            );
            let _ = crate::platform::quiet_command("powershell")
                .args(["-NoProfile", "-NonInteractive", "-Command", &script])
                .output();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = Command::new("pkill")
                .args(["-9", "-f", pattern])
                .output();
        }
    }
}
