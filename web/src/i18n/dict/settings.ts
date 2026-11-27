/** 设置页与分离页残留（pages/Settings、pages/UVR）。键前缀：`settings.` / `uvr.` / `about.` */
export const settings = {
  zh: {
    // ── 下载源选项 ──
    'settings.src_auto': '自动（推荐）',
    'settings.src_official': '官方源',
    'settings.src_cn_mirror': '国内镜像',
    'settings.src_custom': '自定义',
    'settings.src_pypi_official': '官方 PyPI',
    'settings.src_pypi_tuna': '清华镜像',
    'settings.src_pypi_aliyun': '阿里云镜像',

    // ── 模型与依赖下载源卡片 ──
    'settings.download_card': '模型与依赖下载源',
    'settings.download_desc': '基础模型支持多源下载、断点续传与 SHA256 校验；Python 依赖支持 PyPI 镜像。失败都会自动切换下一个源。',
    'settings.hf_endpoint': 'HuggingFace 端点',
    'settings.gh_proxy': 'GitHub 加速前缀（可选）',
    'settings.pip_source': 'Python 依赖源',
    'settings.pip_index': 'PyPI 索引地址',
    'settings.source_hint_auto': '自动探测官方源，不可达时切换到国内镜像；下载中断后再次点击会从断点继续。',
    'settings.source_hint_official': '始终使用 huggingface.co 与 github.com 直连，适合海外网络。',
    'settings.source_hint_cn': '优先使用 hf-mirror.com 与 GitHub 加速前缀，适合国内网络；失败后仍会回退官方源。',
    'settings.source_hint_custom': '先使用你填写的端点，失败后回退官方源。',
    'settings.pip_hint_auto': '自动探测官方 PyPI，不可达时切换清华镜像；首次安装或补齐缺失依赖时生效。',
    'settings.pip_hint_official': '始终使用官方 PyPI，适合海外网络。',
    'settings.pip_hint_tuna': '优先清华 PyPI 镜像，失败后回退官方源。',
    'settings.pip_hint_aliyun': '优先阿里云 PyPI 镜像，失败后回退官方源。',
    'settings.pip_hint_custom': '先使用你填写的索引地址，失败后回退官方源。',
    'settings.download_save_failed': '保存下载设置失败：{err}',

    // ── 语言卡片 ──
    'settings.language_hint': '切换后立即生效，下次启动自动记住。',

    // ── 工作目录卡片 ──
    'settings.workspace_desc': '所有训练数据、模型文件和索引文件都存储在此目录下',
    'settings.workspace_restart_warn': '修改后需要重启应用才能生效',
    'settings.storage_scan_failed': '统计存储占用失败：{err}',

    // ── 工作区结构升级 ──
    'settings.layout_card': '工作区结构',
    'settings.layout_v2_tag': '新布局（v2）',
    'settings.layout_v1_tag': '旧布局（v1）',
    'settings.layout_migrated_at': '升级于 ',
    'settings.layout_v2_desc': '目录已整理为「进 / 算 / 出 / 留」四层：inputs、work、outputs、models。',
    'settings.layout_v1_desc': '可将工作区整理为「进 / 算 / 出 / 留」四层结构：素材进 inputs，计算产物进 work，成品进 outputs，模型资产进 models。迁移前可先预览，失败会自动回滚。',
    'settings.layout_preview_btn': '预览升级',
    'settings.migrate_preview_title': '升级工作区结构（预览）',
    'settings.migrate_preview_summary': '共 {n} 项，合计 {size}。迁移为同卷重命名，瞬时完成；任一项失败会整体回滚。',
    'settings.migrate_preview_empty': '没有需要移动的内容，升级只会建立新目录结构。',
    'settings.migrate_start': '开始升级',
    'settings.migrate_done': '升级完成：移动 {n} 项，共 {size}',
    'settings.migrate_failed': '升级失败：{err}',
    'settings.preview_failed': '生成迁移预览失败：{err}',

    // ── 清理中心 ──
    'settings.cleanup_card': '清理中心',
    'settings.reclaimable_now': '可立即释放',
    'settings.reclaim': '释放空间',
    'settings.reclaim_confirm_title': '确认释放空间',
    'settings.reclaim_confirm_desc': '将删除所有批次的 1_16k_wavs、eval、TensorBoard 事件与孤立 STFT 缓存，预计释放 {size}。训练与转换不受影响。',
    'settings.reclaim_confirm_ok': '释放',
    'settings.reclaim_done': '已释放 {size}（{n} 个批次）',
    'settings.reclaim_failed': '释放失败：{err}',
    'settings.deep_clean_note': '构建缓存（应用本身）单独清理，不影响工作区数据',
    'settings.deep_clean_btn': '清理构建缓存',
    'settings.deep_clean_failed': '清理失败：{err}',
    'settings.scan_storage': '统计存储占用',

    // ── 分离页：选择与校验 ──
    'uvr.folder_pick_failed': '选择文件夹失败：{err}',
    'uvr.pick_song_title': '选择要分离的单首歌曲',
    'uvr.song_selected': '已选择：{name}',
    'uvr.song_pick_failed': '选择歌曲失败：{err}',
    'uvr.input_required': '请填写输入文件夹路径',

    // ── 分离页：模型选项 ──
    'uvr.model_tooltip': '不同的模型擅长不同的分离任务',
    'uvr.model_bs_roformer': 'BS-RoFormer (人声/伴奏)',
    'uvr.model_bs_roformer_desc': '当前最强的人声分离模型，SDR 12.97',
    'uvr.model_mdx_main': 'MDX-Net Main Vocals (主唱)',
    'uvr.model_mdx_main_desc': '专门用于分离主唱，尽可能去除和声与伴奏',
    'uvr.model_mdx_karaoke2': 'MDX-Net Karaoke 2 (去和声)',
    'uvr.model_mdx_karaoke2_desc': '去除伴奏的同时，尽可能剥离背景和声',
    'uvr.model_mdx_voc_ft': 'MDX-Net Voc FT (经典)',
    'uvr.model_mdx_voc_ft_desc': '经典的 MDX-Net 人声提取模型，稳定性好',
    'uvr.model_kim_v2': 'Kim Vocal 2 (清晰人声)',
    'uvr.model_kim_v2_desc': '在人声清晰度上有很好表现的 MDX-Net 模型',

    // ── 分离页：输出与说明 ──
    'uvr.output_dir_default': '默认：{path}',
    'uvr.output_dir_placeholder': '留空则使用默认数据集目录',
    'uvr.single_mode': '单曲模式',
    'uvr.info': '说明',
    'uvr.info_output_files': '输出文件',
    'uvr.info_vocal_desc': '人声（用于语音转换）',
    'uvr.info_instrument_desc': '纯伴奏',
    'uvr.info_tips': '提示',
    'uvr.tip_use_vocal': '分离后的人声文件留到「语音转换」页面使用',
    'uvr.tip_keep_instrument': '伴奏文件先保留，等转换后再混合导出',

    // ── 分离页：进度与结果 ──
    'uvr.progress_file': '文件 {cur}/{total}',
    'uvr.progress_segment': '音频分段 {cur}/{total}',
    'uvr.progress_preparing': '准备中...',
    'uvr.result_detected': '已检测到分离输出文件',
    'uvr.result_missing': '后端已返回，但未检测到输出文件',
    'uvr.not_detected': '（未检测到）',
    'uvr.instrument_fill_hint': '伴奏路径已记录，切换到语音转换页面后会自动填充',
    'uvr.instrument_merge_hint': '伴奏路径已记录，到语音转换页面完成变调后一键合成',
    'uvr.next_steps': '下一步',

    // ── 标点随语言：冒号/分号写进文案，不在代码里硬拼全角符号 ──
    'settings.migrate_failed_detail': '迁移失败：{err}',
    'settings.list_sep': '；',
    'uvr.agg_label': '激进程度：{value}',
  },
  en: {
    // ── 下载源选项 ──
    'settings.src_auto': 'Auto (recommended)',
    'settings.src_official': 'Official only',
    'settings.src_cn_mirror': 'China mirror',
    'settings.src_custom': 'Custom',
    'settings.src_pypi_official': 'Official PyPI',
    'settings.src_pypi_tuna': 'Tsinghua mirror',
    'settings.src_pypi_aliyun': 'Aliyun mirror',

    // ── 模型与依赖下载源卡片 ──
    'settings.download_card': 'Model & dependency sources',
    'settings.download_desc': 'Base models support multiple sources, resume and SHA256 checks; Python packages support PyPI mirrors. Failures fall back automatically.',
    'settings.hf_endpoint': 'HuggingFace endpoint',
    'settings.gh_proxy': 'GitHub proxy prefix (optional)',
    'settings.pip_source': 'Python package source',
    'settings.pip_index': 'PyPI index URL',
    'settings.source_hint_auto': 'Probes the official source first, falls back to a mirror, and resumes interrupted downloads.',
    'settings.source_hint_official': 'Always use huggingface.co and github.com directly; best for networks outside China.',
    'settings.source_hint_cn': 'Prefer hf-mirror.com and a GitHub proxy; falls back to official sources on failure.',
    'settings.source_hint_custom': 'Use your endpoints first, then fall back to official sources.',
    'settings.pip_hint_auto': 'Probes the official PyPI and falls back to the Tsinghua mirror; applies on next install.',
    'settings.pip_hint_official': 'Always use the official PyPI; best for networks outside China.',
    'settings.pip_hint_tuna': 'Prefer the Tsinghua PyPI mirror, then fall back to official PyPI.',
    'settings.pip_hint_aliyun': 'Prefer the Aliyun PyPI mirror, then fall back to official PyPI.',
    'settings.pip_hint_custom': 'Use your index URL first, then fall back to official PyPI.',
    'settings.download_save_failed': 'Failed to save download settings: {err}',

    // ── 语言卡片 ──
    'settings.language_hint': 'Takes effect immediately. Your preference will be remembered on next launch.',

    // ── 工作目录卡片 ──
    'settings.workspace_desc': 'All training data, model files, and index files are stored in this directory.',
    'settings.workspace_restart_warn': 'Changes require a restart to take effect',
    'settings.storage_scan_failed': 'Failed to scan storage usage: {err}',

    // ── 工作区结构升级 ──
    'settings.layout_card': 'Workspace Layout',
    'settings.layout_v2_tag': 'New layout (v2)',
    'settings.layout_v1_tag': 'Old layout (v1)',
    'settings.layout_migrated_at': 'Migrated ',
    'settings.layout_v2_desc': 'Directories are organized into inputs / work / outputs / models.',
    'settings.layout_v1_desc': 'Reorganize the workspace into inputs / work / outputs / models. You can preview first; any failure rolls back automatically.',
    'settings.layout_preview_btn': 'Preview upgrade',
    'settings.migrate_preview_title': 'Upgrade workspace layout (preview)',
    'settings.migrate_preview_summary': '{n} items, {size} total. Moves are same-volume renames and roll back on any failure.',
    'settings.migrate_preview_empty': 'Nothing to move; the upgrade only creates the new directory structure.',
    'settings.migrate_start': 'Upgrade',
    'settings.migrate_done': 'Upgrade complete: moved {n} items, {size} total',
    'settings.migrate_failed': 'Upgrade failed: {err}',
    'settings.preview_failed': 'Failed to generate migration preview: {err}',

    // ── 清理中心 ──
    'settings.cleanup_card': 'Cleanup Center',
    'settings.reclaimable_now': 'Reclaimable now',
    'settings.reclaim': 'Reclaim',
    'settings.reclaim_confirm_title': 'Reclaim space',
    'settings.reclaim_confirm_desc': 'This removes 1_16k_wavs, eval, TensorBoard events and orphan STFT caches for all batches. About {size} will be freed.',
    'settings.reclaim_confirm_ok': 'Reclaim',
    'settings.reclaim_done': 'Freed {size} across {n} batches',
    'settings.reclaim_failed': 'Failed to reclaim space: {err}',
    'settings.deep_clean_note': 'Build caches are cleaned separately without touching workspace data.',
    'settings.deep_clean_btn': 'Clean build cache',
    'settings.deep_clean_failed': 'Cleanup failed: {err}',
    'settings.scan_storage': 'Scan storage',

    // ── 分离页：选择与校验 ──
    'uvr.folder_pick_failed': 'Failed to select folder: {err}',
    'uvr.pick_song_title': 'Select a single song to separate',
    'uvr.song_selected': 'Selected: {name}',
    'uvr.song_pick_failed': 'Failed to select song: {err}',
    'uvr.input_required': 'Please enter the input folder path',

    // ── 分离页：模型选项 ──
    'uvr.model_tooltip': 'Different models excel at different separation tasks',
    'uvr.model_bs_roformer': 'BS-RoFormer (Vocals/Accompaniment)',
    'uvr.model_bs_roformer_desc': 'The strongest vocal separation model so far, SDR 12.97',
    'uvr.model_mdx_main': 'MDX-Net Main Vocals (Lead)',
    'uvr.model_mdx_main_desc': 'Isolates the lead vocal, removing harmonies and accompaniment',
    'uvr.model_mdx_karaoke2': 'MDX-Net Karaoke 2 (Backing Removal)',
    'uvr.model_mdx_karaoke2_desc': 'Removes the accompaniment while stripping backing harmonies',
    'uvr.model_mdx_voc_ft': 'MDX-Net Voc FT (Classic)',
    'uvr.model_mdx_voc_ft_desc': 'Classic MDX-Net vocal extraction model, very stable',
    'uvr.model_kim_v2': 'Kim Vocal 2 (Clean Vocals)',
    'uvr.model_kim_v2_desc': 'MDX-Net model with excellent vocal clarity',

    // ── 分离页：输出与说明 ──
    'uvr.output_dir_default': 'Default: {path}',
    'uvr.output_dir_placeholder': 'Leave empty to use the default dataset folder',
    'uvr.single_mode': 'Single Song Mode',
    'uvr.info': 'Info',
    'uvr.info_output_files': 'Output Files',
    'uvr.info_vocal_desc': 'Vocals (for voice conversion)',
    'uvr.info_instrument_desc': 'Instrumental only',
    'uvr.info_tips': 'Tips',
    'uvr.tip_use_vocal': 'Take separated vocals to the Voice Conversion page',
    'uvr.tip_keep_instrument': 'Keep the instrumental for later mixing',

    // ── 分离页：进度与结果 ──
    'uvr.progress_file': 'File {cur}/{total}',
    'uvr.progress_segment': 'Segment {cur}/{total}',
    'uvr.progress_preparing': 'Preparing...',
    'uvr.result_detected': 'Separation output detected',
    'uvr.result_missing': 'Backend returned but no output detected',
    'uvr.not_detected': '(not found)',
    'uvr.instrument_fill_hint': 'Instrumental path saved. Will auto-fill on Voice Conversion page.',
    'uvr.instrument_merge_hint': 'Instrumental saved. Go to Voice Conversion to mix after pitch shift.',
    'uvr.next_steps': 'Next Steps',

    // ── Language-specific punctuation: kept in the copy, not concatenated in code ──
    'settings.migrate_failed_detail': 'Migration failed: {err}',
    'settings.list_sep': '; ',
    'uvr.agg_label': 'Aggression: {value}',
  },
}
