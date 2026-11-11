use crate::python::PythonExecutor;
use crate::state::AppState;
use crate::utils::kill_orphan_multiprocessing;
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::State;

const EXPERIMENTS_DIR: &str = "experiments";

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainRequest {
    pub exp_dir: String,
    pub sr: String,
    pub trainset_dir: String,
    pub np_process: i32,
    pub f0_method: String,
    pub if_f0: bool,
    pub save_epoch: i32,
    pub total_epoch: i32,
    pub batch_size: i32,
    pub if_save_latest: bool,
    pub if_save_every_weights: bool,
    pub gpus: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainResponse {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrainStatusResponse {
    pub is_running: bool,
    pub pid: Option<String>,
    pub current_epoch: i32,
    pub total_epoch: i32,
    pub latest_loss: String,
    pub percent: f64,
    pub log_tail: String,
    pub error_message: String,
}

#[tauri::command]
pub async fn preprocess_dataset(
    trainset_dir: String,
    exp_dir: String,
    sr: String,
    np_process: i32,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    use std::path::Path;
    
    let trainset_path = Path::new(&trainset_dir);
    if !trainset_path.exists() {
        return Err(format!("训练集路径不存在: {}", trainset_dir));
    }
    if !trainset_path.is_dir() {
        return Err(format!("训练集路径不是一个文件夹: {}", trainset_dir));
    }

    kill_orphan_multiprocessing();
    
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let sr_value = match sr.as_str() {
        "32k" => "32000",
        "40k" => "40000",
        "48k" => "48000",
        _ => "40000",
    };

    // 限制预处理并行进程数为 CPU 核心数的一半，防止系统卡死
    let max_workers = (num_cpus::get() as i32 / 2).max(1);
    let effective_np = np_process.min(max_workers);

    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
    tokio::fs::create_dir_all(&exp_path)
        .await
        .map_err(|e| format!("Failed to create experiment directory: {}", e))?;

    let args = vec![
        trainset_dir,
        sr_value.to_string(),
        effective_np.to_string(),
        exp_path.to_string_lossy().to_string(),
        "False".to_string(),
        "3.0".to_string(),
    ];

    let log_path = exp_path.join("preprocess.log");
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    tokio::fs::write(&log_path, "[backend] preprocess process launching...\n")
        .await
        .map_err(|e| format!("Failed to initialize preprocess log: {}", e))?;

    let result = executor
        .run_detached_with_log("infer/modules/train/preprocess_optimized.py", args, log_path.clone())
        .await;
    let _pid = result.map_err(|e| format!("Failed to start preprocess: {}", e))?;

    Ok(TrainResponse {
        ok: true,
        message: format!("预处理已在后台启动（使用 {}/{} 进程）。数据将保存至: {}", effective_np, max_workers * 2, exp_path.display()),
    })
}

#[tauri::command]
pub async fn extract_f0_features(
    np_process: i32,
    f0_method: String,
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);

    // Clean old (possibly incomplete) F0 output before re-running
    for dir_name in &["2a_f0", "2b-f0nsf"] {
        let dir = exp_path.join(dir_name);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await.ok();
        }
    }

    let log_path = exp_path.join("extract_f0_feature.log");
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    tokio::fs::write(&log_path, "[backend] f0 feature extraction process launching...\n")
        .await
        .map_err(|e| format!("Failed to initialize f0 feature extraction log: {}", e))?;

    let args = vec![
        exp_path.to_string_lossy().to_string(),
        np_process.to_string(),
        f0_method.clone(),
    ];

    let result = executor
        .run_detached_with_log("infer/modules/train/extract/extract_f0_print.py", args, log_path.clone())
        .await;
    let _pid = result.map_err(|e| format!("Failed to start f0 feature extraction: {}", e))?;

    Ok(TrainResponse {
        ok: true,
        message: format!("F0 feature extraction started. Log: {}", log_path.display()),
    })
}

