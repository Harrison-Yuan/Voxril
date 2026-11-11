<div align="center">

[**English**](README.md) | [**中文**](README.zh.md)

</div>

<br>

<div align="center">

# 🎙️ Voxril

**Open-source AI voice changer for macOS — train your own voice model, convert vocals offline, and sing in real time with 100% local AI.**

> AI voice conversion · voice cloning · real-time karaoke · vocal separation · RVC v2 models · Apple Silicon. No cloud, no API keys, no subscriptions — the open-source alternative to ElevenLabs.

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
  <img src="https://img.shields.io/badge/Tauri_2-000?logo=tauri&logoColor=ffc131&style=flat-square" alt="Tauri 2">
  <img src="https://img.shields.io/badge/PyTorch_MPS-000?logo=pytorch&logoColor=ee4c2c&style=flat-square" alt="PyTorch MPS">
  <img src="https://img.shields.io/badge/React_19-000?logo=react&logoColor=61dafb&style=flat-square" alt="React 19">
  <img src="https://img.shields.io/badge/macOS_14+-000?logo=apple&logoColor=fff&style=flat-square" alt="macOS 14+">
  <img src="https://img.shields.io/badge/Apple_Silicon_M1–M4-000?logo=apple&logoColor=fff&style=flat-square" alt="Apple Silicon">
</p>

</div>

---

## 💡 Why Voxril?

| The usual pain | Voxril |
|---|---|
| Cloud voice services charge monthly fees | Free and open source, runs entirely on your Mac |
| RVC-WebUI means a Gradio page in a browser | Native desktop app — Tauri 2 + Rust + React 19 |
| Voice audio uploaded to someone else's server | Audio never leaves the machine; inference is local (PyTorch MPS / ONNX Runtime) |
| Want to sing with a different voice in real time | Live mode: mic → RVC → accompaniment, with LRC lyrics overlay |
| No dataset ready | Built-in RVC v2 pipeline trains from a few minutes of clean vocals |

---

## ✨ Features

| 🎤 Train | 🎵 Separate | 🔄 Convert | 🎙️ Live Singing |
|---|---|---|---|
| preprocess → F0 / feature extraction → train → FAISS index | BS-RoFormer + 4 MDX-Net models | offline conversion with a trained model and retrieval index | mic input converted in real time over accompaniment playback |

- **Native macOS app** — Tauri 2 shell, Rust audio engine, React frontend
- **Local inference** — PyTorch MPS for RVC, ONNX Runtime for HuBERT / RMVPE / FCPE
- **Automatic first-run setup** — creates a Python venv and downloads the ~1 GB base models
- **Desktop lyrics window** — always-on-top transparent overlay driven by LRC + live events
- **Workspace & history** — SQLite keeps experiments/records, workspace directory is configurable
- **End-to-end pipeline** — separation → training → conversion → live singing in one app

---

## 📦 Requirements

