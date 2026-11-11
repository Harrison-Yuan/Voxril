"""
人声分离模块 — 基于 audio-separator 的 BS-RoFormer (SDR 12.97)。
当前最高质量的人声分离模型之一，Apple Silicon MPS + CoreML 原生加速。
"""

import os
import logging
import sys
import time
import shutil
import tempfile

os.environ.setdefault("KMP_DUPLICATE_LIB_OK", "TRUE")
os.environ.setdefault("OMP_NUM_THREADS", "2")
os.environ.setdefault("MKL_NUM_THREADS", "2")
os.environ.setdefault("OPENBLAS_NUM_THREADS", "2")

logger = logging.getLogger(__name__)

_SEPARATOR = None
_env_data = os.environ.get("RVC_DATA_DIR")
if _env_data:
    _MODEL_DIR = os.path.join(_env_data, "assets", "uvr_models")
else:
    _MODEL_DIR = os.path.join(os.path.dirname(os.path.dirname(os.path.dirname(__file__))), "assets", "uvr_models")
_DEFAULT_MODEL = "model_bs_roformer_ep_317_sdr_12.9755.ckpt"

os.makedirs(_MODEL_DIR, exist_ok=True)


def _emit_progress(msg, file_tag="", step="", current=0, total=0):
    tag = f"[UVR_PROGRESS]{file_tag}"
    if step and total:
        tag += f"[{step}]"
        pct = int(current / max(total, 1) * 100)
        sys.stdout.write(f"{tag} {current}/{total} {pct}% {msg}\n")
    else:
        sys.stdout.write(f"{tag} {msg}\n")
    sys.stdout.flush()


def _get_separator(model_name=None):
    global _SEPARATOR
    
    target_model = model_name if model_name else _DEFAULT_MODEL
    
    # 如果已有实例且模型不一致，则销毁旧实例（TODO: 缓存多个模型？）
    if _SEPARATOR is not None:
        if getattr(_SEPARATOR, 'model_file_name', '') == target_model:
            return _SEPARATOR
        else:
            _SEPARATOR = None

    model_path = os.path.join(_MODEL_DIR, target_model)
    if not os.path.exists(model_path):
        _emit_progress(f"模型文件未找到，开始下载 {target_model}...")
    else:
        file_size_mb = os.path.getsize(model_path) / (1024 * 1024)
        _emit_progress(f"加载模型 {target_model} (约 {file_size_mb:.1f}MB)...")

    from audio_separator.separator import Separator
    _SEPARATOR = Separator(
        log_level=20,
        model_file_dir=_MODEL_DIR,
        output_dir=None,
        output_format="WAV",
        use_autocast=True,
    )
    _SEPARATOR.model_file_name = target_model # 记录当前加载的模型
    _emit_progress("模型加载中...")
    _SEPARATOR.load_model(target_model)
    _emit_progress(f"模型 {target_model} 加载完成")
    return _SEPARATOR


