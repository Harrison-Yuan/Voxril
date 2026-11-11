<div align="center">

[**English**](README.md) | [**中文**](README.zh.md)

</div>

<br>

<div align="center">

# 🎙️ Voxril

**Open-source AI voice changer for macOS — train your own voice model, convert vocals offline, and sing in real time with 100% local AI.**

> AI voice conversion · voice cloning · real-time karaoke · vocal separation · RVC-compatible models · Apple Silicon. No cloud, no API keys, no subscriptions — the open-source alternative to ElevenLabs.

<p>
  <a href="https://github.com/Harrison-Yuan/Voxril/releases">
    <img src="https://img.shields.io/badge/📥_Download_for_macOS-8A2BE2?style=for-the-badge&logo=apple&logoColor=white" alt="Download">
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/License-AGPL--3.0-ef4444?style=for-the-badge" alt="License">
  </a>
  <a href="https://github.com/Harrison-Yuan/Voxril">
    <img src="https://img.shields.io/badge/⭐_Star_us-ffd700?style=for-the-badge" alt="Stars">
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

https://github.com/user-attachments/assets/00000000-0000-0000-0000-000000000000

<!-- Demo GIF: 15s screen recording showing: open → select model → sing → real-time AI voice -->

</div>

---

## 💡 Why Voxril?

| Stop paying ElevenLabs $99/mo | Voxril is 100% free, open-source, and runs entirely on your Mac |
|---|---|
| Tired of RVC-WebUI's buggy Gradio UI | Voxril is a **native desktop app** — Tauri + Rust, polished UX |
| Worried about privacy? | Your voice data **never leaves your machine** |
| Want real-time voice changing? | **Sub-100ms latency** on Apple Silicon — sing in real time with AI voice |
| Only have 10 minutes of audio? | Train a usable voice model from **just 10 minutes of samples** |

---

## ✨ Features

| 🎤 **Train** | 🎵 **Separate** | 🔄 **Convert** | 🎙️ **Real-time Karaoke** |
|---|---|---|---|
| Train voice models from your audio samples | Isolate vocals from any song (BS-RoFormer) | Convert any audio into your trained voice | Sing with AI voice + accompaniment, zero latency |

- **Native macOS app** — not a web wrapper, not a Gradio UI
- **Apple Silicon native** — M1–M4 optimized with PyTorch MPS
- **One-click install** — DMG download, first launch auto-configures everything
- **End-to-end pipeline** — separation → training → conversion → karaoke, all in one app

---

## 🚀 Quick Start

```bash
# 1. Download the latest .dmg from Releases → drag to Applications → open

# 2. First launch auto-installs:
#    - Python virtual environment + dependencies (~3 min)
#    - AI models: HuBERT, RMVPE, BS-RoFormer (~5 min)

# 3. That's it. Start training or converting.
```

**System Requirements:** macOS 14.0+ · Apple Silicon (M1/M2/M3/M4) · Python 3.9+ · 5GB free disk

### Development

```bash
git clone https://github.com/Harrison-Yuan/Voxril.git
cd voxril

python3 -m venv .venv && source .venv/bin/activate && pip install -r requirements.txt
cd web && npm install && cd ..

npm run tauri:dev    # hot-reload dev mode
npm run tauri:build  # production .dmg
```

---

## 🧠 Technical Architecture

### System Overview

