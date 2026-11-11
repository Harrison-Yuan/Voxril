use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command as TokioCommand;

/// 返回包含伴生 ffmpeg 目录的 PATH 环境变量值。
/// Python 子进程（如 UVR）直接 subprocess.call("ffmpeg")，
/// 必须把 ffmpeg 所在目录注入 PATH 才能找到。
fn build_path_with_ffmpeg(project_root: &Path) -> String {
    let mut paths: Vec<String> = Vec::new();
    // 当前进程的 PATH
    if let Ok(existing) = std::env::var("PATH") {
        paths.push(existing);
    }
    // 伴生 ffmpeg 可能的位置
    let candidates = [
        project_root.join("bin"),
        project_root.join("src-tauri").join("bin"),
        // DMG 模式: project_root = Resources/_up_/, ffmpeg = Resources/bin/
        project_root.join("..").join("bin"),
        project_root.join("../../bin"),
    ];
    for d in &candidates {
        let p = d.join("ffmpeg");
        if p.exists() {
            let s = d.to_string_lossy().to_string();
            if !paths.contains(&s) {
                paths.push(s);
            }
            break;
        }
    }
    paths.join(":")
}

/// 返回当前可执行文件的路径。
/// 用于 Python 子进程调用本程序的子命令（如 decode-audio）。
fn voxril_bin() -> String {
    std::env::current_exe()
        .unwrap_or_else(|_| PathBuf::from("voxril"))
        .to_string_lossy()
        .to_string()
}

pub fn get_app_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("com.rvc.webui")
}

fn check_python_version(path: &PathBuf) -> bool {
    let output = Command::new(path)
        .arg("--version")
        .output()
        .ok();
    match output {
        Some(out) if out.status.success() => {
            let v_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
            let v_str = v_str.trim_start_matches("Python ");
            let parts: Vec<&str> = v_str.split('.').collect();
            if parts.len() >= 2 {
                if let (Ok(major), Ok(minor)) = (parts[0].parse::<i32>(), parts[1].parse::<i32>()) {
                    let ok = (major, minor) >= (3, 10);
                    if !ok {
                        eprintln!("Python version too old at {}: {}.{} < 3.10", path.display(), major, minor);
                    }
                    return ok;
                }
            }
            false
        }
        _ => false,
    }
}

fn find_venv_python(project_root: &PathBuf, data_dir: &PathBuf) -> Option<PathBuf> {
    let mut candidates = vec![
        // Development mode: project-local .venv takes priority
        project_root.join(".venv/bin/python"),
        project_root.join("venv/bin/python"),
        // Production/DMG mode: app data directory
        data_dir.join("venv/bin/python"),
    ];

    // Homebrew Python 3.10+ (Mac)
    for ver in ["3.12", "3.11", "3.10"] {
        for base in ["/opt/homebrew/bin", "/usr/local/bin"] {
            let p = PathBuf::from(base).join(format!("python{}", ver));
            if p.exists() {
                candidates.push(p);
            }
        }
    }

    for candidate in candidates {
        if candidate.exists() && check_python_version(&candidate) {
            return Some(candidate);
        }
    }

    None
}

pub struct PythonExecutor {
    python_cmd: String,
    venv_path: Option<PathBuf>,
    project_root: PathBuf,
    data_dir: PathBuf,
}

impl PythonExecutor {
    pub fn new(project_root: PathBuf, data_dir: PathBuf) -> Self {
        let (python_cmd, venv_path) = match find_venv_python(&project_root, &data_dir) {
            Some(p) => {
                let v = p.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf());
                (p.to_string_lossy().to_string(), v)
            }
            None => ("python3".to_string(), None),
        };

        // ⚠️ DMG 修复：确保 configs/（含子目录）在可写数据目录中存在。
        // Python config.py 需要读写 configs/v2/40k.json 等文件，
        // 而 .app 内 Resources/ 是只读的。递归复制整个目录树。
        fn copy_dir_recursive(src: &Path, dst: &Path) {
            if !src.exists() { return; }
            let _ = std::fs::create_dir_all(dst);
            if let Ok(entries) = std::fs::read_dir(src) {
                for entry in entries.flatten() {
                    let ft = match entry.file_type() {
                        Ok(t) => t,
                        _ => continue,
                    };
                    let name = entry.file_name();
                    if ft.is_dir() {
                        copy_dir_recursive(&entry.path(), &dst.join(&name));
                    } else if ft.is_file() {
                        let _ = std::fs::copy(entry.path(), dst.join(&name));
                    }
                }
            }
        }
        let src_cfg = project_root.join("configs");
        let dst_cfg = data_dir.join("configs");
        // ⚠️ 检查关键文件 v2/40k.json 是否存在，而非检查目录是否为空。
        // 旧版 flat copy 只复制了顶层文件（config.json/config.py），
        // 但遗漏了 v2/ 子目录。dst 非空但缺少关键文件→递归复制被跳过。
        let key_file = dst_cfg.join("v2").join("40k.json");
        if src_cfg.exists() && !key_file.exists() {
            copy_dir_recursive(&src_cfg, &dst_cfg);
            eprintln!("[Python] configs/ 已递归复制到 {}", dst_cfg.display());
        }

