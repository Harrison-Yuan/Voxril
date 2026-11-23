use crate::cleanup::{move_to_trash, reclaim_work_dir};
use crate::python::PythonExecutor;
use crate::run_manifest::{
    self, AssetRef, DatasetInfo, RunManifest, StageInfo, StageStatus, TrainParams,
};
use crate::state::AppState;
use crate::utils::kill_orphan_multiprocessing;
use crate::workspace_layout::{
    hardlink_or_copy, parse_run_target, RunTarget, WorkspaceLayout,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::State;

/// 批次工作位路径。布局版本由 workspace/layout.json 决定（v1: experiments/，v2: work/runs/）。
fn run_path_for(workspace_root: &Path, exp_dir: &str) -> std::path::PathBuf {
    WorkspaceLayout::detect(workspace_root).run_path(exp_dir)
}

// ───────────────────────── 批次（run）生命周期 ─────────────────────────
//
// 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §3、§4。
// 磁盘上的 `<资产目录>/<voice>/runs/<run_id>.json` 是唯一真源，数据库 runs 表只是查询索引。

fn manifest_file(layout: &WorkspaceLayout, voice: &str, run_id: &str) -> PathBuf {
    layout
        .run_manifests_dir(voice)
        .join(format!("{}.json", run_id))
}

/// 某声音最新的合法批次号（字典序即时间序）；旧布局版本目录（`v2_40k`）不算批次。
fn latest_run_id(layout: &WorkspaceLayout, voice: &str) -> Option<String> {
    let entries = std::fs::read_dir(layout.runs_root().join(voice)).ok()?;
    entries
        .flatten()
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .filter_map(|e| e.file_name().to_str().map(|s| s.to_string()))
        .filter(|name| run_manifest::is_valid_run_id(name))
        .max()
}

/// 在全局范围内分配未占用的 run_id（跨声音唯一，因为 runs 表以 run_id 为主键）。
fn alloc_run_id(layout: &WorkspaceLayout) -> String {
    let mut taken: Vec<String> = Vec::new();

    if let Ok(voices) = std::fs::read_dir(layout.runs_root()) {
        for voice_dir in voices.flatten() {
            if !voice_dir.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            if let Ok(runs) = std::fs::read_dir(voice_dir.path()) {
                for run in runs.flatten() {
                    if let Some(name) = run.file_name().to_str() {
                        taken.push(name.to_string());
                    }
                }
            }
        }
    }
    if let Ok(voices) = std::fs::read_dir(layout.assets_root()) {
        for voice_dir in voices.flatten() {
            if let Ok(runs) = std::fs::read_dir(voice_dir.path().join("runs")) {
                for run in runs.flatten() {
                    if let Some(name) = run.file_name().to_str() {
                        taken.push(name.trim_end_matches(".json").to_string());
                    }
                }
            }
        }
    }

    run_manifest::unique_run_id(&taken, chrono::Local::now())
}

/// 相对工作区根展示路径；不在工作区内时保留绝对路径。
fn relativize(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

/// 数据集内的音频文件数（递归），作为 run.json 的 splits 字段。
fn count_audio_files(dir: &Path) -> u32 {
    const AUDIO_EXTS: [&str; 6] = ["wav", "mp3", "flac", "m4a", "ogg", "aac"];
    let mut count = 0u32;
    for item in walkdir::WalkDir::new(dir).follow_links(false).into_iter().flatten() {
        if !item.file_type().is_file() {
            continue;
        }
        let ext = item
            .path()
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if AUDIO_EXTS.contains(&ext.as_str()) {
            count += 1;
        }
    }
    count
}

/// 目录下是否存在指定扩展名的文件（只看一层，阶段产物都直接落在该目录）。
fn dir_has_ext(dir: &Path, ext: &str) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|e| {
        e.file_type().map(|t| t.is_file()).unwrap_or(false)
            && e.path().extension().and_then(|s| s.to_str()) == Some(ext)
    })
}

