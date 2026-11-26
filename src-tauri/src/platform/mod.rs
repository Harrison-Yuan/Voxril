//! 平台适配层
//!
//! 正式支持：macOS (Apple Silicon) 与 Windows x64。
//! 平台差异点集中在这里，避免散落在业务代码中。
//!
//! 扩展指引：
//! - 新增平台二进制：放到 `src-tauri/bin/<platform>-<arch>/`
//! - 新增平台打包配置：`src-tauri/tauri.<platform>.conf.json`
//! - 新增平台 CI：`.github/workflows/release-<platform>.yml`

use std::path::{Path, PathBuf};
use std::process::Command;

/// Windows 下创建子进程时隐藏控制台窗口（GUI 程序必需）
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 伴生 ffmpeg 可执行文件名
pub fn ffmpeg_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
    }
}

/// 平台专用二进制目录名（相对 `bin/`）。macOS 直接用 `bin/`，返回 None。
pub fn platform_bin_dir() -> Option<&'static str> {
    if cfg!(target_os = "windows") {
        Some("windows-x64")
    } else {
        None
    }
}

/// venv 内 Python 可执行文件的相对路径
pub fn venv_python_rel() -> &'static str {
    if cfg!(target_os = "windows") {
        "Scripts/python.exe"
    } else {
        "bin/python"
    }
}

/// 内置 Python 运行时相对 `bin/` 的路径
pub fn bundled_python_rel() -> &'static str {
    if cfg!(target_os = "windows") {
        "python/python.exe"
    } else {
        "python/bin/python3"
    }
}

/// 内置 Python 运行时候选路径（随包分发的 python-build-standalone）
pub fn bundled_python_candidates(project_root: Option<&Path>) -> Vec<PathBuf> {
    let rel = bundled_python_rel();
    let mut paths = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(bin_dir) = platform_bin_dir() {
                paths.push(dir.join("bin").join(bin_dir).join(rel));
            }
            paths.push(dir.join("bin").join(rel));
            if let Some(res) = dir.parent().map(|p| p.join("Resources")) {
                if let Some(bin_dir) = platform_bin_dir() {
                    paths.push(res.join("bin").join(bin_dir).join(rel));
                }
                paths.push(res.join("bin").join(rel));
                if let Some(res_bin_dir) = platform_bin_dir() {
                    paths.push(res.join("_up_").join("bin").join(res_bin_dir).join(rel));
                }
                paths.push(res.join("_up_").join("bin").join(rel));
            }
        }
    }

    if let Some(root) = project_root {
        if let Some(bin_dir) = platform_bin_dir() {
            paths.push(root.join("src-tauri").join("bin").join(bin_dir).join(rel));
        }
        paths.push(root.join("src-tauri").join("bin").join(rel));
        if let Some(bin_dir) = platform_bin_dir() {
            paths.push(root.join("bin").join(bin_dir).join(rel));
        }
        paths.push(root.join("bin").join(rel));
    }

    paths
}

/// 查找内置 Python 运行时
pub fn find_bundled_python(project_root: Option<&Path>) -> Option<PathBuf> {
    bundled_python_candidates(project_root)
        .into_iter()
        .find(|p| p.exists())
}

/// 找不到 venv 时使用的 Python 命令
pub fn default_python_cmd() -> &'static str {
    if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
    }
}

/// PATH 环境变量的路径分隔符
pub fn path_list_separator() -> char {
    if cfg!(target_os = "windows") {
        ';'
    } else {
        ':'
    }
}

/// 系统 Python 候选路径（按优先级）
pub fn system_python_candidates() -> Vec<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let mut candidates = Vec::new();
        for ver in ["3.12", "3.11", "3.10"] {
            for base in ["/opt/homebrew/bin", "/usr/local/bin"] {
                candidates.push(PathBuf::from(base).join(format!("python{}", ver)));
            }
        }
        candidates
    }
    #[cfg(target_os = "windows")]
    {
        let mut candidates = Vec::new();
        // py launcher 与常见安装目录（优先 3.12 / 3.11 / 3.10）
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            for ver in ["312", "311", "310"] {
                candidates.push(
                    PathBuf::from(&local)
                        .join("Programs")
                        .join("Python")
                        .join(format!("Python{}", ver))
                        .join("python.exe"),
                );
            }
        }
        for ver in ["312", "311", "310"] {
            candidates.push(PathBuf::from(format!("C:\\Python{}\\python.exe", ver)));
        }
        // 最后回退到 PATH 中的 python
        candidates.push(PathBuf::from("python"));
        candidates
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![PathBuf::from("python3")]
    }
}

