"""
检查所有必需模型文件状态，支持单模型下载、状态查询。
每行以 [MODEL] 标记输出，便于前端通过 Tauri 事件流捕获。
"""
import os
import sys
import json
import argparse
from pathlib import Path
from urllib.request import Request, urlopen

# Parse --data-dir CLI argument first (overrides RVC_DATA_DIR env)
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--data-dir")
_known, _rest = _parser.parse_known_args()
if _known.data_dir:
    os.environ["RVC_DATA_DIR"] = _known.data_dir

# Use RVC_DATA_DIR if set (from Rust), otherwise fall back to project root
_env_data = os.environ.get("RVC_DATA_DIR")
if _env_data:
    ASSETS_DIR = Path(_env_data) / "assets"
else:
    ASSETS_DIR = Path(__file__).resolve().parent.parent / "assets"

RVC_HF = "https://huggingface.co/lj1995/VoiceConversionWebUI/resolve/main"
UVR_REPO = "https://github.com/TRvlvr/model_repo/releases/download/all_public_uvr_models"
PRETRAINED_DIR = ASSETS_DIR / "pretrained_v2"

MODELS = {
    "hubert": {
        # vec-768-layer-12.onnx: RVC v2 标准特征提取模型，输出 768 维
        "path": ASSETS_DIR / "hubert" / "vec-768-layer-12.onnx",
        "url": "https://huggingface.co/ozada/onnx_rvc/resolve/main/vec-768-layer-12.onnx",
        # 备用 URL 列表：主 URL 失败时按顺序尝试
        "fallback_urls": [
            "https://huggingface.co/NaruseMioShirakana/MoeSS-SUBModel/resolve/main/vec-768-layer-12.onnx",
        ],
        "min_mb": 200,
        "name": "vec-768-layer-12 (特征提取 ONNX)",
    },
    # 注意：contentvec-f.onnx 已弃用，项目统一使用 vec-768-layer-12.onnx
    "rmvpe": {
        "path": ASSETS_DIR / "rmvpe" / "rmvpe.onnx",
        "url": f"{RVC_HF}/rmvpe.onnx",
        "min_mb": 80,
        "name": "RMVPE (F0 音高提取 ONNX)",
    },
    "fcpe": {
        "path": ASSETS_DIR / "fcpe" / "fcpe.onnx",
        "url": "https://huggingface.co/gzivdo/fcpe-onnx/resolve/main/fcpe.onnx",
        "min_mb": 35,
        "name": "FCPE (F0 音高提取 ONNX，直播实时)",
    },
    "f0G40k": {
        "path": PRETRAINED_DIR / "f0G40k.pth",
        "url": f"{RVC_HF}/pretrained_v2/f0G40k.pth",
        "min_mb": 60,
        "name": "RVC v2 Generator f0G40k (预训练)",
    },
    "f0D40k": {
        "path": PRETRAINED_DIR / "f0D40k.pth",
        "url": f"{RVC_HF}/pretrained_v2/f0D40k.pth",
        "min_mb": 100,
        "name": "RVC v2 Discriminator f0D40k (预训练)",
    },
}

UVR_MODEL_DIR = ASSETS_DIR / "uvr_models"
UVR_MODELS_DEF = {
    "bs_roformer": {
        "filename": "model_bs_roformer_ep_317_sdr_12.9755.ckpt",
        "url": f"{UVR_REPO}/model_bs_roformer_ep_317_sdr_12.9755.ckpt",
        "min_mb": 400,
        "name": "BS-RoFormer (人声/伴奏分离)",
    },
    "inst_main": {
        "filename": "UVR_MDXNET_Main.onnx",
        "url": f"{UVR_REPO}/UVR_MDXNET_Main.onnx",
        "min_mb": 40,
        "name": "MDX-Net Main Vocals (主唱提取)",
    },
    "karaoke": {
        "filename": "UVR-MDX-NET-KARA_2.onnx",
        "url": f"{UVR_REPO}/UVR-MDX-NET-KARA_2.onnx",
        "min_mb": 40,
        "name": "MDX-Net Karaoke 2 (去和声)",
    },
    "voc_ft": {
        "filename": "UVR-MDX-NET-Voc_FT.onnx",
        "url": f"{UVR_REPO}/UVR-MDX-NET-Voc_FT.onnx",
        "min_mb": 40,
        "name": "MDX-Net Voc FT (经典提取)",
    },
    "kim_vocal": {
        "filename": "Kim_Vocal_2.onnx",
        "url": f"{UVR_REPO}/Kim_Vocal_2.onnx",
        "min_mb": 40,
        "name": "Kim Vocal 2 (清晰人声)",
    },
}