#[tauri::command]
pub async fn extract_audio_features(
    gpus: String,
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);

    // Clean old (possibly incomplete) audio feature output before re-running
    for dir_name in &["3_feature256", "3_feature768"] {
        let dir = exp_path.join(dir_name);
        if dir.exists() {
            tokio::fs::remove_dir_all(&dir).await.ok();
        }
    }

    let log_path = exp_path.join("extract_audio_feature.log");
    if log_path.exists() {
        tokio::fs::remove_file(&log_path).await.ok();
    }
    tokio::fs::write(&log_path, "[backend] audio feature extraction process launching...\n")
        .await
        .map_err(|e| format!("Failed to initialize audio feature extraction log: {}", e))?;

    let gpu_list: Vec<&str> = gpus.split('-').collect();
    let args = vec![
        "cuda".to_string(),
        gpu_list.len().to_string(),
        "0".to_string(),
        gpu_list.first().unwrap_or(&"0").to_string(),
        exp_path.to_string_lossy().to_string(),
        "False".to_string(),
    ];

    let result = executor
        .run_detached_with_log("infer/modules/train/extract_feature_print.py", args, log_path.clone())
        .await;
    let _pid = result.map_err(|e| format!("Failed to start audio feature extraction: {}", e))?;

    Ok(TrainResponse {
        ok: true,
        message: format!("Audio feature extraction started. Log: {}", log_path.display()),
    })
}

