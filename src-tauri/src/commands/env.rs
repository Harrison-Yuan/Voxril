//! 运行环境状态：首次启动引导用，判断 Python 依赖是否已安装完成。

use serde::Serialize;
use tauri::State;

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
