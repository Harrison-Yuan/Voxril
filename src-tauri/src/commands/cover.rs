//! 一键翻唱：人声分离 → 声音转换 → 混音的串行编排。
//!
//! 三个阶段直接复用 uvr / infer 的执行体，通过同一个 `cover-progress` 事件通道
//! 推送进度，靠 `stage` 字段区分阶段。全局重任务令牌只由本命令占用一次（`cover`），
//! 避免阶段之间出现锁交接的竞态。

use crate::commands::infer::{self, ConvertRequest};
use crate::commands::uvr::{self, SeparateRequest};
use crate::state::AppState;
use crate::workspace_layout::WorkspaceLayout;
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::{Emitter, State};

/// 一键流程默认使用的人声分离模型（与分离页默认值一致）。
const DEFAULT_SEPARATOR_MODEL: &str = "model_bs_roformer_ep_317_sdr_12.9755.ckpt";
const COVER_EVENT: &str = "cover-progress";
const COVER_LOCK: &str = "cover";

/// 生成配置。未传时使用默认值，即“不配置直接一键生成”。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverParams {
    /// 变调（半音）。
    pub transpose: i32,
    /// 音高提取算法。
    pub f0_method: String,
    /// 检索特征比例。
    pub index_rate: f64,
    /// 混音时人声增益（dB）。
    pub vocal_gain: f32,
}