#[tauri::command]
pub async fn train_model(
    config: TrainRequest,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    // 互斥检查：禁止多个重任务并行
    {
        let task = state.active_heavy_task.lock().await;
        if let Some(ref running) = *task {
            // 检查训练进程是否实际存活，如果已死则自动清除僵尸锁
            let alive = std::process::Command::new("pgrep")
                .args(["-f", "train\\.py"])
                .output()
                .map(|o| o.status.success())
                .unwrap_or(false);
            if !alive {
                drop(task);
                let mut lock = state.active_heavy_task.lock().await;
                if lock.as_deref() == Some("train") {
                    *lock = None;
                }
                eprintln!("[train] 检测到前次训练进程已退出，自动清除任务锁");
            } else {
                return Err(format!(
                    "系统当前有任务正在运行: [{}]，请等待其完成后再开始训练。",
                    running
                ));
            }
        }
    }

    kill_orphan_multiprocessing();

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&config.exp_dir);

    tokio::fs::create_dir_all(&exp_path)
        .await
        .map_err(|e| format!("Failed to create experiment directory: {}", e))?;

    let gt_wavs_dir = exp_path.join("0_gt_wavs");
    let feature_dir = exp_path.join("3_feature768");
    
    if !gt_wavs_dir.exists() || !feature_dir.exists() {
        return Err(format!(
            "Training data not found. Please complete preprocessing and feature extraction first.\nExpected directories:\n- {}\n- {}",
            gt_wavs_dir.display(),
            feature_dir.display()
        ));
    }

    let mut filelist_entries = Vec::new();
    let mut read_dir = tokio::fs::read_dir(&gt_wavs_dir)
        .await
        .map_err(|e| format!("Failed to read gt_wavs directory: {}", e))?;
    
    let pitch_dir = exp_path.join("2a_f0");
    let pitchf_dir = exp_path.join("2b-f0nsf");
    
    let mut total_wavs = 0;
    let mut missing_features = 0;
    let mut missing_pitch = 0;
    
    while let Some(entry) = read_dir.next_entry().await.map_err(|e| format!("Failed to read directory entry: {}", e))? {
        let path = entry.path();
        
        if path.extension().and_then(|s| s.to_str()) == Some("wav") {
            total_wavs += 1;
            
            let file_name = path.file_stem()
                .and_then(|s| s.to_str())
                .ok_or_else(|| format!("Invalid filename: {}", path.display()))?;
            
            let feature_file = feature_dir.join(format!("{}.npy", file_name));
            
            if !feature_file.exists() {
                missing_features += 1;
                continue;
            }
            
            // Use absolute paths for filelist entries
            let rel_wav = path.to_string_lossy().to_string();
            let rel_feature = feature_file.to_string_lossy().to_string();
            
            if config.if_f0 {
                let pitch_file = pitch_dir.join(format!("{}.npy", file_name));
                let pitchf_file = pitchf_dir.join(format!("{}.npy", file_name));
                
                if !pitch_file.exists() || !pitchf_file.exists() {
                    missing_pitch += 1;
                    continue;
                }
                
                let rel_pitch = pitch_file.to_string_lossy().to_string();
                let rel_pitchf = pitchf_file.to_string_lossy().to_string();
                
                filelist_entries.push(format!("{}|{}|{}|{}|0", rel_wav, rel_feature, rel_pitch, rel_pitchf));
            } else {
                filelist_entries.push(format!("{}|{}|0", rel_wav, rel_feature));
            }
        }
    }
    
    if filelist_entries.is_empty() {
        let mut error_msg = format!(
            "No valid training samples found.\n\nStatistics:\n- Total wav files: {}\n- Missing feature files: {}",
            total_wavs, missing_features
        );
        
        if config.if_f0 {
            error_msg.push_str(&format!("\n- Missing pitch files: {}", missing_pitch));
            error_msg.push_str(&format!(
                "\n\nFor f0 models, you need:\n1. Feature files in: {}\n2. Pitch files in: {} and {}",
                feature_dir.display(),
                pitch_dir.display(),
                pitchf_dir.display()
            ));
        } else {
            error_msg.push_str(&format!(
                "\n\nYou need feature files in: {}",
                feature_dir.display()
            ));
        }
        
        error_msg.push_str("\n\nPlease complete preprocessing and feature extraction first.");
        return Err(error_msg);
    }
    
    let filelist_path = exp_path.join("filelist.txt");
    tokio::fs::write(&filelist_path, filelist_entries.join("\n"))
        .await
        .map_err(|e| format!("Failed to write filelist.txt: {}", e))?;

    let config_template_path = state.project_root.join("configs").join("v2").join(format!("{}.json", config.sr));
    if !config_template_path.exists() {
        return Err(format!(
            "Config template not found: {}. Expected path for version v2 and sample rate {}.",
            config_template_path.display(),
            config.sr
        ));
    }
    
    let config_dest_path = exp_path.join("config.json");
    tokio::fs::copy(&config_template_path, &config_dest_path)
        .await
        .map_err(|e| format!("Failed to copy config template: {}", e))?;

    if let Ok(content) = tokio::fs::read_to_string(&config_dest_path).await {
        if let Ok(mut cfg_json) = serde_json::from_str::<serde_json::Value>(&content) {
            cfg_json["train"]["epochs"] = serde_json::json!(config.total_epoch);
            let _ = tokio::fs::write(&config_dest_path, cfg_json.to_string()).await;
        }
    }

    let pretrained_g_default = if config.if_f0 {
        format!("assets/pretrained_v2/f0G{}k.pth", config.sr.trim_end_matches('k'))
    } else {
        format!("assets/pretrained_v2/G{}k.pth", config.sr.trim_end_matches('k'))
    };
    let pretrained_d_default = if config.if_f0 {
        format!("assets/pretrained_v2/f0D{}k.pth", config.sr.trim_end_matches('k'))
    } else {
        format!("assets/pretrained_v2/D{}k.pth", config.sr.trim_end_matches('k'))
    };
    let pretrained_g_path = state.data_dir.join(&pretrained_g_default);
    let pretrained_d_path = state.data_dir.join(&pretrained_d_default);
    let mut missing = Vec::new();
    if !pretrained_g_path.exists() {
        missing.push(format!("预训练 Generator: {}", pretrained_g_path.display()));
    }
    if !pretrained_d_path.exists() {
        missing.push(format!("预训练 Discriminator: {}", pretrained_d_path.display()));
    }
    if !missing.is_empty() {
        return Err(format!(
            "底模缺失，无法开始训练！请前往「模型管理」页面下载以下文件：\n{}",
            missing.join("\n")
        ));
    }

    let args = vec![
        "-e".to_string(),
        exp_path.to_string_lossy().to_string(),
        "-sr".to_string(),
        config.sr.clone(),
        "-f0".to_string(),
        if config.if_f0 { "1" } else { "0" }.to_string(),
        "-bs".to_string(),
        config.batch_size.to_string(),
        "-te".to_string(),
        config.total_epoch.to_string(),
        "-se".to_string(),
        config.save_epoch.to_string(),
        "-pg".to_string(),
        pretrained_g_path.to_string_lossy().to_string(),
        "-pd".to_string(),
        pretrained_d_path.to_string_lossy().to_string(),
        "-l".to_string(),
        if config.if_save_latest { "1" } else { "0" }.to_string(),
        "-c".to_string(),
        "0".to_string(),
        "-sw".to_string(),
        if config.if_save_every_weights { "1" } else { "0" }.to_string(),
        "-v".to_string(),
        "v2".to_string(),
        "-g".to_string(),
        config.gpus,
    ];

    let train_log_path = exp_path.join("train.log");
    {
        use tokio::io::AsyncWriteExt;
        let mut file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&train_log_path)
            .await
            .map_err(|e| format!("Failed to open training log: {}", e))?;
        file.write_all(b"[backend] training process launching...\n")
            .await
            .map_err(|e| format!("Failed to write training log marker: {}", e))?;
    }

    // 设置任务令牌
    *state.active_heavy_task.lock().await = Some("train".to_string());

    let result = executor
        .run_detached_with_log("infer/modules/train/train.py", args, train_log_path.clone())
        .await;
    let pid = match result {
        Ok(p) => p,
        Err(e) => {
            // 启动失败，立即释放令牌
            *state.active_heavy_task.lock().await = None;
            return Err(format!("Failed to start training: {}", e));
        }
    };
    // 记录训练 PID，进程退出或取消时自动清理
    *state.train_pid.lock().await = Some(pid);

    // 后台监视训练进程退出，退出后自动清除令牌
    let task_ref = state.active_heavy_task.clone();
    let exp_path_str_monitor = exp_path.to_string_lossy().to_string();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            // 使用 pgrep 检查 train.py 进程是否存活（与 get_train_status 一致）
            let alive = std::process::Command::new("pgrep")
                .args(["-f", &format!("train.py.*{}", exp_path_str_monitor)])
                .output()
                .map(|o| o.status.success() && !o.stdout.is_empty())
                .unwrap_or(false);
            if !alive {
                let mut lock = task_ref.lock().await;
                if lock.as_deref() == Some("train") {
                    *lock = None;
                }
                break;
            }
        }
    });

    Ok(TrainResponse {
        ok: true,
        message: format!("训练已在后台启动。模型将保存至: {}", exp_path.display()),
    })
}