| Item | Detail |
|---|---|
| OS | macOS 14 (Sonoma) or newer |
| Chip | Apple Silicon (M1–M4). **Intel Macs are not supported** — the bundled `ffmpeg` is arm64 and MPS requires Apple Silicon |
| Python | 3.10+ installed on the system (Homebrew recommended). The app does **not** bundle a Python runtime |
| Disk | ~6 GB free (Python venv ≈4 GB + base models ≈1 GB) |
| Network | Needed on first launch (PyPI, HuggingFace, UVR model repos) |
| Optional | [BlackHole 2ch](https://existential.audio/blackhole/) if you want to route Voxril's output into a DAW or streaming software |

---

## 🚀 Install

1. Download the latest `.dmg` from [Releases](https://github.com/Harrison-Yuan/Voxril/releases) and drag Voxril into **Applications**.
2. The build is **not notarized by Apple**. If macOS blocks it, run:
   ```bash
   xattr -dr com.apple.quarantine /Applications/Voxril.app
   ```
3. First launch configures everything by itself:
   - creates `~/Library/Application Support/com.rvc.webui/venv` and installs `requirements.lock`
   - downloads `vec-768-layer-12` (HuBERT), RMVPE, FCPE, RVC v2 pretrained `f0G40k` / `f0D40k`, BS-RoFormer and the MDX-Net models
   - progress is streamed into the app — no terminal work required
4. Choose a workspace folder, then go **Models → Train → Convert / Live**.

---

## 🛠 Development

Prerequisites: Rust toolchain, Node 20+, Python 3.10+.

```bash
git clone https://github.com/Harrison-Yuan/Voxril.git
cd Voxril

# Python side — `npm run build` runs tools/verify_build_deps.py --quick against this venv
python3 -m venv .venv && source .venv/bin/activate
pip install -r requirements.txt

# Frontend dependencies
npm --prefix web ci

# Tauri CLI
npm install -g @tauri-apps/cli@^2   # or: cargo install tauri-cli --version "^2"

npm run dev    # cd src-tauri && cargo tauri dev
npm run build  # bumps the patch version, then builds the .dmg
```

### Releasing

Push a `v*` tag. [release.yml](.github/workflows/release.yml) builds an `aarch64-apple-darwin` DMG on `macos-latest`, takes the version from the tag, skips the local `beforeBuildCommand` (CI has no `.venv`) and attaches the DMG to a GitHub Release.

---

## 🧠 Technical Architecture

```
┌─────────────────────────────────────────────────────────┐
│                   Desktop Shell (Tauri 2)                │
│  ┌──────────────┐  ┌──────────────┐  ┌────────────────┐ │
│  │  React SPA   │◀─▶  Rust Core   │◀─▶  Python Engine  │ │
│  │  (Vite + TS) │  │  (Commands)  │  │  (PyTorch MPS)  │ │
│  └──────────────┘  └──────────────┘  └────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

### Frontend (`web/`)

| Layer | Technology | Purpose |
|---|---|---|
| Framework | **React 19 + TypeScript 6** | UI rendering |
| Build | **Vite 8** | Dev server and production build |
| UI Library | **Ant Design 6** | Component system, dark mode |
| State | **Zustand 5** | Lightweight global state |
| Routing | **React Router 7** | SPA routing |
| i18n | **React Context** | Chinese / English switching |

**Pages:**

| Route | Page | Description |
|---|---|---|
| `/live` | **AI Live Singing** | Real-time conversion, pitch tracking, accompaniment, lyrics |
| `/train` | **Training** | Multi-step pipeline: preprocess → F0 / feature extraction → train → index |
| `/infer` | **Voice Conversion** | Offline conversion with a trained model + FAISS retrieval |
| `/uvr` | **Vocal Separation** | Extract vocals / accompaniment |
| `/models` | **Models** | Base models (HuBERT, RMVPE, FCPE, BS-RoFormer, MDX) and trained voice models |
| `/settings` | **Settings** | Language, workspace paths, appearance |
| `/desktop-lyrics` | **Desktop Lyrics** | Always-on-top floating lyrics window |

### Rust backend (`src-tauri/src/`)

| Module | Responsibility |
|---|---|
| `commands/` | Tauri command handlers per page (live, infer, train, uvr, config, audio, file, history, workspace, workflow) |
| `audio_engine/` | Real-time DSP: RVC inference, phase-vocoder pitch correction, accompaniment playback, mixer, BlackHole routing, auto-tune, LRC parsing |
| `python.rs` | Python discovery, venv handling, streamed subprocess execution |
| `database.rs` | SQLite storage for experiments and history |
| `state.rs` | Global application state |
| `events.rs` | Tauri event emission (metrics, logs, progress) |
| `process_killer.rs` / `cleanup.rs` | Child-process tracking and graceful shutdown |

### Python engine (`infer/`)

| Submodule | Technology | Purpose |
|---|---|---|
| `modules/vc/` | **PyTorch MPS** | Voice conversion pipeline (PyTorch + ONNX paths) |
| `modules/train/` | **PyTorch** | Preprocess, F0 extraction, feature extraction, training |
| `modules/vocal_sep_mps.py` | **audio-separator** | Vocal separation (BS-RoFormer / MDX-Net, MPS aware) |
| `lib/infer_pack/` | **RVC v2** | Synthesizer core (encoder, generator, F0 predictors) |
| `lib/hubert_mps.py` | **ONNX / MPS** | HuBERT feature extraction |

### Processing pipeline

```
                      ┌─────────────────────┐
                      │   Vocal Separation  │
                      │   (BS-RoFormer)     │
                      └──────┬──────┬───────┘
                             │      │
                     ┌───────┘      └───────┐
                     ▼                      ▼
              ┌──────────────┐    ┌──────────────────┐
              │   Vocals     │    │   Accompaniment  │
              └──────┬───────┘    └──────────────────┘
                     ▼
        ┌──────────────────────────┐
        │  HuBERT features         │
        │  + RMVPE / FCPE pitch    │
        └──────┬──────────┬───────┘
               ▼          ▼
        ┌──────────┐  ┌──────────┐
        │  FAISS   │  │  F0 curve│
        │  index   │  │  (FCPE)  │
        └────┬─────┘  └────┬─────┘
             ▼              ▼
        ┌──────────────────────────┐
        │     RVC synthesizer      │
        └──────┬───────────────────┘
               ▼
        ┌──────────────────────────┐
        │  Phase-vocoder retune    │
        │  (live mode only)        │
        └──────────────────────────┘
```

### Live singing module

1. **F0 detection** — FCPE (ONNX) for low-latency pitch tracking, with RMVPE as fallback
2. **Singing gate** — only sustained, stable F0 frames switch into "singing" state
3. **Target pitch lookup** — a pre-extracted F0 curve aligned to the accompaniment timeline
4. **Retune smoothing** — smoothing keeps vibrato from being flattened
5. **Phase vocoder** — FFT 2048 / hop 512 at 48 kHz (≈10.7 ms frame latency), instantaneous-frequency interpolation and WOLA synthesis

Lyrics sync reads LRC files and pushes `live-lyrics` events to the always-on-top lyrics window.

---

## ⚠️ Known limitations

- **Apple Silicon only** — no Windows / Linux build yet
- The DMG is **unsigned and not notarized**, so Gatekeeper warns on first open
- Base models are **not committed to git** (size / licensing); they download on first launch
- Routing audio into other apps requires a virtual audio device (BlackHole 2ch on macOS)
- First-run setup needs network access and an existing Python 3.10+ install

---

## 🗺 Roadmap

- [x] Voice model training (preprocess → feature extraction → train → index)
- [x] Vocal separation (BS-RoFormer + MDX-Net)
- [x] Offline voice conversion (PyTorch MPS + ONNX)
- [x] Real-time singing with accompaniment
- [x] Desktop lyrics overlay
- [ ] Bundle a Python runtime so users don't need to install Python
- [ ] Pre-trained model showcase (download & sing)
- [ ] Demo video & screenshots
- [ ] Windows / Linux support

---

## 🤝 Contributing

- ⭐ Star the repo if it helps you
- 🐛 Open issues for bugs and feature requests
- 🔧 Pull requests are welcome — build instructions are above
- 📢 Share it with people who want a local, free alternative to cloud voice tools

---

## 🙏 Acknowledgements

- **RVC — Retrieval-based Voice Conversion WebUI** (MIT): `infer/lib/infer_pack`, UVR5 separation code
- **UVR / BS-RoFormer** model repositories
- **HuBERT / ContentVec**, **RMVPE**, **FCPE** model authors

Upstream license notices are collected in [`MIT协议暨相关引用库协议`](MIT协议暨相关引用库协议).

---

## 📄 License

**AGPL-3.0** — see [LICENSE](LICENSE). RVC-derived parts keep their original MIT terms.

---

<div align="center">

**Built with ❤️ for the open-source AI community**

[![Star History](https://api.star-history.com/svg?repos=Harrison-Yuan/Voxril&type=Date)](https://star-history.com/#Harrison-Yuan/Voxril&Date)

### ⭐ If Voxril is useful to you, star the repo. It helps more people find it.

</div>
