use crate::database::Database;
use crate::model_download::ModelDownloader;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// 直播引擎生命周期阶段。
///
/// 配合 `live_singing_stop` 确保 start/stop 在任意阶段都能收敛：
/// `Idle → Starting → Running → Stopping → Idle`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LivePhase {
    Idle,
    Starting,
    Running,
    Stopping,
}

pub struct AppState {
    pub db: Database,
    pub workspace_root: RwLock<PathBuf>,
    pub project_root: PathBuf,
    pub python_cmd: PathBuf,
    pub data_dir: PathBuf,
    /// PID for the running one-shot inference process (offline voice conversion).
    pub infer_pid: Mutex<Option<u32>>,
    pub uvr_pid: Mutex<Option<u32>>,
    /// 全项目唯一的模型下载管理器（启动自检与手动下载共用）。
    pub model_downloader: Arc<ModelDownloader>,
    /// 训练进程 PID
    pub train_pid: Mutex<Option<u32>>,
    /// 全局重任务互斥令牌：同一时刻只允许运行一个 GPU 密集型任务
    /// 取值: Some("train") | Some("uvr") | Some("infer") | None
    pub active_heavy_task: Arc<Mutex<Option<String>>>,
    /// 直播 AI 唱歌引擎的生命周期阶段
    pub live_singing_phase: Arc<RwLock<LivePhase>>,
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
        all_pids.extend(self.model_downloader.take_pids().await);
        if let Some(pid) = *self.train_pid.lock().await {
            all_pids.push(pid);
        }

        // 杀死已知进程 + 兜底孤儿 + Python 残留
        crate::process_killer::kill_all(&all_pids);

        // 停止直播引擎
        let mut phase = self.live_singing_phase.write().await;
        if *phase != LivePhase::Idle {
            if let Some(stop_tx) = self.live_singing_stop.lock().await.take() {
                let _ = stop_tx.send(());
            }
            *phase = LivePhase::Idle;
        }
        drop(phase);
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

    // Production mode: Windows installer places resources next to the exe
    #[cfg(target_os = "windows")]
    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(dir) = exe_path.parent() {
            // 上级目录资源（Tauri v2 将 ../assets 等放到 _up_/ 下）
            let up = dir.join("_up_");
            if up.join("infer").exists() || up.join("tools").exists() {
                return up;
            }
            if dir.join("infer").exists() || dir.join("tools").exists() {
                return dir.to_path_buf();
            }
        }
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

        // 数据库文件名收敛：旧版 workspace/rvc.db → state.db（单份状态文件）。
        // 必须在打开连接之前完成，WAL 分片一并改名，避免旧库残留未提交事务。
        let legacy_db = workspace_root.join("rvc.db");
        let state_db = workspace_root.join("state.db");
        if legacy_db.exists() && !state_db.exists() {
            let mut renamed: Vec<(PathBuf, PathBuf)> = Vec::new();
            for suffix in ["", "-wal", "-shm"] {
                let src = PathBuf::from(format!("{}{}", legacy_db.display(), suffix));
                if !src.exists() {
                    continue;
                }
                let dst = PathBuf::from(format!("{}{}", state_db.display(), suffix));
                match tokio::fs::rename(&src, &dst).await {
                    Ok(()) => renamed.push((src, dst)),
                    Err(e) => {
                        eprintln!("[状态] 迁移数据库文件失败 {}: {}", src.display(), e);
                        // 整体回滚，宁可继续用旧文件名，也不留下半套文件
                        for (from, to) in renamed.iter().rev() {
                            let _ = tokio::fs::rename(to, from).await;
                        }
                        break;
                    }
                }
            }
        }
        let db_path = if state_db.exists() { state_db } else { legacy_db };
        let db = Database::new(&db_path).await?;

        // Find python
        let data_dir = dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("com.rvc.webui");
        let venv_python = crate::platform::venv_python_rel();
        let python_cmd = if project_root.join(".venv").join(venv_python).exists() {
            project_root.join(".venv").join(venv_python)
        } else if project_root.join("venv").join(venv_python).exists() {
            project_root.join("venv").join(venv_python)
        } else if data_dir.join("venv").join(venv_python).exists() {
            // 首次启动由 tools/auto_setup_venv.py 创建的应用级虚拟环境
            data_dir.join("venv").join(venv_python)
        } else {
            PathBuf::from(crate::platform::default_python_cmd())
        };

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

        let model_downloader = Arc::new(ModelDownloader::new(
            project_root.clone(),
            data_dir.clone(),
        ));

        Ok(Self {
            db,
            workspace_root: RwLock::new(workspace_root),
            project_root,
            python_cmd,
            data_dir,
            infer_pid: Mutex::new(None),
            uvr_pid: Mutex::new(None),
            model_downloader,
            train_pid: Mutex::new(None),
            active_heavy_task: Arc::new(Mutex::new(None)),
            live_singing_phase: Arc::new(RwLock::new(LivePhase::Idle)),
            live_singing_stop: Mutex::new(None),
        })
    }
}
