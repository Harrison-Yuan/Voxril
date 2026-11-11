"""
首次启动时自动创建 Python 虚拟环境并安装依赖。
用户无需手动配置 Python 环境，开箱即用。
每行以 [SETUP] 标记输出，前端通过 Tauri 事件流捕获。
"""
import os
import sys
import subprocess
import platform
from pathlib import Path

VERSION_REQUIRED = (3, 10)


def emit(msg):
    print(f"[SETUP] {msg}")
    sys.stdout.flush()


def get_app_data_dir():
    home = Path.home()
    if platform.system() == "Darwin":
        return home / "Library" / "Application Support" / "com.rvc.webui"
    return home / ".rvc-webui"


def find_python():
    """Find the best Python 3.10+ interpreter.
    Priority: Homebrew → existing venv → sys.executable.
    """
    candidates = []
    # Homebrew paths
    for ver in ["3.12", "3.11", "3.10"]:
        for base in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/local/Homebrew/bin"]:
            candidates.append(Path(base) / f"python{ver}")
    # Existing app venv
    candidates.append(get_app_data_dir() / "venv/bin/python")
    # Current interpreter as fallback
    candidates.append(Path(sys.executable))

    seen = set()
    for cand in candidates:
        resolved = cand.resolve()
        if resolved in seen or not resolved.exists():
            continue
        seen.add(resolved)
        try:
            result = subprocess.run(
                [str(resolved), "--version"],
                capture_output=True, text=True, timeout=10,
            )
            if result.returncode != 0:
                continue
            v_str = result.stdout.strip()
            # Parse "Python 3.x.y"
            parts = v_str.replace("Python ", "").split(".")
            if len(parts) >= 2:
                major, minor = int(parts[0]), int(parts[1])
                if (major, minor) >= VERSION_REQUIRED:
                    emit(f"找到 Python {major}.{minor} ({resolved})")
                    return resolved
        except Exception:
            continue

    return None


def check_python(python_bin):
    try:
        result = subprocess.run(
            [str(python_bin), "--version"],
            capture_output=True, text=True, timeout=10,
        )
        if result.returncode == 0:
            emit(f"Python 解释器: {result.stdout.strip()} ({python_bin})")
            return True
    except Exception:
        pass
    emit(f"Python 解释器不可用: {python_bin}")
    return False


def setup_venv(python_bin: Path, venv_path: Path):
    if venv_path.exists():
        existing = venv_path / "bin" / "python"
        if existing.exists():
            result = subprocess.run(
                [str(existing), "--version"],
                capture_output=True, text=True, timeout=10
            )
            if result.returncode == 0:
                version_str = result.stdout.strip()
                # Check if venv Python is too old
                parts = version_str.replace("Python ", "").split(".")
                if len(parts) >= 2:
                    major, minor = int(parts[0]), int(parts[1])
                    if (major, minor) >= VERSION_REQUIRED:
                        emit(f"虚拟环境已存在: {version_str}")
                        return True
                    else:
                        emit(f"虚拟环境 Python 版本过低 ({version_str})，重建以使用 {python_bin}...")
                        import shutil
                        shutil.rmtree(venv_path)
                else:
                    emit("虚拟环境已损坏，重新创建...")
                    import shutil
                    shutil.rmtree(venv_path)
            else:
                emit("虚拟环境已损坏，重新创建...")
                import shutil
                shutil.rmtree(venv_path)
        else:
            emit("虚拟环境不完整，重新创建...")
            import shutil
            shutil.rmtree(venv_path)

    emit(f"正在使用 {python_bin} 创建虚拟环境...")
    result = subprocess.run(
        [str(python_bin), "-m", "venv", str(venv_path)],
        capture_output=True, text=True, timeout=60
    )
    if result.returncode != 0:
        emit(f"创建虚拟环境失败: {result.stderr.strip()}")
        return False
    venv_python = venv_path / "bin" / "python"
    emit(f"虚拟环境已创建: {venv_python}")
    return True


def install_requirements(venv_path: Path, req_path: Path):
    python_bin = venv_path / "bin" / "python"
    pip_bin = venv_path / "bin" / "pip"

    emit("正在升级 pip...")
    subprocess.run(
        [str(python_bin), "-m", "pip", "install", "--upgrade", "pip", "--quiet"],
        capture_output=True, text=True, timeout=120
    )

    emit("正在安装 Python 依赖 (首次安装可能需要 5-15 分钟)...")
    process = subprocess.Popen(
        [
            str(python_bin), "-m", "pip", "install",
            "-r", str(req_path),
            "--progress-bar", "on",
        ],
        stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
        text=True, bufsize=1,
    )

    for line in process.stdout:
        line = line.strip()
        if line:
            emit(line)

    process.wait()
    if process.returncode != 0:
        emit("依赖安装出现错误，请检查网络连接后重启应用")
        return False
    emit("Python 依赖安装完成")
    return True


