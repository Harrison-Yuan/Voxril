use anyhow::Result;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[allow(dead_code)]
pub fn safe_resolve(path: &str) -> PathBuf {
    PathBuf::from(path).canonicalize().unwrap_or_else(|_| PathBuf::from(path))
}

pub fn collect_files_with_extension(dir: &Path, extensions: &[&str]) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    
    for entry in WalkDir::new(dir).follow_links(true) {
        let entry = entry?;
        if entry.file_type().is_file() {
            if let Some(ext) = entry.path().extension() {
                if extensions.iter().any(|&e| ext == e) {
                    files.push(entry.path().to_path_buf());
                }
            }
        }
    }
    
    files.sort();
    Ok(files)
}

pub fn glob_files(dir: &Path, pattern: &str) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    
    for entry in WalkDir::new(dir).follow_links(true) {
        let entry = entry?;
        if entry.file_type().is_file() {
            let file_name = entry.file_name().to_string_lossy();
            if matches_pattern(&file_name, pattern) {
                files.push(entry.path().to_path_buf());
            }
        }
    }
    
    files.sort();
    Ok(files)
}

fn matches_pattern(name: &str, pattern: &str) -> bool {
    if pattern == "index" {
        name.starts_with("added_") && name.ends_with(".index")
    } else if pattern == "checkpoint" {
        regex::Regex::new(r"^G_\d+\.pth$").unwrap().is_match(name)
    } else {
        false
    }
}

#[allow(dead_code)]
pub fn finite_number(value: Option<i32>, fallback: i32) -> i32 {
    value.unwrap_or(fallback)
}

/// ⚠️ 已迁移至 process_killer::kill_multiprocessing_spawn()
/// 保留此函数作为兼容入口，新代码请直接使用 process_killer
pub fn kill_orphan_multiprocessing() {
    crate::process_killer::kill_multiprocessing_spawn();
}

/// Rotate log files > 100MB: rename old → .1.log, keep only current
pub fn truncate_logs_in_dir(dir: &std::path::Path) {
    use std::fs;
    use std::path::Path;

    const MAX_LOG_BYTES: u64 = 100 * 1024 * 1024;

    fn walk(dir: &Path, f: &mut impl FnMut(&Path)) {
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    walk(&path, f);
                } else if path.extension().map_or(false, |e| e == "log") {
                    f(&path);
                }
            }
        }
    }

    walk(dir, &mut |path| {
        if let Ok(meta) = fs::metadata(path) {
            if meta.len() > MAX_LOG_BYTES {
                let backup = path.with_extension("1.log");
                let _ = fs::remove_file(&backup);
                let _ = fs::rename(path, &backup);
                let _ = fs::File::create(path);
            }
        }
    });
}
