#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio_engine;
mod cleanup;
mod commands;
mod database;
mod decode_audio;
mod download_settings;
#[cfg(target_os = "macos")]
mod dock_menu;
mod events;
mod model_download;
mod os_integration;
mod platform;
mod process_killer;
mod python;
mod run_manifest;
mod state;
mod utils;
mod workspace_layout;

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

/// Windows 下 ort 使用 load-dynamic 模式，需要 ORT_DYLIB_PATH 指向
/// onnxruntime.dll。优先复用 Python 环境自带的 DLL，避免依赖外网 CDN。
#[cfg(target_os = "windows")]
fn setup_onnxruntime_dylib() {
    if std::env::var_os("ORT_DYLIB_PATH").is_some() {
        return;
    }
    let site_pkg = PathBuf::from("Lib").join("site-packages").join("onnxruntime").join("capi").join("onnxruntime.dll");
    let mut candidates = Vec::new();
    if let Some(data_dir) = dirs::data_dir() {
        candidates.push(data_dir.join("com.rvc.webui").join("venv").join(&site_pkg));
    }
    if let Ok(cwd) = std::env::current_dir() {
        candidates.push(cwd.join(".venv").join(&site_pkg));
        candidates.push(cwd.join("venv").join(&site_pkg));
        if let Some(parent) = cwd.parent() {
            candidates.push(parent.join(".venv").join(&site_pkg));
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("onnxruntime.dll"));
        }
    }
    if let Some(path) = candidates.into_iter().find(|p| p.exists()) {
        std::env::set_var("ORT_DYLIB_PATH", &path);
        eprintln!("[ORT] 使用 {}", path.display());
    } else {
        eprintln!("[ORT] 未找到 onnxruntime.dll，实时变声功能不可用");
    }
}