```
┌─────────────────────────────────────────────────────────┐
│                    Desktop Shell (Tauri 2.0)              │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐ │
│  │  React SPA   │◀─▶  Rust Core  │◀─▶  Python Engine  │ │
│  │  (Vite + TS) │  │  (Commands) │  │  (PyTorch MPS)  │ │
│  └──────────────┘  └──────────────┘  └────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Frontend (`web/`)

| Layer | Technology | Purpose |
|---|---|---|
| Framework | **React 19 + TypeScript** | UI rendering |
| Build | **Vite 6** | Fast HMR, optimized builds |
| UI Library | **Ant Design 6** | Component system, dark mode |
| State | **Zustand** | Lightweight global state |
| Routing | **React Router 6** | SPA page routing |
| i18n | **React Context** | Chinese / English switching |

**Pages:**

| Route | Page | Description |
|---|---|---|
| `/live` | **AI Live Singing** | Real-time pitch correction (Phase Vocoder), F0 detection, lyrics sync |
| `/train` | **Training** | Multi-step pipeline: preprocess → F0/audio extraction → train → index |
| `/infer` | **Voice Conversion** | Convert audio with trained model + FAISS search |
| `/uvr` | **Vocal Separation** | Separate vocals from accompaniment |
| `/models` | **Models** | Manage base deps (HuBERT, RMVPE, BS-RoFormer) and trained voice models |
| `/settings` | **Settings** | Language, workspace paths, appearance |
| `/desktop-lyrics` | **Desktop Lyrics** | Always-on-top floating lyrics window |

### Rust Backend (`src-tauri/src/`)

| Module | Responsibility |
|---|---|
| `commands/` | Tauri command handlers for each page (live, infer, train, uvr, config, audio, file, history, workspace) |
| `audio_engine/` | Real-time audio DSP: Phase Vocoder pitch correction, accompaniment playback, RVC inference, LRC parsing |
| `python.rs` | Python subprocess management (venv, script execution) |
| `database.rs` | SQLite database for training experiments |
| `state.rs` | Global application state (Mutex-guarded) |
| `events.rs` | Tauri event emission (metrics, logs, progress) |
| `cleanup.rs` | Graceful shutdown and resource cleanup |

### Python Inference Engine (`infer/`)

| Submodule | Technology | Purpose |
|---|---|---|
| `modules/vc/` | **PyTorch MPS** | Voice conversion pipeline (pytorch + onnx inference) |
| `modules/train/` | **PyTorch** | Training: preprocess, F0 extraction, feature extraction, model training |
| `modules/uvr5/` | **MDX-Net + VR** | Vocal separation inference |
| `lib/infer_pack/` | **RVC v2** | Core synthesizer (HuBERT encoder, generator, F0 predictors) |
| `lib/mlx/` | **Apple MLX** | Experimental Apple Silicon inference backend |
| `modules/realtime_worker.py` | **Multiprocessing** | Real-time audio streaming worker |

### Processing Pipeline

```
                      ┌─────────────────────┐
                      │   Vocal Separation   │
                      │   (BS-RoFormer)      │
                      └──────┬──────┬───────┘
                             │      │
                     ┌───────┘      └───────┐
                     ▼                      ▼
              ┌──────────────┐    ┌──────────────────┐
              │   Vocals     │    │   Accompaniment   │
              └──────┬───────┘    └──────────────────┘
                     ▼
        ┌──────────────────────────┐
        │  HuBERT Feature Ext.     │
        │  + RMVPE Pitch Ext.      │
        └──────┬──────────┬───────┘
               ▼          ▼
        ┌──────────┐  ┌──────────┐
        │  FAISS   │  │  F0 Curve│
        │  Index   │  │  (FCPE)  │
        └────┬─────┘  └────┬─────┘
             ▼              ▼
        ┌──────────────────────────┐
        │     RVC Synthesizer      │
        │  (Generative Model)      │
        └──────┬───────────────────┘
               ▼
        ┌──────────────────────────┐
        │  Phase Vocoder Pitch     │
        │  Correction (Live only)  │
        └──────────────────────────┘
```

### AI Live Singing Module

The live singing module is a real-time pitch correction system:

1. **F0 Detection** — FCPE (0.1ms) or RMVPE (3ms) fallback for real-time pitch extraction
2. **Singing Detection** — 3 consecutive stable F0 frames (60ms) triggers singing mode
3. **F0 Profile Lookup** — Pre-extracted target pitch curve matched to accompaniment timeline
4. **Retune Smoothing** — 1st-order low-pass filter (coefficient 0.15) for natural vibrato preservation
5. **Phase Vocoder** — FFT 2048 → instantaneous frequency interpolation → WOLA synthesis

Lyrics synchronization uses LRC files with real-time `live-lyrics` events for desktop lyrics overlay.

---

## 🗺 Roadmap

- [x] Voice model training (preprocess → feature extraction → train → index)
- [x] Vocal separation with BS-RoFormer
- [x] Voice conversion with PyTorch MPS
- [x] Real-time karaoke with accompaniment
- [ ] **Pre-trained model release** (download & play instantly)
- [ ] **Demo video & screenshots**
- [ ] Enterprise edition (REST API, auth, multi-user)
- [ ] Windows / Linux support

---

## 🤝 Join the Community

Voxril is just getting started. Here's how you can help:

- ⭐ **Star this repo** — it helps others discover Voxril
- 🐛 **Open issues** — bug reports, feature requests
- 🔧 **Submit PRs** — code contributions welcome
- 📢 **Share it** — Twitter, Reddit, Hacker News, Product Hunt

---

## 📄 License

**AGPL-3.0** — Free for personal use.
Commercial licenses available for enterprise deployment.

---

<div align="center">

**Built with ❤️ for the open-source AI community**

[![Star History](https://api.star-history.com/svg?repos=Harrison-Yuan/Voxril&type=Date)](https://star-history.com/#Harrison-Yuan/Voxril&Date)

### ⭐ If Voxril is useful to you, star the repo. It helps more people find it.

</div>