#[tauri::command]
pub async fn stop_training(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
    let exp_path_str = exp_path.to_string_lossy().to_string();
    drop(workspace_root);

    // ── 礼貌停止训练进程（SIGTERM → 轮询 8s → SIGKILL）
    let pattern = format!("train.py.*{}", exp_path_str);
    let gracefully_exited = crate::process_killer::kill_by_pattern(&pattern, 8);
    crate::process_killer::kill_by_pattern("torch.distributed", 0);
    crate::process_killer::kill_multiprocessing_spawn();

    // 无论如何，清除任务令牌 + 训练 PID
    {
        let mut lock = state.active_heavy_task.lock().await;
        if lock.as_deref() == Some("train") {
            *lock = None;
        }
    }
    *state.train_pid.lock().await = None;

    let msg = if gracefully_exited {
        "训练已优雅停止（Checkpoint 已安全保存）".to_string()
    } else {
        "训练已强制停止（SIGKILL）".to_string()
    };

    Ok(TrainResponse { ok: true, message: msg })
}

#[tauri::command]
pub async fn train_index(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());
    
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);

    let python_code = format!(
        r#"
import json, os, faiss, numpy as np, struct

exp_dir = '{}'
feature_dir = os.path.join(exp_dir, '3_feature768')
if not os.path.exists(feature_dir):
    print(json.dumps({{'ok': False, 'message': '请先进行特征提取!\\n检查路径: %s' % feature_dir}}))
    exit(0)

npys = []
for name in sorted(os.listdir(feature_dir)):
    npys.append(np.load(os.path.join(feature_dir, name)))

big_npy = np.concatenate(npys, 0)
big_npy_idx = np.arange(big_npy.shape[0])
np.random.shuffle(big_npy_idx)
big_npy = big_npy[big_npy_idx]

n_ivf = min(int(16 * np.sqrt(big_npy.shape[0])), big_npy.shape[0] // 39)
index = faiss.index_factory(768, f'IVF{{n_ivf}},Flat')
index_ivf = faiss.extract_index_ivf(index)
index_ivf.nprobe = 1
index.train(big_npy)

batch_size_add = 8192
for i in range(0, big_npy.shape[0], batch_size_add):
    index.add(big_npy[i:i + batch_size_add])

# 索引生成到 checkpoints/ 目录，与模型文件同目录，消除后续查找歧义
out_dir = os.path.join(exp_dir, 'checkpoints')
os.makedirs(out_dir, exist_ok=True)
index_path = os.path.join(out_dir, f'added_IVF{{n_ivf}}_Flat_nprobe_{{index_ivf.nprobe}}_{{os.path.basename(exp_dir)}}_v2.index')
faiss.write_index(index, index_path)

# 同时生成运行时 IVF 缓存（vectors.bin + meta.txt + ivf.bin）
# 这样 InferCore::load 无需再调用 Python 重建
idx2 = faiss.read_index(index_path)
try:
    idx2.make_direct_map()
except:
    pass
npy2 = idx2.reconstruct_n(0, idx2.ntotal)
npy2.astype('<f4').tofile(index_path + '.vectors.bin')
with open(index_path + '.meta.txt', 'w') as f2:
    f2.write(str(npy2.shape[0]) + '\n')
    f2.write(str(npy2.shape[1]) + '\n')
ivf2 = faiss.extract_index_ivf(idx2)
ncent2 = ivf2.nlist
d2 = npy2.shape[1]
centroids2 = ivf2.quantizer.reconstruct_n(0, ivf2.quantizer.ntotal).astype('<f4')
with open(index_path + '.ivf.bin', 'wb') as f2:
    f2.write(struct.pack('<II', ncent2, d2))
    f2.write(centroids2.tobytes())
    for i in range(ncent2):
        nv = ivf2.invlists.list_size(i)
        f2.write(struct.pack('<I', nv))
        if nv > 0:
            ids_ptr = ivf2.invlists.get_ids(i)
            ids = faiss.rev_swig_ptr(ids_ptr, nv).astype('<u4')
            f2.write(ids.tobytes())
print(json.dumps({{'ok': True, 'message': '成功构建索引 (含 IVF 缓存)'}}))
"#,
        exp_path.to_string_lossy()
    );

    let result = executor
        .run_script("-c", vec![python_code])
        .await
        .map_err(|e| format!("Index training failed: {}", e))?;

    let response: serde_json::Value = serde_json::from_str(&result)
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    Ok(TrainResponse {
        ok: response["ok"].as_bool().unwrap_or(false),
        message: response["message"].as_str().unwrap_or("").to_string(),
    })
}

