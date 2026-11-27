/**
 * 外壳与非组件代码（store 层、TitleBar、PlaybackControls、LiveSinging）。
 * 键前缀：`shell.` / `playback.` / `live.`
 *
 * zh 与 en 键集合、顺序严格一致，逐条并排；已有键（common.* / lyrics.* 等）直接复用，
 * 不在此重复定义。
 */
export const shell = {
  zh: {
    // ── TitleBar 窗口按钮（macOS 红绿灯的 aria-label / tooltip）──
    // 关闭复用 `common.close`
    'shell.win_minimize': '最小化',
    'shell.win_maximize': '最大化',
    'shell.win_restore': '还原',

    // ── store 层写入的一次性消息（日志、进度、错误串）──
    'shell.download_preparing': '准备下载...',
    'shell.failed_detail': '失败：{err}',
    'shell.convert_failed_detail': '转换失败：{err}',
    'shell.cover_failed': '翻唱生成失败',

    // ── PlaybackControls 播放控件 ──
    // 加载歌曲中复用 `lyrics.loading`
    'playback.loaded': '已加载',
    'playback.load_failed': '加载失败: {err}',
    'playback.seek_failed': '跳转失败: {err}',
    'playback.play': '播放',
    'playback.source_live': '真人',
    'playback.source_cover': '翻唱',
    'playback.ai_cover': 'AI 翻唱',

    // ── LiveSinging 直播唱歌（页面 + liveSingingStore）──
    'live.preparing_engine': '正在准备启动直播引擎...',
    'live.engine_ready': '引擎已就绪',
    'live.start_timeout': '启动超时：后端没有返回就绪状态。请检查模型路径、音频设备权限或查看引擎日志。',
    'live.start_failed_no_detail': '启动失败，但后端未返回错误详情',
    'live.fetch_devices_failed': '获取音频设备失败: {err}',
    'live.key_label': '调性',
    'live.key_source_accompaniment': '伴奏',
    'live.key_source_vocal': '人声',
  },
  en: {
    // ── TitleBar window buttons (macOS traffic-light aria-label / tooltip) ──
    // Close reuses `common.close`
    'shell.win_minimize': 'Minimize',
    'shell.win_maximize': 'Maximize',
    'shell.win_restore': 'Restore',

    // ── One-off messages written by stores (log, progress, error strings) ──
    'shell.download_preparing': 'Preparing download...',
    'shell.failed_detail': 'Failed: {err}',
    'shell.convert_failed_detail': 'Conversion failed: {err}',
    'shell.cover_failed': 'Cover generation failed',

    // ── PlaybackControls ──
    // "Loading song" reuses `lyrics.loading`
    'playback.loaded': 'Loaded',
    'playback.load_failed': 'Failed to load: {err}',
    'playback.seek_failed': 'Failed to seek: {err}',
    'playback.play': 'Play',
    'playback.source_live': 'Live',
    'playback.source_cover': 'Cover',
    'playback.ai_cover': 'AI Cover',

    // ── LiveSinging page + liveSingingStore ──
    'live.preparing_engine': 'Preparing live singing engine...',
    'live.engine_ready': 'Engine ready',
    'live.start_timeout': 'Startup timed out: the backend never reported ready. Check the model path, audio device permissions, or the engine log.',
    'live.start_failed_no_detail': 'Start failed, but the backend returned no error details',
    'live.fetch_devices_failed': 'Failed to load audio devices: {err}',
    'live.key_label': 'Key',
    'live.key_source_accompaniment': 'Accompaniment',
    'live.key_source_vocal': 'Vocals',
  },
}
