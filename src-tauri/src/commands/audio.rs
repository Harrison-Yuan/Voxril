use cpal::traits::{DeviceTrait, HostTrait};
use serde::{Deserialize, Serialize};

/// 音频设备类型
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceKind {
    #[serde(rename = "physical")]
    Physical,
    #[serde(rename = "blackhole")]
    BlackHole,
    #[serde(rename = "virtual")]
    Virtual,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AudioDevice {
    pub name: String,
    pub is_default: bool,
    pub kind: DeviceKind,
}

/// 判断设备类型
fn classify_device(name: &str) -> DeviceKind {
    let lower = name.to_lowercase();
    if lower.contains("blackhole") {
        DeviceKind::BlackHole
    } else if lower.contains("soundflower")
        || lower.contains("loopback")
        || lower.contains("wavtap")
        || lower.contains("virtual")
        || lower.contains("总线")
    {
        DeviceKind::Virtual
    } else {
        DeviceKind::Physical
    }
}

#[tauri::command]
pub fn get_audio_devices() -> Result<(Vec<AudioDevice>, Vec<AudioDevice>), String> {
    let host = cpal::default_host();

    let mut input_devices = Vec::new();
    let mut output_devices = Vec::new();

    let default_in = host.default_input_device().and_then(|d| d.name().ok());
    let default_out = host.default_output_device().and_then(|d| d.name().ok());

    if let Ok(devices) = host.input_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                input_devices.push(AudioDevice {
                    is_default: Some(&name) == default_in.as_ref(),
                    name: name.clone(),
                    kind: classify_device(&name),
                });
            }
        }
    }

    if let Ok(devices) = host.output_devices() {
        for device in devices {
            if let Ok(name) = device.name() {
                output_devices.push(AudioDevice {
                    is_default: Some(&name) == default_out.as_ref(),
                    name: name.clone(),
                    kind: classify_device(&name),
                });
            }
        }
    }

    Ok((input_devices, output_devices))
}
