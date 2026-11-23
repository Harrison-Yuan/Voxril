//! 成品库：转换/混音结果的登记、列表、定稿与星标。
//!
//! 设计依据：docs/ARTIFACT_LIFECYCLE_AND_WORKSPACE_LAYOUT.md §6.3。
//! 登记发生在每次转换/混音成功后；列表以数据库为准，并与磁盘做一次对账：
//! 已删除的文件剔除索引，未登记的既有文件补登记（兼容改造前的历史成品）。

use crate::commands::infer::unique_output_path;
use crate::database::OutputRecord;
use crate::state::AppState;
use crate::workspace_layout::{OutputKind, WorkspaceLayout};
use serde::Serialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use tauri::State;

#[derive(Debug, Serialize)]
pub struct OutputItem {
    pub id: i64,
    /// converted | mixed
    pub kind: String,
    pub path: String,
    pub song: String,
    /// 无法从文件名识别声音时为空串
    pub voice: String,
    pub run_id: Option<String>,
    pub transpose: i32,
    pub created_at: i64,
    pub bytes: i64,
    /// WAV 时长（秒）；头部不可解析时为 null
    pub duration_secs: Option<f64>,
    pub starred: bool,
    /// 已定稿：位于 `outputs/final`，不参与清理
    pub finalized: bool,
}

#[derive(Debug, Serialize)]
pub struct FinalizeResponse {
    pub path: String,
}

/// 列出成品库。返回前完成一次磁盘对账，保证列表与文件一致。
#[tauri::command]
pub async fn list_outputs(state: State<'_, AppState>) -> Result<Vec<OutputItem>, String> {
    let layout = {
        let workspace_root = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace_root)
    };

    import_unregistered(&layout, &state).await;

    let records = state
        .db
        .list_outputs(None)
        .await
        .map_err(|e| format!("读取成品索引失败：{}", e))?;

    let mut items = Vec::with_capacity(records.len());
    for record in records {
        let path = PathBuf::from(&record.path);
        if !path.is_file() {
            // 文件已被外部删除/清理，索引随之剔除
            if let Err(e) = state.db.delete_output(record.id).await {
                eprintln!("[成品库] 清理失效索引失败 {}: {}", record.path, e);
            }
            continue;
        }
        items.push(OutputItem {
            id: record.id,
            kind: record.kind,
            path: record.path,
            song: record.song,
            voice: record.voice,
            run_id: record.run_id,
            transpose: record.transpose,
            created_at: record.created_at,
            bytes: record.bytes,
            duration_secs: wav_duration_secs(&path),
            starred: record.starred,
            finalized: record.finalized,
        });
    }
    Ok(items)
}

/// 定稿：把成品移入 `outputs/final`，并同步索引（路径变化故重建记录）。
#[tauri::command]
pub async fn finalize_output(
    id: i64,
    state: State<'_, AppState>,
) -> Result<FinalizeResponse, String> {
    let record = state
        .db
        .get_output(id)
        .await
        .map_err(|e| format!("读取成品失败：{}", e))?
        .ok_or_else(|| format!("成品不存在：{}", id))?;

    if record.finalized {
        return Ok(FinalizeResponse { path: record.path });
    }

    let src = PathBuf::from(&record.path);
    if !src.is_file() {
        return Err(format!("成品文件已不存在：{}", record.path));
    }

    let layout = {
        let workspace_root = state.workspace_root.read().await;
        WorkspaceLayout::detect(&*workspace_root)
    };
    let final_dir = layout.final_output_dir();
    let stem = src
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or_else(|| format!("成品文件名不合法：{}", record.path))?;
    let dst = unique_output_path(&final_dir, stem, "wav");

    move_file(&src, &dst)?;
    // 与成品同名的直播相位缓存一并迁移，保持成对
    let sidecar = src.with_extension("stft.npy");
    if sidecar.is_file() {
        let _ = move_file(&sidecar, &dst.with_extension("stft.npy"));
    }

    let new_path = dst.to_string_lossy().to_string();
    let bytes = std::fs::metadata(&dst)
        .map(|m| m.len() as i64)
        .unwrap_or(record.bytes);
    state
        .db
        .upsert_output(OutputRecord {
            id: 0,
            kind: record.kind.clone(),
            path: new_path.clone(),
            song: record.song.clone(),
            voice: record.voice.clone(),
            run_id: record.run_id.clone(),
            transpose: record.transpose,
            created_at: record.created_at,
            bytes,
            starred: record.starred,
            finalized: true,
        })
        .await
        .map_err(|e| format!("更新成品索引失败：{}", e))?;

    // 新路径登记成功后再删旧记录，避免中途失败导致成品从列表消失
    if let Err(e) = state.db.delete_output(id).await {
        eprintln!("[成品库] 删除旧索引失败 {}: {}", id, e);
    }

    Ok(FinalizeResponse { path: new_path })
}

