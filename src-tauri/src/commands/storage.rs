//! 清理中心：分区体积统计与全局 R1 回收。
//!
//! 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §4.1、§6.4。
//! 只有 R1 允许一键回收；R1'/R2 需要用户在批次维度显式确认，
//! `datasets`/`inputs`/`assets` 永不自动清理。

use crate::cleanup;
use crate::run_manifest;
use crate::state::AppState;
use crate::workspace_layout::{bytes_of, OutputKind, WorkspaceLayout};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Serialize)]
pub struct StorageZone {
    pub key: String,
    pub label: String,
    pub bytes: u64,
    pub note: String,
}

#[derive(Debug, Serialize)]
pub struct StorageReport {
    pub workspace: String,
    pub zones: Vec<StorageZone>,
    /// 一键"释放空间"能立刻拿回的字节数（R1）
    pub reclaimable_now: u64,
}

#[derive(Debug, Serialize)]
pub struct ReclaimAllResponse {
    pub freed_bytes: u64,
    pub batches: u32,
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .map(|entry| entry.path())
        .collect()
}

fn dir_name(path: &Path) -> Option<String> {
    path.file_name().map(|n| n.to_string_lossy().to_string())
}

/// 批次工作位中的 R1 / R1' / R2 体积。
fn work_dir_zones(layout: &WorkspaceLayout) -> (u64, u64, u64) {
    let mut r1 = 0u64;
    let mut r1p = 0u64;
    let mut r2 = 0u64;

    for voice in subdirs(&layout.runs_root()) {
        for run in subdirs(&voice) {
            let Some(name) = dir_name(&run) else { continue };
            if !run_manifest::is_valid_run_id(&name) {
                continue;
            }
            r1 += bytes_of(&run.join("1_16k_wavs")) + bytes_of(&run.join("eval"));
            if let Ok(entries) = fs::read_dir(&run) {
                for entry in entries.flatten() {
                    let item = entry.file_name().to_string_lossy().to_string();
                    if item.starts_with("events.out.tfevents") {
                        r1 += bytes_of(&entry.path());
                    }
                    if (item.starts_with("G_") || item.starts_with("D_")) && item.ends_with(".pth") {
                        r1p += bytes_of(&entry.path());
                    }
                }
            }
            // 3_feature768 在索引完成后已由 finalize_index 自动回收，
            // 未完成索引时删除会失去建索引能力，故不计入可回收体积
            r2 += bytes_of(&run.join("0_gt_wavs"))
                + bytes_of(&run.join("2a_f0"))
                + bytes_of(&run.join("2b-f0nsf"));
        }
    }

    (r1, r1p, r2)
}

/// 没有宿主音频的 `.stft.npy`（直播相位模板缓存）体积。
fn orphan_stft_bytes(layout: &WorkspaceLayout) -> u64 {
    const AUDIO_EXTS: [&str; 5] = ["wav", "mp3", "flac", "m4a", "ogg"];
    const SUFFIX: &str = ".stft.npy";

    let mut roots = vec![
        layout.output_dir(OutputKind::Vocal),
        layout.output_dir(OutputKind::Mixed),
        layout.final_output_dir(),
    ];
    roots.sort();
    roots.dedup();

    let mut total = 0u64;
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
            let Some(stem) = name.strip_suffix(SUFFIX) else {
                continue;
            };
            let dir = entry.path().parent().unwrap_or(Path::new(""));
            let host_exists = AUDIO_EXTS
                .iter()
                .any(|ext| dir.join(format!("{}.{}", stem, ext)).exists());
            if !host_exists {
                total += bytes_of(entry.path());
            }
        }
    }
    total
}

/// 模型资产：各声音的归档权重与批次清单。
fn assets_bytes(layout: &WorkspaceLayout) -> u64 {
    subdirs(&layout.assets_root())
        .iter()
        .map(|voice| {
            bytes_of(&voice.join("weights")) + bytes_of(&voice.join("runs"))
        })
        .sum()
}

