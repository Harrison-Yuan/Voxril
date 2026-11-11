//! DAW（数字音频工作站）进程检测
//!
//! 检测 Studio One、prodriver、Logic Pro 等 DAW 是否正在运行，
//! 用于自动判断是否需要切换到 BlackHole 桥接模式，避免音频设备冲突。

/// DAW 进程关键词列表（用于 pgrep -f 匹配完整命令行）
const DAW_PATTERNS: &[&str] = &[
    "Studio One",
    "prodriver",
    "Logic Pro",
    "MainStage",
    "Ableton",
    "ProTools",
    "REAPER",
    "FL Studio",
    "Cubase",
    "Nuendo",
    "Reason",
    "Bitwig",
    "Audition",
    "GarageBand",
    "MixBus",
    "Ardour",
];

/// 检测是否有 DAW 正在运行
///
/// 使用 `pgrep -f -i` 扫描已知 DAW 进程的完整命令行。
/// macOS 上 DAW 进程名常有版本号后缀（如 "Studio One 6"）或缩写（如 "Live"），
/// 因此用 `-f` 匹配 .app 路径而非 `-x` 精确匹配进程名。
///
/// # Returns
///
/// - `true` — 检测到至少一个 DAW 正在运行
/// - `false` — 未检测到 DAW，或检测失败
#[cfg(target_os = "macos")]
pub fn is_daw_running() -> bool {
    for pattern in DAW_PATTERNS {
        if let Ok(output) = std::process::Command::new("pgrep")
            .arg("-f")
            .arg("-i")
            .arg(pattern)
            .output()
        {
            if output.status.success() {
                eprintln!("[daw] 检测到 DAW: {}", pattern);
                return true;
            }
        }
    }
    false
}

#[cfg(not(target_os = "macos"))]
pub fn is_daw_running() -> bool {
    false
}
