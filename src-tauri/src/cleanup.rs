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

use crate::workspace_layout::{bytes_of, is_safe_component, OutputKind, WorkspaceLayout};
use std::path::Path;
use std::time::{Duration, SystemTime};
use walkdir::WalkDir;

/// `.trash` 条目保留天数，过期后由启动清理删除。
const TRASH_RETENTION_DAYS: u64 = 7;

/// 启动时自动执行的安全清理
pub fn run_cleanup(project_root: &Path, workspace_root: &Path) {
    let cleaned = [
        cleanup_pycache(project_root),
        cleanup_ds_store(project_root),
        cleanup_rotated_logs(workspace_root),
        cleanup_orphaned_f0_profiles(workspace_root),
        cleanup_stale_ivf_cache(workspace_root),
        cleanup_orphaned_stft_cache(workspace_root),
        cleanup_expired_cover_jobs(workspace_root),
        cleanup_expired_trash(workspace_root),
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
    let exp_root = crate::workspace_layout::WorkspaceLayout::detect(workspace_root).runs_root();
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

/// 删除 7 天前的一键翻唱中间产物（每次生成的任务目录）。
///
/// 中间产物只服务于当次生成，成品已落在 outputs；保留一周便于用户回听人声/伴奏。
fn cleanup_expired_cover_jobs(workspace_root: &Path) -> usize {
    let jobs_root = crate::workspace_layout::WorkspaceLayout::detect(workspace_root).cover_jobs_root();
    if !jobs_root.exists() {
        return 0;
    }

    let now = SystemTime::now();
    let mut count = 0;
    for entry in WalkDir::new(&jobs_root)
        .min_depth(1)
        .max_depth(1)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !entry.file_type().is_dir() {
            continue;
        }
        let expired = entry
            .path()
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|mtime| now.duration_since(mtime).ok())
            .map(|age| age >= Duration::from_secs(TRASH_RETENTION_DAYS * 24 * 3600))
            .unwrap_or(false);
        if expired && std::fs::remove_dir_all(entry.path()).is_ok() {
            count += 1;
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个过期翻唱中间产物目录", count);
    }
    count
}

/// 删除孤立的 .f0.json（对应的音频文件已不存在）
fn cleanup_orphaned_f0_profiles(workspace_root: &Path) -> usize {
    let vocal_dir = crate::workspace_layout::WorkspaceLayout::detect(workspace_root)
        .datasets_root()
        .join("vocals");
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
    let exp_root = crate::workspace_layout::WorkspaceLayout::detect(workspace_root).assets_root();
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
//  `.trash` 回收站
// ══════════════════════════════════════════════════════════════════════

/// 当前时间（毫秒 Unix 时间戳）。
fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// 移入 `.trash/<group>/<毫秒时间戳>-<原名>`，返回实际移走的字节数。
/// 文件名前缀记录进入回收站的时间，避免用产物自身的 mtime 判断过期。
/// 同卷 rename 瞬时完成；失败时不删除源，只返回 0。
pub fn move_to_trash(layout: &WorkspaceLayout, group: &str, src: &Path) -> u64 {
    if !src.exists() {
        return 0;
    }
    if !is_safe_component(group) {
        eprintln!("[Cleanup] 回收分组名不合法，跳过: {}", group);
        return 0;
    }
    let trash_dir = layout.trash_dir().join(group);
    if std::fs::create_dir_all(&trash_dir).is_err() {
        return 0;
    }
    let bytes = bytes_of(src);
    let name = src
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "item".to_string());
    let ts = now_millis();
    let mut dst = trash_dir.join(format!("{}-{}", ts, name));
    let mut n = 2;
    while dst.exists() {
        dst = trash_dir.join(format!("{}-{}-{}", ts, name, n));
        n += 1;
    }
    match std::fs::rename(src, &dst) {
        Ok(()) => bytes,
        Err(e) => {
            eprintln!("[Cleanup] 移入回收站失败 {}: {}", src.display(), e);
            0
        }
    }
}

/// 批次工作位的一次性产物回收：R1（`1_16k_wavs`/`eval`/tfevents），
/// `deep` 时追加 R2（`0_gt_wavs`/`2a_f0`/`2b-f0nsf`）与 G_/D_ 权重。
/// 只移动文件，不写清单与数据库，由调用方负责记账。
pub fn reclaim_work_dir(layout: &WorkspaceLayout, group: &str, work_dir: &Path, deep: bool) -> u64 {
    let mut freed = 0;

    for name in ["1_16k_wavs", "eval"] {
        freed += move_to_trash(layout, group, &work_dir.join(name));
    }
    if let Ok(entries) = std::fs::read_dir(work_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("events.out.tfevents") {
                freed += move_to_trash(layout, group, &entry.path());
            }
        }
    }
    if !deep {
        return freed;
    }

    for name in ["0_gt_wavs", "2a_f0", "2b-f0nsf", "checkpoints"] {
        freed += move_to_trash(layout, group, &work_dir.join(name));
    }
    // G_/D_ 必须成对处理：续训时缺任一侧都会破坏一致性，因此整批判走
    if let Ok(entries) = std::fs::read_dir(work_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if (name.starts_with("G_") || name.starts_with("D_")) && name.ends_with(".pth") {
                freed += move_to_trash(layout, group, &entry.path());
            }
        }
    }
    freed
}

