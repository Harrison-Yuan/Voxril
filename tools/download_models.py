"""
下载项目必需依赖模型文件。
- vec-768-layer-12.onnx: RVC v2 标准 ContentVec 特征提取器 ONNX 版 (768d)
- rmvpe.onnx: F0 音高提取 RMVPE (ONNX)
HDEMUCS 人声分离模型由 torchaudio 自动下载，无需手动管理。
"""

import os
from pathlib import Path
from urllib.request import Request, urlopen

RVC_DOWNLOAD_LINK = "https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/main/"
VEC768_URL = "https://huggingface.co/ozada/onnx_rvc/resolve/main/vec-768-layer-12.onnx"

BASE_DIR = Path(__file__).resolve().parent.parent

MODELS = {
    "contentvec": (VEC768_URL, BASE_DIR / "assets/hubert/vec-768-layer-12.onnx"),
    "rmvpe": (RVC_DOWNLOAD_LINK + "rmvpe.onnx", BASE_DIR / "assets/rmvpe/rmvpe.onnx"),
}


def dl_file(url, dest, desc=""):
    os.makedirs(dest.parent, exist_ok=True)
    if dest.exists():
        print(f"  SKIP  already exists: {desc or dest.name} ({dest.stat().st_size / 1024 / 1024:.0f} MB)")
        return True
    print(f"  DOWNLOAD {desc or dest.name} ...")
    print(f"         {url}")
    req = Request(url, headers={"User-Agent": "Mozilla/5.0"})
    with urlopen(req, timeout=600) as response:
        with open(dest, "wb") as f:
            while True:
                chunk = response.read(65536)
                if not chunk:
                    break
                f.write(chunk)
    size_mb = dest.stat().st_size / 1024 / 1024
    print(f"  DONE   {desc or dest.name}  ({size_mb:.0f} MB)")
    return True


def main():
    print("Downloading required model files...\n")
    for name, (url, dest) in MODELS.items():
        dl_file(url, dest, desc=name)
    print("\nAll required models downloaded.")


if __name__ == "__main__":
    main()