fn file_len(path: &Path) -> u64 {
    std::fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

/// 列出批次工作位里的主索引（先 checkpoints/，再工作位根，避免把 sidecar 当索引）。
fn find_index_files(work_dir: &Path) -> Vec<PathBuf> {
    let mut found = crate::utils::glob_files(&work_dir.join("checkpoints"), "index").unwrap_or_default();
    if found.is_empty() {
        found = crate::utils::glob_files(work_dir, "index").unwrap_or_default();
    }
    found
}

/// 从 `.meta.txt` 第一行读取向量总数（train_index 生成）。
fn read_ntotal(index: &Path) -> Option<u64> {
    let sidecar = PathBuf::from(format!("{}.meta.txt", index.display()));
    let content = std::fs::read_to_string(sidecar).ok()?;
    content.lines().next()?.trim().parse().ok()
}

/// 从 `added_IVF{n}_Flat_...` 文件名里读取聚类数。
fn parse_n_ivf(index: &Path) -> Option<u32> {
    let name = index.file_name()?.to_str()?;
    let rest = name.split_once("IVF")?.1;
    let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// 取最后一次启动分隔符之后的日志段，避免读到上一轮训练的残留输出。
fn last_launch_segment(log: &str) -> &str {
    const MARKER: &str = "[backend] training process launching...";
    match log.rfind(MARKER) {
        Some(pos) => &log[pos..],
        None => log,
    }
}

fn read_config_epochs(work_dir: &Path) -> Option<i32> {
    let content = std::fs::read_to_string(work_dir.join("config.json")).ok()?;
    let cfg: serde_json::Value = serde_json::from_str(&content).ok()?;
    cfg["train"]["epochs"].as_i64().map(|v| v as i32)
}

/// 训练完成的硬判据：末轮写出 `saving final ckpt`，且 `model.pth` 已落盘。
/// 返回最终 epoch；未完成时返回 None。detached 进程的退出码不可观测，因此不看 exit code。
fn final_train_epoch(work_dir: &Path) -> Option<i32> {
    let log = std::fs::read_to_string(work_dir.join("train.log")).ok()?;
    let segment = last_launch_segment(&log);
    if !segment.contains("saving final ckpt") || !work_dir.join("model.pth").exists() {
        return None;
    }
    let epoch = segment.lines().rev().find_map(|line| {
        let idx = line.find("Epoch:")?;
        line[idx + "Epoch:".len()..]
            .split_whitespace()
            .next()?
            .parse::<i32>()
            .ok()
    });
    epoch.or_else(|| read_config_epochs(work_dir))
}

/// torch 存档判据：1.6+ 写 zip（`PK\x03\x04`），更早写 pickle（`\x80\x02`~`\x80\x05`）。
/// 用于开训前识别空文件/半截下载，避免 Python 侧加载失败后静默从头训练。
fn looks_like_torch_save(path: &Path) -> bool {
    use std::io::Read;
    if std::fs::metadata(path).map(|m| m.len()).unwrap_or(0) < 4 {
        return false;
    }
    let mut buf = [0u8; 4];
    if std::fs::File::open(path)
        .and_then(|mut f| f.read_exact(&mut buf))
        .is_err()
    {
        return false;
    }
    buf == *b"PK\x03\x04" || (buf[0] == 0x80 && (2..=5).contains(&buf[1]))
}

/// 创建批次：建工作位目录、写 run.json。数据集指纹在此一次性计算。
fn create_batch(
    workspace_root: &Path,
    project_root: &Path,
    layout: &WorkspaceLayout,
    voice: &str,
    sr: &str,
    if_f0: bool,
    trainset_dir: &Path,
) -> Result<String, String> {
    let fingerprint = run_manifest::fingerprint_dir(trainset_dir)
        .map_err(|e| format!("计算数据集指纹失败：{}", e))?;
    let run_id = alloc_run_id(layout);
    let work_dir = layout.run_path(&format!("{}/{}", voice, run_id));

    let dataset = DatasetInfo {
        source: relativize(workspace_root, trainset_dir),
        fingerprint,
        splits: count_audio_files(trainset_dir),
        total_seconds: None,
    };
    let mut manifest = RunManifest::new(
        run_id.clone(),
        voice,
        sr,
        "v2",
        if_f0,
        run_manifest::pipeline_version(project_root),
        dataset,
        None,
    );
    manifest.stages.preprocess = StageInfo {
        status: StageStatus::Running,
        started_at: Some(run_manifest::now_ts()),
        ..Default::default()
    };

    std::fs::create_dir_all(&work_dir)
        .map_err(|e| format!("创建批次目录失败：{}", e))?;
    if let Err(e) = run_manifest::write_manifest(&manifest_file(layout, voice, &run_id), &manifest) {
        let _ = std::fs::remove_dir_all(&work_dir);
        return Err(format!("写入批次清单失败：{}", e));
    }

    Ok(run_id)
}

/// 复用已有批次重跑预处理：校验采样率/F0 一致，刷新数据集指纹并让受影响阶段失效。
fn prepare_existing_batch(
    workspace_root: &Path,
    project_root: &Path,
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    sr: &str,
    if_f0: bool,
    trainset_dir: &Path,
) -> Result<(), String> {
    let path = manifest_file(layout, voice, run_id);
    let mut manifest =
        run_manifest::read_manifest(&path).map_err(|e| format!("读取批次清单失败：{}", e))?;

    if manifest.sr != sr || manifest.if_f0 != if_f0 {
        return Err(format!(
            "批次 {} 的采样率（{}）/ F0（{}）与当前配置（{} / {}）不一致，请新建批次。",
            run_id, manifest.sr, manifest.if_f0, sr, if_f0
        ));
    }

    let fingerprint = run_manifest::fingerprint_dir(trainset_dir)
        .map_err(|e| format!("计算数据集指纹失败：{}", e))?;
    let pipeline_now = run_manifest::pipeline_version(project_root);
    // 先按旧指纹比对：数据集或管线变了，已完成阶段立即降级为 stale
    manifest.invalidate_if_changed(&fingerprint, &pipeline_now);

    manifest.dataset = DatasetInfo {
        source: relativize(workspace_root, trainset_dir),
        fingerprint,
        splits: count_audio_files(trainset_dir),
        total_seconds: manifest.dataset.total_seconds,
    };
    manifest.pipeline_version = pipeline_now;
    manifest.stages.preprocess = StageInfo {
        status: StageStatus::Running,
        started_at: Some(run_manifest::now_ts()),
        ..Default::default()
    };

    run_manifest::write_manifest(&path, &manifest)
        .map_err(|e| format!("写入批次清单失败：{}", e))
}

/// 预处理进程启动失败时把批次标记为 Failed，避免 UI 永远停在 Running。
fn mark_preprocess_failed(layout: &WorkspaceLayout, voice: &str, run_id: &str) {
    let path = manifest_file(layout, voice, run_id);
    let Ok(mut manifest) = run_manifest::read_manifest(&path) else {
        return;
    };
    manifest.stages.preprocess.status = StageStatus::Failed;
    if let Err(e) = run_manifest::write_manifest(&path, &manifest) {
        eprintln!("[train] 写入批次清单失败: {}", e);
    }
}

/// 启动外部进程前把阶段标记为 Running：页面重进时能显示"进行中"，
/// 进程异常退出则由 `demote_dead_running` 降级为失败。
fn mark_stage_running(
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    pick: fn(&mut run_manifest::RunStages) -> &mut StageInfo,
) {
    let path = manifest_file(layout, voice, run_id);
    let Ok(mut manifest) = run_manifest::read_manifest(&path) else {
        return;
    };
    let stage = pick(&mut manifest.stages);
    stage.status = StageStatus::Running;
    stage.started_at = Some(run_manifest::now_ts());
    stage.finished_at = None;
    if let Err(e) = run_manifest::write_manifest(&path, &manifest) {
        eprintln!("[train] 写入批次清单失败: {}", e);
    }
}

/// 进程启动失败时立即把阶段标记为失败，无需等待 `demote_dead_running` 超时。
fn mark_stage_failed(
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    pick: fn(&mut run_manifest::RunStages) -> &mut StageInfo,
) {
    let path = manifest_file(layout, voice, run_id);
    let Ok(mut manifest) = run_manifest::read_manifest(&path) else {
        return;
    };
    let stage = pick(&mut manifest.stages);
    stage.status = StageStatus::Failed;
    stage.finished_at = Some(run_manifest::now_ts());
    if let Err(e) = run_manifest::write_manifest(&path, &manifest) {
        eprintln!("[train] 写入批次清单失败: {}", e);
    }
}

/// 阶段产物探测结果。
#[derive(Default)]
struct StageProbe {
    preprocess: bool,
    f0: bool,
    feature: bool,
    train: Option<i32>,
    index: Option<(u64, u32)>,
}

/// 日志中是否出现完成标记。日志缺失视为未完成——产物目录在运行中就会陆续出现，
/// 只看文件存在会把"进行到一半"误判为"已完成"。
fn log_finished(work_dir: &Path, log_name: &str, marker: &str) -> bool {
    std::fs::read_to_string(work_dir.join(log_name))
        .map(|log| log.contains(marker))
        .unwrap_or(false)
}

fn probe_stages(work_dir: &Path, if_f0: bool) -> StageProbe {
    StageProbe {
        preprocess: dir_has_ext(&work_dir.join("0_gt_wavs"), "wav")
            && log_finished(work_dir, "preprocess.log", "end preprocess"),
        f0: if if_f0 {
            dir_has_ext(&work_dir.join("2a_f0"), "npy")
                && dir_has_ext(&work_dir.join("2b-f0nsf"), "npy")
                && log_finished(work_dir, "extract_f0_feature.log", "all-feature-done")
        } else {
            true
        },
        feature: dir_has_ext(&work_dir.join("3_feature768"), "npy")
            && log_finished(work_dir, "extract_audio_feature.log", "all-feature-done"),
        train: final_train_epoch(work_dir),
        index: find_index_files(work_dir).first().map(|p| {
            (
                read_ntotal(p).unwrap_or(0),
                parse_n_ivf(p).unwrap_or(0),
            )
        }),
    }
}

/// 同步单个阶段：产物在则 done，产物被删则 stale。返回是否有变化。
fn sync_stage(stage: &mut StageInfo, done: bool, hash: impl FnOnce() -> Option<String>) -> bool {
    match (stage.status, done) {
        (StageStatus::Done, false) => {
            stage.status = StageStatus::Stale;
            true
        }
        // 主动回收掉的产物不算"需重跑"，产物重新出现则恢复 Done
        (StageStatus::Reclaimed, false) => false,
        (status, true) if status != StageStatus::Done => {
            stage.status = StageStatus::Done;
            if let Some(h) = hash() {
                stage.manifest_hash = Some(h);
            }
            if stage.finished_at.is_none() {
                stage.finished_at = Some(run_manifest::now_ts());
            }
            true
        }
        _ => false,
    }
}

fn apply_probe(manifest: &mut RunManifest, probe: &StageProbe, work_dir: &Path) -> bool {
    let if_f0 = manifest.if_f0;
    let mut changed = false;

    changed |= sync_stage(&mut manifest.stages.preprocess, probe.preprocess, || {
        run_manifest::fingerprint_dir(&work_dir.join("0_gt_wavs")).ok()
    });
    changed |= sync_stage(&mut manifest.stages.f0, probe.f0, || {
        if if_f0 {
            run_manifest::fingerprint_dir(&work_dir.join("2a_f0")).ok()
        } else {
            None
        }
    });
    changed |= sync_stage(&mut manifest.stages.feature, probe.feature, || {
        run_manifest::fingerprint_dir(&work_dir.join("3_feature768")).ok()
    });

    match (manifest.stages.train.status, probe.train) {
        (StageStatus::Done, None) => {
            manifest.stages.train.status = StageStatus::Stale;
            changed = true;
        }
        (status, Some(epoch)) if status != StageStatus::Done => {
            let stage = &mut manifest.stages.train;
            stage.status = StageStatus::Done;
            stage.epoch = Some(epoch.max(0) as u32);
            stage.started_at.get_or_insert(run_manifest::now_ts());
            stage.finished_at.get_or_insert(run_manifest::now_ts());
            changed = true;
        }
        _ => {}
    }

    match (manifest.stages.index.status, probe.index) {
        (StageStatus::Done, None) => {
            manifest.stages.index.status = StageStatus::Stale;
            changed = true;
        }
        (status, Some((ntotal, n_ivf))) if status != StageStatus::Done => {
            let stage = &mut manifest.stages.index;
            stage.status = StageStatus::Done;
            stage.ntotal = Some(ntotal);
            stage.n_ivf = Some(n_ivf);
            stage.finished_at.get_or_insert(run_manifest::now_ts());
            changed = true;
        }
        _ => {}
    }

    changed
}

/// 五个阶段的字段引用，供批量状态处理复用。
fn each_stage(manifest: &mut RunManifest) -> [&mut StageInfo; 5] {
    [
        &mut manifest.stages.preprocess,
        &mut manifest.stages.f0,
        &mut manifest.stages.feature,
        &mut manifest.stages.train,
        &mut manifest.stages.index,
    ]
}

/// 是否有 python 进程正在处理该工作位。探测失败时保守认为仍在运行。
fn python_alive_for(work_dir: &Path) -> bool {
    let pattern = work_dir.to_string_lossy().to_string();
    match std::process::Command::new("pgrep")
        .args(["-f", &pattern])
        .output()
    {
        Ok(out) => out.status.success() && !out.stdout.is_empty(),
        Err(_) => true,
    }
}

/// 阶段标记为运行中、但对应进程已经退出（应用重启、进程被杀）时降级为失败，
/// 避免界面永远停在"进行中"。启动后 30 秒内不判定，规避进程启动延迟。
fn demote_dead_running(manifest: &mut RunManifest, work_dir: &Path) -> bool {
    let now = run_manifest::now_ts();
    let expired = |stage: &StageInfo| {
        stage.status == StageStatus::Running
            && now.saturating_sub(stage.started_at.unwrap_or(0)) > 30
    };
    if !each_stage(manifest).into_iter().any(|stage| expired(stage)) || python_alive_for(work_dir) {
        return false;
    }
    let mut changed = false;
    for stage in each_stage(manifest) {
        if expired(stage) {
            stage.status = StageStatus::Failed;
            stage.finished_at = Some(now);
            changed = true;
        }
    }
    changed
}

fn dataset_fingerprint_now(workspace_root: &Path, manifest: &RunManifest) -> Option<String> {
    let source = Path::new(&manifest.dataset.source);
    let abs = if source.is_absolute() {
        source.to_path_buf()
    } else {
        workspace_root.join(source)
    };
    run_manifest::fingerprint_dir(&abs).ok()
}

/// 读清单 → 用磁盘产物校准阶段 → 数据集/管线指纹变化则降级 → 写回。
fn refresh_manifest(
    workspace_root: &Path,
    project_root: &Path,
    manifest_path: &Path,
    work_dir: &Path,
) -> Option<RunManifest> {
    let mut manifest = run_manifest::read_manifest(manifest_path).ok()?;
    let probe = probe_stages(work_dir, manifest.if_f0);
    let mut changed = apply_probe(&mut manifest, &probe, work_dir);
    changed |= demote_dead_running(&mut manifest, work_dir);

    let pipeline_now = run_manifest::pipeline_version(project_root);
    if let Some(fp) = dataset_fingerprint_now(workspace_root, &manifest) {
        changed |= manifest.invalidate_if_changed(&fp, &pipeline_now);
    }

    if changed {
        if let Err(e) = run_manifest::write_manifest(manifest_path, &manifest) {
            eprintln!("[train] 刷新批次清单失败: {}", e);
        }
    }
    Some(manifest)
}

/// 归档权重与索引到 `<资产目录>/<voice>/weights/<voice>-<run_id>-e<epoch>.<ext>`。
/// 同卷硬链接零额外空间；目标已存在时跳过（批次不可变）。
fn archive_epoch_assets(
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    epoch: i32,
    work_dir: &Path,
) -> Result<Vec<AssetRef>, String> {
    let weights_dir = layout.weights_dir(voice);
    let mut assets: Vec<AssetRef> = Vec::new();

    let weight_src = work_dir.join("model.pth");
    if weight_src.exists() {
        let name = format!("{}-{}-e{}.pth", voice, run_id, epoch);
        let dst = weights_dir.join(&name);
        if !dst.exists() {
            hardlink_or_copy(&weight_src, &dst).map_err(|e| format!("归档权重失败：{}", e))?;
        }
        assets.push(AssetRef {
            kind: "weights".to_string(),
            path: format!("weights/{}", name),
            bytes: file_len(&dst),
        });
    }

    for index_src in find_index_files(work_dir) {
        let name = format!("{}-{}-e{}.index", voice, run_id, epoch);
        let dst = weights_dir.join(&name);
        if !dst.exists() {
            hardlink_or_copy(&index_src, &dst).map_err(|e| format!("归档索引失败：{}", e))?;
        }
        for suffix in [".vectors.bin", ".meta.txt", ".ivf.bin"] {
            let sidecar_src = PathBuf::from(format!("{}{}", index_src.display(), suffix));
            if !sidecar_src.exists() {
                continue;
            }
            let sidecar_dst = PathBuf::from(format!("{}{}", dst.display(), suffix));
            if !sidecar_dst.exists() {
                hardlink_or_copy(&sidecar_src, &sidecar_dst)
                    .map_err(|e| format!("归档索引缓存失败：{}", e))?;
            }
        }
        assets.push(AssetRef {
            kind: "index".to_string(),
            path: format!("weights/{}", name),
            bytes: file_len(&dst),
        });
    }

    Ok(assets)
}

fn merge_assets(manifest: &mut RunManifest, assets: Vec<AssetRef>) {
    for asset in assets {
        match manifest
            .assets
            .iter_mut()
            .find(|a| a.kind == asset.kind && a.path == asset.path)
        {
            Some(existing) => existing.bytes = asset.bytes,
            None => manifest.assets.push(asset),
        }
    }
}

/// 批次清单 → 数据库索引行。
fn run_record_from_manifest(manifest: &RunManifest, bytes_total: i64) -> crate::database::RunRecord {
    crate::database::RunRecord {
        run_id: manifest.run_id.clone(),
        voice: manifest.voice.clone(),
        sr: manifest.sr.clone(),
        if_f0: manifest.if_f0,
        created_at: manifest.created_at,
        finished_at: manifest.stages.train.finished_at,
        epochs: manifest.params.as_ref().map(|p| p.epochs as i32).unwrap_or(0),
        dataset_fingerprint: manifest.dataset.fingerprint.clone(),
        pipeline_version: manifest.pipeline_version.clone(),
        stages_json: serde_json::to_string(&manifest.stages).unwrap_or_else(|_| "{}".to_string()),
        assets_json: serde_json::to_string(&manifest.assets).unwrap_or_else(|_| "[]".to_string()),
        bytes_total,
        reclaimed_at: manifest.reclaimable.reclaimed_at,
        note: String::new(),
    }
}

/// 把批次清单同步进数据库的查询索引（失败不影响主流程）。
async fn index_run(db: &crate::database::Database, manifest: &RunManifest, work_dir: &Path) {
    let bytes_total = (crate::workspace_layout::bytes_of(work_dir)
        + manifest.assets.iter().map(|a| a.bytes).sum::<u64>()) as i64;
    if let Err(e) = db
        .upsert_run(run_record_from_manifest(manifest, bytes_total))
        .await
    {
        eprintln!("[train] 写入批次索引失败: {}", e);
    }
}

/// 训练进程退出后的收尾：归档权重 → 更新阶段 → R1 回收 → 写清单 → 登记索引。
async fn finalize_training(
    workspace_root: &Path,
    project_root: &Path,
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    work_dir: &Path,
    db: &crate::database::Database,
) {
    let path = manifest_file(layout, voice, run_id);
    let mut manifest = match run_manifest::read_manifest(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[train] 读取批次清单失败: {}", e);
            return;
        }
    };

    match final_train_epoch(work_dir) {
        Some(epoch) => {
            match archive_epoch_assets(layout, voice, run_id, epoch, work_dir) {
                Ok(assets) => merge_assets(&mut manifest, assets),
                Err(e) => eprintln!("[train] 归档失败，工作位保持原样: {}", e),
            }
            let stage = &mut manifest.stages.train;
            stage.status = StageStatus::Done;
            stage.epoch = Some(epoch.max(0) as u32);
            stage.finished_at.get_or_insert(run_manifest::now_ts());

            let freed = reclaim_work_dir(layout, run_id, work_dir, false);
            if freed > 0 {
                manifest.reclaimable.bytes = manifest.reclaimable.bytes.saturating_add(freed);
                manifest.reclaimable.reclaimed_at = Some(run_manifest::now_ts());
            }
        }
        None => {
            manifest.stages.train.status = StageStatus::Failed;
        }
    }

    let probe = probe_stages(work_dir, manifest.if_f0);
    apply_probe(&mut manifest, &probe, work_dir);

    let pipeline_now = run_manifest::pipeline_version(project_root);
    if let Some(fp) = dataset_fingerprint_now(workspace_root, &manifest) {
        manifest.invalidate_if_changed(&fp, &pipeline_now);
    }

    if let Err(e) = run_manifest::write_manifest(&path, &manifest) {
        eprintln!("[train] 写入批次清单失败: {}", e);
    }
    index_run(db, &manifest, work_dir).await;
}

/// 索引构建成功后的收尾：归档索引 → 标记 index 阶段 → 回收 3_feature768。
async fn finalize_index(
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    work_dir: &Path,
    db: &crate::database::Database,
) {
    let path = manifest_file(layout, voice, run_id);
    let mut manifest = match run_manifest::read_manifest(&path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("[train] 读取批次清单失败: {}", e);
            return;
        }
    };

    let epoch = manifest.stages.train.epoch.unwrap_or(0) as i32;
    let archived = match archive_epoch_assets(layout, voice, run_id, epoch, work_dir) {
        Ok(assets) => {
            merge_assets(&mut manifest, assets);
            true
        }
        Err(e) => {
            eprintln!("[train] 索引归档失败: {}", e);
            false
        }
    };

    let probe = probe_stages(work_dir, manifest.if_f0);
    if let Some((ntotal, n_ivf)) = probe.index {
        let stage = &mut manifest.stages.index;
        stage.status = StageStatus::Done;
        stage.ntotal = Some(ntotal);
        stage.n_ivf = Some(n_ivf);
        stage.finished_at.get_or_insert(run_manifest::now_ts());
    }

    // 只有索引已归档且确实存在时才回收特征；否则保留 3_feature768 以便重建
    if archived && probe.index.is_some() {
        let feature_dir = work_dir.join("3_feature768");
        let freed = move_to_trash(layout, run_id, &feature_dir);
        if freed > 0 {
            manifest.reclaimable.bytes = manifest.reclaimable.bytes.saturating_add(freed);
            manifest.reclaimable.reclaimed_at = Some(run_manifest::now_ts());
        }
        // 特征已进索引并移走，标记为"已回收"，避免下次校准把它误判为"需重跑"
        if !feature_dir.exists() {
            manifest.stages.feature.status = StageStatus::Reclaimed;
        }
    }

    if let Err(e) = run_manifest::write_manifest(&path, &manifest) {
        eprintln!("[train] 写入批次清单失败: {}", e);
    }
    index_run(db, &manifest, work_dir).await;
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainRequest {
    pub exp_dir: String,
    pub sr: String,
    pub trainset_dir: String,
    pub np_process: i32,
    pub f0_method: String,
    pub if_f0: bool,
    pub save_epoch: i32,
    pub total_epoch: i32,
    pub batch_size: i32,
    pub if_save_latest: bool,
    pub if_save_every_weights: bool,
    pub gpus: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PreprocessResponse {
    pub ok: bool,
    pub message: String,
    /// 本次使用的工作位限定名 `<voice>/<run_id>`。后续步骤必须原样回传。
    pub exp_dir: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainStatusResponse {
    pub is_running: bool,
    pub pid: Option<String>,
    pub current_epoch: i32,
    pub total_epoch: i32,
    pub latest_loss: String,
    pub percent: f64,
    pub log_tail: String,
    pub error_message: String,
}

#[tauri::command]
pub async fn preprocess_dataset(
    trainset_dir: String,
    exp_dir: String,
    sr: String,
    np_process: i32,
    if_f0: bool,
    state: State<'_, AppState>,
) -> Result<PreprocessResponse, String> {
    let trainset_path = Path::new(&trainset_dir);
    if !trainset_path.exists() {
        return Err(format!("训练集路径不存在: {}", trainset_dir));
    }
    if !trainset_path.is_dir() {
        return Err(format!("训练集路径不是一个文件夹: {}", trainset_dir));
    }

    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let sr_value = match sr.as_str() {
        "32k" => "32000",
        "40k" => "40000",
        "48k" => "48000",
        _ => "40000",
    };

    // 限制预处理并行进程数为 CPU 核心数的一半，防止系统卡死
    let max_workers = (num_cpus::get() as i32 / 2).max(1);
    let effective_np = np_process.min(max_workers);

    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);

    // New：新建批次；Batch：复用已有批次；Legacy：旧布局，不参与批次化
    let (run_name, batch) = match parse_run_target(&exp_dir)? {
        RunTarget::New { voice } => {
            let run_id = create_batch(
                &workspace_root,
                &state.project_root,
                &layout,
                &voice,
                &sr,
                if_f0,
                trainset_path,
            )?;
            let name = format!("{}/{}", voice, run_id);
            (name, Some((voice, run_id)))
        }
        RunTarget::Batch { voice, run_id } => {
            prepare_existing_batch(
                &workspace_root,
                &state.project_root,
                &layout,
                &voice,
                &run_id,
                &sr,
                if_f0,
                trainset_path,
            )?;
            let name = format!("{}/{}", voice, run_id);
            (name, Some((voice, run_id)))
        }
        RunTarget::Legacy => (exp_dir.clone(), None),
    };

    let exp_path = layout.run_path(&run_name);
    tokio::fs::create_dir_all(&exp_path)
        .await
        .map_err(|e| format!("Failed to create experiment directory: {}", e))?;

    let args = vec![
        trainset_dir,
        sr_value.to_string(),
        effective_np.to_string(),
        exp_path.to_string_lossy().to_string(),
        "False".to_string(),
        "3.0".to_string(),
    ];

    let log_path = exp_path.join("preprocess.log");
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    tokio::fs::write(&log_path, "[backend] preprocess process launching...\n")
        .await
        .map_err(|e| format!("Failed to initialize preprocess log: {}", e))?;

    let result = executor
        .run_detached_with_log("infer/modules/train/preprocess_optimized.py", args, log_path.clone())
        .await;
    if let Err(e) = result {
        if let Some((voice, run_id)) = &batch {
            mark_preprocess_failed(&layout, voice, run_id);
        }
        return Err(format!("Failed to start preprocess: {}", e));
    }

    Ok(PreprocessResponse {
        ok: true,
        message: format!(
            "预处理已在后台启动（使用 {}/{} 进程）。数据将保存至: {}",
            effective_np,
            max_workers * 2,
            exp_path.display()
        ),
        exp_dir: run_name,
    })
}

#[tauri::command]
pub async fn extract_f0_features(
    np_process: i32,
    f0_method: String,
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let workspace_root = state.workspace_root.read().await;
    let layout = WorkspaceLayout::detect(&*workspace_root);
    let exp_path = layout.run_path(&exp_dir);
    let batch = match parse_run_target(&exp_dir) {
        Ok(RunTarget::Batch { voice, run_id }) => Some((voice, run_id)),
        _ => None,
    };
    if let Some((voice, run_id)) = &batch {
        mark_stage_running(&layout, voice, run_id, |stages| &mut stages.f0);
    }

    // Clean old (possibly incomplete) F0 output before re-running
    for dir_name in &["2a_f0", "2b-f0nsf"] {
        let dir = exp_path.join(dir_name);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await.ok();
        }
    }

    let log_path = exp_path.join("extract_f0_feature.log");
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    tokio::fs::write(&log_path, "[backend] f0 feature extraction process launching...\n")
        .await
        .map_err(|e| format!("Failed to initialize f0 feature extraction log: {}", e))?;

    let args = vec![
        exp_path.to_string_lossy().to_string(),
        np_process.to_string(),
        f0_method.clone(),
    ];

    let result = executor
        .run_detached_with_log("infer/modules/train/extract/extract_f0_print.py", args, log_path.clone())
        .await;
    if let Err(e) = result {
        if let Some((voice, run_id)) = &batch {
            mark_stage_failed(&layout, voice, run_id, |stages| &mut stages.f0);
        }
        return Err(format!("Failed to start f0 feature extraction: {}", e));
    }

    Ok(TrainResponse {
        ok: true,
        message: format!("F0 feature extraction started. Log: {}", log_path.display()),
    })
}

#[tauri::command]
pub async fn extract_audio_features(
    gpus: String,
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let workspace_root = state.workspace_root.read().await;
    let layout = WorkspaceLayout::detect(&*workspace_root);
    let exp_path = layout.run_path(&exp_dir);
    let batch = match parse_run_target(&exp_dir) {
        Ok(RunTarget::Batch { voice, run_id }) => Some((voice, run_id)),
        _ => None,
    };
    if let Some((voice, run_id)) = &batch {
        mark_stage_running(&layout, voice, run_id, |stages| &mut stages.feature);
    }

    // Clean old (possibly incomplete) audio feature output before re-running
    for dir_name in &["3_feature256", "3_feature768"] {
        let dir = exp_path.join(dir_name);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await.ok();
        }
    }

    let log_path = exp_path.join("extract_audio_feature.log");
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    tokio::fs::write(&log_path, "[backend] audio feature extraction process launching...\n")
        .await
        .map_err(|e| format!("Failed to initialize audio feature extraction log: {}", e))?;

    let gpu_list: Vec<&str> = gpus.split('-').collect();
    let args = vec![
        "cuda".to_string(),
        gpu_list.len().to_string(),
        "0".to_string(),
        gpu_list.first().unwrap_or(&"0").to_string(),
        exp_path.to_string_lossy().to_string(),
        "False".to_string(),
    ];

    let result = executor
        .run_detached_with_log("infer/modules/train/extract_feature_print.py", args, log_path.clone())
        .await;
    if let Err(e) = result {
        if let Some((voice, run_id)) = &batch {
            mark_stage_failed(&layout, voice, run_id, |stages| &mut stages.feature);
        }
        return Err(format!("Failed to start audio feature extraction: {}", e));
    }

    Ok(TrainResponse {
        ok: true,
        message: format!("Audio feature extraction started. Log: {}", log_path.display()),
    })
}