fn build_report(root: &Path) -> StorageReport {
    let layout = WorkspaceLayout::detect(root);
    let (r1, r1p, r2) = work_dir_zones(&layout);
    let stale_stft = orphan_stft_bytes(&layout);

    let zones = vec![
        StorageZone {
            key: "r1".to_string(),
            label: "可立即回收".to_string(),
            bytes: r1 + stale_stft,
            note: "1_16k_wavs、eval、TensorBoard 事件与孤立 STFT 缓存，删除没有代价".to_string(),
        },
        StorageZone {
            key: "r1p".to_string(),
            label: "回收后不能继续训练".to_string(),
            bytes: r1p,
            note: "G_/D_ 权重必须成对保留；回收后该批次只能用于转换".to_string(),
        },
        StorageZone {
            key: "r2".to_string(),
            label: "重算可再生".to_string(),
            bytes: r2,
            note: "预处理与特征产物，需要时可重跑（分钟级）".to_string(),
        },
        StorageZone {
            key: "datasets".to_string(),
            label: "分离数据（不会自动清理）".to_string(),
            bytes: bytes_of(&layout.datasets_root()),
            note: "人声/伴奏分离结果，重跑 UVR 为小时级".to_string(),
        },
        StorageZone {
            key: "assets".to_string(),
            label: "模型资产（永不自动清理）".to_string(),
            bytes: assets_bytes(&layout),
            note: "归档权重、索引与批次清单".to_string(),
        },
        StorageZone {
            key: "trash".to_string(),
            label: "回收站（7 天后自动删除）".to_string(),
            bytes: bytes_of(&layout.trash_dir()),
            note: "所有删除先移入此处，可手工恢复".to_string(),
        },
    ];

    StorageReport {
        workspace: root.to_string_lossy().to_string(),
        zones,
        reclaimable_now: r1 + stale_stft,
    }
}

/// 对所有批次执行 R1 回收；返回释放量与受影响批次数，数据库记账由调用方完成。
fn reclaim_all_locked(root: &Path) -> (u64, u32, Vec<String>) {
    let layout = WorkspaceLayout::detect(root);
    let now = run_manifest::now_ts();
    let mut freed_total = 0u64;
    let mut batches = 0u32;
    let mut touched = Vec::new();

    for voice in subdirs(&layout.runs_root()) {
        let Some(voice_name) = dir_name(&voice) else { continue };
        for run in subdirs(&voice) {
            let Some(run_id) = dir_name(&run) else { continue };
            if !run_manifest::is_valid_run_id(&run_id) {
                continue;
            }
            let manifest_path = layout
                .run_manifests_dir(&voice_name)
                .join(format!("{}.json", run_id));
            let Ok(mut manifest) = run_manifest::read_manifest(&manifest_path) else {
                continue;
            };

            let freed = cleanup::reclaim_work_dir(&layout, &run_id, &run, false);
            if freed == 0 {
                continue;
            }
            manifest.reclaimable.bytes = manifest.reclaimable.bytes.saturating_add(freed);
            manifest.reclaimable.reclaimed_at = Some(now);
            if let Err(e) = run_manifest::write_manifest(&manifest_path, &manifest) {
                eprintln!("[storage] 写入批次清单失败 {}: {}", manifest_path.display(), e);
            }
            freed_total += freed;
            batches += 1;
            touched.push(run_id);
        }
    }

    (freed_total, batches, touched)
}

#[tauri::command]
pub async fn get_storage_report(state: State<'_, AppState>) -> Result<StorageReport, String> {
    let root = state.workspace_root.read().await.clone();
    tokio::task::spawn_blocking(move || build_report(&root))
        .await
        .map_err(|e| format!("统计存储占用失败: {}", e))
}

#[tauri::command]
pub async fn reclaim_all_runs(state: State<'_, AppState>) -> Result<ReclaimAllResponse, String> {
    if let Some(task) = state.active_heavy_task.lock().await.clone() {
        return Err(format!(
            "系统当前有任务正在运行：[{}]，请等待完成后再回收",
            task
        ));
    }

    let root = state.workspace_root.read().await.clone();
    let (freed_bytes, batches, touched) = tokio::task::spawn_blocking(move || reclaim_all_locked(&root))
        .await
        .map_err(|e| format!("回收任务执行失败: {}", e))?;

    let now = run_manifest::now_ts();
    for run_id in touched {
        if let Err(e) = state.db.mark_run_reclaimed(run_id, now).await {
            eprintln!("[storage] 登记回收时间失败: {}", e);
        }
    }

    Ok(ReclaimAllResponse {
        freed_bytes,
        batches,
    })
}
