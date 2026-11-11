#!/usr/bin/env python3
"""导入外部 RVC 音色模型到工作区。

由 Tauri 命令 `import_model` 调用：

    python tools/import_model.py <source> <model_name> <workspace_root>

source 支持：
  - http(s) 下载链接（.pth / .zip）
  - 本地 .pth 文件
  - 本地 .zip（内含 .pth，可附带 .index）

产物（与训练产物保持同一目录结构，便于各页面扫描）：
  <workspace>/experiments/<model_name>/<version>/checkpoints/<model>.pth
  <workspace>/experiments/<model_name>/<version>/checkpoints/<*.index>   # 若提供
  <workspace>/experiments/<model_name>/<version>/<model_name>.onnx       # 供直播模式使用

输出：
  `[IMPORT] ...` 进度行，最后一行 JSON：
  {"ok": bool, "message": str, "output_path": str}
"""

import json
import shutil
import sys
import tempfile
import urllib.parse
import urllib.request
import zipfile
from pathlib import Path

PROJECT_ROOT = Path(__file__).resolve().parent.parent
sys.path.insert(0, str(PROJECT_ROOT))

CHUNK = 1024 * 1024


def log(msg: str) -> None:
    print(f"[IMPORT] {msg}", flush=True)


def emit_result(ok: bool, message: str, output_path: str = "") -> None:
    print(
        json.dumps(
            {"ok": ok, "message": message, "output_path": output_path},
            ensure_ascii=False,
        ),
        flush=True,
    )


def sanitize_name(raw: str) -> str:
    name = Path(raw.strip()).name
    for suffix in (".pth", ".zip", ".onnx", ".pt"):
        if name.lower().endswith(suffix):
            name = name[: -len(suffix)]
            break
    name = name.replace("/", "_").replace("\\", "_").strip(". ")
    return name or "imported_model"


def is_url(source: str) -> bool:
    return source.lower().startswith(("http://", "https://"))


def download(url: str, dest_dir: Path) -> Path:
    filename = urllib.parse.unquote(Path(urllib.parse.urlparse(url).path).name)
    if not filename:
        filename = "model.pth"
    dest = dest_dir / filename

    log("开始下载模型...")
    req = urllib.request.Request(url, headers={"User-Agent": "Voxril/1.0"})
    with urllib.request.urlopen(req, timeout=60) as resp, open(dest, "wb") as f:
        total = int(resp.headers.get("Content-Length") or 0)
        done = 0
        last_pct = -1
        while True:
            chunk = resp.read(CHUNK)
            if not chunk:
                break
            f.write(chunk)
            done += len(chunk)
            if total > 0:
                pct = done * 100 // total
                if pct >= last_pct + 5:
                    last_pct = pct
                    log(f"下载中 {pct}% ({done // (1024 * 1024)}MB / {total // (1024 * 1024)}MB)")
            else:
                log(f"下载中 {done // (1024 * 1024)}MB")
    log(f"下载完成: {dest.name} ({dest.stat().st_size / 1024 / 1024:.1f}MB)")
    return dest


def unzip(archive: Path, dest_dir: Path) -> None:
    log(f"解压 {archive.name} ...")
    with zipfile.ZipFile(archive) as zf:
        for member in zf.namelist():
            # 防止 zip slip
            target = (dest_dir / member).resolve()
            if not str(target).startswith(str(dest_dir.resolve())):
                continue
            zf.extract(member, dest_dir)
    log("解压完成")


def collect_model_files(root: Path) -> tuple[Path | None, list[Path]]:
    """在目录中找出生成器 .pth（排除判别器）与所有 .index 文件。"""
    pths = list(root.rglob("*.pth"))
    generators = [
        p
        for p in pths
        if not p.name.startswith("D_") and "discriminator" not in p.name.lower()
    ]
    # 优先带 f0 / G 的生成器权重
    generators.sort(key=lambda p: (0 if p.name.startswith("f0G") else 1, len(p.name)))
    model_file = generators[0] if generators else (pths[0] if pths else None)
    indexes = sorted(root.rglob("*.index"))
    return model_file, indexes