#[tauri::command]
pub async fn set_output_starred(
    id: i64,
    starred: bool,
    state: State<'_, AppState>,
) -> Result<(), String> {
    state
        .db
        .set_output_starred(id, starred)
        .await
        .map_err(|e| format!("更新星标失败：{}", e))
}

/// 扫描输出目录，把尚未登记的历史成品补入索引。
async fn import_unregistered(layout: &WorkspaceLayout, state: &State<'_, AppState>) {
    // v2 下 converted/mixed 共用 `outputs/draft`，此时按文件名区分类型
    let mut dirs: HashMap<PathBuf, Option<&'static str>> = HashMap::new();
    for (dir, kind) in [
        (layout.output_dir(OutputKind::Vocal), Some("converted")),
        (layout.output_dir(OutputKind::Mixed), Some("mixed")),
        (layout.final_output_dir(), None),
    ] {
        dirs.entry(dir)
            .and_modify(|existing| {
                if *existing != kind {
                    *existing = None;
                }
            })
            .or_insert(kind);
    }

    let existing: std::collections::HashSet<String> = state
        .db
        .list_outputs(None)
        .await
        .map(|records| records.into_iter().map(|r| r.path).collect())
        .unwrap_or_default();

    let final_dir = layout.final_output_dir();
    for (dir, fixed_kind) in dirs {
        let is_final = dir == final_dir;
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() || path.extension().and_then(|e| e.to_str()) != Some("wav") {
                continue;
            }
            let key = path.to_string_lossy().to_string();
            if existing.contains(&key) {
                continue;
            }
            let Some(meta) = parse_output_name(&path) else {
                continue;
            };
            let kind = fixed_kind.unwrap_or(meta.kind).to_string();
            let created_at = entry
                .metadata()
                .ok()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or_else(|| chrono::Local::now().timestamp());
            let bytes = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
            if let Err(e) = state
                .db
                .upsert_output(OutputRecord {
                    id: 0,
                    kind,
                    path: key,
                    song: meta.song,
                    voice: meta.voice,
                    run_id: meta.run_id,
                    transpose: meta.transpose,
                    created_at,
                    bytes,
                    starred: false,
                    finalized: is_final,
                })
                .await
            {
                eprintln!("[成品库] 补登记失败 {}: {}", path.display(), e);
            }
        }
    }
}

struct ParsedName {
    kind: &'static str,
    song: String,
    voice: String,
    run_id: Option<String>,
    transpose: i32,
}

/// 从成品文件名解析元数据。无法识别时仅返回 `song`（文件名本身），其余留空。
///
/// 合成作品有两种历史写法：新的是 `merged_<歌名>_...` 前缀，旧的是 `<歌名>_..._mix<时分>` 后缀，
/// 两者都要认，否则 v2 布局下 `outputs/draft` 里的混音成品会被登记成人声。
fn parse_output_name(path: &Path) -> Option<ParsedName> {
    let full = path.file_stem().and_then(|s| s.to_str())?.to_string();
    let merged_prefix = full.starts_with("merged_");
    let stem = if merged_prefix {
        full["merged_".len()..].to_string()
    } else {
        full
    };
    let re = regex::Regex::new(
        r"^(?P<song>.+)_(?P<voice>[^_]+)_(?P<run>\d{6}-\d{4}(?:-\d+)?)_key(?P<key>[+-]?\d+)_(?P<tail>mix)?\d{4}(?:-\d+)?$",
    )
    .ok()?;
    if let Some(c) = re.captures(&stem) {
        let kind = if merged_prefix || c.name("tail").is_some() {
            "mixed"
        } else {
            "converted"
        };
        return Some(ParsedName {
            kind,
            song: c["song"].to_string(),
            voice: c["voice"].to_string(),
            run_id: Some(c["run"].to_string()),
            transpose: c["key"].parse().unwrap_or(0),
        });
    }
    Some(ParsedName {
        kind: if merged_prefix || stem.contains("_mix") {
            "mixed"
        } else {
            "converted"
        },
        song: stem,
        voice: String::new(),
        run_id: None,
        transpose: 0,
    })
}

