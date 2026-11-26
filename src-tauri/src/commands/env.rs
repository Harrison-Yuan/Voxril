//! 运行环境状态：首次启动引导、数据目录检查与迁移。

use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, State};

use crate::state::AppState;

#[derive(Debug, Serialize)]
pub struct EnvStatus {
    /// 应用级 venv 是否就绪（首次启动由 tools/auto_setup_venv.py 创建）
    pub venv_ready: bool,
}

#[tauri::command]
pub async fn get_env_status(state: State<'_, AppState>) -> Result<EnvStatus, String> {
    let rel = crate::platform::venv_python_rel();
    let candidates = [
        // 开发模式的项目内虚拟环境
        state.project_root.join(".venv").join(rel),
        state.project_root.join("venv").join(rel),
        // 发布模式的应用数据目录虚拟环境
        state.data_dir.join("venv").join(rel),
    ];
    Ok(EnvStatus {
        venv_ready: candidates.iter().any(|p| p.exists()),
    })
}

#[derive(Debug, Serialize)]
pub struct DataDirStatus {
    /// 当前解析出的数据目录（Python 环境、基础模型、配置）
    pub path: String,
    /// 是否可写；不可写时前端引导用户另选目录
    pub writable: bool,
    /// 用户是否已明确确认过位置（选过或迁移过）
    pub confirmed: bool,
    /// 目录里是否已有环境与模型（升级用户）
    pub has_data: bool,
    /// 所在卷剩余空间，取不到为 None
    pub free_bytes: Option<u64>,
    /// 建议预留空间
    pub recommended_bytes: u64,
}

#[tauri::command]
pub async fn get_data_dir_status(state: State<'_, AppState>) -> Result<DataDirStatus, String> {
    let path = state.data_dir.clone();
    let has_data = crate::platform::data_dir_has_payload(&path);
    let confirmed = crate::platform::saved_data_dir().is_some();
    Ok(DataDirStatus {
        free_bytes: crate::platform::volume_available_bytes(&path),
        recommended_bytes: crate::platform::DATA_DIR_RECOMMENDED_BYTES,
        writable: crate::platform::data_dir_writable(&path),
        confirmed,
        has_data,
        path: path.to_string_lossy().to_string(),
    })
}

/// 首启向导：用户改选目录后即时查询该卷剩余空间，确认前就能看到够不够放。
#[tauri::command]
pub async fn get_dir_free_space(path: String) -> Result<Option<u64>, String> {
    let dir = PathBuf::from(path.trim());
    if dir.as_os_str().is_empty() {
        return Err("目录不能为空".to_string());
    }
    Ok(crate::platform::volume_available_bytes(&dir))
}

/// 首启确认数据目录。返回值表示是否需要重启才能生效：
/// 沿用当前解析出的目录时无需重启（直接放行环境安装）；
/// 改选别处则必须重启——进程内的 `data_dir` 与执行器仍指向旧路径。
#[tauri::command]
pub async fn confirm_data_dir(
    state: State<'_, AppState>,
    path: String,
) -> Result<bool, String> {
    let raw = PathBuf::from(path.trim());
    if raw.as_os_str().is_empty() || !raw.is_absolute() {
        return Err("请选择一个完整的数据目录".to_string());
    }
    let current = crate::platform::normalize_path(&state.data_dir);
    let chosen = crate::platform::normalize_path(&raw);
    if !crate::platform::paths_equal(&current, &chosen)
        && (chosen.starts_with(&current) || current.starts_with(&chosen))
    {
        return Err("数据目录不能位于当前目录内部（或包含它）".to_string());
    }
    if !crate::platform::data_dir_writable(&raw) {
        return Err("该目录不可写，请换一个有写入权限的目录".to_string());
    }
    crate::platform::mark_data_dir(&raw);
    crate::platform::save_data_dir(&raw).map_err(|e| format!("保存数据目录失败: {}", e))?;

    if crate::platform::paths_equal(&current, &chosen) {
        let _ = state.env_setup_gate.send(true);
        Ok(false)
    } else {
        Ok(true)
    }
}

#[tauri::command]
pub async fn set_data_dir(path: String) -> Result<(), String> {
    let dir = PathBuf::from(path.trim());
    if dir.as_os_str().is_empty() {
        return Err("数据目录不能为空".to_string());
    }
    if !crate::platform::data_dir_writable(&dir) {
        return Err("该目录不可写，请换一个有写入权限的目录".to_string());
    }
    crate::platform::mark_data_dir(&dir);
    crate::platform::save_data_dir(&dir).map_err(|e| format!("保存数据目录失败: {}", e))
}

#[tauri::command]
pub async fn restart_app(app: tauri::AppHandle) {
    app.restart();
}

#[derive(Clone, Serialize)]
struct MigrateProgress {
    percent: u32,
    message: String,
}

fn emit_progress(app: &AppHandle, percent: u32, message: impl Into<String>) {
    let _ = app.emit(
        "data-migrate-progress",
        MigrateProgress {
            percent,
            message: message.into(),
        },
    );
}