#[tauri::command]
pub async fn train_model(
    config: TrainRequest,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    // 互斥检查：禁止多个重任务并行
    {
        let task = state.active_heavy_task.lock().await;
        if let Some(ref running) = *task {
            // 检查训练进程是否实际存活，如果已死则自动清除僵尸锁
            let alive = std::process::Command::new("pgrep")
                .args(["-f", "train\\.py"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !alive {
                drop(task);
                let mut lock = state.active_heavy_task.lock().await;
                if lock.as_deref() == Some("train") {
                    *lock = None;
                }
                eprintln!("[train] 检测到前次训练进程已退出，自动清除任务锁");
            } else {
                return Err(format!(
                    "系统当前有任务正在运行: [{}]，请等待其完成后再开始训练。",
                    running
                ));
            }
        }
    }

    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);

    // 训练必须落在已建批次上；旧布局目录仍按原语义放行（不写清单、不归档）
    let run_ctx = match parse_run_target(&config.exp_dir)? {
        RunTarget::Batch { voice, run_id } => Some((voice, run_id)),
        RunTarget::Legacy => None,
        RunTarget::New { voice } => {
            return Err(format!(
                "声音「{}」还没有训练批次，请先完成数据预处理。",
                voice
            ));
        }
    };

    let exp_path = layout.run_path(&config.exp_dir);

    tokio::fs::create_dir_all(&exp_path)
        .await
        .map_err(|e| format!("Failed to create experiment directory: {}", e))?;

    let gt_wavs_dir = exp_path.join("0_gt_wavs");
    let feature_dir = exp_path.join("3_feature768");
    
    if !gt_wavs_dir.exists() || !feature_dir.exists() {
        return Err(format!(
            "Training data not found. Please complete preprocessing and feature extraction first.\nExpected directories:\n- {}\n- {}",
            gt_wavs_dir.display(),
            feature_dir.display()
        ));
    }

    let mut filelist_entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&gt_wavs_dir)
        .await
        .map_err(|e| format!("Failed to read gt_wavs directory: {}", e))?;
    
    let pitch_dir = exp_path.join("2a_f0");
    let pitchf_dir = exp_path.join("2b-f0nsf");
    
    let mut total_wavs = 0;
    let mut missing_features = 0;
    let mut missing_pitch = 0;
    
    while let Some(entry) = read_dir.next_entry().await.map_err(|e| format!("Failed to read directory entry: {}", e))? {
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("wav") {
            total_wavs += 1;
            
            let file_name = path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("Invalid filename: {}", path.display()))?;
            
            let feature_file = feature_dir.join(format!("{}.npy", file_name));
            
            if !feature_file.exists() {
                missing_features += 1;
                continue;
            }
            
            // Use absolute paths for filelist entries
            let rel_wav = path.to_string_lossy().to_string();
            let rel_feature = feature_file.to_string_lossy().to_string();
            
            if config.if_f0 {
                let pitch_file = pitch_dir.join(format!("{}.npy", file_name));
                let pitchf_file = pitchf_dir.join(format!("{}.npy", file_name));
                
                if !pitch_file.exists() || !pitchf_file.exists() {
                    missing_pitch += 1;
                    continue;
                }
                
                let rel_pitch = pitch_file.to_string_lossy().to_string();
                let rel_pitchf = pitchf_file.to_string_lossy().to_string();
                
                filelist_entries.push(format!("{}|{}|{}|{}|0", rel_wav, rel_feature, rel_pitch, rel_pitchf));
            } else {
                filelist_entries.push(format!("{}|{}|0", rel_wav, rel_feature));
            }
        }
    }
    
    if filelist_entries.is_empty() {
        let mut error_msg = format!(
            "No valid training samples found.\n\nStatistics:\n- Total wav files: {}\n- Missing feature files: {}",
            total_wavs, missing_features
        );
        
        if config.if_f0 {
            error_msg.push_str(&format!("\n- Missing pitch files: {}", missing_pitch));
            error_msg.push_str(&format!(
                "\n\nFor f0 models, you need:\n1. Feature files in: {}\n2. Pitch files in: {} and {}",
                feature_dir.display(),
                pitch_dir.display(),
                pitchf_dir.display()
            ));
        } else {
            error_msg.push_str(&format!(
                "\n\nYou need feature files in: {}",
                feature_dir.display()
            ));
        }
        
        error_msg.push_str("\n\nPlease complete preprocessing and feature extraction first.");
        return Err(error_msg);
    }
    
    let filelist_path = exp_path.join("filelist.txt");
    tokio::fs::write(&filelist_path, filelist_entries.join("\n"))
        .await
        .map_err(|e| format!("Failed to write filelist.txt: {}", e))?;

    let config_template_path = state.project_root.join("configs").join("v2").join(format!("{}.json", config.sr));
    if !config_template_path.exists() {
        return Err(format!(
            "Config template not found: {}. Expected path for version v2 and sample rate {}.",
            config_template_path.display(),
            config.sr
        ));
    }
    
    let config_dest_path = exp_path.join("config.json");
    tokio::fs::copy(&config_template_path, &config_dest_path)
        .await
        .map_err(|e| format!("Failed to copy config template: {}", e))?;

    if let Ok(content) = tokio::fs::read_to_string(&config_dest_path).await {
        if let Ok(mut cfg_json) = serde_json::from_str::<serde_json::Value>(&content) {
            cfg_json["train"]["epochs"] = serde_json::json!(config.total_epoch);
            let _ = tokio::fs::write(&config_dest_path, cfg_json.to_string()).await;
        }
    }

    let pretrained_g_default = if config.if_f0 {
        format!("assets/pretrained_v2/f0G{}k.pth", config.sr.trim_end_matches('k'))
    } else {
        format!("assets/pretrained_v2/G{}k.pth", config.sr.trim_end_matches('k'))
    };
    let pretrained_d_default = if config.if_f0 {
        format!("assets/pretrained_v2/f0D{}k.pth", config.sr.trim_end_matches('k'))
    } else {
        format!("assets/pretrained_v2/D{}k.pth", config.sr.trim_end_matches('k'))
    };
    let pretrained_g_path = state.data_dir.join(&pretrained_g_default);
    let pretrained_d_path = state.data_dir.join(&pretrained_d_default);
    let mut missing = Vec::new();
    if !pretrained_g_path.exists() {
        missing.push(format!("预训练 Generator: {}", pretrained_g_path.display()));
    }
    if !pretrained_d_path.exists() {
        missing.push(format!("预训练 Discriminator: {}", pretrained_d_path.display()));
    }
    if !missing.is_empty() {
        return Err(format!(
            "底模缺失，无法开始训练！请前往「模型管理」页面下载以下文件：\n{}",
            missing.join("\n")
        ));
    }
    // 底模必须是完整 torch 存档：空文件/半截下载会在 Python 侧静默失败
    for (label, path) in [
        ("预训练 Generator", &pretrained_g_path),
        ("预训练 Discriminator", &pretrained_d_path),
    ] {
        if !looks_like_torch_save(path) {
            return Err(format!(
                "{} 不是有效的 torch 权重（文件为空或已损坏），请重新下载：{}",
                label,
                path.display()
            ));
        }
    }

    let args = vec![
        "-e".to_string(),
        exp_path.to_string_lossy().to_string(),
        "-sr".to_string(),
        config.sr.clone(),
        "-f0".to_string(),
        if config.if_f0 { "1" } else { "0" }.to_string(),
        "-bs".to_string(),
        config.batch_size.to_string(),
        "-te".to_string(),
        config.total_epoch.to_string(),
        "-se".to_string(),
        config.save_epoch.to_string(),
        "-pg".to_string(),
        pretrained_g_path.to_string_lossy().to_string(),
        "-pd".to_string(),
        pretrained_d_path.to_string_lossy().to_string(),
        "-l".to_string(),
        if config.if_save_latest { "1" } else { "0" }.to_string(),
        "-c".to_string(),
        "0".to_string(),
        "-sw".to_string(),
        if config.if_save_every_weights { "1" } else { "0" }.to_string(),
        "-v".to_string(),
        "v2".to_string(),
        "-g".to_string(),
        config.gpus,
    ];

    // 训练参数在真正开跑前才确定，此时回填进批次清单
    if let Some((voice, run_id)) = &run_ctx {
        let path = manifest_file(&layout, voice, run_id);
        let mut manifest =
            run_manifest::read_manifest(&path).map_err(|e| format!("读取批次清单失败：{}", e))?;
        manifest.sr = config.sr.clone();
        manifest.if_f0 = config.if_f0;
        manifest.params = Some(TrainParams {
            epochs: config.total_epoch.max(0) as u32,
            batch_size: config.batch_size.max(0) as u32,
            save_epoch: config.save_epoch.max(0) as u32,
            f0_method: Some(config.f0_method.clone()),
            np: Some(config.np_process.max(0) as u32),
        });
        manifest.stages.train = StageInfo {
            status: StageStatus::Running,
            started_at: Some(run_manifest::now_ts()),
            ..Default::default()
        };
        run_manifest::write_manifest(&path, &manifest)
            .map_err(|e| format!("写入批次清单失败：{}", e))?;
    }

    let train_log_path = exp_path.join("train.log");
    {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&train_log_path)
            .await
            .map_err(|e| format!("Failed to open training log: {}", e))?;
        file.write_all(b"[backend] training process launching...\n")
            .await
            .map_err(|e| format!("Failed to write training log marker: {}", e))?;
    }

    // 设置任务令牌
    *state.active_heavy_task.lock().await = Some("train".to_string());

    let result = executor
        .run_detached_with_log("infer/modules/train/train.py", args, train_log_path.clone())
        .await;
    let pid = match result {
        Ok(p) => p,
        Err(e) => {
            // 启动失败，立即释放令牌
            *state.active_heavy_task.lock().await = None;
            if let Some((voice, run_id)) = &run_ctx {
                mark_stage_failed(&layout, voice, run_id, |stages| &mut stages.train);
            }
            return Err(format!("Failed to start training: {}", e));
        }
    };
    // 记录训练 PID，进程退出或取消时自动清理
    *state.train_pid.lock().await = Some(pid);

    // 后台监视训练进程退出：清令牌 + 批次收尾（归档 → R1 回收 → 写清单 → 登记索引）
    let task_ref = state.active_heavy_task.clone();
    let exp_path_str_monitor = exp_path.to_string_lossy().to_string();
    let monitor_exp_path = exp_path.clone();
    let monitor_ws = workspace_root.clone();
    let monitor_layout = layout.clone();
    let project_root = state.project_root.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut miss_streak = 0u32;
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            // 使用 pgrep 检查 train.py 进程是否存活（与 get_train_status 一致）
            let alive = std::process::Command::new("pgrep")
                .args(["-f", &format!("train.py.*{}", exp_path_str_monitor)])
                .output()
                .map(|o| o.status.success() && !o.stdout.is_empty())
                .unwrap_or(false);
            if alive {
                miss_streak = 0;
                continue;
            }
            miss_streak += 1;
            // 启动初期 pgrep 可能尚未捕捉到进程，给 15 秒宽限，避免误判失败
            if miss_streak < 3 {
                continue;
            }
            {
                let mut lock = task_ref.lock().await;
                if lock.as_deref() == Some("train") {
                    *lock = None;
                }
            }
            if let Some((voice, run_id)) = &run_ctx {
                finalize_training(
                    &monitor_ws,
                    &project_root,
                    &monitor_layout,
                    voice,
                    run_id,
                    &monitor_exp_path,
                    &db,
                )
                .await;
            }
            break;
        }
    });

    Ok(TrainResponse {
        ok: true,
        message: format!("训练已在后台启动。模型将保存至: {}", exp_path.display()),
    })
}