def _is_html_content(data):
    head = data[:512].decode("utf-8", errors="ignore").strip().lower()
    return head.startswith("<!doctype") or head.startswith("<html") or head.startswith("<!")


def dl_file(url, dest, min_mb=0, fallback_urls=None):
    """下载文件，失败时自动尝试 fallback_urls 中的备用地址。"""
    all_urls = [url] + (fallback_urls or [])
    last_err = None
    for attempt_url in all_urls:
        try:
            _dl_file_once(attempt_url, dest, min_mb)
            return  # 成功则直接返回
        except Exception as e:
            last_err = e
            print(f"[MODEL] 下载失败 ({attempt_url}): {e}", flush=True)
            if attempt_url != all_urls[-1]:
                print(f"[MODEL] 尝试备用地址...", flush=True)
    raise RuntimeError(f"所有下载地址均失败: {last_err}")


def _dl_file_once(url, dest, min_mb=0):
    os.makedirs(dest.parent, exist_ok=True)
    tmp = dest.with_suffix(dest.suffix + ".tmp")
    print(f"[MODEL] 下载中 {dest.name} 从 {url} ...", flush=True)
    req = Request(url, headers={"User-Agent": "Mozilla/5.0"})
    try:
        with urlopen(req, timeout=300) as response:
            # 检查 HTTP 状态码
            status = response.status
            if status >= 400:
                raise RuntimeError(f"HTTP {status}: {response.reason}")

            # 检查 Content-Type，拒绝非二进制内容
            content_type = response.headers.get("Content-Type", "")
            if "text/" in content_type or "html" in content_type:
                raise RuntimeError(f"服务器返回了无效 Content-Type: {content_type}")

            total = int(response.headers.get("Content-Length", 0))
            downloaded = 0
            first_chunk = True
            with open(tmp, "wb") as f:
                while True:
                    chunk = response.read(65536)
                    if not chunk:
                        break
                    if first_chunk and _is_html_content(chunk):
                        f.close()
                        tmp.unlink(missing_ok=True)
                        raise RuntimeError("下载内容为 HTML 页面，不是有效的模型文件（可能是错误页面）")
                    first_chunk = False
                    f.write(chunk)
                    downloaded += len(chunk)
                    if total > 0:
                        pct = int(downloaded / total * 100)
                        mb = downloaded / (1024 * 1024)
                        print(f"[MODEL] 下载中 {dest.name} ... {pct}% ({mb:.0f}MB)", flush=True)

        # 最终校验
        file_mb = tmp.stat().st_size / (1024 * 1024)
        if min_mb > 0 and file_mb < min_mb:
            tmp.unlink(missing_ok=True)
            raise RuntimeError(f"文件不完整 ({file_mb:.0f}MB < {min_mb}MB)")
        if file_mb == 0:
            tmp.unlink(missing_ok=True)
            raise RuntimeError("下载文件为空")

        tmp.replace(dest)
        print(f"[MODEL] {dest.name} 下载完成 ({file_mb:.0f}MB)", flush=True)
    except Exception:
        tmp.unlink(missing_ok=True)
        raise


