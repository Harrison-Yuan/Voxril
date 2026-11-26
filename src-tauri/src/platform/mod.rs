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

/// 应用数据目录名，沿用历史标识以兼容旧版本数据。
const DATA_DIR_NAME: &str = "com.rvc.webui";

/// 数据目录标记文件；由本程序写入，用于识别「这确实是 Voxril 的数据目录」。
pub const DATA_DIR_MARKER: &str = ".voxril-data";

/// 建议为数据目录预留的空间（内置环境 + 基础模型 + 训练缓存）。
pub const DATA_DIR_RECOMMENDED_BYTES: u64 = 20 * 1024 * 1024 * 1024;

/// Voxril 自己创建的数据目录条目。清理旧目录时只删这些，绝不整棵递归删用户目录。
pub const DATA_DIR_OWNED_ENTRIES: &[&str] =
    &["venv", "assets", "configs", "window_state.json", DATA_DIR_MARKER];

/// 是否为 Voxril 数据目录：有标记，或存在我们创建的特征文件。
pub fn looks_like_data_dir(dir: &Path) -> bool {
    dir.join(DATA_DIR_MARKER).exists() || data_dir_has_payload(dir)
}

/// 目录里是否已有真实数据（环境或基础模型）。
///
/// 与 [`looks_like_data_dir`] 的区别：不算标记文件。首启判定要在
/// 我们主动创建目录、写标记之前问用户，算上标记就会漏掉引导。
pub fn data_dir_has_payload(dir: &Path) -> bool {
    if dir.join("venv").join("pyvenv.cfg").exists() {
        return true;
    }
    ["hubert", "uvr_models", "pretrained_v2", "rmvpe", "fcpe"]
        .iter()
        .any(|name| dir.join("assets").join(name).exists())
}

/// 在数据目录里补写标记文件（幂等，失败忽略）。
pub fn mark_data_dir(dir: &Path) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let marker = dir.join(DATA_DIR_MARKER);
    if !marker.exists() {
        let _ = std::fs::write(marker, b"voxril-data-dir");
    }
}

/// 只删除 Voxril 自己创建的条目（venv / assets / configs / 标记文件）。
///
/// 返回删除失败的文件数（Windows 上被占用的 dll 等）。两道保险：
/// 只删已知条目、且目标必须已被识别为数据目录，绝不整棵递归删用户目录。
pub fn purge_data_dir(dir: &Path) -> u64 {
    if !looks_like_data_dir(dir) {
        return 0;
    }
    let mut failures = 0u64;
    for name in DATA_DIR_OWNED_ENTRIES {
        let path = dir.join(name);
        match std::fs::symlink_metadata(&path) {
            Err(_) => continue, // 不存在，跳过
            Ok(meta) if meta.is_dir() => failures += remove_tree(&path),
            Ok(_) => {
                if std::fs::remove_file(&path).is_err() {
                    failures += 1;
                }
            }
        }
    }
    // 空了就顺手移除目录本身；非空（用户自己的文件）则保留
    let _ = std::fs::remove_dir(dir);
    failures
}

/// 数据目录清理后仍残留的非本程序条目数（目录已删则返回 0）。
/// 用于如实告知用户旧目录里还剩多少自己的文件，不会谎报「已清空」。
pub fn data_dir_leftovers(dir: &Path) -> u64 {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|e| e.file_name() != ".DS_Store")
            .count() as u64,
        Err(_) => 0,
    }
}

/// 递归删除一棵目录树，返回失败文件数。遇到符号链接只删链接本身，不跟随。
fn remove_tree(dir: &Path) -> u64 {
    let mut failures = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return if dir.is_dir() && !dir.exists() { 0 } else { 1 };
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // file_type 不跟随符号链接：链接一律按文件删，避免越界删到链接指向的内容
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        if is_dir {
            failures += remove_tree(&path);
        } else if std::fs::remove_file(&path).is_err() {
            failures += 1;
        }
    }
    if std::fs::remove_dir(dir).is_err() && dir.exists() {
        failures += 1;
    }
    failures
}

/// 解析应用数据目录（Python 环境、基础模型、运行配置）。
///
/// 优先级：
/// 1. 环境变量 `VOXRIL_DATA_DIR`（测试与高级用法）
/// 2. 用户显式选择过的目录（含迁移后的目标）——只要选过就一直沿用它
/// 3. 旧版数据目录（Windows 上是 `%APPDATA%\com.rvc.webui`，已有 venv/assets）
///    —— 原地沿用，升级后不会重新下载数 GB
/// 4. Windows 正式版：安装目录下的 `data/`，让「装到哪，数据就放哪」
///    （是否可写由 [`data_dir_writable`] 判断，不可写时由界面引导用户另选目录，
///     不再静默回退到系统盘用户目录）
/// 5. 平台标准目录：macOS `~/Library/Application Support`
///
/// macOS 的 `.app` 位于 /Applications 且只读，因此不参与第 4 步。
pub fn resolve_data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("VOXRIL_DATA_DIR") {
        let path = PathBuf::from(dir);
        if !path.as_os_str().is_empty() {
            return path;
        }
    }

    if let Some(path) = saved_data_dir() {
        return path;
    }

    // 旧版目录：macOS 上即标准目录；Windows 上为 Roaming AppData
    let legacy = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(DATA_DIR_NAME);
    if legacy.join("venv").exists() || legacy.join("assets").exists() {
        return legacy;
    }

    if cfg!(target_os = "windows") {
        // 开发构建（target/debug）沿用系统目录，避免清理 target 时丢模型
        if !cfg!(debug_assertions) {
            if let Ok(exe) = std::env::current_exe() {
                if let Some(dir) = exe.parent() {
                    return dir.join("data");
                }
            }
        }
        return legacy;
    }

    legacy
}