/// 为已存在的 .index 文件生成运行时 IVF 缓存（vectors.bin + meta.txt + ivf.bin）。
/// 当缓存缺失或损坏时调用此命令修复，无需重新训练。
#[tauri::command]
pub async fn build_index_cache(
    index_path: String,
    state: State<'_, AppState>,
) -> Result<TrainResponse, String> {
    if !Path::new(&index_path).exists() {
        return Ok(TrainResponse {
            ok: false,
            message: format!("index 文件不存在: {}", index_path),
        });
    }

    let executor = PythonExecutor::new(state.project_root.clone(), state.data_dir.clone());

    let python_code = format!(
        r#"
import faiss, numpy as np, os, struct

idx_path = r'{}'
idx = faiss.read_index(idx_path)
try:
    idx.make_direct_map()
except:
    pass
npy = idx.reconstruct_n(0, idx.ntotal)
npy.astype('<f4').tofile(idx_path + '.vectors.bin')
with open(idx_path + '.meta.txt', 'w') as f:
    f.write(str(npy.shape[0]) + '\n')
    f.write(str(npy.shape[1]) + '\n')
ivf = faiss.extract_index_ivf(idx)
ncent = ivf.nlist
d = npy.shape[1]
centroids = ivf.quantizer.reconstruct_n(0, ivf.quantizer.ntotal).astype('<f4')
with open(idx_path + '.ivf.bin', 'wb') as f:
    f.write(struct.pack('<II', ncent, d))
    f.write(centroids.tobytes())
    for i in range(ncent):
        nv = ivf.invlists.list_size(i)
        f.write(struct.pack('<I', nv))
        if nv > 0:
            ids_ptr = ivf.invlists.get_ids(i)
            ids = faiss.rev_swig_ptr(ids_ptr, nv).astype('<u4')
            f.write(ids.tobytes())
print(f"[DONE] IVF 缓存生成成功: {{npy.shape[0]}} vecs x {{d}}, {{ncent}} centroids")
"#,
        index_path
    );

    let result = executor
        .run_script("-c", vec![python_code])
        .await
        .map_err(|e| format!("索引缓存生成失败: {}", e))?;

    Ok(TrainResponse {
        ok: true,
        message: result,
    })
}

