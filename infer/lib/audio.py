import platform, os
import subprocess
import numpy as np
import av
from io import BytesIO
import traceback
import re


def wav2(i, o, format):
    inp = av.open(i, "rb")
    if format == "m4a":
        format = "mp4"
    out = av.open(o, "wb", format=format)
    if format == "ogg":
        format = "libvorbis"
    if format == "mp4":
        format = "aac"

    ostream = out.add_stream(format)

    for frame in inp.decode(audio=0):
        for p in ostream.encode(frame):
            out.mux(p)

    for p in ostream.encode(None):
        out.mux(p)

    out.close()
    inp.close()


def load_audio(file, sr):
    """
    使用 Rust decode-audio 工具解码音频，无需 ffmpeg 或 PyAV 子进程。

    支持所有常见格式（mp3, wav, flac, m4a, ogg, wma 等），
    自动混音为单声道、重采样到目标采样率。
    """
    file = clean_path(file)
    if not os.path.exists(file):
        raise RuntimeError(f"音频文件不存在: {file}")

    bin_path = os.environ.get("VOXRIL_BIN", "voxril")
    try:
        result = subprocess.run(
            [bin_path, "decode-audio", "--input", file, "--sr", str(sr)],
            capture_output=True,
            timeout=30,
        )
        if result.returncode != 0:
            raise RuntimeError(
                f"解码失败 (exit={result.returncode}): {result.stderr.decode()}"
            )
        audio = np.frombuffer(result.stdout, dtype=np.float32).flatten()
        if len(audio) == 0:
            raise RuntimeError("解码结果为空")
        return audio
    except FileNotFoundError:
        raise RuntimeError(
            f"找不到 voxril 可执行文件: {bin_path}"
        )
    except subprocess.TimeoutExpired:
        raise RuntimeError("音频解码超时 (30s)")



def clean_path(path_str):
    if platform.system() == "Windows":
        path_str = path_str.replace("/", "\\")
    path_str = re.sub(r'[\u202a\u202b\u202c\u202d\u202e]', '', path_str)  # 移除 Unicode 控制字符
    return path_str.strip(" ").strip('"').strip("\n").strip('"').strip(" ")