/// 虚拟声卡提示名（macOS: BlackHole，Windows: VB-Cable）
#[allow(dead_code)] // 预留给后续平台/UI 提示使用
pub fn virtual_audio_hint() -> &'static str {
    if cfg!(target_os = "windows") {
        "VB-Audio Virtual Cable"
    } else {
        "BlackHole 2ch"
    }
}

/// 构造一个不弹出控制台窗口的普通命令（Windows 有效）
pub fn quiet_command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 构造一个不弹出控制台窗口的 tokio 命令（Windows 有效）
pub fn quiet_tokio_command(program: &str) -> tokio::process::Command {
    #[allow(unused_mut)]
    let mut cmd = tokio::process::Command::new(program);
    #[cfg(target_os = "windows")]
    {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// 构造 Python 解释器命令：
/// - macOS：通过 `nice` 降低后台任务优先级，保持前台应用流畅
/// - Windows：无 `nice`，直接调用并隐藏控制台窗口
pub fn python_command(python: &str, nice_level: i32) -> Command {
    #[cfg(target_os = "windows")]
    {
        let _ = nice_level;
        quiet_command(python)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let n = if nice_level > 0 { nice_level } else { 5 };
        let mut cmd = Command::new("nice");
        cmd.arg(format!("-n{}", n)).arg(python);
        cmd
    }
}

/// tokio 版本的 Python 解释器命令
pub fn tokio_python_command(python: &str, nice_level: i32) -> tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        let _ = nice_level;
        quiet_tokio_command(python)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let n = if nice_level > 0 { nice_level } else { 5 };
        let mut cmd = tokio::process::Command::new("nice");
        cmd.arg(format!("-n{}", n)).arg(python);
        cmd
    }
}

/// 判断进程是否存活（用于单实例锁的僵尸清理）
pub fn process_alive(pid: u32) -> bool {
    #[cfg(target_os = "windows")]
    {
        quiet_command("tasklist")
            .args(["/FI", &format!("PID eq {}", pid), "/NH"])
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains(&pid.to_string()))
            .unwrap_or(false)
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("kill")
            .args(["-0", &pid.to_string()])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
}

/// ffmpeg 候选路径（按优先级排列）
pub fn ffmpeg_candidates(project_root: Option<&Path>) -> Vec<PathBuf> {
    let name = ffmpeg_filename();
    let mut paths = Vec::new();

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            if let Some(bin_dir) = platform_bin_dir() {
                paths.push(dir.join("bin").join(bin_dir).join(name));
            }
            paths.push(dir.join("bin").join(name));
            if let Some(res) = dir.parent().map(|p| p.join("Resources")) {
                if let Some(bin_dir) = platform_bin_dir() {
                    paths.push(res.join("bin").join(bin_dir).join(name));
                }
                paths.push(res.join("bin").join(name));
                if let Some(res_bin_dir) = platform_bin_dir() {
                    paths.push(res.join("_up_").join("bin").join(res_bin_dir).join(name));
                }
                paths.push(res.join("_up_").join("bin").join(name));
            }
        }
    }

    if let Some(root) = project_root {
        if let Some(bin_dir) = platform_bin_dir() {
            paths.push(root.join("src-tauri").join("bin").join(bin_dir).join(name));
        }
        paths.push(root.join("src-tauri").join("bin").join(name));
        if let Some(bin_dir) = platform_bin_dir() {
            paths.push(root.join("bin").join(bin_dir).join(name));
        }
        paths.push(root.join("bin").join(name));
    }

    paths
}

/// 查找 ffmpeg，找不到时回退到 PATH
pub fn find_ffmpeg(project_root: Option<&Path>) -> PathBuf {
    for p in ffmpeg_candidates(project_root) {
        if p.exists() {
            return p;
        }
    }
    PathBuf::from(ffmpeg_filename())
}

/// 在文件管理器中打开路径
pub fn open_path(path: &str) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        Command::new("explorer")
            .arg(path)
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map(|_| ())
    }
    #[cfg(not(target_os = "windows"))]
    {
        Command::new("open").arg(path).spawn().map(|_| ())
    }
}