/// 复制时忽略的文件：标记与探针文件会在两侧各自生成，计入会误判不一致。
fn is_ignored_entry(name: &str) -> bool {
    name == crate::platform::DATA_DIR_MARKER || name == ".write-test"
}

/// 目录遍历的最大深度，防止异常符号链接成环导致无限递归。
const MAX_WALK_DEPTH: u32 = 32;

/// 统计目录下的条目数与总字节数。
///
/// 条目数按 `lstat` 计（符号链接算一个条目），字节数按跟随链接后的实际内容计。
/// 这样无论链接在目标位置被原样重建，还是因系统不允许而建不出来降级为内容复制，
/// 两侧统计都相等，完整性校验不会误判成「复制不完整」。
fn collect_size(root: &Path) -> std::io::Result<(u64, u64)> {
    let mut bytes = 0u64;
    let mut files = 0u64;
    walk(root, &mut |path, _rel| {
        bytes += path.metadata().map(|m| m.len()).unwrap_or(0);
        files += 1;
        Ok(())
    })?;
    Ok((bytes, files))
}

/// 深度优先遍历 `root` 下的所有条目；真实目录递归，其余（普通文件、符号链接）回调 `on_file`。
///
/// 符号链接必须走回调而不是被跳过：venv 里的 `bin/python -> python3`
/// 若按 `file_type().is_file()` 判定会既不算文件也不算目录，整条丢失，
/// 迁过去的环境就没有解释器。
fn walk<F>(root: &Path, on_file: &mut F) -> std::io::Result<()>
where
    F: FnMut(&Path, &Path) -> std::io::Result<()>,
{
    let mut stack = vec![(root.to_path_buf(), 0u32)];
    while let Some((dir, depth)) = stack.pop() {
        let read = std::fs::read_dir(&dir)
            .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {}", e, dir.display())))?;
        for entry in read {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if is_ignored_entry(&name) {
                continue;
            }
            let path = entry.path();
            // 这里不跟随符号链接：指向目录的链接交给 on_file 原样重建，
            // 避免把同一棵树复制两遍
            let ft = entry.file_type()?;
            if ft.is_dir() {
                if depth + 1 > MAX_WALK_DEPTH {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("目录层级过深: {}", path.display()),
                    ));
                }
                stack.push((path, depth + 1));
            } else {
                let rel = path.strip_prefix(root).unwrap_or(&path);
                on_file(&path, rel)?;
            }
        }
    }
    Ok(())
}

/// 在目标位置重建符号链接。
///
/// 规则（与 `cp -a` / `rsync -a` 一致）：
/// - 相对链接原样保留，树内相对关系迁移后仍然自洽；
/// - 绝对链接若落在旧数据目录内部，改写为新目录里的对应位置，
///   否则清理旧目录后这些链接会全部悬空（venv 解释器直接失效）；
/// - 指向包内运行时等目录外目标的绝对链接保持不变，跨盘迁移后依旧有效。
fn recreate_symlink(src: &Path, target: &Path, src_root: &Path, dst_root: &Path) -> std::io::Result<()> {
    let raw = std::fs::read_link(src)?;
    let final_target = if raw.is_absolute() {
        match std::fs::canonicalize(&raw) {
            Ok(resolved) => match resolved.strip_prefix(src_root) {
                Ok(rel) => dst_root.join(rel),
                Err(_) => raw,
            },
            Err(_) => raw,
        }
    } else {
        raw
    };
    let link_result = {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&final_target, target)
        }
        #[cfg(windows)]
        {
            if final_target.is_dir() {
                std::os::windows::fs::symlink_dir(&final_target, target)
            } else {
                std::os::windows::fs::symlink_file(&final_target, target)
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = &final_target;
            Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "不支持符号链接"))
        }
    };
    match link_result {
        Ok(()) => Ok(()),
        // 无权限建链接（Windows 非开发者模式）时降级为复制内容；
        // 指向目录的链接无法降级，直接报错让上层回滚
        Err(e) if final_target.is_dir() => Err(e),
        Err(_) => std::fs::copy(src, target).map(|_| ()),
    }
}

/// 把 `src_root` 整棵复制到 `dst_root`，按字节推进进度事件，返回复制的字节数。
fn copy_tree(app: &AppHandle, src_root: &Path, dst_root: &Path, total: u64) -> Result<u64, std::io::Error> {
    let mut copied = 0u64;
    let mut last = Instant::now();
    // 改写链接目标要用真实路径：macOS 的 /tmp 实为 /private/tmp，Windows 存在 8.3 短名
    let src_base = std::fs::canonicalize(src_root).unwrap_or_else(|_| src_root.to_path_buf());
    let dst_base = std::fs::canonicalize(dst_root).unwrap_or_else(|_| dst_root.to_path_buf());
    let mut tick = |bytes: u64| {
        copied += bytes;
        if last.elapsed() >= Duration::from_millis(300) {
            let percent = if total == 0 {
                100
            } else {
                ((copied as f64 / total as f64) * 100.0).min(100.0) as u32
            };
            emit_progress(app, percent, format!("正在复制 {percent}%"));
            last = Instant::now();
        }
    };
    walk(src_root, &mut |path, rel| {
        let target = dst_root.join(rel);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let meta = std::fs::symlink_metadata(path)?;
        if meta.is_symlink() {
            recreate_symlink(path, &target, &src_base, &dst_base)?;
            tick(path.metadata().map(|m| m.len()).unwrap_or(0));
        } else {
            std::fs::copy(path, &target)?;
            tick(meta.len());
        }
        Ok(())
    })?;
    Ok(copied)
}