def detect_version_dir(pth: Path) -> str:
    """从 checkpoint 读取 RVC 版本与采样率，生成类似 v2_40k 的目录名。"""
    try:
        import torch

        try:
            cpt = torch.load(pth, map_location="cpu", weights_only=False)
        except TypeError:  # torch < 2.0 没有 weights_only 参数
            cpt = torch.load(pth, map_location="cpu")
    except Exception as exc:  # noqa: BLE001
        log(f"警告: 无法读取 checkpoint 元信息（{exc}），按 v2_40k 处理")
        return "v2_40k"

    version = str(cpt.get("version") or "v2")
    sr_raw = None
    config = cpt.get("config")
    if isinstance(config, (list, tuple)) and config:
        sr_raw = config[-1]
    if sr_raw is None:
        sr_raw = cpt.get("sr")

    if isinstance(sr_raw, str) and sr_raw.strip():
        tag = sr_raw.strip().lower()
    elif isinstance(sr_raw, (int, float)) and sr_raw > 0:
        khz = round(float(sr_raw) / 1000)
        tag = f"{khz}k"
    else:
        tag = "40k"

    return f"{version}_{tag}"


def export_onnx(pth: Path, onnx_path: Path) -> None:
    from infer.modules.onnx.export import export_onnx as _export

    log("正在导出 ONNX（直播模式需要）...")
    _export(str(pth), str(onnx_path))
    log(f"ONNX 导出完成: {onnx_path.name}")


def main() -> None:
    if len(sys.argv) < 4:
        emit_result(False, "参数不足：需要 <source> <model_name> <workspace_root>")
        return

    source, raw_name, workspace_root = sys.argv[1], sys.argv[2], sys.argv[3]
    model_name = sanitize_name(raw_name)
    workspace = Path(workspace_root).expanduser()
    log(f"导入目标: {model_name}")

    with tempfile.TemporaryDirectory(prefix="voxril_import_") as tmp:
        tmp_dir = Path(tmp)
        work_dir = tmp_dir / "src"
        work_dir.mkdir()

        try:
            if is_url(source):
                local = download(source, tmp_dir)
            else:
                local = Path(source).expanduser()
                if not local.exists():
                    emit_result(False, f"源文件不存在: {source}")
                    return

            if local.suffix.lower() == ".zip":
                unzip(local, work_dir)
                pth, indexes = collect_model_files(work_dir)
            else:
                pth = local
                indexes = sorted(local.parent.glob("*.index"))

            if pth is None or not pth.exists():
                emit_result(False, "未在导入内容中找到 .pth 模型文件")
                return
            if pth.suffix.lower() == ".onnx":
                emit_result(False, "暂不支持直接导入 .onnx，请提供 .pth 或包含 .pth 的 zip")
                return

            log("校验模型文件...")
            version_dir = detect_version_dir(pth)
            checkpoints = workspace / "experiments" / model_name / version_dir / "checkpoints"
            checkpoints.mkdir(parents=True, exist_ok=True)

            pth_dest = checkpoints / pth.name
            shutil.copy2(pth, pth_dest)
            log(f"已写入 {pth_dest.relative_to(workspace)}")

            for idx in indexes:
                shutil.copy2(idx, checkpoints / idx.name)
                log(f"已写入索引 {idx.name}")

            onnx_path = checkpoints.parent / f"{model_name}.onnx"
            try:
                export_onnx(pth_dest, onnx_path)
                output_path = str(onnx_path)
                note = ""
                if not indexes:
                    note = "（未附带 index，检索增强不可用）"
                emit_result(
                    True,
                    f"导入完成: {model_name} / {version_dir}{note}",
                    output_path,
                )
            except Exception as exc:  # noqa: BLE001
                log(f"ONNX 导出失败，仅保留 .pth: {exc}")
                emit_result(
                    True,
                    f"模型已导入（ONNX 导出失败，直播模式不可用）: {exc}",
                    str(pth_dest),
                )
        except Exception as exc:  # noqa: BLE001
            emit_result(False, f"导入失败: {exc}")


if __name__ == "__main__":
    main()