def verify_imports(venv_path: Path):
    python_bin = venv_path / "bin" / "python"
    key_packages = [
        "torch",
        "torchaudio",
        "numpy",
        "scipy",
        "librosa",
        "soundfile",
        "faiss",
        "audio_separator",
        "onnxruntime",
    ]
    missing = []
    for pkg in key_packages:
        result = subprocess.run(
            [str(python_bin), "-c", f"import {pkg}"],
            capture_output=True, text=True, timeout=15,
        )
        if result.returncode == 0:
            ver = subprocess.run(
                [str(python_bin), "-c", f"import {pkg}; print(getattr({pkg}, '__version__', 'ok'))"],
                capture_output=True, text=True, timeout=15,
            )
            version_str = ver.stdout.strip()
            emit(f"  ✓ {pkg} ({version_str})")
        else:
            emit(f"  ✗ {pkg} 导入失败")
            missing.append(pkg)
    return missing


def main():
    emit(f"系统: {platform.system()} {platform.release()} ({platform.machine()})")
    emit("开始检查 Python 环境...")

    python_bin = find_python()
    if python_bin is None:
        emit(f"未找到 Python {VERSION_REQUIRED[0]}.{VERSION_REQUIRED[1]}+，尝试安装...")
        if platform.system() == "Darwin":
            emit("正在通过 Homebrew 安装 Python 3.12...")
            result = subprocess.run(
                ["brew", "install", "python@3.12"],
                capture_output=True, text=True, timeout=300,
            )
            if result.returncode == 0:
                python_bin = find_python()
            else:
                emit(f"安装失败: {result.stderr.strip()}")
                emit("请手动安装: brew install python@3.12")
                sys.exit(1)
        else:
            emit("请安装 Python 3.10+ 后重启应用")
            sys.exit(1)

    if not check_python(python_bin):
        sys.exit(1)

    script_dir = Path(__file__).resolve().parent.parent
    req_path = script_dir / "requirements.txt"

    app_data = get_app_data_dir()
    venv_path = app_data / "venv"

    emit(f"应用数据目录: {app_data}")
    os.makedirs(app_data, exist_ok=True)

    venv_was_created = not venv_path.exists()
    if not setup_venv(python_bin, venv_path):
        sys.exit(1)

    installed_flag = venv_path / ".installed"
    need_full_install = venv_was_created or not installed_flag.exists()
    if need_full_install:
        # 优先使用 requirements.lock（精确版本锁定），回退到 requirements.txt
        lock_path = script_dir / "requirements.lock"
        if lock_path.exists():
            emit(f"使用精确版本锁文件安装依赖 ({lock_path.name})...")
            if not install_requirements(venv_path, lock_path):
                # lock 安装失败时回退到宽松版本
                emit("锁文件安装失败，回退到 requirements.txt...")
                if not install_requirements(venv_path, req_path):
                    sys.exit(1)
        else:
            emit("未找到 requirements.lock，使用 requirements.txt（版本可能与开发环境不一致）")
            if not install_requirements(venv_path, req_path):
                sys.exit(1)
        installed_flag.write_text("")

    # ⚠️ 每次启动都升级 certifi，解决 macOS 上 Python SSL 证书过旧的问题
    # 见: SSL: UNEXPECTED_EOF_WHILE_READING（GitHub API 连接失败）
    venv_python = venv_path / "bin" / "python"
    emit("正在更新 SSL 证书...")
    subprocess.run(
        [str(venv_python), "-m", "pip", "install", "--upgrade", "certifi", "--quiet"],
        capture_output=True, text=True, timeout=60,
    )

    # ── 每次启动都验证关键依赖 ──────────────────────────────────────
    # 即使 venv 已存在且 .installed 已设置，也须验证，因为包可能在
    # 升级/权限/磁盘问题中丢失（audio-separator 是原生扩展，最易丢）
    emit("正在验证关键依赖...")
    missing = verify_imports(venv_path)
    if missing:
        emit(f"以下包导入失败: {', '.join(missing)}，正尝试安装缺失包...")
        venv_python = venv_path / "bin" / "python"
        pip_name_map = {
            "audio_separator": "audio-separator",
        }
        all_ok = True
        for pkg in missing:
            pip_name = pip_name_map.get(pkg, pkg)
            emit(f"  安装 {pip_name}...")
            result = subprocess.run(
                [str(venv_python), "-m", "pip", "install", pip_name],
                capture_output=True, text=True, timeout=300,
            )
            if result.returncode == 0:
                emit(f"  ✓ {pkg} 安装成功")
            else:
                emit(f"  ✗ {pkg} 安装失败: {result.stderr.strip()}")
                all_ok = False
        if not all_ok:
            # 删除 .installed 标志，下次启动时触发完整重装
            try:
                installed_flag.unlink(missing_ok=True)
            except Exception:
                pass
            emit("部分依赖安装失败，请检查网络后重启应用")
            sys.exit(1)
        emit("缺失依赖安装完成")

    emit("Python 环境配置完成！")
    emit(f"虚拟环境路径: {venv_path}")


if __name__ == "__main__":
    main()