impl Default for CoverParams {
    fn default() -> Self {
        Self {
            transpose: 0,
            f0_method: "rmvpe".to_string(),
            index_rate: 0.75,
            vocal_gain: 6.0,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct CoverRequest {
    /// 具体声音权重，来自 `list_cover_voices` 的 `model_name`。
    pub model_name: String,
    /// 原唱歌曲路径。
    pub song_path: String,
    /// 生成配置；缺省即使用默认配置。
    #[serde(default)]
    pub params: Option<CoverParams>,
}

#[derive(Debug, Serialize)]
pub struct CoverResponse {
    pub ok: bool,
    pub message: String,
    /// 最终翻唱作品（人声 + 伴奏）。
    pub mixed_path: String,
}

#[derive(Debug, Serialize)]
pub struct CoverParamsResponse {
    pub params: CoverParams,
}

#[derive(Debug, Serialize)]
pub struct CoverVoice {
    pub voice: String,
    /// 该声音下最适合推理的权重标识。
    pub model_name: String,
    pub updated_at: i64,
}

#[derive(Debug, Serialize, Clone)]
struct CoverStagePayload {
    event_id: String,
    stage: String,
    kind: String,
    percent: i32,
    message: String,
}

fn emit_stage(
    app: &tauri::AppHandle,
    event_id: &str,
    stage: &str,
    percent: i32,
    message: impl Into<String>,
) {
    let _ = app.emit(
        COVER_EVENT,
        CoverStagePayload {
            event_id: event_id.to_string(),
            stage: stage.to_string(),
            kind: "progress".to_string(),
            percent,
            message: message.into(),
        },
    );
}

#[tauri::command]
pub async fn cover_generate(
    request: CoverRequest,
    event_id: String,
    state: State<'_, AppState>,
    app: tauri::AppHandle,
) -> Result<CoverResponse, String> {
    // 全局重任务互斥：与训练/分离/转换共用一把锁，训练进程已退出时自动清除僵尸锁。
    {
        let mut task = state.active_heavy_task.lock().await;
        if let Some(ref running) = *task {
            let zombie_train = running == "train" && !is_train_process_alive();
            if !zombie_train {
                return Err(format!(
                    "系统当前有任务正在运行: [{}]，请等待其完成后再生成翻唱。",
                    running
                ));
            }
            eprintln!("[cover] 检测到训练进程已退出，自动清除任务锁");
            *task = None;
        }
        *task = Some(COVER_LOCK.to_string());
    }

    let result = run_cover(&request, &event_id, &state, &app).await;

    // 无论成功失败：清掉本阶段可能残留的进程并释放令牌。
    {
        let mut pids = Vec::new();
        if let Some(pid) = state.uvr_pid.lock().await.take() {
            pids.push(pid);
        }
        if let Some(pid) = state.infer_pid.lock().await.take() {
            pids.push(pid);
        }
        for pid in pids {
            let _ = std::process::Command::new("kill")
                .args(["-9", &pid.to_string()])
                .output();
        }
        let mut lock = state.active_heavy_task.lock().await;
        if lock.as_deref() == Some(COVER_LOCK) {
            *lock = None;
        }
    }

    result
}

#[tauri::command]
pub async fn cancel_cover(state: State<'_, AppState>) -> Result<(), String> {
    let mut pids = Vec::new();
    if let Some(pid) = state.uvr_pid.lock().await.take() {
        pids.push(pid);
    }
    if let Some(pid) = state.infer_pid.lock().await.take() {
        pids.push(pid);
    }
    for pid in pids {
        let _ = std::process::Command::new("kill")
            .args(["-9", &pid.to_string()])
            .output();
    }
    let mut lock = state.active_heavy_task.lock().await;
    if lock.as_deref() == Some(COVER_LOCK) {
        *lock = None;
    }
    Ok(())
}

async fn run_cover(
    request: &CoverRequest,
    event_id: &str,
    state: &State<'_, AppState>,
    app: &tauri::AppHandle,
) -> Result<CoverResponse, String> {
    let params = request.params.clone().unwrap_or_default();
    let song_path = request.song_path.trim().to_string();
    if song_path.is_empty() {
        return Err("请选择原唱歌曲".to_string());
    }
    if request.model_name.trim().is_empty() {
        return Err("请选择声音模型".to_string());
    }

    // 每次任务独立目录，避免同名歌曲的人声/伴奏互相覆盖。
    let job_dir = {
        let workspace = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace).cover_job_dir(&job_id_for(&song_path))
    };
    std::fs::create_dir_all(&job_dir).map_err(|e| format!("创建翻唱任务目录失败: {}", e))?;

    // 1/3 人声分离
    emit_stage(app, event_id, "separate", 0, "正在分离人声与伴奏...");
    let separated = uvr::do_separate_vocals_streaming(
        SeparateRequest {
            model_name: DEFAULT_SEPARATOR_MODEL.to_string(),
            input_path: song_path,
            output_dir: Some(job_dir.to_string_lossy().to_string()),
            agg: 10,
            format: "wav".to_string(),
        },
        event_id.to_string(),
        COVER_EVENT,
        "separate",
        state,
        app.clone(),
    )
    .await?;

    let vocal_path = separated.vocal_files.first().cloned().unwrap_or_default();
    let instrumental_path = separated
        .instrumental_files
        .first()
        .cloned()
        .unwrap_or_default();
    if !separated.ok || vocal_path.is_empty() || instrumental_path.is_empty() {
        return Err(format!(
            "人声分离失败：{}",
            if separated.message.is_empty() {
                "未得到人声或伴奏".to_string()
            } else {
                separated.message
            }
        ));
    }
    emit_stage(app, event_id, "separate", 100, "人声与伴奏已分离");

    // 2/3 声音转换
    emit_stage(app, event_id, "convert", 0, "正在转换成目标音色...");
    let converted = infer::do_convert_voice_streaming(
        ConvertRequest {
            model_name: request.model_name.trim().to_string(),
            source_path: vocal_path,
            transpose: params.transpose,
            f0_method: params.f0_method.clone(),
            index_rate: params.index_rate,
            filter_radius: 3,
            resample_sr: 0,
            rms_mix_rate: 1.0,
            protect: 0.33,
        },
        event_id.to_string(),
        COVER_EVENT,
        "convert",
        state,
        app.clone(),
    )
    .await?;
    if !converted.ok || converted.output_path.is_empty() {
        return Err(format!("声音转换失败：{}", converted.message));
    }
    emit_stage(app, event_id, "convert", 100, "音色转换完成");

    // 3/3 混音
    emit_stage(app, event_id, "merge", 0, "正在合成翻唱作品...");
    let merged = infer::do_merge_audio(
        converted.output_path,
        instrumental_path,
        params.vocal_gain,
        state,
    )
    .await?;
    if !merged.ok || merged.output_path.is_empty() {
        return Err(format!("合成失败：{}", merged.message));
    }
    emit_stage(app, event_id, "merge", 100, "翻唱作品已生成");

    Ok(CoverResponse {
        ok: true,
        message: "翻唱生成完成".to_string(),
        mixed_path: merged.output_path,
    })
}

/// 列出可用于翻唱的声音：按声音聚合，只保留最适合推理的一份权重。
#[tauri::command]
pub async fn list_cover_voices(state: State<'_, AppState>) -> Result<Vec<CoverVoice>, String> {
    let entries = infer::collect_models(&state).await?;
    let assets_root = {
        let workspace = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace).assets_root()
    };

    // voice -> (model_name, tier, epoch, updated_at)
    let mut best: HashMap<String, (String, u8, i64, i64)> = HashMap::new();
    for entry in entries {
        let voice = entry.split('/').next().unwrap_or(&entry).to_string();
        let file = if entry.ends_with(".pth") {
            assets_root.join(&entry)
        } else {
            assets_root.join(&entry).join("model.pth")
        };
        let updated_at = std::fs::metadata(&file)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (tier, epoch) = model_preference(&entry);

        let candidate = (entry, tier, epoch, updated_at);
        let is_better = match best.get(&voice) {
            Some(current) => (tier, epoch, updated_at) > (current.1, current.2, current.3),
            None => true,
        };
        if is_better {
            best.insert(voice, candidate);
        }
    }

    let mut voices: Vec<CoverVoice> = best
        .into_iter()
        .map(|(voice, (model_name, _, _, updated_at))| CoverVoice {
            voice,
            model_name,
            updated_at,
        })
        .collect();
    voices.sort_by(|a, b| b.updated_at.cmp(&a.updated_at).then(a.voice.cmp(&b.voice)));
    Ok(voices)
}

/// 读取某声音的默认生成配置；未保存过则返回默认值。
#[tauri::command]
pub async fn get_cover_params(
    voice: String,
    state: State<'_, AppState>,
) -> Result<CoverParamsResponse, String> {
    match state.db.get_setting(&cover_params_key(&voice)).await {
        Ok(Some(raw)) => Ok(CoverParamsResponse {
            params: serde_json::from_str(&raw).unwrap_or_default(),
        }),
        Ok(None) => Ok(CoverParamsResponse {
            params: CoverParams::default(),
        }),
        Err(e) => Err(format!("读取生成配置失败: {}", e)),
    }
}

#[tauri::command]
pub async fn save_cover_params(
    voice: String,
    params: CoverParams,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let raw = serde_json::to_string(&params).map_err(|e| e.to_string())?;
    state
        .db
        .set_setting(cover_params_key(&voice), raw)
        .await
        .map_err(|e| format!("保存生成配置失败: {}", e))
}

fn cover_params_key(voice: &str) -> String {
    format!("cover_params:{}", voice)
}

/// 权重优先级：归档权重 > 训练检查点 > 旧布局 model.pth；同层再比 epoch 与修改时间。
fn model_preference(entry: &str) -> (u8, i64) {
    let tier = if entry.contains("/weights/") {
        3
    } else if entry.contains("/checkpoints/") {
        2
    } else {
        1
    };
    (tier, checkpoint_epoch(entry))
}

/// 从文件名中取最大的 `_e<数字>` / `-e<数字>`，解析不到返回 0。
///
/// 两种分隔符都要识别：旧布局检查点形如 `harrison/2_40k/checkpoints/x_e12_s2400.pth`，
/// 而本项目的训练产物命名是 `{voice}-{run_id}-e{epoch}.pth`（如 `harrison-261122-0156-e150.pth`）。
fn checkpoint_epoch(name: &str) -> i64 {
    let bytes = name.as_bytes();
    let mut best = 0i64;
    let mut i = 0;
    while i + 1 < bytes.len() {
        if (bytes[i] == b'_' || bytes[i] == b'-') && bytes[i + 1] == b'e' {
            let mut j = i + 2;
            let mut value = 0i64;
            let mut has_digit = false;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                value = value * 10 + (bytes[j] - b'0') as i64;
                j += 1;
                has_digit = true;
            }
            if has_digit {
                best = best.max(value);
                i = j;
                continue;
            }
        }
        i += 1;
    }
    best
}

