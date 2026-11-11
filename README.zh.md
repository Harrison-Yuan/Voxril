<div align="center">

[**English**](README.md) | [**中文**](README.zh.md)

</div>

<br>

<div align="center">

# 🎙️ VoiceCraft

**开源 AI 变声 · 训练自己的声音模型 · 实时翻唱 · 100% 本地运行，100% 免费。**

> **ElevenLabs 的开源替代品 — 不上云，不花 API 费用，无隐私担忧。**

<p>
  <a href="https://github.com/Harrison-Yuan/voice-training/releases">
    <img src="https://img.shields.io/badge/📥_下载_for_macOS-8A2BE2?style=for-the-badge&logo=apple&logoColor=white" alt="下载">
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/许可证-AGPL--3.0-ef4444?style=for-the-badge" alt="License">
  </a>
  <a href="https://github.com/Harrison-Yuan/voice-training">
    <img src="https://img.shields.io/badge/⭐_点个_Star-ffd700?style=for-the-badge" alt="Star">
  </a>
</p>

<p>
  <img src="https://img.shields.io/badge/Rust-000?logo=rust&logoColor=fff&style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/Tauri-000?logo=tauri&logoColor=ffc131&style=flat-square" alt="Tauri">
  <img src="https://img.shields.io/badge/PyTorch_MPS-000?logo=pytorch&logoColor=ee4c2c&style=flat-square" alt="PyTorch MPS">
  <img src="https://img.shields.io/badge/React-000?logo=react&logoColor=61dafb&style=flat-square" alt="React">
  <img src="https://img.shields.io/badge/macOS_14+-000?logo=apple&logoColor=fff&style=flat-square" alt="macOS">
  <img src="https://img.shields.io/badge/Apple_Silicon_M1–M4-000?logo=apple&logoColor=fff&style=flat-square" alt="Apple Silicon">
</p>

<!-- 演示 GIF：15 秒录屏 → 打开 APP → 选择模型 → 唱歌 → 实时变声 -->

</div>

---

## 💡 为什么选择 VoiceCraft？

| 每个月给 ElevenLabs 交 $99？ | VoiceCraft 完全免费、开源，运行在你的 Mac 上 |
|---|---|
| 受够了 RVC-WebUI 的 Gradio 界面？ | VoiceCraft 是 **原生桌面应用** — Tauri + Rust，精致体验 |
| 担心语音数据隐私？ | **你的声音数据永远不会离开你的电脑** |
| 想要实时变声？ | 在 Apple Silicon 上 **延迟低于 100ms**，实时翻唱 |
| 只有 10 分钟录音？ | **10 分钟样本就能训练可用模型** |

---

## ✨ 功能

| 🎤 **训练模型** | 🎵 **人声分离** | 🔄 **语音转换** | 🎙️ **实时翻唱** |
|---|---|---|---|
| 用你的音频样本训练声音模型 | BS-RoFormer 引擎分离人声 | 将任意音频转换为目标音色 | 实时 AI 变声 + 伴奏播放 |

- **原生 macOS 应用** — 不是网页套壳，不是 Gradio UI
- **Apple Silicon 原生加速** — M1–M4 PyTorch MPS 优化
- **一键安装** — 下载 DMG → 首次启动自动配置完成
- **完整管线** — 分离 → 训练 → 转换 → 翻唱，一个应用全搞定

---

## 🚀 快速开始

```bash
# 1. 从 Releases 下载 .dmg → 拖入 Applications 文件夹 → 打开

# 2. 首次启动自动完成：
#    - Python 虚拟环境 + 依赖 (~3 分钟)
#    - AI 模型下载：HuBERT, RMVPE, BS-RoFormer (~5 分钟)

# 3. 大功告成。开始训练或转换。
```

**系统要求：** macOS 14.0+ · Apple Silicon (M1/M2/M3/M4) · Python 3.9+ · 5GB 磁盘空间

### 开发者模式

```bash
git clone https://github.com/Harrison-Yuan/voice-training.git
cd voicecraft

python3 -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt
cd web && npm install && cd ..

npm run tauri:dev    # 热重载开发模式
npm run tauri:build  # 构建 .dmg
```

---

## 🧠 技术架构

### 系统概览

```
┌──────────────────────────────────────────────────────────┐
│                     桌面框架 (Tauri 2.0)                   │
│  ┌──────────────┐  ┌──────────────┐  ┌─────────────────┐ │
│  │  React SPA   │◀─▶  Rust 核心  │◀─▶  Python 推理引擎 │ │
│  │  (Vite + TS) │  │  (命令层)   │  │  (PyTorch MPS)  │ │
│  └──────────────┘  └──────────────┘  └─────────────────┘ │
└──────────────────────────────────────────────────────────┘
```

### 前端 (`web/`)

| 层 | 技术 | 用途 |
|---|---|---|
| 框架 | **React 19 + TypeScript** | UI 渲染 |
| 构建 | **Vite 6** | 快速 HMR，优化构建 |
| UI 库 | **Ant Design 6** | 组件系统，暗色模式 |
| 状态管理 | **Zustand** | 轻量级全局状态 |
| 路由 | **React Router 6** | SPA 页面路由 |
| 国际化 | **React Context** | 中英文切换 |

**页面：**