fn migrate_blocking(app: &AppHandle, source: &Path, target: &Path) -> Result<String, String> {
    if !source.exists() {
        crate::platform::save_data_dir(target).map_err(|e| format!("保存数据目录失败: {}", e))?;
        crate::platform::mark_data_dir(target);
        return Ok("数据目录已更新，重启后生效".to_string());
    }

    let (total_bytes, total_files) = collect_size(source).map_err(|e| format!("统计现有数据失败: {}", e))?;
    if total_bytes == 0 {
        crate::platform::save_data_dir(target).map_err(|e| format!("保存数据目录失败: {}", e))?;
        crate::platform::mark_data_dir(target);
        return Ok("数据目录已更新，重启后生效".to_string());
    }

    emit_progress(app, 0, "开始复制…");
    if let Err(e) = copy_tree(app, source, target, total_bytes) {
        let _ = crate::platform::purge_data_dir(target);
        return Err(format!("复制失败: {}", e));
    }

    // 校验完整性，避免指针切过去后数据不全
    let (dst_bytes, dst_files) = collect_size(target).map_err(|e| format!("校验新目录失败: {}", e))?;
    if dst_bytes != total_bytes || dst_files != total_files {
        let _ = crate::platform::purge_data_dir(target);
        return Err(format!(
            "复制不完整（{dst_bytes} / {total_bytes} 字节），已回滚，旧目录未改动，请重试"
        ));
    }

    emit_progress(app, 100, "复制完成，正在清理旧目录…");
    crate::platform::save_data_dir(target).map_err(|e| format!("保存数据目录失败: {}", e))?;
    crate::platform::mark_data_dir(target);

    // 立即清理旧目录里属于本程序的条目，释放原位置空间；
    // 被进程占用的文件留到下次启动补删
    let failures = crate::platform::purge_data_dir(source);
    let leftovers = crate::platform::data_dir_leftovers(source);
    let size_gb = total_bytes as f64 / 1_073_741_824.0;
    let mut msg = format!("已迁移 {total_files} 个文件（{size_gb:.1} GB）");
    if failures > 0 {
        let _ = crate::platform::add_pending_cleanup(source);
        msg.push_str(&format!("；旧目录有 {failures} 个文件正被占用，将在下次启动时清理"));
    } else {
        let _ = crate::platform::remove_pending_cleanup(source);
        if leftovers > 0 {
            msg.push_str(&format!(
                "；旧目录中另有 {leftovers} 个非本程序文件未删除，可手动清理 {}",
                source.display()
            ));
        } else {
            msg.push_str("，旧目录已清理");
        }
    }
    Ok(format!("{msg}，重启后生效"))
}

/// 把现有 Python 环境与模型复制到新目录，成功后写入指针，重启生效。
#[tauri::command]
pub async fn migrate_data_dir(
    app: AppHandle,
    state: State<'_, AppState>,
    target: String,
) -> Result<String, String> {
    let source = state.data_dir.clone();
    let raw_target = PathBuf::from(target.trim());
    if raw_target.as_os_str().is_empty() {
        return Err("请选择目标目录".to_string());
    }

    let source_abs = crate::platform::normalize_path(&source);
    let target_abs = crate::platform::normalize_path(&raw_target);
    if source_abs == target_abs {
        return Err("目标目录与当前数据目录相同".to_string());
    }
    if target_abs.starts_with(&source_abs) || source_abs.starts_with(&target_abs) {
        return Err("目标目录不能位于当前数据目录内部（或反之）".to_string());
    }

    if let Some(task) = state.active_heavy_task.lock().await.clone() {
        return Err(format!("有任务正在运行（{task}），请结束后再迁移"));
    }
    if *state.live_singing_phase.read().await != crate::state::LivePhase::Idle {
        return Err("直播修音引擎正在运行，请先下播再迁移".to_string());
    }
    if state.model_downloader.active_model().await.is_some() {
        return Err("模型正在下载，请下载完成后再迁移".to_string());
    }

    // 目标必须是不存在的目录或空目录：否则复制失败时的回滚可能删掉用户自己的文件
    if let Ok(mut entries) = std::fs::read_dir(&raw_target) {
        if entries.next().is_some() {
            return Err("目标目录不是空目录，请新建一个空文件夹再迁移".to_string());
        }
    }
    if !crate::platform::data_dir_writable(&raw_target) {
        return Err("目标目录不可写，请换一个目录".to_string());
    }

    let app_handle = app.clone();
    let source_walk = source_abs.clone();
    let target_walk = raw_target.clone();
    tauri::async_runtime::spawn_blocking(move || migrate_blocking(&app_handle, &source_walk, &target_walk))
        .await
        .map_err(|e| format!("迁移任务异常: {}", e))?
}
