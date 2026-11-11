#!/usr/bin/env python3
"""
构建前依赖验证脚本 (verify_build_deps.py)
========================================
在全新的隔离 venv 中安装所有依赖并验证关键 import，
复现用户首次安装的环境。

用法:
  python tools/verify_build_deps.py           # 完整验证（慢，用于 CI / tauri build）
  python tools/verify_build_deps.py --quick   # 仅用当前 .venv 验证 import（快，用于开发时检查）

退出码: 0 = 成功, 1 = 失败（会阻断 tauri build）
"""
import os
import sys
import venv
import subprocess
import tempfile
import argparse
import platform
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# 必须能 import 的关键包（对应产品功能）
CRITICAL_PACKAGES = [
    ("torch",            "训练 / 推理 / 实时变声"),
    ("torchaudio",       "音频处理"),
    ("numpy",            "数值计算"),
    ("scipy",            "信号处理"),
    ("librosa",          "音频分析"),
    ("soundfile",        "音频读写"),
    ("faiss",            "特征索引"),
    ("audio_separator",  "歌声分离 (UVR)"),
    ("onnxruntime",      "ONNX 推理"),
    ("torchcrepe",       "F0 提取"),
]


def log(msg: str, level: str = "INFO"):
    symbols = {"INFO": "▸", "OK": "✓", "FAIL": "✗", "WARN": "⚠"}
    print(f"  {symbols.get(level, '·')} {msg}", flush=True)


def find_python_bin() -> Path:
    """优先用项目 .venv，fallback 到 Homebrew Python"""
    candidates = [
        ROOT / ".venv/bin/python",
        ROOT / "venv/bin/python",
    ]
    for ver in ["3.12", "3.11", "3.10"]:
        for base in ["/opt/homebrew/bin", "/usr/local/bin"]:
            candidates.append(Path(base) / f"python{ver}")

    for c in candidates:
        if c.exists():
            try:
                r = subprocess.run([str(c), "--version"], capture_output=True, text=True, timeout=5)
                if r.returncode == 0:
                    parts = r.stdout.strip().replace("Python ", "").split(".")
                    if len(parts) >= 2 and (int(parts[0]), int(parts[1])) >= (3, 10):
                        return c
            except Exception:
                continue
    raise RuntimeError("未找到 Python 3.10+，请先安装: brew install python@3.12")


def verify_imports(python_bin: Path) -> list[str]:
    """用给定 Python 逐一 import 关键包，返回失败的包名列表"""
    failed = []
    for pkg, desc in CRITICAL_PACKAGES:
        r = subprocess.run(
            [str(python_bin), "-c", f"import {pkg}; v = getattr({pkg}, '__version__', 'ok'); print(v)"],
            capture_output=True, text=True, timeout=15,
        )
        if r.returncode == 0:
            ver = r.stdout.strip()
            log(f"{pkg:<22} {ver:<15} ({desc})", "OK")
        else:
            log(f"{pkg:<22} — MISSING ({desc})", "FAIL")
            failed.append(pkg)
    return failed


def quick_check():
    """快速模式：直接用 .venv 或 sys.executable 验证"""
    try:
        python_bin = find_python_bin()
    except RuntimeError as e:
        print(f"\n❌ {e}")
        sys.exit(1)

    print(f"\n🔍 快速验证模式 (python: {python_bin})\n")
    failed = verify_imports(python_bin)
    if failed:
        print(f"\n❌ 缺失包: {', '.join(failed)}")
        print("   修复: .venv/bin/pip install " + " ".join(pkg.replace("_", "-") for pkg in failed))
        sys.exit(1)
    print("\n✅ 所有关键依赖验证通过")


def full_check():
    """完整模式：在全新 temp venv 中安装后验证"""
    lock_file = ROOT / "requirements.lock"
    req_file = ROOT / "requirements.txt"

    install_from = lock_file if lock_file.exists() else req_file
    mode = "锁文件" if lock_file.exists() else "requirements.txt（建议生成 lock 文件）"

    print(f"\n🔍 完整验证模式 — 使用: {install_from.name} ({mode})\n")

    try:
        base_python = find_python_bin()
    except RuntimeError as e:
        print(f"\n❌ {e}")
        sys.exit(1)

    with tempfile.TemporaryDirectory(prefix="rvc_build_verify_") as tmp:
        tmp_path = Path(tmp)
        log(f"创建临时 venv ({base_python})...")
        venv.create(str(tmp_path), with_pip=True, symlinks=True)
        python_bin = tmp_path / "bin" / "python"
        pip_bin = tmp_path / "bin" / "pip"

        log("升级 pip...")
        subprocess.run([str(pip_bin), "install", "--upgrade", "pip", "-q"],
                       capture_output=True, timeout=60)

        log(f"安装依赖 ({install_from.name})...")
        t0 = time.time()
        result = subprocess.run(
            [str(pip_bin), "install", "-r", str(install_from), "-q"],
            capture_output=True, text=True, timeout=600,
        )
        elapsed = time.time() - t0
        if result.returncode != 0:
            print(f"\n❌ 依赖安装失败 ({elapsed:.0f}s):\n{result.stderr[-1000:]}")
            sys.exit(1)
        log(f"依赖安装完成 ({elapsed:.0f}s)", "OK")

        print(f"\n  验证 import:\n")
        failed = verify_imports(python_bin)

    if failed:
        print(f"\n❌ 打包验证失败 — 以下包无法 import:\n   {', '.join(failed)}")
        print("\n  这意味着打包安装后用户也会遇到相同错误。")
        print("  修复: 检查 requirements.txt 或 requirements.lock，确保包名正确")
        sys.exit(1)

    print(f"\n✅ 全部 {len(CRITICAL_PACKAGES)} 个关键依赖验证通过 — 可以安全打包")


def main():
    parser = argparse.ArgumentParser(description="构建前依赖验证")
    parser.add_argument("--quick", action="store_true",
                        help="快速模式：仅验证当前 .venv，不创建新 venv（用于开发时检查）")
    args = parser.parse_args()

    print("=" * 60)
    print("  RVC 构建依赖验证")
    print(f"  系统: {platform.system()} {platform.machine()}")
    print("=" * 60)

    if args.quick:
        quick_check()
    else:
        full_check()


if __name__ == "__main__":
    main()