        Self {
            python_cmd,
            venv_path,
            project_root,
            data_dir,
        }
    }

    pub fn venv_exists(&self) -> bool {
        self.venv_path.is_some()
    }

    #[allow(dead_code)]
    pub fn python_cmd(&self) -> &str {
        &self.python_cmd
    }

    /// Build a pre-configured tokio Command with all required env vars set.
    /// All processes get nice=5 so other apps (browser, music, etc.) stay responsive.
    /// nice_level: override if needed (0=normal, 5=politely yield, 10=very polite)
    pub fn build_command(&self, script_path: &str, args: Vec<String>, nice_level: i32) -> TokioCommand {
        let n = if nice_level > 0 { nice_level } else { 5 };
        let mut cmd = TokioCommand::new("nice");
        cmd.arg(&format!("-n{}", n));
        cmd.arg(&self.python_cmd);
        let data_dir_str = self.data_dir.to_string_lossy().to_string();
        let hubert_model_path = format!("{}/assets/hubert/vec-768-layer-12.onnx", data_dir_str);
        let configs_dir = self.data_dir.join("configs").to_string_lossy().to_string();
        cmd.arg(script_path)
            .args(args)
            .current_dir(&self.project_root)
            .env("PYTHONPATH", &self.project_root)
            .env("PYTHONUNBUFFERED", "1")
            .env("KMP_DUPLICATE_LIB_OK", "TRUE")
            .env("OMP_NUM_THREADS", "2")
            .env("MKL_NUM_THREADS", "2")
            .env("OPENBLAS_NUM_THREADS", "2")
            .env("NUMEXPR_NUM_THREADS", "2")
            .env("VECLIB_MAXIMUM_THREADS", "2")
            .env("RVC_DATA_DIR", &data_dir_str)
            .env("RVC_CONFIG_DIR", &configs_dir)
            .env("HUBERT_MODEL_PATH", &hubert_model_path)
            .env("VOXRIL_BIN", voxril_bin())
            .env("PATH", build_path_with_ffmpeg(&self.project_root));
        cmd
    }

    fn build_native_command(&self, script_path: &str, args: Vec<String>) -> Command {
        let mut cmd = Command::new("nice");
        cmd.arg("-n5");
        cmd.arg(&self.python_cmd);
        let data_dir_str = self.data_dir.to_string_lossy().to_string();
        let hubert_model_path = format!("{}/assets/hubert/vec-768-layer-12.onnx", data_dir_str);
        let configs_dir = self.data_dir.join("configs").to_string_lossy().to_string();
        cmd.arg(script_path)
            .args(args)
            .current_dir(&self.project_root)
            .env("PYTHONPATH", &self.project_root)
            .env("PYTHONUNBUFFERED", "1")
            .env("KMP_DUPLICATE_LIB_OK", "TRUE")
            .env("OMP_NUM_THREADS", "2")
            .env("MKL_NUM_THREADS", "2")
            .env("OPENBLAS_NUM_THREADS", "2")
            .env("NUMEXPR_NUM_THREADS", "2")
            .env("VECLIB_MAXIMUM_THREADS", "2")
            .env("RVC_DATA_DIR", &data_dir_str)
            .env("RVC_CONFIG_DIR", &configs_dir)
            .env("HUBERT_MODEL_PATH", &hubert_model_path)
            .env("VOXRIL_BIN", voxril_bin())
            .env("PATH", build_path_with_ffmpeg(&self.project_root));
        cmd
    }

    pub async fn run_script(
        &self,
        script_path: &str,
        args: Vec<String>,
    ) -> Result<String> {
        let mut cmd = self.build_command(script_path, args, 0);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd.output().await?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            anyhow::bail!("Python script failed:\nstdout: {}\nstderr: {}", stdout, stderr)
        }
    }

    pub async fn run_script_streaming<F>(
        &self,
        script_path: &str,
        args: Vec<String>,
        mut callback: F,
    ) -> Result<()>
    where
        F: FnMut(String) + Send + 'static,
    {
        let mut cmd = self.build_command(script_path, args, 0);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd.spawn()?;

        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break,
                Ok(_) => {
                    let trimmed = line.trim().to_string();
                    if !trimmed.is_empty() {
                        callback(trimmed);
                    }
                }
                Err(_) => break,
            }
        }

        child.wait().await?;
        Ok(())
    }

    pub async fn run_detached_with_log(&self, script_path: &str, args: Vec<String>, log_path: PathBuf) -> Result<u32> {
        let data_dir_str = self.data_dir.to_string_lossy().to_string();
        let hubert_model_path = format!("{}/assets/hubert/vec-768-layer-12.onnx", data_dir_str);
        let rmvpe_root = format!("{}/assets/rmvpe", data_dir_str);
        let mut cmd = Command::new("nice");
        cmd.arg("-n10")
            .arg(&self.python_cmd)
            .arg(script_path)
            .args(args)
            .current_dir(&self.project_root)
            .env("PYTHONPATH", &self.project_root)
            .env("PYTHONUNBUFFERED", "1")
            .env("KMP_DUPLICATE_LIB_OK", "TRUE")
            .env("OMP_NUM_THREADS", "1")
            .env("MKL_NUM_THREADS", "1")
            .env("OPENBLAS_NUM_THREADS", "1")
            .env("NUMEXPR_NUM_THREADS", "1")
            .env("VECLIB_MAXIMUM_THREADS", "1")
            .env("RVC_DATA_DIR", &data_dir_str)
            .env("HUBERT_MODEL_PATH", &hubert_model_path)
            .env("rmvpe_root", &rmvpe_root)
            .env("RVC_CONFIG_DIR", &format!("{}/configs", data_dir_str))
            .env("VOXRIL_BIN", voxril_bin())
            .env("PATH", build_path_with_ffmpeg(&self.project_root))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn()?;
        let pid = child.id();

        let stdout = child.stdout.take().context("Failed to get stdout")?;
        let stderr = child.stderr.take().context("Failed to get stderr")?;

        std::thread::spawn(move || {
            use std::io::{BufRead, Write};
            const MAX_LOG_BYTES: u64 = 100 * 1024 * 1024;

            let mut file = OpenOptions::new().create(true).append(true).open(&log_path).ok();
            let mut reader = std::io::BufReader::new(stdout);
            let mut line = String::new();
            let mut written_since_check: u64 = 0;

            loop {
                line.clear();
                if BufRead::read_line(&mut reader, &mut line).unwrap_or(0) == 0 {
                    break;
                }
                let mut need_rotate = false;
                if let Some(ref mut handle) = file {
                    let _ = handle.write_all(line.as_bytes());
                    let _ = handle.flush();
                    written_since_check += line.len() as u64;
                    if written_since_check >= 10 * 1024 * 1024 {
                        need_rotate = handle.metadata().map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false);
                        written_since_check = 0;
                    }
                }
                if need_rotate {
                    file.take();
                    let backup = log_path.with_extension("1.log");
                    let _ = std::fs::remove_file(&backup);
                    let _ = std::fs::rename(&log_path, &backup);
                    file = OpenOptions::new().create(true).append(true).open(&log_path).ok();
                }
            }
            if let Some(ref mut handle) = file {
                let _ = handle.write_all(b"\n--- STDERR ---\n");
                let _ = handle.flush();
            }
            let mut err_reader = std::io::BufReader::new(stderr);
            written_since_check = 0;
            loop {
                line.clear();
                if BufRead::read_line(&mut err_reader, &mut line).unwrap_or(0) == 0 {
                    break;
                }
                let mut need_rotate = false;
                if let Some(ref mut handle) = file {
                    let _ = handle.write_all(line.as_bytes());
                    let _ = handle.flush();
                    written_since_check += line.len() as u64;
                    if written_since_check >= 10 * 1024 * 1024 {
                        need_rotate = handle.metadata().map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false);
                        written_since_check = 0;
                    }
                }
                if need_rotate {
                    file.take();
                    let backup = log_path.with_extension("1.log");
                    let _ = std::fs::remove_file(&backup);
                    let _ = std::fs::rename(&log_path, &backup);
                    file = OpenOptions::new().create(true).append(true).open(&log_path).ok();
                }
            }
        });

        Ok(pid)
    }
}
