<div align="center">

[**English**](README.md) | [**中文**](README.zh.md)

</div>

<br>

<div align="center">

# 🎙️ Voxril

**开源的 macOS AI 变声器 —— 训练专属音色模型、离线人声转换、实时 AI 翻唱，全程本地运行。**

> AI 变声 · 声音克隆 · 实时翻唱 · 人声分离 · 兼容 RVC v2 模型 · Apple Silicon 优化。不上云、不花 API 费用、无需订阅 —— ElevenLabs 的开源替代方案。

<p>
  <a href="https://github.com/Harrison-Yuan/Voxril/releases">
    <img src="https://img.shields.io/badge/📥_下载_for_macOS-8A2BE2?style=for-the-badge&logo=apple&logoColor=white" alt="下载">
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/许可证-AGPL--3.0-ef4444?style=for-the-badge" alt="License">
  </a>
  <a href="https://github.com/Harrison-Yuan/Voxril">
    <img src="https://img.shields.io/badge/⭐_点个_Star-ffd700?style=for-the-badge" alt="Star">
  </a>
</p>

<p>
  <img src="https://img.shields.io/badge/Rust-000?logo=rust&logoColor=fff&style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/Tauri_2-000?logo=tauri&logoColor=ffc131&style=flat-square" alt="Tauri 2">
  <img src="https://img.shields.io/badge/PyTorch_MPS-000?logo=pytorch&logoColor=ee4c2c&style=flat-square" alt="PyTorch MPS">
  <img src="https://img.shields.io/badge/React_19-000?logo=react&logoColor=61dafb&style=flat-square" alt="React 19">
  <img src="https://img.shields.io/badge/macOS_14+-000?logo=apple&logoColor=fff&style=flat-square" alt="macOS 14+">
  <img src="https://img.shields.io/badge/Apple_Silicon_M1–M4-000?logo=apple&logoColor=fff&style=flat-square" alt="Apple Silicon">
</p>

</div>

---

## 💡 为什么选择 Voxril？

| 常见痛点 | Voxril |
|---|---|
| 云端变声服务按月收费 | 完全免费开源，整条链路跑在本地 Mac 上 |
| RVC-WebUI 只能在浏览器里开 Gradio 页面 | 原生桌面应用 —— Tauri 2 + Rust + React 19 |
| 声音要上传到别人的服务器 | 音频不出本机，推理全部在本地完成（PyTorch MPS / ONNX Runtime） |
| 想用别人的声音实时唱歌 | 直播模式：麦克风 → RVC → 伴奏，叠加 LRC 桌面歌词 |
| 没有现成数据集 | 内置 RVC v2 流程，几分钟干净人声就能训练一个音色 |

---

## ✨ 功能

| 🎤 训练 | 🎵 分离 | 🔄 转换 | 🎙️ 实时翻唱 |
|---|---|---|---|
| 预处理 → F0 / 特征提取 → 训练 → FAISS 索引 | BS-RoFormer + 4 个 MDX-Net 模型 | 用训练好的模型 + 检索索引做离线转换 | 麦克风输入实时变声，叠加伴奏播放 |

- **原生 macOS 应用** —— Tauri 2 外壳、Rust 音频引擎、React 前端
- **本地推理** —— RVC 走 PyTorch MPS，HuBERT / RMVPE / FCPE 走 ONNX Runtime
- **首次启动自动配置** —— 自动创建 Python 虚拟环境并下载约 1 GB 基础模型
- **桌面歌词窗口** —— 置顶透明悬浮窗，由 LRC 与实时事件驱动
- **工作区与历史** —— SQLite 保存实验与记录，工作区目录可切换
- **端到端闭环** —— 人声分离 → 训练 → 转换 → 实时翻唱，一个应用内完成

---

## � 运行要求