#[tauri::command]
pub async fn stop_training(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    let workspace_root = state.workspace_root.read().await;
    let exp_path = run_path_for(&workspace_root, &exp_dir);
    let exp_path_str = exp_path.to_string_lossy().to_string();
    drop(workspace_root);

    // ── 礼貌停止训练进程（SIGTERM → 轮询 8s → SIGKILL）
    let pattern = format!("train.py.*{}", exp_path_str);
    let gracefully_exited = crate::process_killer::kill_by_pattern(&pattern, 8);
    crate::process_killer::kill_by_pattern("torch.distributed", 0);
    crate::process_killer::kill_multiprocessing_spawn();

    // 无论如何，清除任务令牌 + 训练 PID
    {
        let mut lock = state.active_heavy_task.lock().await;
        if lock.as_deref() == Some("train") {
            *lock = None;
        }
    }
    *state.train_pid.lock().await = None;

    let msg = if gracefully_exited {
        "训练已优雅停止（Checkpoint 已安全保存）".to_string()
    } else {
        "训练已强制停止（SIGKILL）".to_string()
    };

    Ok(TrainResponse { ok: true, message: msg })
}

#[tauri::command]
pub async fn train_index(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);
    // 索引归档需要批次号；旧布局目录不做归档
    let run_ctx = match parse_run_target(&exp_dir)? {
        RunTarget::Batch { voice, run_id } => Some((voice, run_id)),
        _ => None,
    };
    let exp_path = layout.run_path(&exp_dir);

    if let Some((voice, run_id)) = &run_ctx {
        mark_stage_running(&layout, voice, run_id, |stages| &mut stages.index);
    }

    // 重建索引前清掉旧索引及 sidecar：n_ivf 随特征数量变化，旧文件名会与新文件共存
    for stale in find_index_files(&exp_path) {
        for suffix in ["", ".vectors.bin", ".meta.txt", ".ivf.bin"] {
            let p = PathBuf::from(format!("{}{}", stale.display(), suffix));
            if p.exists() {
                let _ = tokio::fs::remove_file(&p).await;
            }
        }
    }

    let python_code = format!(
        r#"
import json, os, faiss, numpy as np, struct

exp_dir = '{}'
feature_dir = os.path.join(exp_dir, '3_feature768')
if not os.path.exists(feature_dir):
    print(json.dumps({{'ok': False, 'message': '请先进行特征提取!\\n检查路径: %s' % feature_dir}}))
    exit(0)

npys = []
for name in sorted(os.listdir(feature_dir)):
    npys.append(np.load(os.path.join(feature_dir, name)))

big_npy = np.concatenate(npys, 0)
big_npy_idx = np.arange(big_npy.shape[0])
np.random.shuffle(big_npy_idx)
big_npy = big_npy[big_npy_idx]

n_ivf = min(int(16 * np.sqrt(big_npy.shape[0])), big_npy.shape[0] // 39)
index = faiss.index_factory(768, f'IVF{{n_ivf}},Flat')
index_ivf = faiss.extract_index_ivf(index)
index_ivf.nprobe = 1
index.train(big_npy)

batch_size_add = 8192
for i in range(0, big_npy.shape[0], batch_size_add):
    index.add(big_npy[i:i + batch_size_add])

# 索引生成到 checkpoints/ 目录，与模型文件同目录，消除后续查找歧义
out_dir = os.path.join(exp_dir, 'checkpoints')
os.makedirs(out_dir, exist_ok=True)
index_path = os.path.join(out_dir, f'added_IVF{{n_ivf}}_Flat_nprobe_{{index_ivf.nprobe}}_{{os.path.basename(exp_dir)}}_v2.index')
faiss.write_index(index, index_path)

# 同时生成运行时 IVF 缓存（vectors.bin + meta.txt + ivf.bin）
# 这样 InferCore::load 无需再调用 Python 重建
idx2 = faiss.read_index(index_path)
try:
    idx2.make_direct_map()
except:
    pass
npy2 = idx2.reconstruct_n(0, idx2.ntotal)
npy2.astype('<f4').tofile(index_path + '.vectors.bin')
with open(index_path + '.meta.txt', 'w') as f2:
    f2.write(str(npy2.shape[0]) + '\n')
    f2.write(str(npy2.shape[1]) + '\n')
ivf2 = faiss.extract_index_ivf(idx2)
ncent2 = ivf2.nlist
d2 = npy2.shape[1]
centroids2 = ivf2.quantizer.reconstruct_n(0, ivf2.quantizer.ntotal).astype('<f4')
with open(index_path + '.ivf.bin', 'wb') as f2:
    f2.write(struct.pack('<II', ncent2, d2))
    f2.write(centroids2.tobytes())
    for i in range(ncent2):
        nv = ivf2.invlists.list_size(i)
        f2.write(struct.pack('<I', nv))
        if nv > 0:
            ids_ptr = ivf2.invlists.get_ids(i)
            ids = faiss.rev_swig_ptr(ids_ptr, nv).astype('<u4')
            f2.write(ids.tobytes())
print(json.dumps({{'ok': True, 'message': '成功构建索引 (含 IVF 缓存)'}}))
"#,
        exp_path.to_string_lossy()
    );

    let result = match executor
        .run_script("-c", vec![python_code])
        .await
    {
        Ok(result) => result,
        Err(e) => {
            if let Some((voice, run_id)) = &run_ctx {
                mark_stage_failed(&layout, voice, run_id, |stages| &mut stages.index);
            }
            return Err(format!("Index training failed: {}", e));
        }
    };

    let response: serde_json::Value = match serde_json::from_str(&result) {
        Ok(value) => value,
        Err(e) => {
            if let Some((voice, run_id)) = &run_ctx {
                mark_stage_failed(&layout, voice, run_id, |stages| &mut stages.index);
            }
            return Err(format!("Failed to parse response: {}", e));
        }
    };

    let ok = response["ok"].as_bool().unwrap_or(false);
    // 索引建成后才归档并回收特征；归档失败时保留 3_feature768，便于重试
    if ok {
        if let Some((voice, run_id)) = &run_ctx {
            finalize_index(&layout, voice, run_id, &exp_path, &state.db).await;
        }
    } else if let Some((voice, run_id)) = &run_ctx {
        mark_stage_failed(&layout, voice, run_id, |stages| &mut stages.index);
    }

    Ok(TrainResponse {
        ok,
        message: response["message"].as_str().unwrap_or("").to_string(),
    })
}

