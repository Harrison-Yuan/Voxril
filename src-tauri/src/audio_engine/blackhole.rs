//! BlackHole 虚拟音频设备检测与路由决策
//!
//! BlackHole 是 macOS 上的虚拟音频桥接设备，让不同音频应用共享音频数据。
//! 本模块固定使用 2ch 模式（行业标准实践），输入输出均使用 ch1-2。
//! 如需多通道路由（如 DAW Send/Return），用户应在 DAW 端自行配置。

use cpal::traits::{DeviceTrait, HostTrait};

/// BlackHole 路由决策结果
#[derive(Debug, Clone)]
pub struct BlackholeRoute {
    /// 路由模式标识
    /// - `"direct"` — 直通物理设备
    /// - `"blackhole"` — 使用 BlackHole 2ch 桥接
    pub mode: &'static str,

    /// BlackHole 设备名称（`mode == "blackhole"` 时有效）
    pub device_name: Option<String>,
}

impl BlackholeRoute {
    pub fn is_blackhole(&self) -> bool {
        self.mode == "blackhole"
    }
}

/// 检测 BlackHole 设备并决定是否启用桥接模式。
///
/// # 逻辑
/// 1. 无 BlackHole → 直通模式
/// 2. 有 BlackHole + 无 DAW → 直通模式
/// 3. 有 BlackHole + 有 DAW → BlackHole 2ch 桥接模式
pub fn detect_route(host: &cpal::Host) -> BlackholeRoute {
    let bh_found = find_blackhole(host);

    let Some(bh_name) = bh_found else {
        eprintln!("[blackhole] 未检测到 BlackHole 设备，使用直通模式");
        return BlackholeRoute { mode: "direct", device_name: None };
    };

    eprintln!("[blackhole] 检测到: {}", bh_name);

    if !super::daw::is_daw_running() {
        eprintln!("[blackhole] 未检测到 DAW，保持直通模式");
        return BlackholeRoute { mode: "direct", device_name: Some(bh_name) };
    }

    eprintln!("[blackhole] DAW 运行中 → 启用 BlackHole 2ch 输出桥接");
    eprintln!("[blackhole] 输出: BlackHole ch1-2 → DAW 可选接收 | 输入: 仍使用物理麦克风");
    BlackholeRoute { mode: "blackhole", device_name: Some(bh_name) }
}

/// 查找系统中第一个名称包含 "blackhole" 的音频设备，返回设备名
fn find_blackhole(host: &cpal::Host) -> Option<String> {
    let is_bh = |name: &str| name.to_lowercase().contains("blackhole");

    for list in [host.output_devices(), host.input_devices()] {
        if let Ok(devices) = list {
            for device in devices {
                if let Ok(name) = device.name() {
                    if is_bh(&name) {
                        return Some(name);
                    }
                }
            }
        }
    }
    None
}
