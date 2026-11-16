# 可以认领的任务

下面是已经想清楚、可以直接动手的任务。开 Issue 时可以复制对应段落作为正文，并加上 `good first issue`（适合新手）或 `help wanted` 标签。

每个任务都写明了起点文件、验收标准和难度。动手前建议先读 [CONTRIBUTING.md](../CONTRIBUTING.md)。

---

## 1. 英文界面完整化：把页面里硬编码的中文抽到 i18n

**难度**：🟢 新手友好 · **技能**：React / TypeScript

**背景**：`web/src/i18n/zh.ts`、`en.ts` 已支持中英切换，但「语音转换」「人声分离」「模型管理」「设置」等页面仍有大量中文字符串写死在组件里，切到 English 后中英混排。

**起点**：`web/src/pages/Infer/index.tsx`、`UVR/index.tsx`、`Models/index.tsx`、`Settings/index.tsx`，文案表在 `web/src/i18n/`。

**验收**：语言切到 English 后，上述页面不再出现中文（专有名词如 BS-RoFormer 除外）；中文界面文案保持不变。

---

## 2. 首次启动引导向导

**难度**：🟡 中等 · **技能**：React / TypeScript（少量 Rust 命令调用）

**背景**：新用户第一次打开会看到空白工作区与「模型下载中」，不知道该做什么。Roadmap 里的「首次使用引导向导」可以拆成一个小 PR。

**起点**：`web/src/App.tsx` 路由、`web/src/pages/Settings/index.tsx`、`web/src/api/infer.ts` 里已有的 `get_base_models_status` / `get_workspace`。

**建议方案**：检测「未设置工作区」或「存在未下载的基础模型」时，在主界面顶部显示一条三步引导：检查 Python → 选择工作区 → 下载模型，每步带跳转按钮。

**验收**：全新配置下能看到引导，完成后不再出现；不改变已有用户的使用路径。

---

## 3. 错误提示可操作化

**难度**：🟡 中等 · **技能**：React / TypeScript

**背景**：后端返回的错误常常是原始 Python traceback 或英文异常，用户看不懂也不知道下一步做什么（例如「底模缺失」现在是一句 message）。

**起点**：`web/src/pages/Infer/index.tsx`、`UVR/index.tsx`、`Train/index.tsx` 的 `message.error` 调用；后端错误来源在 `src-tauri/src/commands/`。

**建议方案**：抽一个 `friendlyError(raw): { title, hint, action? }`，把常见错误映射成「一句话原因 + 下一步按钮」（去模型管理下载 / 打开日志 / 检查文件名空格等）。

**验收**：至少覆盖 5 种常见错误；映射之外的情况保留原始信息。

---

## 4. Windows 运行时适配

**难度**：🔴 较难 · **技能**：Rust / Windows

**背景**：构建配置已预留（`src-tauri/tauri.windows.conf.json`、`src-tauri/bin/windows-x64/`、`.github/workflows/release-windows.yml`），平台适配层在 `src-tauri/src/platform/mod.rs`，但运行时还没有跑通。

**待办**：

- `ffmpeg.exe` 的读取路径与打包（`src-tauri/src/decode_audio.rs`、`python.rs`）
- 音频设备枚举与虚拟声卡（`src-tauri/src/audio_engine/blackhole.rs` 目前是 macOS 专用）
- Windows 安装包图标与 NSIS 配置
- `tools/auto_setup_venv.py` 的平台分支

**验收**：Windows 11 上能启动、能完成一次分离 + 转换 + 合成；PR 里附上实际运行截图。

---

## 5. 实时翻唱稳定性

**难度**：🔴 较难 · **技能**：Rust / 实时音频 DSP

**背景**：`/live` 页面属于实验特性，存在爆音、卡顿、启动失败等问题。

**起点**：`src-tauri/src/audio_engine/`（`engine.rs`、`mixer.rs`、`pitch_correction.rs`、`blackhole.rs`）、`web/src/pages/LiveSinging/index.tsx`。

**需要的材料**：一份能稳定复现的步骤、机器型号、采样率设置、应用日志（页面底部「引擎日志」）。

**建议**：先只做「启动失败时报错更明确」或「爆音时记录 underrun 次数」这样的可观测性改进，再动 DSP。

---

## 6. 导入模型的索引体检

**难度**：🟡 中等 · **技能**：Python / Rust

**背景**：`tools/import_model.py` 支持导入 `.pth` / `.zip`，但如果压缩包里带的 `.index` 缺少 IVF 缓存文件（`.vectors.bin` / `.meta.txt` / `.ivf.bin`），检索效果会退化，界面也不会提示。

**起点**：`tools/import_model.py`、`src-tauri/src/commands/infer.rs`（模型列表）、`web/src/pages/Models/index.tsx`。

**建议方案**：导入后检测索引配套文件，缺失时在模型列表里给出「索引不完整，建议重新构建」的提示（是否支持重建需要先调研，写在 Issue 里讨论）。

---

## 7. 音频工具函数的单元测试

**难度**：🟢 新手友好 · **技能**：Rust

**背景**：`src-tauri/src/audio_engine/` 里已有不少纯函数（重采样、平滑、LRC 解析、增益换算），目前没有测试。

**起点**：`src-tauri/src/audio_engine/resampler.rs`、`smoother.rs`、`lrc.rs`；测试放同文件的 `#[cfg(test)] mod tests`。

**验收**：`cargo test --manifest-path src-tauri/Cargo.toml` 通过，并新增 PR 检查里的 `cargo test`。

---

## 8. 文档：英文教程校对与补充截图

**难度**：🟢 新手友好 · **技能**：写作 / 英文

**背景**：`README.md` 面向英文用户，但界面本身是中文，很多按钮名需要对照解释。母语者校对能显著提升可读性。

**起点**：`README.md`、`docs/screenshots/`。

**验收**：不改变事实的前提下，让英文表达更自然；如补充截图，使用 `docs/screenshots/` 下已有命名风格。

---

## 认领方式

1. 在 Issue 里回复「我来做」，简单说下计划
2. 完成后在 PR 里关联 Issue（`Closes #xx`）
3. 如果中途卡住，直接在 Issue 里说，不需要勉强提交
