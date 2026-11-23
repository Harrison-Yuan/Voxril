"""
检查并下载项目必需模型文件，支持单模型下载、状态查询。

特性：
- 多下载源：官方 / 国内镜像 / 自定义，自动模式按可用性排序
- 断点续传：中断后保留 .part，下次从断点继续
- 完整性校验：已知文件的 SHA256 + 精确字节数 + 大小下限
- 失败自动切换下一个源

每行以 [MODEL] 标记输出，便于前端通过 Tauri 事件流捕获。
"""
import argparse
import hashlib
import json
import os
import sys
import time
from pathlib import Path
from urllib.error import HTTPError, URLError
from urllib.parse import urlparse
from urllib.request import Request, urlopen

# Parse CLI arguments first (overrides RVC_DATA_DIR env)
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--data-dir")
_parser.add_argument("--source", default="auto")
_parser.add_argument("--hf-endpoint", default="")
_parser.add_argument("--gh-proxy", default="")
_known, _rest = _parser.parse_known_args()
if _known.data_dir:
    os.environ["RVC_DATA_DIR"] = _known.data_dir

# 下载源：auto（自动排序）| official（官方）| cn（国内镜像）| custom（自定义）
SOURCE = (_known.source or "auto").strip().lower()
HF_ENDPOINT_OVERRIDE = (_known.hf_endpoint or "").strip().rstrip("/")
GH_PROXY_OVERRIDE = (_known.gh_proxy or "").strip().rstrip("/")

DEFAULT_HF = "https://huggingface.co"
MIRROR_HF = "https://hf-mirror.com"
# 公共 GitHub 加速前缀；仅在自动/国内模式下作为直连失败后的回退
GH_PROXIES = ["https://ghfast.top", "https://gh-proxy.com"]

# Use RVC_DATA_DIR if set (from Rust), otherwise fall back to project root
_env_data = os.environ.get("RVC_DATA_DIR")
if _env_data:
    ASSETS_DIR = Path(_env_data) / "assets"
else:
    ASSETS_DIR = Path(__file__).resolve().parent.parent / "assets"

PRETRAINED_DIR = ASSETS_DIR / "pretrained_v2"

MODELS = {
    "hubert": {
        # vec-768-layer-12.onnx: RVC v2 标准特征提取模型，输出 768 维
        "path": ASSETS_DIR / "hubert" / "vec-768-layer-12.onnx",
        "repo": "ozada/onnx_rvc",
        "file": "vec-768-layer-12.onnx",
        # 备用仓库：主仓库不可用时按顺序尝试
        "fallbacks": [("NaruseMioShirakana/MoeSS-SUBModel", "vec-768-layer-12.onnx")],
        "sha256": "b3886e7dff1495cda514f94f4680a7b1261e05d6929f5c764cdb17934b413c2a",
        "min_mb": 200,
        "name": "vec-768-layer-12 (特征提取 ONNX)",
    },
    # 注意：contentvec-f.onnx 已弃用，项目统一使用 vec-768-layer-12.onnx
    "rmvpe": {
        "path": ASSETS_DIR / "rmvpe" / "rmvpe.onnx",
        "repo": "lj1995/VoiceConversionWebUI",
        "file": "rmvpe.onnx",
        "sha256": "5370e71ac80af8b4b7c793d27efd51fd8bf962de3a7ede0766dac0befa3660fd",
        "min_mb": 80,
        "name": "RMVPE (F0 音高提取 ONNX)",
    },
    "fcpe": {
        "path": ASSETS_DIR / "fcpe" / "fcpe.onnx",
        "repo": "gzivdo/fcpe-onnx",
        "file": "fcpe.onnx",
        "sha256": "b7e4f3871b10641869b7ac5a2d56ed94deb37552c0336d77e17ad6e66760adf0",
        "min_mb": 35,
        "name": "FCPE (F0 音高提取 ONNX，直播实时)",
    },
    "f0G40k": {
        "path": PRETRAINED_DIR / "f0G40k.pth",
        "repo": "lj1995/VoiceConversionWebUI",
        "file": "pretrained_v2/f0G40k.pth",
        "sha256": "3b2c44035e782c4b14ddc0bede9e2f4a724d025cd073f736d4f43708453adfcb",
        "min_mb": 60,
        "name": "RVC v2 Generator f0G40k (预训练)",
    },
    "f0D40k": {
        "path": PRETRAINED_DIR / "f0D40k.pth",
        "repo": "lj1995/VoiceConversionWebUI",
        "file": "pretrained_v2/f0D40k.pth",
        "sha256": "6b6ab091e70801b28e3f41f335f2fc5f3f35c75b39ae2628d419644ec2b0fa09",
        "min_mb": 100,
        "name": "RVC v2 Discriminator f0D40k (预训练)",
    },
}

