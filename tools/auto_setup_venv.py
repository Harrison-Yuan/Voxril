"""
首次启动时自动创建 Python 虚拟环境并安装依赖。
用户无需手动配置 Python 环境，开箱即用。
每行以 [SETUP] 标记输出，前端通过 Tauri 事件流捕获。
"""
import argparse
import os
import shutil
import sys
import subprocess
import platform
from pathlib import Path
from urllib.request import Request, urlopen

VERSION_REQUIRED = (3, 10)

# ── pip 源：auto | official | tuna | aliyun | custom ─────────────────
_parser = argparse.ArgumentParser(add_help=False)
_parser.add_argument("--python", default="")
_parser.add_argument("--pip-source", default="auto")
_parser.add_argument("--pip-index", default="")
_known, _unknown = _parser.parse_known_args()

PYTHON_OVERRIDE = (_known.python or "").strip()
PIP_SOURCE = (_known.pip_source or "auto").strip().lower()
PIP_INDEX_OVERRIDE = (_known.pip_index or "").strip()

OFFICIAL_PYPI = None  # None 表示 pip 默认源
TUNA_PYPI = "https://pypi.tuna.tsinghua.edu.cn/simple"
ALIYUN_PYPI = "https://mirrors.aliyun.com/pypi/simple/"

_pypi_probe = {}


def emit(msg):
    print(f"[SETUP] {msg}")
    sys.stdout.flush()


def _pypi_reachable(timeout=4):
    """探测官方 PyPI 是否可达，结果缓存。"""
    if "ok" in _pypi_probe:
        return _pypi_probe["ok"]
    ok = False
    try:
        req = Request("https://pypi.org/simple/pip/", headers={"User-Agent": "Mozilla/5.0"})
        with urlopen(req, timeout=timeout) as response:
            response.read(1)
            ok = True
    except Exception:
        ok = False
    _pypi_probe["ok"] = ok
    return ok


def pip_indexes():
    """按优先级返回 pip 源列表；None 表示官方默认源。"""
    if PIP_SOURCE == "official":
        return [OFFICIAL_PYPI]
    if PIP_SOURCE == "tuna":
        return [TUNA_PYPI, OFFICIAL_PYPI]
    if PIP_SOURCE == "aliyun":
        return [ALIYUN_PYPI, OFFICIAL_PYPI]
    if PIP_SOURCE == "custom" and PIP_INDEX_OVERRIDE:
        return [PIP_INDEX_OVERRIDE, OFFICIAL_PYPI]
    # auto：官方可达则先官方，否则先用清华镜像
    return [OFFICIAL_PYPI, TUNA_PYPI] if _pypi_reachable() else [TUNA_PYPI, OFFICIAL_PYPI]


def index_label(index_url):
    return "官方 PyPI" if index_url is None else index_url


def pip_install_cmd(python_bin, args, index_url):
    cmd = [str(python_bin), "-m", "pip", "install"] + args
    if index_url:
        cmd += ["--index-url", index_url]
    return cmd


def get_app_data_dir():
    home = Path.home()
    if platform.system() == "Darwin":
        return home / "Library" / "Application Support" / "com.rvc.webui"
    if platform.system() == "Windows":
        base = os.environ.get("APPDATA")
        if base:
            return Path(base) / "com.rvc.webui"
        return home / "AppData" / "Roaming" / "com.rvc.webui"
    return home / ".rvc-webui"


def venv_python_path(venv_path: Path) -> Path:
    """venv 内 Python 可执行文件路径（Windows 为 Scripts/python.exe）。"""
    if platform.system() == "Windows":
        return venv_path / "Scripts" / "python.exe"
    return venv_path / "bin" / "python"


def find_python(explicit=None):
    """Find the best Python 3.10+ interpreter.
    Priority: explicit (bundled runtime) → current interpreter → platform installs → existing venv.
    """
    candidates = []
    if explicit:
        candidates.append(Path(explicit))
    # 当前解释器优先：打包后即内置运行时，开发时可能是项目 .venv
    candidates.append(Path(sys.executable))
    if platform.system() == "Windows":
        local = os.environ.get("LOCALAPPDATA")
        if local:
            for ver in ["312", "311", "310"]:
                candidates.append(
                    Path(local) / "Programs" / "Python" / f"Python{ver}" / "python.exe"
                )
        for ver in ["312", "311", "310"]:
            candidates.append(Path(f"C:/Python{ver}/python.exe"))
        # py launcher（若存在）
        py_launcher = shutil.which("py")
        if py_launcher:
            for ver in ["3.12", "3.11", "3.10"]:
                try:
                    probe = subprocess.run(
                        [py_launcher, f"-{ver}", "-c", "import sys; print(sys.executable)"],
                        capture_output=True, text=True, timeout=10,
                    )
                    if probe.returncode == 0 and probe.stdout.strip():
                        candidates.append(Path(probe.stdout.strip()))
                except Exception:
                    pass
        candidates.append(venv_python_path(get_app_data_dir() / "venv"))
    else:
        # Homebrew paths
        for ver in ["3.12", "3.11", "3.10"]:
            for base in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/local/Homebrew/bin"]:
                candidates.append(Path(base) / f"python{ver}")
        # Existing app venv
        candidates.append(venv_python_path(get_app_data_dir() / "venv"))

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
        existing = venv_python_path(venv_path)
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
    venv_python = venv_python_path(venv_path)
    emit(f"虚拟环境已创建: {venv_python}")
    return True


