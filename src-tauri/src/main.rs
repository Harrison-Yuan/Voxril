#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_engine;
mod cleanup;
mod commands;
mod database;
mod decode_audio;
mod dock_menu;
mod events;
mod os_integration;
mod process_killer;
mod python;
mod state;
mod utils;

use state::AppState;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::Emitter;
use tauri::Manager;

/// PID 锁文件 — 进程退出时自动清理
struct AppLock {
    path: PathBuf,
}
impl Drop for AppLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[tokio::main]
async fn main() {
    // ⚠️ 子命令路由：`voxril decode-audio --input ... --sr ...`
    //     用 Rust symphonia 解码音频，输出 raw f32 PCM 到 stdout。
    //     替换 Python ffmpeg/PyAV 子进程，消除系统依赖。
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(|s| s.as_str()) == Some("decode-audio") {
        decode_audio::run(&args);
        return;
    }

    // ═══════════════════════════════════════════════
    //  单实例锁定：防止用户重复打开应用
    // ═══════════════════════════════════════════════
    let lock_dir = dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.voxril.app");
    let _ = std::fs::create_dir_all(&lock_dir);
    let lock_path = lock_dir.join("voxril.lock");
    // 检查锁文件：如果存在但对应进程已死，自动清理
    if lock_path.exists() {
        let stale = if let Ok(content) = std::fs::read_to_string(&lock_path) {
            if let Ok(pid) = content.trim().parse::<u32>() {
                std::process::Command::new("kill")
                    .args(["-0", &pid.to_string()]) // kill -0 只检测进程存在性
                    .output()
                    .map(|o| !o.status.success())
                    .unwrap_or(true)
            } else { true }
        } else { true };
        if stale {
            let _ = std::fs::remove_file(&lock_path);
        }
    }
    let _lock = match File::create_new(&lock_path) {
        Ok(mut f) => {
            let _ = f.write_all(format!("{}", std::process::id()).as_bytes());
            Some(AppLock { path: lock_path })
        }
        Err(_) => {
            eprintln!("Voxril 已在运行中");
            std::process::exit(0);
        }
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let app_state = AppState::new().await.expect("Failed to initialize app state");
    let project_root = app_state.project_root.clone();
    let data_dir = app_state.data_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .setup(move |app| {
            // macOS Dock 菜单 + 菜单栏
            if let Err(e) = dock_menu::setup_dock(app) {
                eprintln!("[Dock] 初始化失败: {}", e);
            }

            let handle = app.handle().clone();
              let executor = Arc::new(python::PythonExecutor::new(project_root, data_dir.clone()));
              let model_executor = executor.clone();

              // Cleanup live singing + download processes on window close
              let handle_clone = handle.clone();
              let data_dir_ws = data_dir.clone();
              if let Some(window) = app.get_webview_window("main") {
                  // 恢复窗口状态
                  crate::os_integration::restore_window_state(&data_dir, &window);
                  let win_for_close = window.clone();
                  window.on_window_event(move |event| {
                      if let tauri::WindowEvent::CloseRequested { .. } = event {
                          // 保存窗口状态
                          crate::os_integration::save_window_state(&data_dir_ws, &win_for_close);
                          let rt = tokio::runtime::Runtime::new().unwrap();
                          rt.block_on(async {
                              let state = handle_clone.state::<AppState>();
                              state.kill_all_children().await;
                          });
                      }
                  });
              }

             tokio::spawn(async move {
                 let setup_h = handle.clone();

                 // ═══════════════════════════════════════════════
                 //  启动时自动清理垃圾数据
                 // ═══════════════════════════════════════════════
                 {
                     let state = handle.state::<AppState>();
                     let project = state.project_root.clone();
                     let ws = state.workspace_root.read().await;
                     crate::cleanup::run_cleanup(&project, &ws);
                     // 清理过大的旧日志文件（已有逻辑，保留）
                     let exp_root = ws.join("experiments");
                     if exp_root.exists() {
                         crate::utils::truncate_logs_in_dir(&exp_root);
                     }
                 }
                 let _ = executor
                     .run_script_streaming("tools/auto_setup_venv.py", vec![], move |line| {
                         if line.starts_with("[SETUP]") {
                             let _ = setup_h.emit("setup-log", &line);
                         }
                     })
                     .await;

                 let cleanup_h = handle.clone();
                 let _ = model_executor
                     .run_script_streaming(
                         "tools/check_download_models.py",
                         vec!["clean".to_string()],
                         move |line| {
                             if line.starts_with("[MODEL]") {
                                 let _ = cleanup_h.emit("model-download-log", line);
                             }
                         },
                     )
                     .await;

                 let model_h = handle.clone();
                 let done_h = handle.clone();
                 let _ = model_executor
                     .run_script_streaming(
                         "tools/check_download_models.py",
                         vec![],
                         move |line| {
                             if line.starts_with("[MODEL]") || line.starts_with("[DONE]") {
                                 let _ = model_h.emit("model-download-log", line);
                             }
                         },
                     )
                     .await;
                 let _ = done_h.emit("model-download-done", ());
             });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::get_workspace,
            commands::workspace::set_workspace,
            commands::workspace::select_workspace,
            commands::workspace::deep_clean_cache,
            commands::config::get_config,
            commands::config::get_all_configs,
            commands::config::save_config,
            commands::config::delete_config,
            commands::config::get_last_experiment,
            commands::config::set_last_experiment,
            commands::history::get_all_history,
            commands::history::add_history,
            commands::history::update_history,
            commands::history::delete_history,
            commands::history::delete_all_history,
            commands::workflow::get_workflow,
            commands::workflow::save_workflow,
            commands::workflow::delete_workflow,
            commands::train::preprocess_dataset,
            commands::train::extract_f0_features,
            commands::train::extract_audio_features,
            commands::train::train_model,
            commands::train::stop_training,
            commands::train::train_index,
            commands::train::build_index_cache,
            commands::train::get_train_status,
            commands::train::check_preprocess_status,
            commands::train::get_experiment_path,
            commands::train::list_experiments,
            commands::train::start_log_stream,
            commands::train::start_train_progress_stream,
            commands::infer::convert_voice_streaming,
            commands::infer::cancel_infer,
            commands::infer::merge_audio,
            commands::infer::list_models,
            commands::infer::delete_model,
            commands::infer::import_model,
            commands::infer::get_base_models_status,
            commands::infer::download_base_model,
            commands::infer::delete_base_model,
            commands::infer::get_model_storage_paths,
            commands::uvr::separate_vocals,
            commands::uvr::separate_vocals_streaming,
            commands::uvr::cancel_uvr,
            commands::file::file_exists,
            commands::file::list_artifacts,
            commands::file::get_checkpoint_iteration,
            commands::file::read_text_file,
            commands::file::delete_file,
            commands::file::list_audio_files,
            commands::file::open_in_finder,
            commands::file::open_in_vscode,
            commands::file::delete_training_result,
            commands::file::clear_training_artifacts,
            commands::audio::get_audio_devices,
            commands::live::start_live_singing,
            commands::live::set_live_f0_up_key,
            commands::live::set_live_index_rate,
            commands::live::set_audio_source,
            commands::live::set_accompaniment_pause,
            commands::live::seek_accompaniment,
            commands::live::get_accompaniment_status,
            commands::live::play_cover,
            commands::live::preload_cover,
            commands::live::stop_cover,
            commands::live::play_accompaniment,
            commands::live::play_current_accompaniment,
            commands::live::stop_accompaniment,
            commands::live::set_lrc_path,
            commands::live::stop_live_singing,
            commands::live::get_live_singing_status,
            commands::live::list_live_models,
            commands::live::export_pth_to_onnx,
            commands::live::extract_f0_profile,
            commands::live::extract_lyrics,
            commands::live::search_lyrics,
            commands::live::download_lyrics,
            commands::live::open_desktop_lyrics,
            commands::live::read_lrc_file,
            commands::live::list_separated_songs,
            commands::live::get_model_index_path,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