| 路由 | 页面 | 描述 |
|---|---|---|
| `/live` | **AI 直播唱歌** | 实时修音（Phase Vocoder），F0 检测，歌词同步 |
| `/train` | **训练模型** | 多步流程：预处理 → F0/音频提取 → 训练 → 建索引 |
| `/infer` | **语音转换** | 使用训练好的模型 + FAISS 检索进行音频转换 |
| `/uvr` | **人声分离** | 将人声从伴奏中分离 |
| `/models` | **模型管理** | 管理基础依赖（HuBERT、RMVPE、BS-RoFormer）和训练的声音模型 |
| `/settings` | **设置** | 语言、工作目录、外观 |
| `/desktop-lyrics` | **桌面歌词** | 始终置顶的浮动歌词窗口 |

### Rust 后端 (`src-tauri/src/`)

| 模块 | 职责 |
|---|---|
| `commands/` | 各页面的 Tauri 命令处理（live, infer, train, uvr, config, audio, file, history, workspace） |
| `audio_engine/` | 实时音频 DSP：Phase Vocoder 修音、伴奏播放、RVC 推理、LRC 解析 |
| `python.rs` | Python 子进程管理（虚拟环境、脚本执行） |
| `database.rs` | 训练实验的 SQLite 数据库 |
| `state.rs` | 全局应用状态（Mutex 保护） |
| `events.rs` | Tauri 事件发射（指标、日志、进度） |
| `cleanup.rs` | 优雅关闭和资源清理 |

### Python 推理引擎 (`infer/`)

| 子模块 | 技术 | 用途 |
|---|---|---|
| `modules/vc/` | **PyTorch MPS** | 语音转换流水线（PyTorch + ONNX 推理） |
| `modules/train/` | **PyTorch** | 训练：预处理、F0 提取、特征提取、模型训练 |
| `modules/uvr5/` | **MDX-Net + VR** | 人声分离推理 |
| `lib/infer_pack/` | **RVC v2** | 核心合成器（HuBERT 编码器、生成器、F0 预测器） |
| `lib/mlx/` | **Apple MLX** | 实验性 Apple Silicon 推理后端 |
| `modules/realtime_worker.py` | **多进程** | 实时音频流工作进程 |

### 处理流水线

```
                      ┌──────────────────────┐
                      │     人声分离           │
                      │   (BS-RoFormer)       │
                      └──────┬──────┬────────┘
                             │      │
                     ┌───────┘      └───────┐
                     ▼                      ▼
              ┌──────────────┐    ┌──────────────────┐
              │    人声       │    │     伴奏          │
              └──────┬───────┘    └──────────────────┘
                     ▼
        ┌──────────────────────────┐
        │  HuBERT 特征提取          │
        │  + RMVPE 音高提取         │
        └──────┬──────────┬───────┘
               ▼          ▼
        ┌──────────┐  ┌──────────┐
        │  FAISS   │  │  F0 曲线 │
        │  索引     │  │  (FCPE)  │
        └────┬─────┘  └────┬─────┘
             ▼              ▼
        ┌──────────────────────────┐
        │     RVC 合成器            │
        │   (生成模型)             │
        └──────┬───────────────────┘
               ▼
        ┌──────────────────────────┐
        │  Phase Vocoder 实时修音   │
        │  (仅直播模式)           │
        └──────────────────────────┘
```

### AI 直播唱歌模块

直播唱歌模块是一个实时音高修正系统：

1. **F0 检测** — FCPE（0.1ms）实时提取基频，失败自动降级到 RMVPE（3ms）
2. **唱歌检测** — 连续 3 帧（60ms）稳定 F0 自动判定为唱歌状态
3. **谱子查表** — 按伴奏播放进度查找对应帧的目标 F0 音高
4. **Retune 平滑** — 0.15 系数一阶低通滤波，保留自然颤音
5. **Phase Vocoder 移调** — FFT 2048 → 瞬时频率插值 → WOLA 合成

歌词同步使用 LRC 文件，通过 `live-lyrics` 事件实时推送，支持桌面歌词悬浮窗。

---

## 🗺 路线图

- [x] 声音模型训练（预处理 → 特征提取 → 训练 → 索引）
- [x] 人声分离（BS-RoFormer）
- [x] 语音转换（PyTorch MPS）
- [x] 实时翻唱 + 伴奏
- [ ] **发布预训练模型**（下载即玩）
- [ ] **演示视频 & 截图**
- [ ] 企业版（REST API、鉴权、多用户）
- [ ] Windows / Linux 支持

---

## 🤝 加入社区

VoiceCraft 刚刚起步，你的支持就是最大的动力：

- ⭐ **点个 Star** — 让更多人看到 VoiceCraft
- 🐛 **提 Issue** — 报告 Bug、提功能需求
- 🔧 **提交 PR** — 代码贡献同样欢迎
- 📢 **分享出去** — Twitter、Reddit、即刻、知乎

---

## 📄 许可证

**AGPL-3.0** — 个人免费使用。
企业部署需购买商业授权。

---

<div align="center">

**为开源 AI 社区而建 ❤️**

[![Star History](https://api.star-history.com/svg?repos=Harrison-Yuan/voice-training&type=Date)](https://star-history.com/#Harrison-Yuan/voice-training&Date)

### ⭐ 如果 VoiceCraft 对你有帮助，点个 Star。让更多需要它的人找到它。

</div>
