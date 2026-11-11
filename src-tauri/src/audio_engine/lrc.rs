//! LRC 歌词解析器
//!
//! 支持标准 LRC 格式：
//!   [mm:ss.xx]歌词内容
//!   [mm:ss.xx][mm:ss.xx]歌词内容（多时间戳）
//!
//! 生成 Rust 结构体，供直播引擎实时推送到前端。

use serde::Serialize;

/// 单行歌词
#[derive(Debug, Clone, Serialize)]
pub struct LrcLine {
    /// 时间戳（秒）
    pub time_secs: f64,
    /// 歌词文本
    pub text: String,
}

/// 歌词文件完整数据
#[derive(Debug, Clone, Serialize)]
pub struct LrcData {
    pub lines: Vec<LrcLine>,
}

impl LrcData {
    /// 从 LRC 文本解析
    pub fn parse(content: &str) -> Self {
        let mut lines = Vec::new();

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }

            // 匹配 [mm:ss.xx] 或 [mm:ss.xxx] 格式
            // 支持一行多个时间戳: [00:01.00][00:05.00]歌词
            let mut timestamps = Vec::new();
            let mut remaining = line;

            loop {
                if let Some(rest) = remaining.strip_prefix('[') {
                    if let Some(close) = rest.find(']') {
                        let ts_str = &rest[..close];
                        if let Some(secs) = parse_lrc_timestamp(ts_str) {
                            timestamps.push(secs);
                        }
                        remaining = &rest[close + 1..];
                        continue;
                    }
                }
                break;
            }

            if timestamps.is_empty() {
                continue;
            }

            let text = remaining.trim().to_string();
            if text.is_empty() {
                continue;
            }

            for ts in timestamps {
                lines.push(LrcLine {
                    time_secs: ts,
                    text: text.clone(),
                });
            }
        }

        // 按时间戳排序
        lines.sort_by(|a, b| a.time_secs.partial_cmp(&b.time_secs).unwrap_or(std::cmp::Ordering::Equal));

        Self { lines }
    }

    /// 获取总行数
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// 是否为空
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// 获取总时长（最后一行的结束时间）
    pub fn total_duration_secs(&self) -> f64 {
        self.lines.last().map(|l| l.time_secs).unwrap_or(0.0)
    }

    /// 根据当前时间（秒）查找当前歌词行的索引
    ///
    /// 返回当前行索引和下一行索引：
    /// - current_index: 当前应该高亮的歌词行（-1 表示没有匹配）
    /// - next_index: 下一行歌词的索引（-1 表示已经到了最后一行）
    pub fn find_line_at(&self, secs: f64) -> (isize, isize) {
        if self.lines.is_empty() {
            return (-1, -1);
        }

        // 二分查找最后一个 time_secs <= secs 的行
        let mut lo = 0isize;
        let mut hi = self.lines.len() as isize - 1;
        let mut result = -1;

        while lo <= hi {
            let mid = lo + (hi - lo) / 2;
            if self.lines[mid as usize].time_secs <= secs {
                result = mid;
                lo = mid + 1;
            } else {
                hi = mid - 1;
            }
        }

        if result < 0 {
            (-1, 0)
        } else {
            let next = if (result as usize) < self.lines.len() - 1 {
                result + 1
            } else {
                -1
            };
            (result, next)
        }
    }

    /// 生成标准 LRC 文本
    pub fn to_lrc_text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let minutes = (line.time_secs as u64) / 60;
            let seconds = (line.time_secs as u64) % 60;
            let centisecs = ((line.time_secs.fract() * 100.0).round() as u64).min(99);
            out.push_str(&format!("[{:02}:{:02}.{:02}]{}\n", minutes, seconds, centisecs, line.text));
        }
        out
    }
}

/// 解析 LRC 时间戳 [mm:ss.xx] 或 [mm:ss.xx-N]（网易云格式）
fn parse_lrc_timestamp(ts: &str) -> Option<f64> {
    let ts = ts.trim();
    // 处理网易云后缀：去掉末尾的 -N（如 "00:00.00-1" → "00:00.00"）
    let ts = if let Some(pos) = ts.find('-') {
        &ts[..pos]
    } else {
        ts
    };
    if let Some((min_str, rest)) = ts.split_once(':') {
        let minutes: f64 = min_str.parse().ok()?;
        let seconds: f64 = rest.parse().ok()?;
        Some(minutes * 60.0 + seconds)
    } else {
        ts.parse::<f64>().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_basic() {
        let lrc = "[00:01.50]第一行歌词\n[00:05.00]第二行歌词\n";
        let data = LrcData::parse(lrc);
        assert_eq!(data.lines.len(), 2);
        assert!((data.lines[0].time_secs - 1.5).abs() < 0.01);
        assert_eq!(data.lines[0].text, "第一行歌词");
        assert!((data.lines[1].time_secs - 5.0).abs() < 0.01);
        assert_eq!(data.lines[1].text, "第二行歌词");
    }

    #[test]
    fn test_parse_multi_timestamp() {
        let lrc = "[00:01.00][00:05.00]重复歌词\n";
        let data = LrcData::parse(lrc);
        assert_eq!(data.lines.len(), 2);
        assert_eq!(data.lines[0].text, "重复歌词");
        assert_eq!(data.lines[1].text, "重复歌词");
    }

    #[test]
    fn test_find_line() {
        let lrc = "[00:01.00]第一行\n[00:03.00]第二行\n[00:05.00]第三行\n";
        let data = LrcData::parse(lrc);
        assert_eq!(data.find_line_at(0.5), (-1, 0));
        assert_eq!(data.find_line_at(1.0), (0, 1));
        assert_eq!(data.find_line_at(2.0), (0, 1));
        assert_eq!(data.find_line_at(3.0), (1, 2));
        assert_eq!(data.find_line_at(4.0), (1, 2));
        assert_eq!(data.find_line_at(5.0), (2, -1));
        assert_eq!(data.find_line_at(10.0), (2, -1));
    }

    #[test]
    fn test_roundtrip() {
        let lrc = "[00:01.50]hello world\n[00:03.00]foo bar\n";
        let data = LrcData::parse(lrc);
        let text = data.to_lrc_text();
        let data2 = LrcData::parse(&text);
        assert_eq!(data.lines.len(), data2.lines.len());
        for (a, b) in data.lines.iter().zip(data2.lines.iter()) {
            assert!((a.time_secs - b.time_secs).abs() < 0.01);
            assert_eq!(a.text, b.text);
        }
    }
}
