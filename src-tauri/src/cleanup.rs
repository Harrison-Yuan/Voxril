//! 启动时自动清理垃圾数据
//!
//! 分层设计：
//!   - `run_cleanup()`：启动时自动执行的安全清理（不删关键缓存）
//!   - `deep_clean()`：用户从设置页主动触发的深度清理（含 target/ 等）
//!
//! 安全策略：
//!   - 只删除可无损重建的数据
//!   - 不删除用户资产（模型、音频、配置、数据库）
//!   - 不删除正在训练中的实验目录
//!   - 不删除 .venv / venv / node_modules（重装太慢）

use std::path::Path;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

/// 启动时自动执行的安全清理
pub fn run_cleanup(project_root: &Path, workspace_root: &Path) {
    let cleaned = [
        cleanup_pycache(project_root),
        cleanup_ds_store(project_root),
        cleanup_rotated_logs(workspace_root),
        cleanup_orphaned_f0_profiles(workspace_root),
        cleanup_stale_ivf_cache(workspace_root),
        cleanup_npm_cache(project_root),
        cleanup_py_egg_info(project_root),
    ];
    let total: usize = cleaned.iter().sum();
    if total > 0 {
        eprintln!("[Cleanup] ✅ 本次启动清理了 {} 个垃圾文件", total);
    }
}

/// 深度清理（由用户从设置页触发）：包含安全清理 + 构建产物
/// 返回 总共释放了多少字节
pub fn deep_clean(project_root: &Path, workspace_root: &Path) -> u64 {
    // 先跑常规安全清理
    run_cleanup(project_root, workspace_root);

    let mut freed: u64 = 0;
    // Rust 编译缓存
    freed += remove_dir_size(&project_root.join("src-tauri").join("target"));
    // 前端构建产物
    freed += remove_dir_size(&project_root.join("web").join("dist"));
    // Python .mypy_cache / .pytest_cache
    freed += remove_dir_size(&project_root.join(".mypy_cache"));
    freed += remove_dir_size(&project_root.join(".pytest_cache"));

    if freed > 0 {
        eprintln!("[Cleanup] 🧹 深度清理释放了 {:.2} GB", freed as f64 / (1024.0 * 1024.0 * 1024.0));
    }
    freed
}

// ══════════════════════════════════════════════════════════════════════
//  安全清理项（启动时自动执行）
// ══════════════════════════════════════════════════════════════════════

/// 递归删除 Python __pycache__ 目录
fn cleanup_pycache(root: &Path) -> usize {
    let mut count = 0;
    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_dir()
            && entry.file_name().to_string_lossy() == "__pycache__"
        {
            if std::fs::remove_dir_all(entry.path()).is_ok() {
                count += 1;
            }
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个 __pycache__ 目录", count);
    }
    count
}

/// 删除 macOS .DS_Store 元数据文件
fn cleanup_ds_store(root: &Path) -> usize {
    let mut count = 0;
    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file()
            && entry.file_name().to_string_lossy() == ".DS_Store"
        {
            if std::fs::remove_file(entry.path()).is_ok() {
                count += 1;
            }
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个 .DS_Store 文件", count);
    }
    count
}

/// 删除 npm/vite 内部缓存（node_modules/.cache）
fn cleanup_npm_cache(root: &Path) -> usize {
    let cache_dir = root.join("web").join("node_modules").join(".cache");
    if !cache_dir.exists() {
        return 0;
    }
    let count = remove_dir_content_count(&cache_dir);
    if count > 0 {
        eprintln!("[Cleanup] 清空了 npm/vite 缓存目录 ({} 个文件)", count);
    }
    count
}