#[tauri::command]
pub async fn get_train_status(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<TrainStatusResponse, String> {
    use std::process::Command;

    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
    let exp_path_str = exp_path.to_string_lossy();

    let output = Command::new("pgrep")
        .args(&["-f", &format!("train.py.*{}", exp_path_str)])
        .output()
        .map_err(|e| format!("Failed to check training status: {}", e))?;

    let is_running = output.status.success() && !output.stdout.is_empty();

    let mut current_epoch = 0i32;
    let mut total_epoch = 0i32;
    let mut latest_loss = String::new();

    let mut log_tail = String::new();
    let mut error_message = String::new();

    let train_log_path = exp_path.join("train.log");
    if train_log_path.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&train_log_path).await {
            log_tail = content.lines().rev().take(20).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");

            // Only read epoch/loss from lines AFTER the last restart marker.
            // This prevents stale data from a previous terminated run.
            let cutoff = content.rfind("[backend] training process launching...")
                .map(|pos| pos + content[pos..].find('\n').unwrap_or(0) + 1)
                .unwrap_or(0);
            let relevant = &content[cutoff..];

            for line in relevant.lines().rev() {
                if line.contains("Epoch:") {
                    if let Some(epoch_str) = line.split("Epoch:").nth(1) {
                        if let Some(num) = epoch_str.trim().split_whitespace().next() {
                            current_epoch = num.parse().unwrap_or(0);
                        }
                    }
                    break;
                }
            }
            for line in relevant.lines().rev() {
                if line.contains("loss_disc=") || line.contains("loss_gen=") {
                    let mut parts = Vec::new();
                    for part in line.split(',').take(3) {
                        if let Some(v) = part.split('=').nth(1) {
                            parts.push(v.trim().to_string());
                        }
                    }
                    latest_loss = parts.join(", ");
                    break;
                }
            }

            let error_keywords = ["Traceback (most recent call last)", "RuntimeError", "Killed", "exitcode", "SIGTERM", "out of memory", "Segmentation fault", "Bus error"];
            for line in content.lines().rev().take(50) {
                for kw in &error_keywords {
                    if line.contains(kw) {
                        if error_message.is_empty() {
                            error_message = line.to_string();
                        }
                    }
                }
            }
        }
    }

    let config_path = exp_path.join("config.json");
    if config_path.exists() {
        if let Ok(cfg_content) = tokio::fs::read_to_string(&config_path).await {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&cfg_content) {
                total_epoch = cfg["train"]["epochs"].as_i64().unwrap_or(0) as i32;
            }
        }
    }

    // If no train.log / no epoch was read from log, fall back to checkpoint files.
    // This handles imported experiments (trained in original RVC CLI) that have
    // merged checkpoints like harrison_e200_s60000.pth but no train.log.
    if current_epoch == 0 {
        for search_dir in &[exp_path.join("checkpoints"), exp_path.clone()] {
            if !search_dir.is_dir() {
                continue;
            }
            if let Ok(entries) = std::fs::read_dir(search_dir) {
                let mut max_epoch = 0i32;
                for entry in entries.flatten() {
                    let fname = entry.file_name().to_string_lossy().to_string();
                    if !fname.ends_with(".pth") {
                        continue;
                    }
                    // Parse epoch from checkpoint filenames like *_e200_*.pth or *_e200.pth
                    if let Some(e_pos) = fname.find("_e") {
                        let after_e = &fname[e_pos + 2..];
                        if let Some(epoch_str) = after_e.split('_').next() {
                            if let Ok(ep) = epoch_str.parse::<i32>() {
                                max_epoch = max_epoch.max(ep);
                            }
                        }
                    }
                }
                if max_epoch > 0 {
                    current_epoch = max_epoch;
                    if total_epoch == 0 {
                        total_epoch = max_epoch;
                    }
                    // Imported experiments with checkpoints are considered done
                    if !is_running && error_message.is_empty() && current_epoch >= total_epoch {
                        // leave as-is; frontend will mark train='done'
                    }
                }
            }
        }
    }

    let percent = if total_epoch > 0 {
        current_epoch as f64 / total_epoch as f64 * 100.0
    } else {
        0.0
    };

    Ok(TrainStatusResponse {
        is_running,
        pid: None,
        current_epoch,
        total_epoch,
        latest_loss,
        percent,
        log_tail,
        error_message,
    })
}