UVR_REPO = "https://github.com/TRvlvr/model_repo/releases/download/all_public_uvr_models"
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

_probe_cache = {}


def _probe(url, timeout=4):
    """探测源是否可用（只读 1 字节），结果缓存。"""
    if url in _probe_cache:
        return _probe_cache[url]
    ok = False
    try:
        req = Request(url, headers={"User-Agent": "Mozilla/5.0"})
        with urlopen(req, timeout=timeout) as response:
            response.read(1)
            ok = getattr(response, "status", 200) < 400
    except Exception:
        ok = False
    _probe_cache[url] = ok
    return ok


def _hf_endpoints():
    """按当前下载源返回 HuggingFace 端点顺序。"""
    custom = HF_ENDPOINT_OVERRIDE
    if SOURCE == "official":
        return [DEFAULT_HF]
    if SOURCE == "cn":
        return [custom] if custom else [MIRROR_HF]
    if SOURCE == "custom" and custom:
        return [custom, DEFAULT_HF]
    # auto：官方优先，不可达时镜像优先
    if custom:
        return [custom, DEFAULT_HF]
    if _probe(f"{DEFAULT_HF}/api/models/lj1995/VoiceConversionWebUI"):
        return [DEFAULT_HF, MIRROR_HF]
    return [MIRROR_HF, DEFAULT_HF]


def _hf_urls(repo, path):
    base = f"{repo}/resolve/main/{path}"
    return [f"{endpoint}/{base}" for endpoint in _hf_endpoints()]


def _gh_urls(url):
    """按当前下载源返回 GitHub 直链及其加速前缀。"""
    if SOURCE == "official":
        return [url]
    proxies = []
    if GH_PROXY_OVERRIDE:
        proxies.append(GH_PROXY_OVERRIDE)
    if SOURCE in ("auto", "cn"):
        proxies.extend(p for p in GH_PROXIES if p not in proxies)
    proxied = [f"{p}/{url}" for p in proxies]
    # 国内模式先走加速，失败再直连；自动模式先直连，失败再走加速
    return (proxied + [url]) if SOURCE == "cn" else ([url] + proxied)


def _model_urls(model_key):
    info = MODELS[model_key]
    urls = _hf_urls(info["repo"], info["file"])
    for repo, path in info.get("fallbacks", []):
        urls.extend(_hf_urls(repo, path))
    return urls


def _uvr_urls(model_key):
    info = UVR_MODELS_DEF[model_key]
    return _gh_urls(info["url"]) if info.get("url") else []


def _is_html_content(data):
    head = data[:512].decode("utf-8", errors="ignore").strip().lower()
    return head.startswith("<!doctype") or head.startswith("<html") or head.startswith("<!")


def _mb(size):
    return size / (1024 * 1024)


def _sha256_of(path):
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def _discard(part, meta_path):
    part.unlink(missing_ok=True)
    meta_path.unlink(missing_ok=True)


