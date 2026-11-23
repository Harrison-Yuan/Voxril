//! 模型下载的统一管理。
//!
//! 所有模型（基础模型、UVR 分离模型、预训练）都只由
//! `tools/check_download_models.py` 下载。启动自检与「模型管理」页的手动
//! 下载都走 `ModelDownloader::run`，共用：
//! - 同一个进程内互斥锁，避免并发拉起两个下载进程写坏同一个 `.part`
//! - 同一套事件名与日志字段
//! - 同一份 PID 记录，关闭应用时统一清理

use std::path::PathBuf;
use std::process::Stdio;

use tauri::{AppHandle, Emitter};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::sync::Mutex;

use crate::python::PythonExecutor;

/// 唯一的下载日志事件名。
pub const EVENT_LOG: &str = "model-download-log";

#[derive(Clone, serde::Serialize)]
pub struct DownloadLogPayload {
    /// 手动下载时携带调用方的事件 ID；启动自检为 None。
    pub event_id: Option<String>,
    /// 日志当前归属的模型（由脚本的 `[MODEL-BEGIN]/[MODEL-END]` 标记给出）。
    pub model_id: Option<String>,
    pub message: String,
}

/// 脚本 `[MODEL-BEGIN]` 标记内容。
#[derive(Clone, serde::Deserialize)]
struct ModelMarker {
    id: String,
    #[serde(default)]
    name: String,
}

pub struct ModelDownloader {
    executor: PythonExecutor,
    /// 串行化所有下载任务。
    gate: Mutex<()>,
    /// 正在运行的下载进程，退出应用时统一清理。
    pids: Mutex<Vec<u32>>,
    /// 当前正在处理的模型，供前端恢复进度、等待提示展示。
    active: Mutex<Option<ModelMarker>>,
}

impl ModelDownloader {
    pub fn new(project_root: PathBuf, data_dir: PathBuf) -> Self {
        Self {
            executor: PythonExecutor::new(project_root, data_dir),
            gate: Mutex::new(()),
            pids: Mutex::new(Vec::new()),
            active: Mutex::new(None),
        }
    }

    pub async fn take_pids(&self) -> Vec<u32> {
        self.pids.lock().await.drain(..).collect()
    }

    /// 当前正在下载的模型 id；空闲时返回 None。
    pub async fn active_model(&self) -> Option<String> {
        self.active.lock().await.as_ref().map(|m| m.id.clone())
    }

    /// 当前正在下载的模型展示名；空闲时返回 None。
    async fn active_display(&self) -> Option<String> {
        self.active
            .lock()
            .await
            .as_ref()
            .map(|m| if m.name.is_empty() { m.id.clone() } else { m.name.clone() })
    }

    /// 统一下载入口。
    ///
    /// `args` 直接透传给 `tools/check_download_models.py`（如
    /// `download bs_roformer --source auto`），`event_id` 仅用于前端过滤。
    pub async fn run(
        &self,
        app: &AppHandle,
        args: Vec<String>,
        event_id: Option<String>,
    ) -> Result<(), String> {
        let _guard = match self.gate.try_lock() {
            Ok(guard) => guard,
            Err(_) => {
                let message = match self.active_display().await {
                    Some(name) => format!("已有模型下载任务在进行（当前：{}），等待其完成...", name),
                    None => "已有模型下载任务在进行，等待其完成...".to_string(),
                };
                emit(app, event_id.clone(), download_target(&args), &message);
                self.gate.lock().await
            }
        };

        let mut cmd = self
            .executor
            .build_command("tools/check_download_models.py", args, 10);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = cmd
            .spawn()
            .map_err(|e| format!("启动下载进程失败: {}", e))?;

        let pid = child.id();
        if let Some(p) = pid {
            self.pids.lock().await.push(p);
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| "无法读取下载进程输出".to_string())?;
        let mut stderr_pipe = child.stderr.take();
        let stderr_task = tokio::spawn(async move {
            let mut buf = String::new();
            if let Some(mut err) = stderr_pipe.take() {
                let _ = err.read_to_string(&mut buf).await;
            }
            buf
        });

        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let mut last_error = String::new();
        let mut current_model: Option<String> = None;
        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    // 模型归属标记：只用于把后续日志路由到对应模型，不展示给用户
                    if let Some(raw) = trimmed.strip_prefix("[MODEL-BEGIN]") {
                        if let Ok(marker) = serde_json::from_str::<ModelMarker>(raw.trim()) {
                            current_model = Some(marker.id.clone());
                            *self.active.lock().await = Some(marker);
                        }
                        continue;
                    }
                    if trimmed.strip_prefix("[MODEL-END]").is_some() {
                        *self.active.lock().await = None;
                        current_model = None;
                        continue;
                    }
                    let message = if let Some(msg) = trimmed.strip_prefix("[MODEL]") {
                        msg.trim()
                    } else if let Some(msg) = trimmed.strip_prefix("[DONE]") {
                        msg.trim()
                    } else {
                        continue;
                    };
                    if is_error_line(message) {
                        last_error = message.to_string();
                    }
                    emit(app, event_id.clone(), current_model.clone(), message);
                }
                Err(_) => break,
            }
        }

        let status = child.wait().await.map_err(|e| format!("下载进程异常: {}", e))?;
        *self.active.lock().await = None;
        if let Some(p) = pid {
            self.pids.lock().await.retain(|x| *x != p);
        }
        let stderr_text = stderr_task.await.unwrap_or_default();

        if !status.success() {
            let stderr_text = stderr_text.trim();
            if !stderr_text.is_empty() {
                return Err(stderr_text.to_string());
            }
            if !last_error.is_empty() {
                return Err(last_error);
            }
            return Err("下载失败，请检查网络连接后重试".to_string());
        }
        Ok(())
    }
}

/// `download <id> ...` 中的目标模型 id；批量下载（启动自检）和清理时为 None。
fn download_target(args: &[String]) -> Option<String> {
    match args.first().map(String::as_str) {
        Some("download") => args.get(1).cloned(),
        _ => None,
    }
}

fn emit(app: &AppHandle, event_id: Option<String>, model_id: Option<String>, message: &str) {
    let _ = app.emit(
        EVENT_LOG,
        DownloadLogPayload {
            event_id,
            model_id,
            message: message.to_string(),
        },
    );
}

fn is_error_line(message: &str) -> bool {
    message.contains("失败")
        || message.contains("ERROR")
        || message.contains("error")
        || message.contains("Error")
        || message.contains("未找到")
}
