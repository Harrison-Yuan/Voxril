use crate::cleanup;
use crate::run_manifest::{self, StageStatus};
use crate::state::AppState;
use crate::utils;
use crate::workspace_layout::{
    parse_run_target, OutputKind, RunTarget, WorkspaceLayout,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::State;

#[derive(Debug, Serialize, Deserialize)]
pub struct FileExistsResponse {
    pub exists: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct GlobFilesResponse {
    pub files: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioFile {
    pub name: String,
    pub path: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioFilesResponse {
    pub files: Vec<AudioFile>,
}

#[tauri::command]
pub async fn file_exists(path: String) -> Result<FileExistsResponse, String> {
    let path = PathBuf::from(path);
    Ok(FileExistsResponse {
        exists: path.exists(),
    })
}

#[tauri::command]
pub async fn list_artifacts(
    dir: String,
    kind: String,
    _state: State<'_, AppState>,
) -> Result<GlobFilesResponse, String> {
    let dir_path = PathBuf::from(dir);
    let files = utils::glob_files(&dir_path, &kind)
        .map_err(|e| format!("Failed to glob files: {}", e))?;

    Ok(GlobFilesResponse {
        files: files
            .into_iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect(),
    })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CheckpointIterationResponse {
    pub iteration: i32,
}

#[tauri::command]
pub async fn get_checkpoint_iteration(path: String) -> Result<CheckpointIterationResponse, String> {
    let re = regex::Regex::new(r"G_(\d+)\.pth$").map_err(|e| e.to_string())?;
    
    if let Some(caps) = re.captures(&path) {
        if let Some(iter_str) = caps.get(1) {
            let iteration = iter_str.as_str().parse::<i32>().unwrap_or(0);
            return Ok(CheckpointIterationResponse { iteration });
        }
    }
    
    Ok(CheckpointIterationResponse { iteration: 0 })
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ReadTextResponse {
    pub content: String,
}

#[tauri::command]
pub async fn read_text_file(path: String) -> Result<ReadTextResponse, String> {
    let content = tokio::fs::read_to_string(&path)
        .await
        .map_err(|e| format!("Failed to read file: {}", e))?;
    
    Ok(ReadTextResponse { content })
}

#[tauri::command]
pub async fn list_audio_files(dir: String) -> Result<AudioFilesResponse, String> {
    collect_audio_files(&PathBuf::from(dir))
}

/// 列出数据集中的某一类音频（`kind` 只能是 `vocals` 或 `instrumental`）。
/// 目录由后端按工作区布局解析，前端不再自行拼接路径。
#[tauri::command]
pub async fn list_dataset_audio(
    kind: String,
    state: State<'_, AppState>,
) -> Result<AudioFilesResponse, String> {
    if kind != "vocals" && kind != "instrumental" {
        return Err(format!("未知的数据集类别: {}", kind));
    }
    let workspace_root = state.workspace_root.read().await;
    let dir = crate::workspace_layout::WorkspaceLayout::detect(&*workspace_root)
        .datasets_root()
        .join(&kind);
    collect_audio_files(&dir)
}

fn collect_audio_files(dir_path: &std::path::Path) -> Result<AudioFilesResponse, String> {
    let extensions = ["wav", "mp3", "flac", "m4a", "ogg", "aac"];

    let files = utils::collect_files_with_extension(dir_path, &extensions)
        .map_err(|e| format!("Failed to list audio files: {}", e))?;

    Ok(AudioFilesResponse {
        files: files
            .into_iter()
            .map(|p| AudioFile {
                name: p.file_name().unwrap().to_string_lossy().to_string(),
                path: p.to_string_lossy().to_string(),
            })
            .collect(),
    })
}

#[tauri::command]
pub async fn open_in_finder(path: String) -> Result<(), String> {
    crate::platform::open_path(&path)
        .map_err(|e| format!("Failed to open in Finder: {}", e))
}

#[tauri::command]
pub async fn open_in_vscode(path: String) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    {
        crate::platform::quiet_command("cmd")
            .args(["/C", "code", &path])
            .spawn()
            .map_err(|e| format!("Failed to open in VS Code: {}", e))?;
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::process::Command::new("code")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open in VS Code: {}", e))?;
    }

    Ok(())
}

// ───────────────────────── 批次删除语义 ─────────────────────────
//
// 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §4.3、§4.4。
// 所有删除都先移入 `.trash/<run_id>/`，7 天后由启动清理过期删除；
// 磁盘上的 run.json 是唯一真源，清单缺失时拒绝任何删除操作。

/// 已建批次的工作位限定名解析结果。
struct BatchTarget {
    voice: String,
    run_id: String,
}

impl BatchTarget {
    /// 只接受 `<voice>/<run_id>`；尚未建批或旧布局工作位都拒绝。
    fn parse(exp_dir: &str) -> Result<Self, String> {
        match parse_run_target(exp_dir)? {
            RunTarget::Batch { voice, run_id } => Ok(Self { voice, run_id }),
            RunTarget::New { .. } => {
                Err("该声音尚未建立训练批次，请先完成数据预处理".to_string())
            }
            RunTarget::Legacy => Err(format!(
                "旧布局工作位不支持批次操作：{}（请先完成工作区迁移）",
                exp_dir
            )),
        }
    }

    fn manifest_path(&self, layout: &WorkspaceLayout) -> PathBuf {
        layout
            .run_manifests_dir(&self.voice)
            .join(format!("{}.json", self.run_id))
    }
}

struct BatchContext {
    layout: WorkspaceLayout,
    target: BatchTarget,
    work_dir: PathBuf,
    manifest_path: PathBuf,
    manifest: run_manifest::RunManifest,
}

/// 重任务运行期间全局禁用删除，避免训练中途删除产物（历史事故点）。
async fn ensure_idle(state: &State<'_, AppState>) -> Result<(), String> {
    match state.active_heavy_task.lock().await.clone() {
        Some(task) => Err(format!(
            "系统当前有任务正在运行：[{}]，请等待完成后再执行删除或回收。",
            task
        )),
        None => Ok(()),
    }
}

async fn load_batch(state: &State<'_, AppState>, exp_dir: &str) -> Result<BatchContext, String> {
    let target = BatchTarget::parse(exp_dir)?;
    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);
    let work_dir = layout.run_path(exp_dir);
    if !work_dir.is_dir() {
        return Err(format!("批次工作位不存在：{}", work_dir.display()));
    }
    let manifest_path = target.manifest_path(&layout);
    let manifest = run_manifest::read_manifest(&manifest_path)
        .map_err(|e| format!("批次清单不可用，拒绝操作：{}", e))?;
    Ok(BatchContext {
        layout,
        target,
        work_dir,
        manifest_path,
        manifest,
    })
}

#[derive(Debug, Serialize)]
pub struct ReclaimRunResponse {
    /// 本次移入回收站的字节数
    pub freed_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct ResetExperimentResponse {
    pub freed_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct PurgeRunResponse {
    pub freed_bytes: u64,
}

#[derive(Debug, Serialize)]
pub struct RemoveArtifactsResponse {
    pub removed: u32,
    pub freed_bytes: u64,
}

/// 回收批次的一次性产物：默认 R1，`deep` 时追加 R2 与 G_/D_ 权重。
#[tauri::command]
pub async fn reclaim_run(
    exp_dir: String,
    deep: bool,
    state: State<'_, AppState>,
) -> Result<ReclaimRunResponse, String> {
    ensure_idle(&state).await?;
    let mut batch = load_batch(&state, &exp_dir).await?;

    let freed = cleanup::reclaim_work_dir(
        &batch.layout,
        &batch.target.run_id,
        &batch.work_dir,
        deep,
    );
    if freed == 0 {
        return Ok(ReclaimRunResponse { freed_bytes: 0 });
    }

    // 深度回收会带走预处理/F0 产物，阶段改记"已回收"而非"需重跑"
    if deep {
        let gone = |name: &str| !batch.work_dir.join(name).exists();
        if gone("0_gt_wavs") {
            batch.manifest.stages.preprocess.status = StageStatus::Reclaimed;
        }
        if gone("2a_f0") && gone("2b-f0nsf") {
            batch.manifest.stages.f0.status = StageStatus::Reclaimed;
        }
    }

    let now = run_manifest::now_ts();
    batch.manifest.reclaimable.bytes = batch.manifest.reclaimable.bytes.saturating_add(freed);
    batch.manifest.reclaimable.reclaimed_at = Some(now);
    run_manifest::write_manifest(&batch.manifest_path, &batch.manifest)
        .map_err(|e| format!("写入批次清单失败：{}", e))?;
    if let Err(e) = state
        .db
        .mark_run_reclaimed(batch.target.run_id.clone(), now)
        .await
    {
        eprintln!("[file] 登记回收时间失败: {}", e);
    }
    Ok(ReclaimRunResponse { freed_bytes: freed })
}

/// 重置批次工作位：清空全部产物与阶段记录，保留归档权重与批次清单。
#[tauri::command]
pub async fn reset_experiment(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<ResetExperimentResponse, String> {
    ensure_idle(&state).await?;
    let mut batch = load_batch(&state, &exp_dir).await?;

    let mut freed = 0u64;
    let entries = std::fs::read_dir(&batch.work_dir)
        .map_err(|e| format!("读取批次目录失败：{}", e))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        // run.json 与归档权重不属于工作位产物，永不随重置删除
        if name == "run.json" || name == "weights" {
            continue;
        }
        freed += cleanup::move_to_trash(&batch.layout, &batch.target.run_id, &entry.path());
    }

    batch.manifest.stages = Default::default();
    batch.manifest.params = None;
    if freed > 0 {
        let now = run_manifest::now_ts();
        batch.manifest.reclaimable.bytes = batch.manifest.reclaimable.bytes.saturating_add(freed);
        batch.manifest.reclaimable.reclaimed_at = Some(now);
    }
    run_manifest::write_manifest(&batch.manifest_path, &batch.manifest)
        .map_err(|e| format!("写入批次清单失败：{}", e))?;
    Ok(ResetExperimentResponse { freed_bytes: freed })
}

/// 删除历史批次：归档权重、工作位、批次清单一并移入回收站。
#[tauri::command]
pub async fn purge_run(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<PurgeRunResponse, String> {
    ensure_idle(&state).await?;
    let batch = load_batch(&state, &exp_dir).await?;

    // 保护规则：被成品引用的批次不得删除，避免成品失去来源
    let referenced = find_output_references(&batch.layout, &batch.target.run_id);
    if !referenced.is_empty() {
        return Err(format!(
            "批次 {} 仍被 {} 个成品引用，无法删除：{}",
            batch.target.run_id,
            referenced.len(),
            referenced.join("、")
        ));
    }

    let mut freed = 0u64;
    // 归档名固定为 `<voice>-<run_id>-e<epoch>.<ext>`；用完整前缀匹配，
    // 避免 `261122-0156` 误伤 `261122-0156-2` 这类同分钟冲突批次。
    let asset_prefix = format!("{}-{}-e", batch.target.voice, batch.target.run_id);
    if let Ok(entries) = std::fs::read_dir(batch.layout.weights_dir(&batch.target.voice)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&asset_prefix) {
                freed +=
                    cleanup::move_to_trash(&batch.layout, &batch.target.run_id, &entry.path());
            }
        }
    }
    freed += cleanup::move_to_trash(&batch.layout, &batch.target.run_id, &batch.work_dir);
    freed += cleanup::move_to_trash(&batch.layout, &batch.target.run_id, &batch.manifest_path);

    if let Err(e) = state.db.delete_run(batch.target.run_id.clone()).await {
        eprintln!("[file] 删除批次索引失败: {}", e);
    }
    Ok(PurgeRunResponse { freed_bytes: freed })
}

/// 重建索引前清掉工作位里的旧索引及 sidecar：n_ivf 随特征数量变化，旧文件名会残留。
#[tauri::command]
pub async fn remove_index_artifacts(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<RemoveArtifactsResponse, String> {
    ensure_idle(&state).await?;
    let batch = load_batch(&state, &exp_dir).await?;

    let mut indexes =
        utils::glob_files(&batch.work_dir.join("checkpoints"), "index").unwrap_or_default();
    if indexes.is_empty() {
        indexes = utils::glob_files(&batch.work_dir, "index").unwrap_or_default();
    }

    let mut removed = 0u32;
    let mut freed = 0u64;
    for index in indexes {
        for suffix in ["", ".vectors.bin", ".meta.txt", ".ivf.bin"] {
            let path = if suffix.is_empty() {
                index.clone()
            } else {
                PathBuf::from(format!("{}{}", index.display(), suffix))
            };
            freed += cleanup::move_to_trash(&batch.layout, &batch.target.run_id, &path);
            if suffix.is_empty() && !path.exists() {
                removed += 1;
            }
        }
    }
    Ok(RemoveArtifactsResponse {
        removed,
        freed_bytes: freed,
    })
}

/// 在 outputs 下查找引用了该批次的成品文件（保护规则 §4.4-1）。
fn find_output_references(layout: &WorkspaceLayout, run_id: &str) -> Vec<String> {
    let mut roots = vec![
        layout.output_dir(OutputKind::Vocal),
        layout.output_dir(OutputKind::Mixed),
        layout.final_output_dir(),
    ];
    roots.sort();
    roots.dedup();

    let mut hits = Vec::new();
    for root in roots {
        for entry in walkdir::WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .flatten()
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.contains(run_id) {
                hits.push(name);
            }
        }
    }
    hits
}
