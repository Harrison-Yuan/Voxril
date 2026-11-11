use tauri::{AppHandle, Emitter, Manager};
use std::path::Path;
use std::time::{Duration, Instant};
use tokio::fs::File;
use tokio::io::{AsyncBufReadExt, BufReader, AsyncSeekExt};
use crate::utils::kill_orphan_multiprocessing;

const EXPERIMENTS_DIR: &str = "experiments";

fn parse_duration_to_secs(dur: &str) -> f64 {
    let parts: Vec<&str> = dur.split(':').collect();
    if parts.len() == 3 {
        let h: f64 = parts[0].parse().unwrap_or(0.0);
        let m: f64 = parts[1].parse().unwrap_or(0.0);
        let s: f64 = parts[2].parse().unwrap_or(0.0);
        return h * 3600.0 + m * 60.0 + s;
    }
    0.0
}

pub async fn stream_log_file(app: AppHandle, log_path: String, event_id: String) {
    let path = Path::new(&log_path);
    
    let mut last_position = 0u64;
    let mut retry_count = 0;
    const MAX_RETRIES: u32 = 60;
    let mut wait_emit_count = 0;
    
    loop {
        if !path.exists() {
            if retry_count >= MAX_RETRIES {
                let _ = app.emit(&event_id, serde_json::json!({
                    "type": "error",
                    "message": "Log file not found after waiting"
                }));
                break;
            }
            retry_count += 1;
            tokio::time::sleep(Duration::from_secs(1)).await;
            continue;
        }

        let file = match File::open(path).await {
            Ok(f) => f,
            Err(e) => {
                let _ = app.emit(&event_id, serde_json::json!({
                    "type": "error",
                    "message": format!("Failed to open log file: {}", e)
                }));
                break;
            }
        };

        let mut reader = BufReader::new(file);
        
        if let Err(e) = reader.seek(std::io::SeekFrom::Start(last_position)).await {
            let _ = app.emit(&event_id, serde_json::json!({
                "type": "error",
                "message": format!("Failed to seek log file: {}", e)
            }));
            break;
        }

        let mut lines = reader.lines();
        let mut has_new_content = false;

        while let Ok(Some(line)) = lines.next_line().await {
            has_new_content = true;
            let _ = app.emit(&event_id, serde_json::json!({
                "type": "log",
                "text": line
            }));
            last_position += line.len() as u64 + 1;
        }

        if !has_new_content {
            wait_emit_count += 1;
            if wait_emit_count == 10 {
                let _ = app.emit(&event_id, serde_json::json!({
                    "type": "log",
                    "text": "⏳ 预处理正在加载 Python 依赖，请耐心等待..."
                }));
            } else if wait_emit_count == 30 {
                let _ = app.emit(&event_id, serde_json::json!({
                    "type": "log",
                    "text": "⏳ 依赖加载时间较长，请确认 Python 环境是否完整"
                }));
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
}

pub async fn stream_train_progress(
    app: AppHandle,
    exp_dir: String,
    total_epoch: i32,
    event_id: String,
) {
    let state = app.state::<crate::state::AppState>();
    let mut last_epoch = 0;
    let mut first_epoch_at: Option<Instant> = None;
    let mut recovery_elapsed_offset = Duration::from_secs(0);
    let start_time = Instant::now();
    let mut last_loss = String::new();
    let mut no_change_count = 0;
    const MAX_NO_CHANGE: u32 = 300;
    let mut file_position: u64 = 0;
    let error_keywords = ["Traceback (most recent call last)", "RuntimeError", "Killed", "exitcode", "SIGTERM", "out of memory", "Segmentation fault", "Bus error"];
    let mut epoch_durations: Vec<f64> = Vec::new();

    // Pre-scan the log to skip stale data from previous runs. Only read after the last
    // restart marker, so resumed training shows the correct (lower) epoch number.
    {
        let workspace_root = state.workspace_root.read().await;
        let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
        let train_log_path = exp_path.join("train.log");
        drop(workspace_root);
        if train_log_path.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&train_log_path).await {
                if let Some(pos) = content.rfind("[backend] training process launching...") {
                    let after_newline = content[pos..].find('\n').map(|n| pos + n + 1).unwrap_or(pos);
                    file_position = after_newline as u64;
                }
            }
        }
    }

    loop {
        tokio::time::sleep(Duration::from_secs(2)).await;

        let workspace_root = state.workspace_root.read().await;
        let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
        let exp_path_str = exp_path.to_string_lossy();
        let train_log_path = exp_path.join("train.log");
        drop(workspace_root);

        let output = tokio::process::Command::new("pgrep")
            .args(&["-f", &format!("train.py.*{}", exp_path_str)])
            .output()
            .await;

        let is_running = output.map(|o| o.status.success() && !o.stdout.is_empty()).unwrap_or(false);

        if train_log_path.exists() {
            if last_epoch == 0 && first_epoch_at.is_none() {
                if let Ok(meta) = tokio::fs::metadata(&train_log_path).await {
                    if let Ok(mtime) = meta.modified() {
                        if let Ok(since) = mtime.elapsed() {
                            recovery_elapsed_offset = since;
                        }
                    }
                }
            }

            if let Ok(file) = File::open(&train_log_path).await {
                let mut reader = BufReader::new(file);
                if reader.seek(std::io::SeekFrom::Start(file_position)).await.is_ok() {
                    let mut lines = reader.lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        file_position += line.len() as u64 + 1;

                        let mut is_error = false;
                        for kw in &error_keywords {
                            if line.contains(kw) {
                                is_error = true;
                                break;
                            }
                        }
                        if is_error {
                            let _ = app.emit(&event_id, serde_json::json!({
                                "type": "error",
                                "message": format!("训练异常: {}", line)
                            }));
                        }

                        if line.contains("Epoch:") {
                            if let Some(epoch_str) = line.split("Epoch:").nth(1) {
                                if let Some(epoch_num) = epoch_str.trim().split_whitespace().next() {
                                    if let Ok(epoch) = epoch_num.parse::<i32>() {
                                        if last_epoch == 0 && epoch == 1 && first_epoch_at.is_none() {
                                            first_epoch_at = Some(Instant::now());
                                        }
                                        last_epoch = epoch;
                                    }
                                }
                            }
                            if let (Some(open), Some(close)) = (line.find('('), line.rfind(')')) {
                                if close > open + 1 {
                                    let dur_str = &line[open + 1..close];
                                    let secs = parse_duration_to_secs(dur_str);
                                    if secs > 0.0 {
                                        epoch_durations.push(secs);
                                        if epoch_durations.len() > 20 {
                                            epoch_durations.remove(0);
                                        }
                                    }
                                }
                            }
                        }

                        if line.contains("loss_disc=") || line.contains("loss_gen=") {
                            let parts: Vec<&str> = line.split(',').collect();
                            let mut loss_parts = Vec::new();
                            for part in parts.iter().take(3) {
                                if let Some(value) = part.split('=').nth(1) {
                                    loss_parts.push(value.trim());
                                }
                            }
                            if !loss_parts.is_empty() {
                                last_loss = loss_parts.join(", ");
                            }
                        }
                    }
                }
            }
        }

        let current_status = if is_running {
            "running"
        } else if last_epoch >= total_epoch {
            "done"
        } else {
            "waiting"
        };

        let avg_epoch_time = if !epoch_durations.is_empty() {
            (epoch_durations.iter().sum::<f64>() / epoch_durations.len() as f64) as i64
        } else if let Some(ft) = first_epoch_at {
            let wall_elapsed = ft.elapsed().saturating_add(recovery_elapsed_offset).as_secs_f64();
            (wall_elapsed / last_epoch.max(1) as f64) as i64
        } else if recovery_elapsed_offset > Duration::from_secs(0) && last_epoch > 0 {
            (recovery_elapsed_offset.as_secs_f64() / last_epoch.max(1) as f64) as i64
        } else {
            0i64
        };

        let estimated_seconds = if avg_epoch_time > 0 {
            ((total_epoch - last_epoch) as i64 * avg_epoch_time).max(0)
        } else {
            0i64
        };

        let mut payload = serde_json::json!({
            "type": "progress",
            "epoch": last_epoch,
            "percent": (last_epoch as f64 / total_epoch as f64 * 100.0),
            "loss": last_loss,
            "status": current_status,
            "estimated_seconds": estimated_seconds,
            "avg_epoch_time": avg_epoch_time,
        });

        if current_status == "running" && last_epoch > 0 {
            let elapsed_total = start_time.elapsed().as_secs() as i64;
            payload["elapsed_seconds"] = serde_json::json!(elapsed_total);
            payload["total_epoch"] = serde_json::json!(total_epoch);
        }

        let _ = app.emit(&event_id, &payload);

        if !is_running {
            no_change_count += 1;
            if no_change_count == 1 && last_epoch > 0 && last_epoch < total_epoch {
                if let Ok(content) = tokio::fs::read_to_string(&train_log_path).await {
                    let tail_lines: Vec<&str> = content.lines().rev().take(50).collect();
                    for kw in &error_keywords {
                        if let Some(line) = tail_lines.iter().find(|l| l.contains(kw)) {
                            let _ = app.emit(&event_id, serde_json::json!({
                                "type": "error",
                                "message": format!("训练异常退出: {}", line)
                            }));
                            break;
                        }
                    }
                }
            }
            if no_change_count >= MAX_NO_CHANGE || last_epoch >= total_epoch {
                let _ = app.emit(&event_id, serde_json::json!({
                    "type": "progress",
                    "epoch": last_epoch,
                    "percent": 100.0,
                    "loss": last_loss,
                    "status": "done"
                }));
                // Emit a separate event so frontend knows to auto-trigger index
                let _ = app.emit(&event_id, serde_json::json!({
                    "type": "train_complete",
                    "epoch": last_epoch,
                    "total_epoch": total_epoch
                }));
                break;
            }
        } else {
            no_change_count = 0;
        }

        if current_status == "done" {
            break;
        }
    }
    tokio::task::spawn_blocking(kill_orphan_multiprocessing);
}