#[tauri::command]
pub async fn check_preprocess_status(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
    let preprocess_log = exp_path.join("preprocess.log");
    let feature_log = exp_path.join("extract_f0_feature.log");
    
    let mut status = serde_json::json!({
        "preprocess_done": false,
        "features_done": false,
        "preprocess_running": false,
        "features_running": false,
        "error": null
    });

    if preprocess_log.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&preprocess_log).await {
            status["preprocess_done"] = serde_json::Value::Bool(content.contains("end preprocess"));
            status["preprocess_running"] = serde_json::Value::Bool(
                content.contains("start preprocess") && !content.contains("end preprocess")
            );
            if content.contains("Fail") || content.contains("错误") {
                status["error"] = serde_json::Value::String("预处理失败，请查看日志".to_string());
            }
        }
    }

    if feature_log.exists() {
        if let Ok(content) = tokio::fs::read_to_string(&feature_log).await {
            status["features_done"] = serde_json::Value::Bool(content.contains("all-feature-done"));
            status["features_running"] = serde_json::Value::Bool(
                content.contains("todo-f0") && !content.contains("all-feature-done")
            );
            if content.contains("Fail") || content.contains("错误") {
                status["error"] = serde_json::Value::String("特征提取失败，请查看日志".to_string());
            }
        }
    }

    Ok(status)
}

#[tauri::command]
pub async fn get_experiment_path(
    exp_dir: String,
    state: State<'_, AppState>,
) -> Result<serde_json::Value, String> {
    let workspace_root = state.workspace_root.read().await;
    let exp_path = workspace_root.join(EXPERIMENTS_DIR).join(&exp_dir);
    Ok(serde_json::json!({
        "experimentPath": exp_path.to_string_lossy()
    }))
}

#[tauri::command]
pub async fn list_experiments(
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    use std::collections::BTreeSet;
    
    let workspace_root = state.workspace_root.read().await;
    let exp_root = workspace_root.join(EXPERIMENTS_DIR);
    let mut experiments = BTreeSet::new();

    tokio::fs::create_dir_all(&exp_root).await.ok();

    if let Ok(mut entries) = tokio::fs::read_dir(&exp_root).await {
        while let Ok(Some(entry)) = entries.next_entry().await {
            if entry.file_type().await.map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    experiments.insert(name.to_string());
                }
            }
        }
    }

    Ok(experiments.into_iter().collect())
}

#[tauri::command]
pub async fn start_log_stream(
    log_path: String,
    event_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tokio::spawn(async move {
        crate::events::stream_log_file(app, log_path, event_id).await;
    });
    Ok(())
}

#[tauri::command]
pub async fn start_train_progress_stream(
    exp_dir: String,
    total_epoch: i32,
    event_id: String,
    app: tauri::AppHandle,
) -> Result<(), String> {
    tokio::spawn(async move {
        crate::events::stream_train_progress(app, exp_dir, total_epoch, event_id).await;
    });
    Ok(())
}
