//! 工作区布局升级（v1 → v2）：预览、执行与回滚。
//!
//! 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §5.2、§5.3。
//! 迁移即终态：逐项 `rename`（同卷瞬时），任一项失败整体回滚；
//! 全部成功后才写 `layout.json`，读侧据此切换路径语义。

use crate::run_manifest;
use crate::state::AppState;
use crate::workspace_layout::{
    bytes_of, write_layout_file, LayoutFile, MigrationEntry, WorkspaceLayout,
    LAYOUT_SCHEMA_VERSION,
};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

const AUDIO_EXTS: [&str; 6] = ["wav", "mp3", "flac", "m4a", "ogg", "aac"];

/// 迁移项：`from`/`to` 均为相对工作区根的可读路径。
#[derive(Debug, Clone, Serialize)]
pub struct MigrationItem {
    pub from: String,
    pub to: String,
    pub bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct LayoutStatus {
    /// 1 = 旧布局，2 = 新布局
    pub version: u32,
    pub is_v2: bool,
    pub workspace: String,
    pub migrated_at: Option<i64>,
    pub mapping_count: usize,
}

#[derive(Debug, Serialize)]
pub struct MigrationPreview {
    pub version: u32,
    pub items: Vec<MigrationItem>,
    pub total_bytes: u64,
    /// 阻止迁移的原因；非空时前端禁用确认按钮
    pub blockers: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct MigrationResult {
    /// 实际移动的文件/目录条目数
    pub moved: usize,
    pub total_bytes: u64,
}

struct PlanOp {
    from: PathBuf,
    to: PathBuf,
}

fn rel(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

fn has_audio_ext(path: &Path) -> bool {
    path.extension()
        .and_then(|s| s.to_str())
        .map(|s| AUDIO_EXTS.contains(&s.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

/// 目录第一层是否存在音频文件（用于识别"训练素材目录"，避免误搬无关目录）。
fn dir_has_audio(dir: &Path) -> bool {
    let Ok(entries) = fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        entry.file_type().map(|t| t.is_file()).unwrap_or(false) && has_audio_ext(&entry.path())
    })
}

/// 拆分 `experiments/<voice>/`：批次清单与归档权重进 `models/`，
/// 批次工作位进 `work/runs/`，旧版工作位（`v2_40k` 等）保留为只读资产。
fn plan_experiments(root: &Path, exp_root: &Path, ops: &mut Vec<PlanOp>) -> Result<(), String> {
    let voices = fs::read_dir(exp_root).map_err(|e| format!("读取 experiments 失败: {}", e))?;
    for voice in voices.flatten() {
        if !voice.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        let voice_name = voice.file_name().to_string_lossy().to_string();
        let entries = fs::read_dir(voice.path())
            .map_err(|e| format!("读取 experiments/{} 失败: {}", voice_name, e))?;
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let to = if name == "runs" {
                root.join("models").join(&voice_name).join("runs")
            } else if name == "weights" {
                root.join("models").join(&voice_name).join("weights")
            } else if run_manifest::is_valid_run_id(&name) {
                root.join("work").join("runs").join(&voice_name).join(&name)
            } else {
                root.join("models").join(&voice_name).join(&name)
            };
            ops.push(PlanOp {
                from: entry.path(),
                to,
            });
        }
    }
    Ok(())
}

/// 生成迁移计划（不落地）。
fn build_plan(root: &Path) -> Result<Vec<PlanOp>, String> {
    let mut ops = Vec::new();
    let entries = fs::read_dir(root).map_err(|e| format!("读取工作区失败: {}", e))?;

    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let path = entry.path();
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);

        if is_dir {
            match name.as_str() {
                // 已是新布局或与素材无关的目录，保持不动
                "inputs" | "work" | "outputs" | "models" | ".trash" | "logs" | "node_modules"
                | ".venv" | "venv" => continue,
                "experiments" => plan_experiments(root, &path, &mut ops)?,
                "separated" => ops.push(PlanOp {
                    from: path,
                    to: root.join("work").join("datasets"),
                }),
                "converted" | "mixed" => ops.push(PlanOp {
                    from: path,
                    to: root.join("outputs").join("draft"),
                }),
                "uvr_models" => ops.push(PlanOp {
                    from: path,
                    to: root.join("work").join("cache").join("uvr"),
                }),
                "trainsets" => ops.push(PlanOp {
                    from: path,
                    to: root.join("work").join("cache").join("trainsets"),
                }),
                _ if name.starts_with('.') => continue,
                _ if dir_has_audio(&path) => ops.push(PlanOp {
                    from: path,
                    to: root.join("inputs").join("training").join(&name),
                }),
                _ => continue,
            }
        } else if has_audio_ext(&path) {
            ops.push(PlanOp {
                from: path,
                to: root.join("inputs").join("raw").join(&name),
            });
        }
    }

