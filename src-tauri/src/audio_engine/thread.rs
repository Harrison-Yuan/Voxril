//! 音频线程提权 — macOS QoS 优先级提升
//!
//! `boost_audio_thread_priority()` 将当前线程设为 `QOS_CLASS_USER_INTERACTIVE`，
//! 避免被 OBS/游戏等高负载进程抢占 CPU 时间片导致音频断流。
//! 免 root 调用，所有 macOS 应用均可使用。

/// 将当前线程提升至用户交互级实时优先级。
///
/// 应在每个音频处理线程启动时立即调用。
pub fn boost_audio_thread_priority() {
    #[cfg(target_os = "macos")]
    unsafe {
        let ret = libc::pthread_set_qos_class_self_np(
            libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE,
            0,
        );
        if ret != 0 {
            eprintln!("[线程] ⚠️ 设置 QoS 优先级失败: {}", ret);
        } else {
            eprintln!("[线程] ✅ 已提升至 QOS_CLASS_USER_INTERACTIVE");
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        // 非 macOS 平台暂不处理
    }
}