/// 为已存在的 .index 文件生成运行时 IVF 缓存（vectors.bin + meta.txt + ivf.bin）。
/// 当缓存缺失或损坏时调用此命令修复，无需重新训练。
#[tauri::command]
pub async fn build_index_cache(
    index_path: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    if !Path::new(&index_path).exists() {
        return Ok(TrainResponse {
            ok: false,
            message: format!("index 文件不存在: {}", index_path),
        });
    }

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let python_code = format!(
        r#"
import faiss, numpy as np, os, struct

idx_path = r'{}'
idx = faiss.read_index(idx_path)
try:
    idx.make_direct_map()
except:
    pass
npy = idx.reconstruct_n(0, idx.ntotal)
npy.astype('<f4').tofile(idx_path + '.vectors.bin')
with open(idx_path + '.meta.txt', 'w') as f:
    f.write(str(npy.shape[0]) + '\n')
    f.write(str(npy.shape[1]) + '\n')
ivf = faiss.extract_index_ivf(idx)
ncent = ivf.nlist
d = npy.shape[1]
centroids = ivf.quantizer.reconstruct_n(0, ivf.quantizer.ntotal).astype('<f4')
with open(idx_path + '.ivf.bin', 'wb') as f:
    f.write(struct.pack('<II', ncent, d))
    f.write(centroids.tobytes())
    for i in range(ncent):
        nv = ivf.invlists.list_size(i)
        f.write(struct.pack('<I', nv))
        if nv > 0:
            ids_ptr = ivf.invlists.get_ids(i)
            ids = faiss.rev_swig_ptr(ids_ptr, nv).astype('<u4')
            f.write(ids.tobytes())
print(f"[DONE] IVF 缓存生成成功: {{npy.shape[0]}} vecs x {{d}}, {{ncent}} centroids")
"#,
        index_path
    );

    let result = executor
        .run_script("-c", vec![python_code])
        .await
        .map_err(|e| format!("索引缓存生成失败: {}", e))?;

    Ok(TrainResponse {
        ok: true,
        message: result,
    })
}