/// 所在卷的剩余可用字节数（普通用户可用的空间）。
/// 目标目录可能尚未创建，因此向上找最近的已存在祖先；取不到返回 None。
pub fn volume_available_bytes(path: &Path) -> Option<u64> {
    let mut probe = path;
    loop {
        if probe.exists() {
            break;
        }
        match probe.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => probe = parent,
            _ => return None,
        }
    }

    #[cfg(target_os = "macos")]
    {
        use std::os::unix::ffi::OsStrExt;
        let c_path = std::ffi::CString::new(probe.as_os_str().as_bytes()).ok()?;
        let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
            return None;
        }
        let block = if stat.f_frsize > 0 { stat.f_frsize } else { stat.f_bsize };
        Some(stat.f_bavail as u64).and_then(|n| n.checked_mul(block as u64))
    }

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStrExt;
        let wide: Vec<u16> = probe.as_os_str().encode_wide().chain(std::iter::once(0)).collect();
        let mut available = 0u64;
        let mut total = 0u64;
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW(
                wide.as_ptr(),
                &mut available,
                &mut total,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            None
        } else {
            Some(available)
        }
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = probe;
        None
    }
}

/// 数据目录是否可读写：不存在则尝试创建，再写入并删除一个探针文件。
/// 成功后补写标记文件，方便日后识别「这是我们自己的数据目录」。
pub fn data_dir_writable(dir: &Path) -> bool {
    if std::fs::create_dir_all(dir).is_err() {
        return false;
    }
    let probe = dir.join(".write-test");
    let ok = std::fs::write(&probe, b"voxril").is_ok();
    let _ = std::fs::remove_file(&probe);
    if ok {
        mark_data_dir(dir);
    }
    ok
}

/// 用户自定义数据目录的指针文件。
/// 只有程序装在不可写目录、或迁移过数据目录后才会写入；只有几十字节。
fn data_dir_pointer_file() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("com.voxril").join("data-dir.txt"))
}

/// 读取用户保存的数据目录。
///
/// 即使该目录此刻不可用（移动硬盘没插、盘符变了）也照常返回：
/// 用户既然选过位置，就不能静默退回安装目录重新下载数 GB，
/// 而是由界面守卫提示「目录不可写」让用户显式改选。
pub fn saved_data_dir() -> Option<PathBuf> {
    let text = std::fs::read_to_string(data_dir_pointer_file()?).ok()?;
    let path = PathBuf::from(text.trim());
    // 指针文件只有几十字节，被改写或损坏成相对路径时会在当前目录建出垃圾目录
    if path.as_os_str().is_empty() || !path.is_absolute() {
        return None;
    }
    Some(path)
}

/// 保存用户选择的数据目录，下次启动生效。
pub fn save_data_dir(path: &Path) -> std::io::Result<()> {
    write_local_config(data_dir_pointer_file(), &path.to_string_lossy())
}

/// 「待清理的旧数据目录」列表文件；一行一个路径。
fn pending_cleanup_file() -> Option<PathBuf> {
    dirs::data_local_dir().map(|d| d.join("com.voxril").join("cleanup-after-migrate.txt"))
}

/// 待清理的旧数据目录列表。
pub fn pending_cleanup_list() -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(pending_cleanup_file().unwrap_or_default()) else {
        return Vec::new();
    };
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// 覆盖写入待清理列表（空列表时删除文件）。
pub fn save_pending_cleanup_list(list: &[PathBuf]) -> std::io::Result<()> {
    let text = list
        .iter()
        .map(|p| p.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    if list.is_empty() {
        if let Some(file) = pending_cleanup_file() {
            let _ = std::fs::remove_file(file);
        }
        return Ok(());
    }
    write_local_config(pending_cleanup_file(), &text)
}

/// 追加一条待清理记录（去重）。
pub fn add_pending_cleanup(old: &Path) -> std::io::Result<()> {
    let mut list = pending_cleanup_list();
    if !list.iter().any(|p| paths_equal(p, old)) {
        list.push(old.to_path_buf());
    }
    save_pending_cleanup_list(&list)
}

/// 移除一条待清理记录（清理完成后调用）。
pub fn remove_pending_cleanup(old: &Path) -> std::io::Result<()> {
    let list: Vec<PathBuf> = pending_cleanup_list()
        .into_iter()
        .filter(|p| !paths_equal(p, old))
        .collect();
    save_pending_cleanup_list(&list)
}

/// 写入 `%LOCALAPPDATA%\\com.voxril` 下的小配置文件（几十字节，不含数据）。
fn write_local_config(file: Option<PathBuf>, content: &str) -> std::io::Result<()> {
    let file = file.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Unsupported, "无法定位本地配置目录"))?;
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, content)
}

/// 比较两个路径是否指向同一位置：Windows 上忽略大小写。
pub fn paths_equal(a: &Path, b: &Path) -> bool {
    let left = std::path::absolute(a).unwrap_or_else(|_| a.to_path_buf());
    let right = std::path::absolute(b).unwrap_or_else(|_| b.to_path_buf());
    #[cfg(windows)]
    {
        left.to_string_lossy().to_lowercase() == right.to_string_lossy().to_lowercase()
    }
    #[cfg(not(windows))]
    {
        left == right
    }
}

/// 解析成可用于「包含关系」判断的绝对路径；尽量取真实路径。
pub fn normalize_path(path: &Path) -> PathBuf {
    let resolved = std::fs::canonicalize(path).unwrap_or_else(|_| std::path::absolute(path).unwrap_or_else(|_| path.to_path_buf()));
    #[cfg(windows)]
    return PathBuf::from(resolved.to_string_lossy().to_lowercase());
    #[cfg(not(windows))]
    return resolved;
}
