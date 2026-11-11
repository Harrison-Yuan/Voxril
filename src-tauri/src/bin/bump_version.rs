//! 每次构建时自动递增补丁版本号（0.1.N → 0.1.N+1）。
//!
//! 更新 src-tauri/Cargo.toml, tauri.conf.json, package.json 三处版本号。
//! 用法：`cargo run --bin bump_version` 或由 npm run build 自动触发。

use std::fs;
use std::path::PathBuf;

fn project_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    // 如果在 src-tauri 目录内，返回到父目录
    if p.file_name().and_then(|s| s.to_str()) == Some("src-tauri") {
        p.pop();
    }
    p
}

fn read_version(path: &PathBuf) -> String {
    let content = fs::read_to_string(path).expect("读取 Cargo.toml 失败");
    for line in content.lines() {
        if let Some(ver) = line.strip_prefix("version = \"") {
            if let Some(end) = ver.find('"') {
                return ver[..end].to_string();
            }
        }
    }
    panic!("Cargo.toml 中找不到 version 字段");
}

fn write_version_cargo(path: &PathBuf, old: &str, new: &str) {
    let content = fs::read_to_string(path).expect("读取 Cargo.toml 失败");
    let new_content = content.replace(&format!("version = \"{old}\""), &format!("version = \"{new}\""));
    fs::write(path, &new_content).expect("写入 Cargo.toml 失败");
}

fn write_version_json(path: &PathBuf, old: &str, new: &str) {
    let content = fs::read_to_string(path).expect("读取 JSON 失败");
    let new_content = content.replace(
        &format!("\"version\": \"{old}\""),
        &format!("\"version\": \"{new}\""),
    );
    fs::write(path, &new_content).expect("写入 JSON 失败");
}

fn bump_patch(version: &str) -> String {
    let mut parts: Vec<u32> = version.split('.').map(|s| s.parse().unwrap_or(0)).collect();
    if parts.len() < 3 {
        parts.resize(3, 0);
    }
    parts[2] += 1;
    format!("{}.{}.{}", parts[0], parts[1], parts[2])
}

fn main() {
    let root = project_root();
    let cargo_toml = root.join("src-tauri").join("Cargo.toml");
    let tauri_conf = root.join("src-tauri").join("tauri.conf.json");
    let pkg_json = root.join("package.json");

    let old_ver = read_version(&cargo_toml);
    let new_ver = bump_patch(&old_ver);

    write_version_cargo(&cargo_toml, &old_ver, &new_ver);
    write_version_json(&tauri_conf, &old_ver, &new_ver);
    write_version_json(&pkg_json, &old_ver, &new_ver);

    println!("[版本] {} → {}", old_ver, new_ver);
}