/// 条目进入回收站的时间：优先取文件名前缀的毫秒时间戳，回落到文件 mtime。
fn trash_entry_time(path: &Path) -> Option<SystemTime> {
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if let Ok(ms) = name.split('-').next()?.parse::<u64>() {
            return std::time::UNIX_EPOCH.checked_add(Duration::from_millis(ms));
        }
    }
    std::fs::metadata(path).ok()?.modified().ok()
}

/// 删除 `.trash` 下超过保留期的条目，并移除已空的批次分组目录。
fn cleanup_expired_trash(workspace_root: &Path) -> usize {
    let trash = WorkspaceLayout::detect(workspace_root).trash_dir();
    let Ok(groups) = std::fs::read_dir(&trash) else {
        return 0;
    };
    let now = SystemTime::now();
    let retention = Duration::from_secs(TRASH_RETENTION_DAYS * 24 * 3600);
    let mut count = 0;

    for group in groups.flatten() {
        if !group.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(group.path()) {
            for entry in entries.flatten() {
                let expired = trash_entry_time(&entry.path())
                    .and_then(|t| now.duration_since(t).ok())
                    .map(|age| age >= retention)
                    .unwrap_or(false);
                if !expired {
                    continue;
                }
                let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                let removed = if is_dir {
                    std::fs::remove_dir_all(entry.path()).is_ok()
                } else {
                    std::fs::remove_file(entry.path()).is_ok()
                };
                if removed {
                    count += 1;
                }
            }
        }
        // 分组清空后移除空目录，避免 .trash 里堆一串空壳
        let empty = std::fs::read_dir(group.path())
            .map(|mut it| it.next().is_none())
            .unwrap_or(false);
        if empty {
            let _ = std::fs::remove_dir(group.path());
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个过期回收站条目", count);
    }
    count
}

/// 删除没有宿主音频的 `.stft.npy`（直播相位模板缓存，宿主被删后即成孤儿）。
fn cleanup_orphaned_stft_cache(workspace_root: &Path) -> usize {
    let layout = WorkspaceLayout::detect(workspace_root);
    let mut roots = vec![
        layout.output_dir(OutputKind::Vocal),
        layout.output_dir(OutputKind::Mixed),
        layout.final_output_dir(),
    ];
    roots.sort();
    roots.dedup();

    const AUDIO_EXTS: [&str; 5] = ["wav", "mp3", "flac", "m4a", "ogg"];
    const SUFFIX: &str = ".stft.npy";
    let mut count = 0;

    for root in roots {
        if !root.exists() {
            continue;
        }
        for entry in WalkDir::new(&root)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            let Some(stem) = name.strip_suffix(SUFFIX) else {
                continue;
            };
            let dir = entry.path().parent().unwrap_or(Path::new(""));
            let host_exists = AUDIO_EXTS
                .iter()
                .any(|ext| dir.join(format!("{}.{}", stem, ext)).exists());
            if !host_exists && std::fs::remove_file(entry.path()).is_ok() {
                count += 1;
            }
        }
    }
    if count > 0 {
        eprintln!("[Cleanup] 删除了 {} 个孤立的 STFT 缓存", count);
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