#[tauri::command]
pub async fn get_train_status(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainStatusResponse, String> {
    use std::process::Command;

    let workspace_root = state.workspace_root.read().await;
    let exp_path = run_path_for(&workspace_root, &exp_dir);
    let exp_path_str = exp_path.to_string_lossy();

    let output = Command::new("pgrep")
        .args(&["-f", &format!("train.py.*{}", exp_path_str)])
        .output()
        .map_err(|e| format!("Failed to check training status: {}", e))?;

    let is_running = output.status.success() && !output.stdout.is_empty();

    let mut current_epoch = 0i32;
    let mut total_epoch = 0i32;
    let mut latest_loss = String::new();

    let mut log_tail = String::new();
    let mut error_message = String::new();

    let train_log_path = exp_path.join("train.log");
    if train_log_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&train_log_path).await {
            log_tail = content.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");

            // Only read epoch/loss from lines AFTER the last restart marker.
            // This prevents stale data from a previous terminated run.
            let cutoff = content.rfind("[backend] training process launching...")
                .map(|pos| pos + content[pos..].find('\n').unwrap_or(0) + 1)
                .unwrap_or(0);
            let relevant = &content[cutoff..];

            for line in relevant.lines().rev() {
                if line.contains("Epoch:") {
                    if let Some(epoch_str) = line.split("Epoch:").nth(1) {
                        if let Some(num) = epoch_str.trim().split_whitespace().next() {
                            current_epoch = num.parse().unwrap_or(0);
                        }
                    }
                    break;
                }
            }
            for line in relevant.lines().rev() {
                if line.contains("loss_disc=") || line.contains("loss_gen=") {
                    let mut parts = Vec::new();
                    for part in line.split(',').take(3) {
                        if let Some(v) = part.split('=').nth(1) {
                            parts.push(v.trim().to_string());
                        }
                    }
                    latest_loss = parts.join(", ");
                    break;
                }
            }

            let error_keywords = ["Traceback (most recent call last)", "RuntimeError", "Killed", "exitcode", "SIGTERM", "out of memory", "Segmentation fault", "Bus error"];
            for line in content.lines().rev().take(50) {
                for kw in &error_keywords {
                    if line.contains(kw) {
                        if error_message.is_empty() {
                            error_message = line.to_string();
                        }
                    }
                }
            }
        }
    }

    let config_path = exp_path.join("config.json");
    if config_path.exists() {
        if let Ok(cfg_content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&cfg_content) {
                total_epoch = cfg["train"]["epochs"].as_i64().unwrap_or(0) as i32;
            }
        }
    }

    // If no train.log / no epoch was read from log, fall back to checkpoint files.
    // This handles imported experiments (trained in original RVC CLI) that have
    // merged checkpoints like harrison_e200_s60000.pth but no train.log.
    if current_epoch == 0 {
        for search_dir in &[exp_path.join("checkpoints"), exp_path.clone()] {
            if !search_dir.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(search_dir) {
                let mut max_epoch = 0i32;
                for entry in entries.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if !fname.ends_with(".pth") {
                        continue;
                    }
                    // Parse epoch from checkpoint filenames like *_e200_*.pth or *_e200.pth
                    if let Some(e_pos) = fname.find("_e") {
                        let after_e = &fname[e_pos + 2..];
                        if let Some(epoch_str) = after_e.split('_').next() {
                            if let Ok(ep) = epoch_str.parse::<i32>() {
                                max_epoch = max_epoch.max(ep);
                            }
                        }
                    }
                }
                if max_epoch > 0 {
                    current_epoch = max_epoch;
                    if total_epoch == 0 {
                        total_epoch = max_epoch;
                    }
                    // Imported experiments with checkpoints are considered done
                    if !is_running && error_message.is_empty() && current_epoch >= total_epoch {
                        // leave as-is; frontend will mark train='done'
                    }
                }
            }
        }
    }

    let percent = if total_epoch > 0 {
        current_epoch as f64 / total_epoch as f64 * 100.0
    } else {
        0.0
    };

    Ok(TrainStatusResponse {
        is_running,
        pid: None,
        current_epoch,
        total_epoch,
        latest_loss,
        percent,
        log_tail,
        error_message,
    })
}