def _finalize(part, dest, meta_path, total, min_mb, sha256):
    """校验并提交 .part 文件。校验失败时删除，避免下次续传坏文件。"""
    size = part.stat().st_size
    if total and size != total:
        if size > total:
            _discard(part, meta_path)
            raise RuntimeError(f"下载大小异常 ({size} > {total})")
        raise RuntimeError(
            f"文件不完整 ({_mb(size):.0f}MB / {_mb(total):.0f}MB)，下次将续传"
        )
    if min_mb and size < min_mb * 1024 * 1024:
        _discard(part, meta_path)
        raise RuntimeError(f"文件不完整 ({_mb(size):.0f}MB < {min_mb}MB)")
    if size == 0:
        _discard(part, meta_path)
        raise RuntimeError("下载文件为空")
    if sha256:
        print(f"[MODEL] 校验 {dest.name} ...", flush=True)
        actual = _sha256_of(part)
        if actual != sha256.lower():
            _discard(part, meta_path)
            raise RuntimeError(
                f"SHA256 校验失败（{actual[:12]}… != {sha256[:12]}…），已删除损坏文件"
            )
        print(f"[MODEL] 校验通过 {dest.name}", flush=True)
    os.replace(part, dest)
    meta_path.unlink(missing_ok=True)
    print(f"[MODEL] {dest.name} 下载完成 ({_mb(size):.0f}MB)", flush=True)


def _dl_once(url, dest, min_mb=0, sha256=None, retry_416=True):
    """从单个 URL 下载，支持 Range 断点续传。失败时保留 .part 供下次续传。"""
    os.makedirs(dest.parent, exist_ok=True)
    part = dest.with_name(dest.name + ".part")
    meta_path = dest.with_name(dest.name + ".part.json")

    resume = part.stat().st_size if part.exists() else 0
    etag = ""
    if resume and meta_path.exists():
        try:
            etag = json.loads(meta_path.read_text()).get("etag", "")
        except Exception:
            etag = ""

    headers = {"User-Agent": "Mozilla/5.0", "Accept-Encoding": "identity"}
    if resume:
        headers["Range"] = f"bytes={resume}-"
        if etag:
            headers["If-Range"] = etag
        print(f"[MODEL] 继续下载 {dest.name}（已有 {_mb(resume):.0f}MB）", flush=True)
    print(f"[MODEL] 下载中 {dest.name} 从 {urlparse(url).netloc} ...", flush=True)

    req = Request(url, headers=headers)
    try:
        with urlopen(req, timeout=30) as response:
            status = getattr(response, "status", 200)
            if status >= 400:
                raise RuntimeError(f"HTTP {status}: {response.reason}")

            content_type = response.headers.get("Content-Type", "")
            if "text/" in content_type or "html" in content_type:
                raise RuntimeError(f"服务器返回了无效 Content-Type: {content_type}")

            ranged = resume > 0 and status == 206
            if resume and not ranged:
                print("[MODEL] 服务器不支持断点续传，从头开始", flush=True)
                _discard(part, meta_path)
                resume = 0

            total = 0
            content_range = response.headers.get("Content-Range", "")
            if "/" in content_range:
                try:
                    total = int(content_range.rsplit("/", 1)[1])
                except ValueError:
                    total = 0
            elif response.headers.get("Content-Length"):
                total = resume + int(response.headers["Content-Length"])

            new_etag = response.headers.get("ETag", "")
            if new_etag:
                try:
                    meta_path.write_text(json.dumps({"etag": new_etag}))
                except Exception:
                    pass

            if total and resume >= total:
                _finalize(part, dest, meta_path, total, min_mb, sha256)
                return

            last_pct = -1
            last_report = 0.0
            mode = "ab" if ranged else "wb"
            with open(part, mode) as f:
                while True:
                    chunk = response.read(262144)
                    if not chunk:
                        break
                    if mode == "wb" and f.tell() == 0 and _is_html_content(chunk):
                        f.close()
                        _discard(part, meta_path)
                        raise RuntimeError("下载内容为 HTML 页面，不是有效的模型文件（可能是错误页面）")
                    f.write(chunk)
                    written = f.tell()
                    if total > 0:
                        pct = min(99, int(written * 100 / total))
                        if pct != last_pct:
                            last_pct = pct
                            print(
                                f"[MODEL] 下载中 {dest.name} ... {pct}% ({_mb(written):.0f}MB)",
                                flush=True,
                            )
                    elif time.time() - last_report > 5:
                        last_report = time.time()
                        print(f"[MODEL] 下载中 {dest.name} ... {_mb(written):.0f}MB", flush=True)

        _finalize(part, dest, meta_path, total, min_mb, sha256)
    except HTTPError as e:
        if e.code == 416 and part.exists() and retry_416:
            print("[MODEL] 断点已失效，重新下载", flush=True)
            _discard(part, meta_path)
            return _dl_once(url, dest, min_mb, sha256, retry_416=False)
        raise RuntimeError(f"HTTP {e.code}: {e.reason}")
    except URLError as e:
        raise RuntimeError(f"网络错误: {e.reason}")