def install_requirements(venv_path: Path, req_path: Path):
    python_bin = venv_python_path(venv_path)
    indexes = pip_indexes()

    emit("pip 源: " + " → ".join(index_label(i) for i in indexes))
    emit("正在升级 pip...")
    for index_url in indexes:
        result = subprocess.run(
            pip_install_cmd(python_bin, ["--upgrade", "pip", "--quiet"], index_url),
            capture_output=True, text=True, timeout=120,
        )
        if result.returncode == 0:
            break

    emit("正在安装 Python 依赖 (首次安装可能需要 5-15 分钟)...")
    for index_url in indexes:
        emit(f"使用 {index_label(index_url)} 安装依赖...")
        process = subprocess.Popen(
            pip_install_cmd(
                python_bin,
                ["-r", str(req_path), "--progress-bar", "on"],
                index_url,
            ),
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
            text=True, bufsize=1,
        )

        for line in process.stdout:
            line = line.strip()
            if line:
                emit(line)

        process.wait()
        if process.returncode == 0:
            emit("Python 依赖安装完成")
            return True
        emit(f"{index_label(index_url)} 安装失败，尝试下一个源...")

    emit("依赖安装出现错误，请检查网络连接后重启应用")
    return False


def verify_imports(venv_path: Path):
    python_bin = venv_python_path(venv_path)
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

    python_bin = find_python(PYTHON_OVERRIDE)
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
        # Windows 的 requirements.lock 含 macOS 专用轮子（mlx 等），直接使用 requirements.txt
        lock_path = script_dir / "requirements.lock"
        if platform.system() != "Windows" and lock_path.exists():
            emit(f"使用精确版本锁文件安装依赖 ({lock_path.name})...")
            if not install_requirements(venv_path, lock_path):
                # lock 安装失败时回退到宽松版本
                emit("锁文件安装失败，回退到 requirements.txt...")
                if not install_requirements(venv_path, req_path):
                    sys.exit(1)
        else:
            emit("使用 requirements.txt 安装依赖（含平台条件标记）...")
            if not install_requirements(venv_path, req_path):
                sys.exit(1)
        installed_flag.write_text("")

    # ⚠️ 每次启动都升级 certifi，解决 Python SSL 证书过旧的问题
    # 见: SSL: UNEXPECTED_EOF_WHILE_READING（GitHub API 连接失败）
    venv_python = venv_python_path(venv_path)
    emit("正在更新 SSL 证书...")
    for index_url in pip_indexes():
        result = subprocess.run(
            pip_install_cmd(venv_python, ["--upgrade", "certifi", "--quiet"], index_url),
            capture_output=True, text=True, timeout=60,
        )
        if result.returncode == 0:
            break

    # ── 每次启动都验证关键依赖 ──────────────────────────────────────
    # 即使 venv 已存在且 .installed 已设置，也须验证，因为包可能在
    # 升级/权限/磁盘问题中丢失（audio-separator 是原生扩展，最易丢）
    emit("正在验证关键依赖...")
    missing = verify_imports(venv_path)
    if missing:
        emit(f"以下包导入失败: {', '.join(missing)}，正尝试安装缺失包...")
        venv_python = venv_python_path(venv_path)
        pip_name_map = {
            "audio_separator": "audio-separator",
        }
        all_ok = True
        for pkg in missing:
            pip_name = pip_name_map.get(pkg, pkg)
            emit(f"  安装 {pip_name}...")
            installed = False
            last_err = ""
            for index_url in pip_indexes():
                result = subprocess.run(
                    pip_install_cmd(venv_python, [pip_name], index_url),
                    capture_output=True, text=True, timeout=300,
                )
                if result.returncode == 0:
                    installed = True
                    break
                last_err = result.stderr.strip()
                emit(f"  {index_label(index_url)} 安装失败，尝试下一个源...")
            if installed:
                emit(f"  ✓ {pkg} 安装成功")
            else:
                emit(f"  ✗ {pkg} 安装失败: {last_err}")
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