/// 读取并校准批次清单：以磁盘产物为准，数据集/管线指纹变化会把已完成阶段降级为 stale。
/// `exp_dir` 为 `<voice>/<run_id>` 时读取该批次；为 `<voice>` 时读取该声音最新的批次，
/// 供训练页恢复上次训练上下文。
#[tauri::command]
pub async fn get_run_manifest(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<RunManifest, String> {
    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);
    let (voice, run_id) = match parse_run_target(&exp_dir)? {
        RunTarget::Batch { voice, run_id } => (voice, run_id),
        RunTarget::New { voice } => {
            let run_id = latest_run_id(&layout, &voice)
                .ok_or_else(|| format!("声音 {} 尚无批次", voice))?;
            (voice, run_id)
        }
        _ => return Err(format!("不是批次工作位：{}", exp_dir)),
    };
    let path = manifest_file(&layout, &voice, &run_id);
    if !path.exists() {
        return Err(format!("批次清单不存在：{}", path.display()));
    }
    let work_dir = layout.run_path(&format!("{}/{}", voice, run_id));
    refresh_manifest(&workspace_root, &state.project_root, &path, &work_dir)
        .ok_or_else(|| format!("批次清单解析失败：{}", path.display()))
}

#[tauri::command]
pub async fn get_experiment_path(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let workspace_root = state.workspace_root.read().await;
    let exp_path = run_path_for(&workspace_root, &exp_dir);
    Ok(serde_json::json!({
        "experimentPath": exp_path.to_string_lossy()
    }))
}

