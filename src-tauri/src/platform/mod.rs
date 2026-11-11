//! 平台适配层
//!
//! 正式支持：macOS (Apple Silicon)；Intel Mac 不提供支持。
//! Windows 的差异点集中在这里，便于后续扩展，不影响现有 macOS 逻辑。
//!
//! 扩展指引：
//! - 新增平台二进制：放到 `src-tauri/bin/<platform>-<arch>/`
//! - 新增平台打包配置：`src-tauri/tauri.<platform>.conf.json`
//! - 新增平台 CI：`.github/workflows/release-<platform>.yml`（手动触发，不参与 tag 发版）

use std::path::PathBuf;

/// 伴生 ffmpeg 可执行文件名
pub fn ffmpeg_filename() -> &'static str {
    if cfg!(target_os = "windows") {
        "ffmpeg.exe"
    } else {
        "ffmpeg"
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

/// 找不到 venv 时使用的 Python 命令
pub fn default_python_cmd() -> &'static str {
    if cfg!(target_os = "windows") {
        "python"
    } else {
        "python3"
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
        // 预留：Windows 上通过 PATH / py launcher 查找
        vec![PathBuf::from("python")]
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        vec![PathBuf::from("python3")]
    }
}

/// 虚拟声卡提示名（macOS: BlackHole，Windows 预留: VB-Cable）
#[allow(dead_code)] // 预留给后续平台/UI 提示使用
pub fn virtual_audio_hint() -> &'static str {
    if cfg!(target_os = "windows") {
        "VB-Audio Virtual Cable"
    } else {
        "BlackHole 2ch"
    }
}