def download_uvr_model(model_key="bs_roformer"):
    model_def = UVR_MODELS_DEF[model_key]
    filename = model_def["filename"]
    uvr_path = UVR_MODEL_DIR / filename
    min_mb = model_def.get("min_mb", 0)
    os.makedirs(UVR_MODEL_DIR, exist_ok=True)

    url = model_def.get("url")
    if url:
        dl_file(url, uvr_path, min_mb=min_mb)
    else:
        print(f"[MODEL] 下载中 {filename} ...", flush=True)
        from audio_separator.separator import Separator
        separator = Separator(log_level=20, model_file_dir=str(UVR_MODEL_DIR))
        separator.load_model(filename)
        mb = uvr_path.stat().st_size / (1024 * 1024) if uvr_path.exists() else 0
        print(f"[MODEL] {filename} 下载完成 ({mb:.0f}MB)", flush=True)


def check_uvr_model(model_key="bs_roformer"):
    model_def = UVR_MODELS_DEF[model_key]
    filename = model_def["filename"]
    min_mb = model_def.get("min_mb", 0)
    uvr_path = UVR_MODEL_DIR / filename
    need_download = False
    if uvr_path.exists():
        mb = uvr_path.stat().st_size / (1024 * 1024)
        if mb < min_mb:
            print(f"[MODEL] {filename} 文件不完整 ({mb:.0f}MB < {min_mb}MB)，重新下载...", flush=True)
            need_download = True
        else:
            print(f"[MODEL] {filename} 模型已存在 ({mb:.0f}MB)", flush=True)
    else:
        print(f"[MODEL] {filename} 模型未找到，需要下载 (约{min_mb}MB)...", flush=True)
        need_download = True
    if need_download:
        download_uvr_model(model_key)


def get_status():
    status = []
    for key, info in MODELS.items():
        path = info["path"]
        exists = path.exists()
        size_mb = path.stat().st_size / (1024 * 1024) if exists else 0
        status.append({"id": key, "name": info["name"], "exists": exists, "size_mb": size_mb})
    for key, info in UVR_MODELS_DEF.items():
        model_path = UVR_MODEL_DIR / info["filename"]
        exists = model_path.exists()
        size_mb = model_path.stat().st_size / (1024 * 1024) if exists else 0
        min_mb = info.get("min_mb", 0)
        if exists and size_mb < min_mb:
            exists = False
        status.append({"id": key, "name": info["name"], "exists": exists, "size_mb": size_mb})
    print(json.dumps(status), flush=True)


def download_specific(model_id):
    if model_id in MODELS:
        info = MODELS[model_id]
        dl_file(info["url"], info["path"],
                min_mb=info.get("min_mb", 0),
                fallback_urls=info.get("fallback_urls"))
    elif model_id in UVR_MODELS_DEF:
        download_uvr_model(model_id)
    else:
        print(f"Unknown model id: {model_id}", flush=True)
        sys.exit(1)