def dl_file(urls, dest, min_mb=0, sha256=None):
    """按顺序尝试所有下载源，失败自动换源；网络错误在当前源重试一次。"""
    last_err = None
    for index, url in enumerate(urls):
        for attempt in (1, 2):
            try:
                _dl_once(url, dest, min_mb=min_mb, sha256=sha256)
                return
            except Exception as e:
                last_err = e
                print(f"[MODEL] 下载失败（{urlparse(url).netloc}）：{e}", flush=True)
                # 校验失败或 4xx 重试没有意义，直接换源
                fatal = "校验失败" in str(e) or "HTTP 4" in str(e)
                if fatal:
                    break
                if attempt == 1:
                    time.sleep(2)
                    print("[MODEL] 重试当前源...", flush=True)
        if index + 1 < len(urls):
            print("[MODEL] 切换下一个下载源...", flush=True)
    raise RuntimeError(f"所有下载源均失败: {last_err}")


def download_uvr_model(model_key="bs_roformer"):
    model_def = UVR_MODELS_DEF[model_key]
    filename = model_def["filename"]
    uvr_path = UVR_MODEL_DIR / filename
    min_mb = model_def.get("min_mb", 0)
    os.makedirs(UVR_MODEL_DIR, exist_ok=True)

    urls = _uvr_urls(model_key)
    if urls:
        dl_file(urls, uvr_path, min_mb=min_mb)
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
            uvr_path.unlink(missing_ok=True)
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
        if exists and info.get("min_mb", 0) and size_mb < info["min_mb"]:
            exists = False
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
        dl_file(_model_urls(model_id), info["path"],
                min_mb=info.get("min_mb", 0), sha256=info.get("sha256"))
    elif model_id in UVR_MODELS_DEF:
        download_uvr_model(model_id)
    else:
        print(f"Unknown model id: {model_id}", flush=True)
        sys.exit(1)


def clean_stale():
    """清理旧版 .tmp 残留，以及超过 7 天的未完成断点。"""
    cleaned = 0
    now = time.time()

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
                continue
            if f.name.endswith(".part") or f.name.endswith(".part.json"):
                # 保留断点供续传，仅清理超过 7 天的
                try:
                    if now - f.stat().st_mtime > 7 * 86400:
                        f.unlink(missing_ok=True)
                        print(f"[MODEL] 清理过期断点: {f.name}", flush=True)
                        cleaned += 1
                except OSError:
                    pass

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

    # Rebuild argv without consumed CLI options
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
        min_mb = info.get("min_mb", 0)
        if path.exists():
            mb = path.stat().st_size / (1024 * 1024)
            if min_mb and mb < min_mb:
                print(f"[MODEL] {path.name} 不完整 ({mb:.0f}MB)，重新下载...", flush=True)
                path.unlink(missing_ok=True)
            else:
                print(f"[MODEL] {path.name} 已存在 ({mb:.0f}MB)", flush=True)
                continue
        print(f"[MODEL] {path.name} 未找到，开始下载...", flush=True)
        dl_file(_model_urls(key), path, min_mb=min_mb, sha256=info.get("sha256"))
    for key in UVR_MODELS_DEF:
        check_uvr_model(key)
    print("[MODEL] 所有模型检查完成", flush=True)


if __name__ == "__main__":
    main()