def separate(input_path, vocal_dir, instrumental_dir, format="wav", file_tag="", model_name=None):
    t_start = time.time()
    basename = os.path.splitext(os.path.basename(input_path))[0]
    logger.info(f"vocal_sep: input={input_path}, model={model_name}")

    try:
        separator = _get_separator(model_name)

        os.makedirs(vocal_dir, exist_ok=True)
        os.makedirs(instrumental_dir, exist_ok=True)

        out_dir = tempfile.mkdtemp()

        _emit_progress("开始分离...", file_tag)
        separator.output_dir = out_dir
        if hasattr(separator, 'model_instance') and separator.model_instance:
            separator.model_instance.output_dir = out_dir
        _emit_progress("正在处理音频 (BS-RoFormer MPS 加速)...", file_tag)
        separator.separate(input_path)
        _emit_progress("分离完成，查找输出文件...", file_tag)

        found = []
        for fname in os.listdir(out_dir):
            fp = os.path.join(out_dir, fname)
            if not os.path.isfile(fp):
                continue
            ext = os.path.splitext(fname)[1].lower()
            if ext in ['.wav', '.flac', '.mp3', '.ogg', '.m4a', '.aac']:
                found.append(fp)

        found.sort()

        vocal_path = ""
        instrumental_path = ""

        for src in found:
            fname = os.path.basename(src)
            fn_lower = fname.lower()

            if "vocals" in fn_lower or "voc" in fn_lower:
                dst = os.path.join(vocal_dir, f"vocal_{basename}.{format}")
                shutil.move(src, dst)
                vocal_path = dst
            elif "instrumental" in fn_lower or "inst" in fn_lower or "other" in fn_lower:
                dst = os.path.join(instrumental_dir, f"instrument_{basename}.{format}")
                shutil.move(src, dst)
                instrumental_path = dst
            elif "no_vocals" in fn_lower:
                dst = os.path.join(instrumental_dir, f"instrument_{basename}.{format}")
                shutil.move(src, dst)
                instrumental_path = dst

        # Scan remaining files in out_dir — files the loop didn't match
        def _scan_remaining():
            items = []
            for fname in os.listdir(out_dir):
                fp = os.path.join(out_dir, fname)
                if not os.path.isfile(fp):
                    continue
                ext = os.path.splitext(fname)[1].lower()
                if ext in ['.wav', '.flac', '.mp3', '.ogg', '.m4a', '.aac']:
                    items.append(fp)
            items.sort()
            return items

        remaining = _scan_remaining()
        if not vocal_path and remaining:
            dst = os.path.join(vocal_dir, f"vocal_{basename}.{format}")
            shutil.move(remaining[0], dst)
            vocal_path = dst

        remaining = _scan_remaining()
        if not instrumental_path and remaining:
            dst = os.path.join(instrumental_dir, f"instrument_{basename}.{format}")
            shutil.move(remaining[-1], dst)
            instrumental_path = dst

        shutil.rmtree(out_dir, ignore_errors=True)

        elapsed = time.time() - t_start
        _emit_progress(f"分离完成 ({elapsed:.1f}s)", file_tag)
        logger.info(f"vocal_sep: done in {elapsed:.1f}s")

        return {
            "ok": bool(vocal_path) and bool(instrumental_path),
            "vocal_path": vocal_path,
            "instrumental_path": instrumental_path,
            "message": f"分离完成 ({elapsed:.1f}s)\nvocal: {vocal_path}\ninstrumental: {instrumental_path}",
        }

    except Exception as e:
        import traceback
        logger.error(f"vocal_sep: {traceback.format_exc()}")
        return {
            "ok": False,
            "vocal_path": "",
            "instrumental_path": "",
            "message": f"分离失败: {e}",
        }
    finally:
        # 主动释放 MPS 显存，避免后续任务（如声音转换）发生 OOM
        try:
            import torch
            if torch.backends.mps.is_available():
                torch.mps.empty_cache()
        except Exception:
            pass


def separate_batch(input_dir, vocal_dir, instrumental_dir, format="wav", model_name=None):
    results = []
    audio_exts = {".wav", ".mp3", ".flac", ".ogg", ".m4a", ".aac", ".opus"}

    files = sorted([
        f for f in os.listdir(input_dir)
        if os.path.splitext(f)[1].lower() in audio_exts
    ])

    total_files = len(files)
    for idx, f in enumerate(files):
        file_tag = f"[{idx+1}/{total_files}]"
        _emit_progress(f"开始分离: {f}", file_tag, "file", idx + 1, total_files)
        input_path = os.path.join(input_dir, f)
        result = separate(input_path, vocal_dir, instrumental_dir, format, file_tag=file_tag, model_name=model_name)
        results.append(result)
        status = "完成" if result.get("ok") else "失败"
        _emit_progress(f"分离{status}: {f}", file_tag, "file", idx + 1, total_files)

    return results