def clean_stale():
    """清理残留的 .tmp 文件和过小/损坏的模型文件。"""
    cleaned = 0
    # 清理 .tmp 残留
    dirs_to_scan = set()
    for info in MODELS.values():
        dirs_to_scan.add(info["path"].parent)
    dirs_to_scan.add(UVR_MODEL_DIR)

    for d in dirs_to_scan:
        if not d.exists():
            continue
        for f in d.iterdir():
            if f.suffix == ".tmp" or f.name.endswith(".tmp"):
                f.unlink(missing_ok=True)
                print(f"[MODEL] 清理中断残留: {f.name}", flush=True)
                cleaned += 1

    # 清理小于 min_mb 的模型文件（不完整下载）
    for info in MODELS.values():
        path = info["path"]
        min_mb = info.get("min_mb", 0)
        if path.exists() and min_mb > 0:
            mb = path.stat().st_size / (1024 * 1024)
            if mb < min_mb:
                path.unlink(missing_ok=True)
                print(f"[MODEL] 清理不完整模型: {path.name} ({mb:.0f}MB < {min_mb}MB)", flush=True)
                cleaned += 1

    for info in UVR_MODELS_DEF.values():
        model_path = UVR_MODEL_DIR / info["filename"]
        min_mb = info.get("min_mb", 0)
        if model_path.exists() and min_mb > 0:
            mb = model_path.stat().st_size / (1024 * 1024)
            if mb < min_mb:
                model_path.unlink(missing_ok=True)
                print(f"[MODEL] 清理不完整模型: {info['filename']} ({mb:.0f}MB < {min_mb}MB)", flush=True)
                cleaned += 1

    # 清理内容为 HTML 的模型文件（下载到错误页面）
    for info in MODELS.values():
        path = info["path"]
        if path.exists() and path.stat().st_size < 1024 * 1024:
            try:
                head = path.read_bytes()[:512]
                if _is_html_content(head):
                    path.unlink(missing_ok=True)
                    print(f"[MODEL] 清理损坏模型(HTML): {path.name}", flush=True)
                    cleaned += 1
            except Exception:
                pass

    for info in UVR_MODELS_DEF.values():
        model_path = UVR_MODEL_DIR / info["filename"]
        if model_path.exists() and model_path.stat().st_size < 1024 * 1024:
            try:
                head = model_path.read_bytes()[:512]
                if _is_html_content(head):
                    model_path.unlink(missing_ok=True)
                    print(f"[MODEL] 清理损坏模型(HTML): {info['filename']}", flush=True)
                    cleaned += 1
            except Exception:
                pass

    print(f"[MODEL] 清理完成，共清理 {cleaned} 个文件", flush=True)


def migrate_old_assets():
    """从项目本地 assets/ 迁移到 RVC_DATA_DIR 下的统一目录。"""
    old_base = Path(__file__).resolve().parent.parent / "assets"
    if not old_base.exists():
        return
    for info in MODELS.values():
        new_path = info["path"]
        if new_path.exists():
            continue
        old_name = new_path.name
        # Search in the old flat structure or by relative path
        old_file = old_base / old_name
        if not old_file.exists():
            sub = old_base / new_path.parent.name / old_name
            if sub.exists():
                old_file = sub
            else:
                continue
        if old_file.exists():
            new_path.parent.mkdir(parents=True, exist_ok=True)
            import shutil
            shutil.copy2(old_file, new_path)
            mb = new_path.stat().st_size / (1024 * 1024)
            print(f"[MODEL] 已迁移 {old_name} ({mb:.0f}MB)", flush=True)
    for info in UVR_MODELS_DEF.values():
        model_path = UVR_MODEL_DIR / info["filename"]
        if model_path.exists():
            continue
        old_file = old_base / "uvr_models" / info["filename"]
        if not old_file.exists():
            old_file = old_base / info["filename"]
        if old_file.exists():
            model_path.parent.mkdir(parents=True, exist_ok=True)
            import shutil
            shutil.copy2(old_file, model_path)
            mb = model_path.stat().st_size / (1024 * 1024)
            print(f"[MODEL] 已迁移 {info['filename']} ({mb:.0f}MB)", flush=True)


def main():
    # Run migration on first launch
    if os.environ.get("RVC_DATA_DIR"):
        migrate_old_assets()

    # Rebuild argv without --data-dir consumed above
    args = _rest
    if args:
        cmd = args[0]
        if cmd == "status":
            get_status()
            return
        elif cmd == "download" and len(args) > 1:
            download_specific(args[1])
            return
        elif cmd == "clean":
            clean_stale()
            return

    print("[MODEL] 开始检查模型文件...", flush=True)
    for key, info in MODELS.items():
        path = info["path"]
        url = info["url"]
        if path.exists():
            mb = path.stat().st_size / (1024 * 1024)
            print(f"[MODEL] {path.name} 已存在 ({mb:.0f}MB)", flush=True)
        else:
            print(f"[MODEL] {path.name} 未找到，开始下载...", flush=True)
            dl_file(url, path, min_mb=info.get("min_mb", 0),
                    fallback_urls=info.get("fallback_urls"))
    for key in UVR_MODELS_DEF:
        check_uvr_model(key)
    print("[MODEL] 所有模型检查完成", flush=True)


if __name__ == "__main__":
    main()