/// 任务目录名：时间戳 + 歌名，便于在磁盘上辨认。分隔符与控制字符会被替换。
fn job_id_for(song_path: &str) -> String {
    let stem = std::path::Path::new(song_path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("song");
    let safe: String = stem
        .chars()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '_'
            } else {
                c
            }
        })
        .take(40)
        .collect();
    let safe = safe.trim();
    format!(
        "{}-{}",
        Local::now().format("%y%m%d-%H%M%S"),
        if safe.is_empty() { "song" } else { safe }
    )
}

fn is_train_process_alive() -> bool {
    std::process::Command::new("pgrep")
        .args(["-f", "train\\.py"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_epoch_takes_max_number() {
        assert_eq!(checkpoint_epoch("voices/harrison/x_e12_s2400.pth"), 12);
        assert_eq!(checkpoint_epoch("harrison-261122-0156-e150.pth"), 150);
        assert_eq!(checkpoint_epoch("model.pth"), 0);
    }

    #[test]
    fn model_preference_ranks_archive_weights_first() {
        assert!(model_preference("v/weights/a.pth") > model_preference("v/v2_40k/checkpoints/a_e100.pth"));
        assert!(model_preference("v/v2_40k/checkpoints/a_e100.pth") > model_preference("v/v2_40k"));
    }

    #[test]
    fn job_id_is_single_path_component() {
        let id = job_id_for("/tmp/枪火/原唱 01.mp3");
        assert!(!id.contains('/'));
        assert!(id.ends_with("原唱 01"));
    }
}
