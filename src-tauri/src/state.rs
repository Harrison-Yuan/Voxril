use crate::database::Database;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

pub struct AppState {
    pub db: Database,
    pub workspace_root: RwLock<PathBuf>,
    pub project_root: PathBuf,
    pub python_cmd: PathBuf,
    pub data_dir: PathBuf,
    /// PID for the running one-shot inference process (offline voice conversion).
    pub infer_pid: Mutex<Option<u32>>,
    pub uvr_pid: Mutex<Option<u32>>,
    pub download_pids: Mutex<Vec<u32>>,
    /// 训练进程 PID
    pub train_pid: Mutex<Option<u32>>,
    /// 全局重任务互斥令牌：同一时刻只允许运行一个 GPU 密集型任务
    /// 取值: Some("train") | Some("uvr") | Some("infer") | None
    pub active_heavy_task: Arc<Mutex<Option<String>>>,
    /// 直播 AI 唱歌引擎状态
    pub live_singing_active: Arc<RwLock<bool>>,
    pub live_singing_stop: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

impl AppState {
    /// 关闭时清理所有子进程，不放过任何僵尸
    pub async fn kill_all_children(&self) {
        let mut all_pids = Vec::new();

        // 收集所有已知 PID
        if let Some(pid) = *self.infer_pid.lock().await {
            all_pids.push(pid);
        }
        if let Some(pid) = *self.uvr_pid.lock().await {
            all_pids.push(pid);
        }
        all_pids.extend(self.download_pids.lock().await.drain(..));
        if let Some(pid) = *self.train_pid.lock().await {
            all_pids.push(pid);
        }

        // 杀死已知进程 + 兜底孤儿 + Python 残留
        crate::process_killer::kill_all(&all_pids);

        // 停止直播引擎
        let mut live_active = self.live_singing_active.write().await;
        if *live_active {
            if let Some(stop_tx) = self.live_singing_stop.lock().await.take() {
                let _ = stop_tx.send(());
            }
            *live_active = false;
        }
        drop(live_active);
    }
}

fn detect_project_root() -> PathBuf {
    let current_dir = std::env::current_dir().unwrap_or_default();

    // Development mode: running from src-tauri/ directory
    if current_dir.join("Cargo.toml").exists()
        && current_dir.join("tauri.conf.json").exists()
        && current_dir.file_name().and_then(|s| s.to_str()) == Some("src-tauri")
    {
        if let Some(parent) = current_dir.parent() {
            return parent.to_path_buf();
        }
        return current_dir;
    }

    // Production mode: check if running inside a macOS .app bundle
    if let Ok(exe_path) = std::env::current_exe() {
        let path_str = exe_path.to_string_lossy();
        if path_str.contains(".app/Contents/MacOS/") {
            let resources = exe_path
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("Resources"));
            if let Some(ref r) = resources {
                // Tauri v2 places external resources (../*) under _up_/
                let up = r.join("_up_");
                if up.join("infer").exists() || up.join("tools").exists() {
                    return up;
                }
                if r.join("infer").exists() || r.join("tools").exists() {
                    return r.clone();
                }
            }
        }
    }

    // Fallback: search upward for project markers (infer/ + web/)
    let mut dir = Some(current_dir.as_path());
    while let Some(d) = dir {
        if d.join("infer").exists() && d.join("web").exists() {
            return d.to_path_buf();
        }
        dir = d.parent();
    }

    current_dir
}

impl AppState {
    pub async fn new() -> Result<Self> {
        let project_root = detect_project_root();

        // Migrate old workspace config location to new one
        let new_config = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".rvc-workspace-config.json");
        let old_config = project_root.join(".workspace-config.json");

        if !new_config.exists() && old_config.exists() {
            if let Ok(content) = tokio::fs::read_to_string(&old_config).await {
                let _ = tokio::fs::write(&new_config, &content).await;
            }
        }

        let workspace_root = if new_config.exists() {
            let content = tokio::fs::read_to_string(&new_config).await?;
            let config: serde_json::Value = serde_json::from_str(&content)?;
            let path = PathBuf::from(config["workspaceRoot"].as_str().unwrap_or(""));
            if !path.as_os_str().is_empty() {
                path
            } else {
                dirs::desktop_dir().unwrap_or_else(|| dirs::home_dir().unwrap()).join("RVC-Workspace")
            }
        } else {
            let desktop = dirs::desktop_dir().unwrap_or_else(|| dirs::home_dir().unwrap());
            desktop.join("RVC-Workspace")
        };

        tokio::fs::create_dir_all(&workspace_root).await?;

        let db = Database::new(&workspace_root.join("rvc.db")).await?;

        // Find python
        let python_cmd = if project_root.join(".venv/bin/python").exists() {
            project_root.join(".venv/bin/python")
        } else if project_root.join("venv/bin/python").exists() {
            project_root.join("venv/bin/python")
        } else {
            PathBuf::from("python3")
        };

        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("com.rvc.webui");

        // ⚠️ DMG 修复：将 configs/ 从只读 bundle 复制到可写数据目录
        //      Python config.py 需要写 configs/v2/40k.json（use_fp32_config）
        //      在 .app 内 Resources/ 是只读的，必须复制到 data_dir。
        {
            let src = project_root.join("configs");
            let dst = data_dir.join("configs");
            if src.exists() && (!dst.exists() || dst.read_dir().map(|mut d| d.next().is_none()).unwrap_or(true)) {
                let _ = std::fs::create_dir_all(&dst);
                if let Ok(entries) = std::fs::read_dir(&src) {
                    for entry in entries.flatten() {
                        let ft = entry.file_type().ok();
                        let name = entry.file_name();
                        if ft.map(|t| t.is_file()).unwrap_or(false) {
                            let _ = std::fs::copy(entry.path(), dst.join(&name));
                        }
                    }
                }
                eprintln!("[状态] configs/ 已复制到 {}", dst.display());
            }
        }

        Ok(Self {
            db,
            workspace_root: RwLock::new(workspace_root),
            project_root,
            python_cmd,
            data_dir,
            infer_pid: Mutex::new(None),
            uvr_pid: Mutex::new(None),
            download_pids: Mutex::new(Vec::new()),
            train_pid: Mutex::new(None),
            active_heavy_task: Arc::new(Mutex::new(None)),
            live_singing_active: Arc::new(RwLock::new(false)),
            live_singing_stop: Mutex::new(None),
        })
    }
}