/// 删除 Python egg-info 残留目录
fn cleanup_py_egg_info(root: &Path) -> usize {
    let mut count = 0;
    for entry in WalkDir::new(root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if name.ends_with(".egg-info") || name.ends_with(".egg") {
            if std::fs::remove_dir_all(entry.path()).is_ok() {
                count += 1;
            }
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个 Python egg-info 残留目录", count);
    }
    count
}

/// 删除 7 天前的轮转日志备份 (*.1.log, *.2.log 等)
fn cleanup_rotated_logs(workspace_root: &Path) -> usize {
    let exp_root = workspace_root.join("experiments");
    if !exp_root.exists() {
        return 0;
    }

    let now = SystemTime::now();
    let seven_days = Duration::from_secs(7 * 24 * 3600);
    let mut count = 0;

    for entry in WalkDir::new(&exp_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy();
        if !name.ends_with(".log") {
            continue;
        }
        // 匹配轮转备份：train.1.log, train.2.log 等
        let stem = entry.path().file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !stem.contains(".1") && !stem.contains(".2") && !stem.contains(".3")
            && !stem.contains(".4") && !stem.contains(".5")
        {
            continue;
        }
        if let Ok(meta) = entry.path().metadata() {
            if let Ok(mtime) = meta.modified() {
                if let Ok(age) = now.duration_since(mtime) {
                    if age >= seven_days {
                        if std::fs::remove_file(entry.path()).is_ok() {
                            count += 1;
                        }
                    }
                }
            }
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个过期轮转日志", count);
    }
    count
}

/// 删除孤立的 .f0.json（对应的音频文件已不存在）
fn cleanup_orphaned_f0_profiles(workspace_root: &Path) -> usize {
    let vocal_dir = workspace_root.join("separated").join("vocals");
    if !vocal_dir.exists() {
        return 0;
    }

    let mut count = 0;
    let audio_extensions = ["wav", "flac", "mp3", "ogg", "m4a"];
    for entry in WalkDir::new(&vocal_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        if path.extension().map_or(false, |e| e == "json") {
            let fname = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if let Some(base_name) = fname.strip_suffix(".f0") {
                let audio_exists = audio_extensions.iter().any(|ext| {
                    path.with_file_name(format!("{}.{}", base_name, ext)).exists()
                        || path.with_file_name(format!("vocal_{}.{}", base_name, ext)).exists()
                        || path.with_file_name(format!("instrumental_{}.{}", base_name, ext)).exists()
                });
                if !audio_exists && std::fs::remove_file(path).is_ok() {
                    count += 1;
                }
            }
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个孤立的 F0 Profile", count);
    }
    count
}

/// 删除孤立的 IVF 缓存文件（.vectors.bin / .meta.txt / .ivf.bin）
///
/// ⚠️ 命名规则（build_index_cache 生成）：
///   index 文件：       xxx.index
///   meta 缓存：        xxx.index.meta.txt
///   vectors 缓存：     xxx.index.vectors.bin
///   ivf 缓存：         xxx.index.ivf.bin
///
/// 从 .meta.txt 推导 index 路径时必须去掉 .index.meta.txt 后缀，
/// 用 with_extension("") 会只去掉最后一个 .txt，得到错误的 xxx.index.meta。
fn cleanup_stale_ivf_cache(workspace_root: &Path) -> usize {
    let exp_root = workspace_root.join("experiments");
    if !exp_root.exists() {
        return 0;
    }

    let suffix = ".meta.txt";
    let mut count = 0;

    for entry in WalkDir::new(&exp_root)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.ends_with(suffix) {
            continue;
        }

        // 去掉 .meta.txt 后缀得到 index 路径
        let base_name = name[..name.len() - suffix.len()].to_string();
        let index_path = entry.path().parent().unwrap_or(Path::new("")).join(&base_name);

        // index 文件还在 → 缓存有效，跳过
        if index_path.exists() {
            continue;
        }

        // 删除配套的三个缓存文件（.meta.txt, .vectors.bin, .ivf.bin）
        // 注意：不删 .index 文件本身
        let dir = entry.path().parent().unwrap_or(Path::new(""));
        for fname in [&format!("{}.vectors.bin", base_name), &format!("{}.ivf.bin", base_name)] {
            let p = dir.join(fname);
            if p.exists() && std::fs::remove_file(&p).is_ok() {
                count += 1;
            }
        }
        // 最后删 .meta.txt 自身
        if std::fs::remove_file(entry.path()).is_ok() {
            count += 1;
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个孤立的 IVF 缓存文件", count);
    }
    count
}

// ══════════════════════════════════════════════════════════════════════
//  工具函数
// ══════════════════════════════════════════════════════════════════════

/// 删除整个目录（返回释放的字节数），不存在的目录返回 0
fn remove_dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    let size = dir_size(path);
    if std::fs::remove_dir_all(path).is_ok() {
        size
    } else {
        0
    }
}

/// 递归计算目录总大小
fn dir_size(path: &Path) -> u64 {
    let mut total = 0u64;
    for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_file() {
            if let Ok(meta) = entry.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

/// 清空目录内容（不删目录本身），返回删除的文件/目录数
fn remove_dir_content_count(path: &Path) -> usize {
    if !path.exists() {
        return 0;
    }
    let mut count = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if std::fs::remove_dir_all(entry.path()).is_ok() {
                    count += 1;
                }
            } else if std::fs::remove_file(entry.path()).is_ok() {
                count += 1;
            }
        }
    }
    count
}
