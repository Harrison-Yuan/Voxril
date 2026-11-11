//! macOS Dock 菜单 + 菜单栏
//!
//! - 屏幕顶部菜单栏: Voxril > 导航 > 窗口
//! - 程序坞图标: 右键/长按弹出快捷菜单（含最近项目和快捷操作）
//! - 点击 Dock 图标: 聚焦时通知前端

use tauri::Emitter;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

/// 注册 macOS 菜单栏和 Dock 菜单
pub fn setup_dock(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    let handle = app.handle().clone();

    // ── 屏幕顶部菜单栏 ──
    let file_sub = Submenu::with_items(app, "Voxril", true, &[
        &MenuItem::with_id(app, "open_project", "打开项目...", true, None::<&str>)?,
        &MenuItem::with_id(app, "new_project", "新建项目", true, None::<&str>)?,
        &PredefinedMenuItem::separator(app)?,
        &PredefinedMenuItem::quit(app, Some("退出 Voxril"))?,
    ])?;

    let nav_sub = Submenu::with_items(app, "导航", true, &[
        &MenuItem::with_id(app, "nav_infer", "语音转换", true, None::<&str>)?,
        &MenuItem::with_id(app, "nav_uvr", "人声分离", true, None::<&str>)?,
        &MenuItem::with_id(app, "nav_train", "模型训练", true, None::<&str>)?,
    ])?;

    let win_sub = Submenu::with_items(app, "窗口", true, &[
        &PredefinedMenuItem::minimize(app, Some("最小化"))?,
        &MenuItem::with_id(app, "settings", "设置...", true, None::<&str>)?,
    ])?;

    let menu_bar = Menu::with_items(app, &[&file_sub, &nav_sub, &win_sub])?;
    app.set_menu(menu_bar)?;

    // ── 菜单事件 ──
    let nav_handle = handle.clone();
    app.on_menu_event(move |_app, event| {
        let id = event.id().0.as_str();
        let path = match id {
            "open_project" => Some("/projects"),
            "nav_infer" => Some("/infer"),
            "nav_uvr" => Some("/uvr"),
            "nav_train" => Some("/train"),
            "new_project" => Some("/train"),
            "settings" => Some("/settings"),
            _ => None,
        };
        if let Some(p) = path {
            let _ = nav_handle.emit("navigate", p);
        }
    });

    // ── Dock 菜单（右键/长按程序坞图标） ──
    // 当前版本用顶部菜单栏替代，Dock 点击聚焦由 macOS 原生处理

    Ok(())
}