    Ok(ops)
}

fn layout_status(root: &Path) -> LayoutStatus {
    let layout = WorkspaceLayout::detect(root);
    let file = crate::workspace_layout::read_layout_file(root);
    LayoutStatus {
        version: if layout.is_v2() { 2 } else { 1 },
        is_v2: layout.is_v2(),
        workspace: root.to_string_lossy().to_string(),
        migrated_at: file.as_ref().map(|f| f.migrated_at),
        mapping_count: file.map(|f| f.mapping.len()).unwrap_or(0),
    }
}

fn preview_locked(root: &Path, busy: bool) -> MigrationPreview {
    let layout = WorkspaceLayout::detect(root);
    let mut blockers = Vec::new();
    if busy {
        blockers.push("有任务正在运行，请等待完成后再迁移".to_string());
    }
    if layout.is_v2() {
        blockers.push("当前已是新布局（v2），无需迁移".to_string());
        return MigrationPreview {
            version: 2,
            items: Vec::new(),
            total_bytes: 0,
            blockers,
        };
    }

    let plan = match build_plan(root) {
        Ok(plan) => plan,
        Err(e) => {
            blockers.push(e);
            Vec::new()
        }
    };
    let items: Vec<MigrationItem> = plan
        .iter()
        .map(|op| MigrationItem {
            from: rel(root, &op.from),
            to: rel(root, &op.to),
            bytes: bytes_of(&op.from),
        })
        .collect();
    let total_bytes = items.iter().map(|item| item.bytes).sum();

    MigrationPreview {
        version: 1,
        items,
        total_bytes,
        blockers,
    }
}

/// 目标已存在时生成不冲突的名字（文件按 `-2/-3`，目录追加同样后缀）。
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let parent = path.parent().unwrap_or(Path::new(""));
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "item".to_string());
    let (stem, ext) = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) if !path.is_dir() => (
            name.trim_end_matches(&format!(".{}", ext)).to_string(),
            format!(".{}", ext),
        ),
        _ => (name, String::new()),
    };
    for n in 2.. {
        let candidate = parent.join(format!("{}-{}{}", stem, n, ext));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!()
}

fn move_one(src: &Path, dst: &Path, done: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("创建目标目录失败 {}: {}", parent.display(), e))?;
    }
    fs::rename(src, dst).map_err(|e| {
        format!(
            "移动失败 {} → {}: {}",
            src.display(),
            dst.display(),
            e
        )
    })?;
    done.push((src.to_path_buf(), dst.to_path_buf()));
    Ok(())
}

/// 移动文件或目录；目标已存在且双方都是目录时递归合并。
fn move_path(src: &Path, dst: &Path, done: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), String> {
    if !src.exists() {
        return Ok(());
    }
    if dst.exists() && src.is_dir() && dst.is_dir() {
        let entries = fs::read_dir(src).map_err(|e| format!("读取目录失败 {}: {}", src.display(), e))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("读取目录项失败: {}", e))?;
            move_path(&entry.path(), &dst.join(entry.file_name()), done)?;
        }
        let _ = fs::remove_dir(src);
        return Ok(());
    }
    let target = if dst.exists() { unique_path(dst) } else { dst.to_path_buf() };
    move_one(src, &target, done)
}

fn rollback(done: &[(PathBuf, PathBuf)]) {
    for (src, dst) in done.iter().rev() {
        if !dst.exists() {
            continue;
        }
        if let Some(parent) = src.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if fs::rename(dst, src).is_err() {
            eprintln!(
                "[layout] 回滚失败，需手工恢复：{} → {}",
                dst.display(),
                src.display()
            );
        }
    }
}

fn write_marker(root: &Path, mapping: Vec<MigrationEntry>) -> Result<(), String> {
    let file = LayoutFile {
        version: LAYOUT_SCHEMA_VERSION,
        migrated_at: run_manifest::now_ts(),
        mapping,
    };
    write_layout_file(root, &file).map_err(|e| format!("写入 layout.json 失败: {}", e))
}

fn execute(root: &Path) -> Result<MigrationResult, String> {
    if WorkspaceLayout::detect(root).is_v2() {
        return Err("当前已是新布局（v2），无需迁移".to_string());
    }

    let plan = build_plan(root)?;
    let total_bytes: u64 = plan.iter().map(|op| bytes_of(&op.from)).sum();
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();

    for op in &plan {
        if let Err(e) = move_path(&op.from, &op.to, &mut done) {
            rollback(&done);
            return Err(format!("迁移失败，已回滚：{}", e));
        }
    }

    let mapping: Vec<MigrationEntry> = plan
        .iter()
        .map(|op| MigrationEntry {
            from: rel(root, &op.from),
            to: rel(root, &op.to),
        })
        .collect();
    if let Err(e) = write_marker(root, mapping) {
        rollback(&done);
        return Err(format!("{}，已回滚", e));
    }

    // 四层目录一次建齐，让"进/算/出/留"在文件管理器里一眼可见
    for dir in [
        "inputs/raw",
        "inputs/training",
        "work/runs",
        "work/datasets",
        "work/cache",
        "outputs/draft",
        "outputs/final",
    ] {
        let _ = fs::create_dir_all(root.join(dir));
    }

    Ok(MigrationResult {
        moved: done.len(),
        total_bytes,
    })
}

#[tauri::command]
pub async fn get_layout_status(state: State<'_, AppState>) -> Result<LayoutStatus, String> {
    let root = state.workspace_root.read().await.clone();
    Ok(layout_status(&root))
}

#[tauri::command]
pub async fn preview_layout_migration(
    state: State<'_, AppState>,
) -> Result<MigrationPreview, String> {
    let root = state.workspace_root.read().await.clone();
    let busy = state.active_heavy_task.lock().await.is_some();
    tokio::task::spawn_blocking(move || preview_locked(&root, busy))
        .await
        .map_err(|e| format!("预览迁移失败: {}", e))
}

#[tauri::command]
pub async fn migrate_layout(state: State<'_, AppState>) -> Result<MigrationResult, String> {
    if let Some(task) = state.active_heavy_task.lock().await.clone() {
        return Err(format!(
            "系统当前有任务正在运行：[{}]，请等待完成后再迁移",
            task
        ));
    }
    let root = state.workspace_root.read().await.clone();
    tokio::task::spawn_blocking(move || execute(&root))
        .await
        .map_err(|e| format!("迁移任务执行失败: {}", e))?
}