/// 读取 WAV 头部计算时长：`data` 块字节数 / `byte_rate`。失败返回 None。
fn wav_duration_secs(path: &Path) -> Option<f64> {
    use std::io::{Read, Seek, SeekFrom};

    let mut file = std::fs::File::open(path).ok()?;
    let mut header = [0u8; 12];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" {
        return None;
    }

    let mut byte_rate: Option<u32> = None;
    let mut data_bytes: Option<u32> = None;
    let file_len = file.metadata().ok()?.len();

    let mut pos: u64 = 12;
    while pos + 8 <= file_len {
        file.seek(SeekFrom::Start(pos)).ok()?;
        let mut chunk = [0u8; 8];
        file.read_exact(&mut chunk).ok()?;
        let id = &chunk[0..4];
        let size = u32::from_le_bytes([chunk[4], chunk[5], chunk[6], chunk[7]]);
        let body = pos + 8;

        if id == b"fmt " {
            let mut fmt = vec![0u8; size as usize];
            if file.read_exact(&mut fmt).is_ok() && fmt.len() >= 16 {
                byte_rate = Some(u32::from_le_bytes([fmt[8], fmt[9], fmt[10], fmt[11]]));
            }
        } else if id == b"data" {
            // 部分写入器把 data 大小留为 0 或 0xFFFFFFFF，此时按剩余长度推算
            data_bytes = if size == 0 || size == u32::MAX || body + size as u64 > file_len {
                Some((file_len.saturating_sub(body)) as u32)
            } else {
                Some(size)
            };
            break;
        }

        // 块大小为奇数时补齐一字节
        pos = body + size as u64 + (size as u64 % 2);
    }

    match (byte_rate, data_bytes) {
        (Some(rate), Some(bytes)) if rate > 0 => Some(bytes as f64 / rate as f64),
        _ => None,
    }
}

fn move_file(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("创建目录失败 {}: {}", parent.display(), e))?;
    }
    match std::fs::rename(src, dst) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(src, dst)
                .map_err(|e| format!("复制成品失败 {}: {}", src.display(), e))?;
            std::fs::remove_file(src)
                .map_err(|e| format!("删除原成品失败 {}: {}", src.display(), e))?;
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_converted_name_extracts_metadata() {
        let parsed = parse_output_name(Path::new("枪火_harrison_261122-0156_key-3_0930.wav")).unwrap();
        assert_eq!(parsed.kind, "converted");
        assert_eq!(parsed.song, "枪火");
        assert_eq!(parsed.voice, "harrison");
        assert_eq!(parsed.run_id.as_deref(), Some("261122-0156"));
        assert_eq!(parsed.transpose, -3);
    }

    #[test]
    fn parse_mixed_name_and_strip_collision_suffix() {
        let parsed = parse_output_name(Path::new("枪火_harrison_261122-0156_key-3_mix0955-2.wav")).unwrap();
        assert_eq!(parsed.kind, "mixed");
        assert_eq!(parsed.song, "枪火");
        assert_eq!(parsed.run_id.as_deref(), Some("261122-0156"));
        assert_eq!(parsed.transpose, -3);
    }

    #[test]
    fn parse_merged_prefixed_name_is_mixed() {
        let parsed =
            parse_output_name(Path::new("merged_枪火_harrison_261122-0156_key-3_0955.wav")).unwrap();
        assert_eq!(parsed.kind, "mixed");
        assert_eq!(parsed.song, "枪火");
        assert_eq!(parsed.voice, "harrison");
        assert_eq!(parsed.run_id.as_deref(), Some("261122-0156"));
        assert_eq!(parsed.transpose, -3);

        let loose = parse_output_name(Path::new("merged_枪火_0955.wav")).unwrap();
        assert_eq!(loose.kind, "mixed");
        assert_eq!(loose.song, "枪火_0955");
    }

    #[test]
    fn parse_unrecognized_name_falls_back_to_stem() {
        let parsed = parse_output_name(Path::new("legacy_vocal.wav")).unwrap();
        assert_eq!(parsed.kind, "converted");
        assert_eq!(parsed.song, "legacy_vocal");
        assert!(parsed.voice.is_empty());
        assert!(parsed.run_id.is_none());
    }
}