| 项目 | 说明 |
|---|---|
| 系统 | macOS 14 (Sonoma) 及以上 |
| 芯片 | Apple Silicon（M1–M4）。**不支持 Intel Mac** —— 随包 ffmpeg 为 arm64，且 MPS 依赖 Apple Silicon |
| Python | 系统内需自行安装 3.10+（推荐 Homebrew）。应用**不内置** Python 运行时 |
| 磁盘 | 约 6 GB 可用空间（Python 虚拟环境约 4 GB + 基础模型约 1 GB） |
| 网络 | 首次启动需要联网（PyPI、HuggingFace、UVR 模型仓库） |
| 可选 | [BlackHole 2ch](https://existential.audio/blackhole/)，用于把输出路由到 DAW / 直播软件 |

---

## 🚀 安装

1. 到 [Releases](https://github.com/Harrison-Yuan/Voxril/releases) 下载最新 `.dmg`，把 Voxril 拖入「应用程序」。
2. 安装包**未做 Apple 公证**。如果被系统拦截，执行：
   ```bash
   xattr -dr com.apple.quarantine /Applications/Voxril.app
   ```
3. 首次启动会自动完成配置：
   - 创建 `~/Library/Application Support/com.rvc.webui/venv` 并安装 `requirements.lock`
   - 下载 `vec-768-layer-12`（HuBERT）、RMVPE、FCPE、RVC v2 预训练底模 `f0G40k` / `f0D40k`、BS-RoFormer 与 MDX-Net 系列模型
   - 进度实时显示在应用内，无需命令行操作
4. 选择工作区目录，然后按 **模型 → 训练 → 转换 / 直播** 的流程使用。

---

## 🛠 开发

前置条件：Rust 工具链、Node 20+、pnpm 10+、Python 3.10+。

```bash
git clone https://github.com/Harrison-Yuan/Voxril.git
cd Voxril

# Python 侧 —— `pnpm build` 会用这个 .venv 跑 tools/verify_build_deps.py --quick
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

# 前端依赖（pnpm workspace 已包含 web/）
pnpm install

# Tauri CLI
cargo install tauri-cli --version "^2"   # 或：npm install -g @tauri-apps/cli@^2

pnpm dev    # 启动 Vite (1420) + Tauri 窗口
pnpm build  # 先递增补丁版本号，再打包 .dmg
```

### 发版

推一个 `v*` tag 即可。[release.yml](.github/workflows/release.yml) 会在 `macos-latest` 上构建 `aarch64-apple-darwin` 的 DMG，版本号取自 tag，跳过本地的 `beforeBuildCommand`（CI 里没有 `.venv`），并把 DMG 附加到 GitHub Release。

---

## 🧠 技术架构

```
┌─────────────────────────────────────────────────────────┐
│                  桌面外壳（Tauri 2）                      │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐ │
│  │  React SPA   │◀─▶  Rust 核心   │◀─▶  Python 引擎   │ │
│  │  (Vite + TS) │  │   (Commands) │  │  (PyTorch MPS) │ │
│  └──────────────┘  └──────────────┘  └────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### 前端（`web/`）

| 层次 | 技术 | 用途 |
|---|---|---|
| 框架 | **React 19 + TypeScript 6** | 界面渲染 |
| 构建 | **Vite 8** | 开发服务器与生产构建 |
| 组件库 | **Ant Design 6** | 组件体系、暗色主题 |
| 状态 | **Zustand 5** | 轻量全局状态 |
| 路由 | **React Router 7** | 单页路由 |
| 多语言 | **React Context** | 中 / 英文切换 |

**页面：**

| 路由 | 页面 | 说明 |
|---|---|---|
| `/live` | **AI 实时翻唱** | 实时变声、音高跟踪、伴奏播放、歌词 |
| `/train` | **模型训练** | 多步流水线：预处理 → F0 / 特征提取 → 训练 → 索引 |
| `/infer` | **语音转换** | 用训练好的模型 + FAISS 检索做离线转换 |
| `/uvr` | **人声分离** | 提取人声 / 伴奏 |
| `/models` | **模型管理** | 基础模型（HuBERT、RMVPE、FCPE、BS-RoFormer、MDX）与自训练音色模型 |
| `/settings` | **设置** | 语言、工作区路径、外观 |
| `/desktop-lyrics` | **桌面歌词** | 置顶悬浮歌词窗口 |

### Rust 后端（`src-tauri/src/`）

| 模块 | 职责 |
|---|---|
| `commands/` | 各页面的 Tauri 命令（live、infer、train、uvr、config、audio、file、history、workspace、workflow） |
| `audio_engine/` | 实时音频 DSP：RVC 推理、相位声码器修音、伴奏播放、混音、BlackHole 路由、自动修音、LRC 解析 |
| `python.rs` | Python 探测、venv 管理、子进程流式执行 |
| `database.rs` | SQLite 存储实验与历史 |
| `state.rs` | 全局应用状态 |
| `events.rs` | Tauri 事件推送（指标、日志、进度） |
| `process_killer.rs` / `cleanup.rs` | 子进程跟踪与退出清理 |

### Python 引擎（`infer/`）

| 子模块 | 技术 | 用途 |
|---|---|---|
| `modules/vc/` | **PyTorch MPS** | 语音转换流水线（PyTorch + ONNX 两条路径） |
| `modules/train/` | **PyTorch** | 预处理、F0 提取、特征提取、训练 |
| `modules/vocal_sep_mps.py` | **audio-separator** | 人声分离（BS-RoFormer / MDX-Net，适配 MPS） |
| `lib/infer_pack/` | **RVC v2** | 合成器核心（编码器、生成器、F0 预测器） |
| `lib/hubert_mps.py` | **ONNX / MPS** | HuBERT 特征提取 |

### 处理流程

```
                      ┌─────────────────────┐
                      │   人声分离           │
                      │   (BS-RoFormer)     │
                      └──────┬──────┬───────┘
                             │      │
                     ┌───────┘      └───────┐
                     ▼                      ▼
              ┌──────────────┐    ┌──────────────────┐
              │    人声       │    │      伴奏        │
              └──────┬───────┘    └──────────────────┘
                     ▼
        ┌──────────────────────────┐
        │  HuBERT 特征              │
        │  + RMVPE / FCPE 音高      │
        └──────┬──────────┬───────┘
               ▼          ▼
        ┌──────────┐  ┌──────────┐
        │  FAISS   │  │  F0 曲线  │
        │  索引     │  │  (FCPE)  │
        └────┬─────┘  └────┬─────┘
             ▼              ▼
        ┌──────────────────────────┐
        │     RVC 合成器            │
        └──────┬───────────────────┘
               ▼
        ┌──────────────────────────┐
        │  相位声码器修音            │
        │  （仅实时模式）            │
        └──────────────────────────┘
```

### 实时翻唱模块

1. **F0 检测** —— FCPE（ONNX）负责低延迟基频跟踪，RMVPE 作为兜底
2. **唱歌判定** —— 只有持续稳定的 F0 帧才切入「唱歌」状态
3. **目标音高查表** —— 预先提取的 F0 曲线与伴奏时间轴对齐
4. **修音平滑** —— 平滑处理避免把自然颤音抹平
5. **相位声码器** —— 48 kHz 下 FFT 2048 / hop 512（帧延迟约 10.7 ms），瞬时频率插值 + WOLA 合成

歌词同步读取 LRC 文件，通过 `live-lyrics` 事件推送到置顶歌词窗口。

---

## ⚠️ 已知限制

- **仅支持 Apple Silicon**，暂无 Windows / Linux 版本
- DMG **未签名、未公证**，首次打开会被 Gatekeeper 拦截
- 基础模型**不随仓库分发**（体积与授权原因），首次启动时下载
- 把声音送入其他应用需要虚拟声卡（macOS 上是 BlackHole 2ch）
- 首次配置需要联网，且系统里要已有 Python 3.10+

---

## 🗺 路线图

- [x] 声音模型训练（预处理 → 特征提取 → 训练 → 索引）
- [x] 人声分离（BS-RoFormer + MDX-Net）
- [x] 离线语音转换（PyTorch MPS + ONNX）
- [x] 实时翻唱 + 伴奏
- [x] 桌面歌词悬浮窗
- [ ] 内置 Python 运行时（用户无需自行安装 Python）
- [ ] 预训练音色展示（下载即玩）
- [ ] 演示视频与截图
- [ ] Windows / Linux 支持

---

## 🤝 参与贡献

- ⭐ 觉得有用就点个 Star
- 🐛 提 Issue 报告 Bug 或功能需求
- 🔧 欢迎 PR —— 构建方式见上文
- 📢 分享给需要「本地免费变声」的朋友

---

## 🙏 致谢

- **RVC — Retrieval-based Voice Conversion WebUI**（MIT）：`infer/lib/infer_pack`、UVR5 分离代码
- **UVR / BS-RoFormer** 模型仓库
- **HuBERT / ContentVec**、**RMVPE**、**FCPE** 模型作者

上游许可声明汇总在 [`MIT协议暨相关引用库协议`](MIT协议暨相关引用库协议)。

---

## 📄 许可证

**AGPL-3.0** —— 详见 [LICENSE](LICENSE)。源自 RVC 的部分仍遵循其原始 MIT 许可。

---

<div align="center">

**为开源 AI 社区而建 ❤️**

[![Star History](https://api.star-history.com/svg?repos=Harrison-Yuan/Voxril&type=Date)](https://star-history.com/#Harrison-Yuan/Voxril&Date)

### ⭐ 如果 Voxril 对你有帮助，点个 Star。让更多需要它的人找到它。

</div>
