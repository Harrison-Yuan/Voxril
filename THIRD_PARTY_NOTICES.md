# 第三方组件与许可声明

Voxril 的自有代码使用 [PolyForm Noncommercial 1.0.0](LICENSE)。
从 RVC 衍生的代码使用 MIT，全文与适用范围见 [LICENSE-RVC](LICENSE-RVC)。
本文件汇总其余第三方代码、模型权重和随包分发的二进制。

---

## 1. 源码衍生部分

| 组件 | 来源 | 许可证 | 在本仓库中的位置 |
|---|---|---|---|
| RVC — Retrieval-based Voice Conversion WebUI | <https://github.com/RVC-Project/Retrieval-based-Voice-Conversion-WebUI> | MIT | `infer/lib/infer_pack/`、`infer/modules/vc/`、`infer/modules/train/`、`infer/modules/onnx/`、`infer/lib/rmvpe.py`、`infer/lib/audio.py` |
| audio-slicer | <https://github.com/openvpi/audio-slicer> | MIT | `infer/lib/slicer2.py` |

audio-slicer 的 MIT 许可全文：

```
MIT License

Copyright (c) 2022 Team OpenVPI

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

---

## 2. 模型权重

模型权重**不随 git 仓库分发**，由应用在首次启动时按需下载。

| 模型 | 下载来源 | 许可证 |
|---|---|---|
| ContentVec / vec-768-layer-12 ONNX | <https://huggingface.co/ozada/onnx_rvc>（备用：`NaruseMioShirakana/MoeSS-SUBModel`） | MIT |
| RMVPE ONNX | <https://huggingface.co/lj1995/VoiceConversionWebUI> | MIT |
| FCPE ONNX | <https://huggingface.co/gzivdo/fcpe-onnx> | MIT |
| RVC v2 预训练 `f0G40k.pth` / `f0D40k.pth` | <https://huggingface.co/lj1995/VoiceConversionWebUI> | MIT |
| BS-RoFormer | <https://github.com/TRvlvr/model_repo> | ⚠️ 该仓库未声明许可证 |
| MDX-Net Main / Karaoke 2 / Voc FT / Kim Vocal 2 | <https://github.com/TRvlvr/model_repo> | ⚠️ 该仓库未声明许可证 |

说明：

- `TRvlvr/model_repo` 没有 license 字段。Voxril 只引导用户在运行时从原仓库下载，不重新分发这些权重。
- 如果你要**镜像、打包或再分发**这些权重，请先向上游确认授权。
- 用户自行训练或导入的 `.pth` / `.index` 由用户自行负责其来源与授权。

---

## 3. 随包分发的二进制

| 二进制 | 版本 | 许可证 | 说明 |
|---|---|---|---|
| FFmpeg（macOS arm64） | `git-2026-06-10-5f998e3` | LGPL v3（当前随包构建） | 源码：<https://github.com/FFmpeg/FFmpeg>；构建参考：<https://github.com/BtbN/FFmpeg-Builds>；许可证全文见 `licenses/LGPL-3.0.txt` 与 `licenses/GPL-3.0.txt` |

FFmpeg 以独立可执行文件随包分发（应用通过子进程调用），未做静态链接或修改。若后续替换 ffmpeg 二进制或改变构建配置（例如启用 GPL 组件），请同步更新本节并重新确认合规义务。

---

## 4. 运行时依赖

以下依赖通过 pip / npm 安装，各自遵循其上游许可证，Voxril 不修改其源码。

| 依赖 | 许可证 |
|---|---|
| PyTorch / torchaudio | BSD-3-Clause / BSD-2-Clause |
| NumPy / SciPy / SoundFile | BSD-3-Clause |
| librosa | ISC |
| faiss | MIT |
| onnxruntime | MIT |
| audio-separator | MIT |
| torchfcpe | MIT |
| HuggingFace Transformers / huggingface_hub | Apache-2.0 / Apache-2.0 |
| Tauri | MIT 或 Apache-2.0 |
| React / React Router / Zustand | MIT |
| Ant Design / @ant-design/icons | MIT |
| Vite | MIT |

其余 Python 依赖见 [`requirements.lock`](requirements.lock)，其余前端依赖见 [`web/package.json`](web/package.json)；每个包的完整许可文本在其安装目录的 `LICENSE` 文件中。

---

## 5. 用户内容

代码许可证不覆盖用户输入和输出。翻唱涉及的音乐、录音、声音形象（voice likeness）等权利由用户自行取得授权并承担责任。