#[cfg(not(target_os = "windows"))]
fn setup_onnxruntime_dylib() {}

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
                !crate::platform::process_alive(pid)
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
        Err(e) => {
            if lock_path.exists() {
                // 另一个实例确实持有锁
                eprintln!("Voxril 已在运行中");
                std::process::exit(0);
            }
            // 目录/文件不可写（受限环境）时降级为不加锁启动，避免完全无法打开
            eprintln!("[启动] 无法创建单实例锁 {:?}: {}，继续启动", lock_path, e);
            None
        }
    };

    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    setup_onnxruntime_dylib();

    let app_state = AppState::new().await.expect("Failed to initialize app state");
    let project_root = app_state.project_root.clone();
    let data_dir = app_state.data_dir.clone();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .setup(move |app| {
            // macOS Dock 菜单 + 菜单栏（其他平台使用系统默认菜单）
            #[cfg(target_os = "macos")]
            if let Err(e) = dock_menu::setup_dock(app) {
                eprintln!("[Dock] 初始化失败: {}", e);
            }

            let handle = app.handle().clone();
              let executor = Arc::new(python::PythonExecutor::new(project_root, data_dir.clone()));
              let downloader = {
                  let state = handle.state::<AppState>();
                  state.model_downloader.clone()
              };

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

                 // 启动自动下载也要遵循用户在设置里选择的下载源
                 let (download_args, mut setup_args) = {
                     let state = handle.state::<AppState>();
                     let settings = crate::download_settings::DownloadSettings::load(&state.db).await;
                     (settings.cli_args(), settings.pip_cli_args())
                 };
                 // 让安装脚本复用当前解释器（优先内置 Python），不再依赖系统 Python
                 setup_args.insert(0, executor.python_cmd().to_string());
                 setup_args.insert(0, "--python".to_string());

                 // ═══════════════════════════════════════════════
                 //  启动时自动清理垃圾数据
                 // ═══════════════════════════════════════════════
                 {
                     let state = handle.state::<AppState>();
                     let project = state.project_root.clone();
                     let ws = state.workspace_root.read().await;
                     crate::cleanup::run_cleanup(&project, &ws);
                     // 清理过大的旧日志文件（已有逻辑，保留）
                     let exp_root = crate::workspace_layout::WorkspaceLayout::detect(&*ws).runs_root();
                     if exp_root.exists() {
                         crate::utils::truncate_logs_in_dir(&exp_root);
                     }
                 }
                 let _ = executor
                     .run_script_streaming("tools/auto_setup_venv.py", setup_args, move |line| {
                         if line.starts_with("[SETUP]") {
                             let _ = setup_h.emit("setup-log", &line);
                         }
                     })
                     .await;

                 let cleanup_args = {
                     let mut args = vec!["clean".to_string()];
                     args.extend(download_args.clone());
                     args
                 };
                 let _ = downloader.run(&handle, cleanup_args, None).await;

                 let _ = downloader.run(&handle, download_args, None).await;
                 let _ = handle.emit("model-download-done", ());
             });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::workspace::get_workspace,
            commands::workspace::get_datasets_root,
            commands::workspace::set_workspace,
            commands::workspace::select_workspace,
            commands::workspace::deep_clean_cache,
            commands::config::get_config,
            commands::config::get_all_configs,
            commands::config::save_config,
            commands::config::delete_config,
            commands::config::get_last_experiment,
            commands::config::set_last_experiment,
            commands::download::get_download_settings,
            commands::download::set_download_settings,
            commands::update::check_update,
            commands::update::open_url,
            commands::env::get_env_status,
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
            commands::train::get_run_manifest,
            commands::train::get_run_usage,
            commands::train::get_experiment_path,
            commands::train::list_experiments,
            commands::train::start_log_stream,
            commands::train::start_train_progress_stream,
            commands::infer::convert_voice_streaming,
            commands::infer::cancel_infer,
            commands::infer::merge_audio,
            commands::infer::list_models,
            commands::infer::delete_model,
            commands::infer::delete_output,
            commands::infer::import_model,
            commands::infer::get_base_models_status,
            commands::infer::get_active_model_download,
            commands::infer::download_base_model,
            commands::infer::delete_base_model,
            commands::infer::get_model_storage_paths,
            commands::uvr::separate_vocals,
            commands::uvr::separate_vocals_streaming,
            commands::uvr::cancel_uvr,
            commands::cover::cover_generate,
            commands::cover::cancel_cover,
            commands::cover::list_cover_voices,
            commands::cover::get_cover_params,
            commands::cover::save_cover_params,
            commands::file::file_exists,
            commands::file::list_artifacts,
            commands::file::get_checkpoint_iteration,
            commands::file::read_text_file,
            commands::file::list_audio_files,
            commands::file::list_dataset_audio,
            commands::file::open_in_finder,
            commands::file::open_in_vscode,
            commands::file::reclaim_run,
            commands::file::reset_experiment,
            commands::file::purge_run,
            commands::file::remove_index_artifacts,
            commands::layout::get_layout_status,
            commands::layout::preview_layout_migration,
            commands::layout::migrate_layout,
            commands::storage::get_storage_report,
            commands::storage::reclaim_all_runs,
            commands::outputs::list_outputs,
            commands::outputs::finalize_output,
            commands::outputs::set_output_starred,
            commands::audio::get_audio_devices,
            commands::live::start_live_singing,
            commands::live::set_live_f0_up_key,
            commands::live::set_audio_source,
            commands::live::set_accompaniment_pause,
            commands::live::seek_accompaniment,
            commands::live::get_accompaniment_status,
            commands::live::play_cover,
            commands::live::preload_cover,
            commands::live::stop_cover,
            commands::live::play_accompaniment,
            commands::live::play_current_accompaniment,
            commands::live::get_player_state,
            commands::live::stop_accompaniment,
            commands::live::stop_live_singing,
            commands::live::get_live_singing_status,
            commands::live::list_live_models,
            commands::live::export_pth_to_onnx,
            commands::live::extract_lyrics,
            commands::live::search_lyrics,
            commands::live::download_lyrics,
            commands::live::open_desktop_lyrics,
            commands::live::read_lrc_file,
            commands::live::list_separated_songs,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
