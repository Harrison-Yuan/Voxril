//! 操作系统集成工具
//!
//! - 窗口状态持久化（保存/恢复位置和大小）
//! - 系统主题检测

use std::path::PathBuf;
use tauri::{Emitter, Manager};
use tauri::WebviewWindow;

const WINDOW_STATE_FILE: &str = "window_state.json";

#[derive(serde::Serialize, serde::Deserialize, Default)]
pub struct WindowState {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub maximized: bool,
}

/// 保存窗口位置/大小到 JSON 文件
pub fn save_window_state(data_dir: &PathBuf, window: &WebviewWindow) {
    if let (Ok(pos), Ok(size)) = (window.outer_position(), window.outer_size()) {
        let state = WindowState {
            x: pos.x,
            y: pos.y,
            width: size.width,
            height: size.height,
            maximized: window.is_maximizable().unwrap_or(false),
        };
        if let Ok(json) = serde_json::to_string(&state) {
            let _ = std::fs::write(data_dir.join(WINDOW_STATE_FILE), json);
        }
    }
}

/// 恢复窗口位置/大小
pub fn restore_window_state(data_dir: &PathBuf, window: &WebviewWindow) {
    let path = data_dir.join(WINDOW_STATE_FILE);
    if !path.exists() {
        return;
    }
    if let Ok(json) = std::fs::read_to_string(&path) {
        if let Ok(state) = serde_json::from_str::<WindowState>(&json) {
            use tauri::LogicalPosition;
            use tauri::LogicalSize;
            let _ = window.set_position(LogicalPosition::new(state.x as f64, state.y as f64));
            let _ = window.set_size(LogicalSize::new(state.width as f64, state.height as f64));
        }
    }
}