/// 单个批次的结构化摘要。训练页批次列表与后续模型选择都以它为准，
/// 前端不再自行拼路径或猜测目录层级。
#[derive(Debug, Clone, Serialize)]
pub struct ExperimentSummary {
    pub voice: String,
    pub run_id: String,
    /// 采样率（`32k` / `40k` / `48k`）。清单缺失且无法从目录名解析时为 None。
    pub sr: Option<String>,
    /// 旧布局版本目录（`v2_40k`），不参与批次化，仅用于兼容展示。
    pub legacy: bool,
    /// 训练完成时的最终 epoch；未训练或未完成时为 None。
    pub epoch: Option<u32>,
    /// 数据集指纹（`run.json.dataset.fingerprint`）；清单缺失时为 None。
    pub fingerprint: Option<String>,
    pub has_model: bool,
    pub has_index: bool,
    /// 批次工作位当前占用的字节数。
    pub bytes: u64,
}

/// 旧布局版本目录名（`v2_40k`）可反推出采样率；新批次号一律解析失败。
fn legacy_version_sr(dir_name: &str) -> Option<String> {
    let re = regex::Regex::new(r"^v\d+_(32|40|48)k$").ok()?;
    let captures = re.captures(dir_name)?;
    captures.get(1).map(|m| format!("{}k", m.as_str()))
}

/// 判断批次是否已有可用的权重/索引：先看归档（`weights/<voice>-<run>-e*`），
/// 再回落到工作位内尚未归档的 `model.pth` / `added_*.index`。
fn batch_assets(
    layout: &WorkspaceLayout,
    voice: &str,
    run_id: &str,
    work_dir: &Path,
) -> (bool, bool) {
    let asset_prefix = format!("{}-{}-e", voice, run_id);
    let mut has_model = false;
    let mut has_index = false;

    if let Ok(entries) = std::fs::read_dir(layout.weights_dir(voice)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with(&asset_prefix) {
                continue;
            }
            if name.ends_with(".pth") {
                has_model = true;
            } else if name.ends_with(".index") {
                has_index = true;
            }
        }
    }

    has_model |= work_dir.join("model.pth").is_file()
        || dir_has_ext(&work_dir.join("checkpoints"), "pth");
    has_index |= !find_index_files(work_dir).is_empty();

    (has_model, has_index)
}

/// 批次工作位的分阶段体积，供训练页展示"本步产出"与"可回收"。
#[derive(Debug, Serialize)]
pub struct RunUsage {
    pub preprocess: u64,
    pub f0: u64,
    pub feature: u64,
    pub train: u64,
    pub index: u64,
    /// 工作位内不属于任何阶段的文件（日志、config.json 等）。
    pub other: u64,
    /// 当前仍在盘上、可被回收的一次性或可再生产物（R1+R1'+R2）。
    pub reclaimable: u64,
    /// 该批次已自动回收的累计字节（取自 run.json）。
    pub reclaimed: u64,
}

fn sum_paths(paths: impl IntoIterator<Item = PathBuf>) -> u64 {
    paths
        .into_iter()
        .map(|p| crate::workspace_layout::bytes_of(&p))
        .sum()
}

/// 工作位根目录下的 `G_*.pth` / `D_*.pth`。
fn epoch_weight_files(work_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(work_dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            (name.starts_with("G_") || name.starts_with("D_")) && name.ends_with(".pth")
        })
        .collect()
}

#[tauri::command]
pub async fn get_run_usage(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<RunUsage, String> {
    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);
    let work_dir = run_path_for(&workspace_root, &exp_dir);
    if !work_dir.is_dir() {
        return Err(format!("批次工作位不存在：{}", exp_dir));
    }

    let weights = epoch_weight_files(&work_dir);
    let preprocess = sum_paths([work_dir.join("0_gt_wavs"), work_dir.join("1_16k_wavs")]);
    let f0 = sum_paths([work_dir.join("2a_f0"), work_dir.join("2b-f0nsf")]);
    let feature = crate::workspace_layout::bytes_of(&work_dir.join("3_feature768"));
    let train = crate::workspace_layout::bytes_of(&work_dir.join("model.pth"))
        + crate::workspace_layout::bytes_of(&work_dir.join("checkpoints"))
        + sum_paths(weights.iter().cloned());

    let mut index = 0u64;
    for index_file in find_index_files(&work_dir) {
        index += file_len(&index_file);
        for suffix in [".vectors.bin", ".meta.txt", ".ivf.bin"] {
            index += file_len(&PathBuf::from(format!("{}{}", index_file.display(), suffix)));
        }
    }

    let total = crate::workspace_layout::bytes_of(&work_dir);
    let other = total.saturating_sub(preprocess + f0 + feature + train + index);

    let manifest = parse_run_target(&exp_dir)
        .ok()
        .and_then(|target| match target {
            RunTarget::Batch { voice, run_id } => {
                run_manifest::read_manifest(&manifest_file(&layout, &voice, &run_id)).ok()
            }
            _ => None,
        });

    // R1（1_16k_wavs/eval/tfevents）+ R2（0_gt_wavs/2a_f0/2b-f0nsf/checkpoints）
    // + R1'（G_/D_）
    let mut reclaimable = sum_paths([
        work_dir.join("1_16k_wavs"),
        work_dir.join("eval"),
        work_dir.join("0_gt_wavs"),
        work_dir.join("2a_f0"),
        work_dir.join("2b-f0nsf"),
        work_dir.join("checkpoints"),
    ]) + sum_paths(weights.iter().cloned());
    // 3_feature768 只有索引完成后才允许回收（finalize_index 已自动回收，
    // 这里兜底统计索引完成但特征仍在的情况）
    if manifest
        .as_ref()
        .map(|m| m.stages.index.status == StageStatus::Done)
        .unwrap_or(false)
    {
        reclaimable += crate::workspace_layout::bytes_of(&work_dir.join("3_feature768"));
    }
    if let Ok(entries) = std::fs::read_dir(&work_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with("events.out.tfevents") {
                reclaimable += file_len(&entry.path());
            }
        }
    }

    let reclaimed = manifest.map(|m| m.reclaimable.bytes).unwrap_or(0);

    Ok(RunUsage {
        preprocess,
        f0,
        feature,
        train,
        index,
        other,
        reclaimable,
        reclaimed,
    })
}

#[tauri::command]
pub async fn list_experiments(
    state: State<'_, AppState>,
) -> Result<Vec<ExperimentSummary>, String> {
    let workspace_root = state.workspace_root.read().await.clone();
    let layout = WorkspaceLayout::detect(&workspace_root);
    let exp_root = layout.runs_root();
    tokio::fs::create_dir_all(&exp_root).await.ok();

    let mut experiments = Vec::new();

    let mut voices = match tokio::fs::read_dir(&exp_root).await {
        Ok(entries) => entries,
        Err(_) => return Ok(experiments),
    };
    while let Ok(Some(voice_entry)) = voices.next_entry().await {
        if !voice_entry
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let voice = voice_entry.file_name().to_string_lossy().to_string();
        if !crate::workspace_layout::is_safe_component(&voice) {
            continue;
        }

        let mut runs = match tokio::fs::read_dir(voice_entry.path()).await {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        while let Ok(Some(run_entry)) = runs.next_entry().await {
            if !run_entry
                .file_type()
                .await
                .map(|t| t.is_dir())
                .unwrap_or(false)
            {
                continue;
            }
            let run_id = run_entry.file_name().to_string_lossy().to_string();
            // 白名单：新批次号或旧布局版本目录；`_backup_*` 之类不当作批次
            let legacy_sr = legacy_version_sr(&run_id);
            if !run_manifest::is_valid_run_id(&run_id) && legacy_sr.is_none() {
                continue;
            }

            let work_dir = run_entry.path();
            let manifest = run_manifest::read_manifest(&manifest_file(&layout, &voice, &run_id)).ok();
            let sr = legacy_sr
                .clone()
                .or_else(|| manifest.as_ref().map(|m| m.sr.clone()));
            let epoch = manifest
                .as_ref()
                .and_then(|m| m.stages.train.epoch);
            let fingerprint = manifest
                .as_ref()
                .map(|m| m.dataset.fingerprint.clone());
            let (has_model, has_index) = batch_assets(&layout, &voice, &run_id, &work_dir);

            experiments.push(ExperimentSummary {
                voice: voice.clone(),
                run_id,
                sr,
                legacy: legacy_sr.is_some(),
                epoch,
                fingerprint,
                has_model,
                has_index,
                bytes: crate::workspace_layout::bytes_of(&work_dir),
            });
        }
    }

    // 声音名 → 批次号（批次号字典序即时间序）
    experiments.sort_by(|a, b| a.voice.cmp(&b.voice).then(a.run_id.cmp(&b.run_id)));
    Ok(experiments)
}

#[tauri::command]
pub async fn start_log_stream(
    log_path: String,
    event_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tokio::spawn(async move {
        crate::events::stream_log_file(app, log_path, event_id).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn start_train_progress_stream(
    exp_dir: String,
    total_epoch: i32,
    event_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tokio::spawn(async move {
        crate::events::stream_train_progress(app, exp_dir, total_epoch, event_id).await;
    });
    Ok(())
}
