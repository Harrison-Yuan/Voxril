#!/usr/bin/env bash
# 拉取内置 Python 运行时（python-build-standalone）。
#
# - macOS arm64 -> src-tauri/bin/python
# - Windows x64 -> src-tauri/bin/windows-x64/python
#
# 运行时体积较大，不进入 git；本地打包与 CI 在构建前执行本脚本即可。
# 版本固定，升级时同时修改 PB_TAG / PY_VERSION。
set -euo pipefail

PB_TAG="${VOXRIL_PYTHON_TAG:-20261124}"
PY_VERSION="${VOXRIL_PYTHON_VERSION:-3.12.16}"

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

HOST_OS="$(uname -s)"
HOST_ARCH="${VOXRIL_PYTHON_ARCH:-$(uname -m)}"
# Rosetta 终端会把 arm64 主机报成 x86_64，按硬件能力纠正
if [ "$HOST_OS" = "Darwin" ] && [ "$HOST_ARCH" = "x86_64" ]; then
  if [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = "1" ]; then
    HOST_ARCH="arm64"
  fi
fi

case "$HOST_OS-$HOST_ARCH" in
  Darwin-arm64|Darwin-aarch64)
    ASSET="cpython-${PY_VERSION}+${PB_TAG}-aarch64-apple-darwin-install_only.tar.gz"
    DEST="$ROOT/src-tauri/bin/python"
    MARKER="$DEST/bin/python3"
    ;;
  MINGW*|MSYS*|CYGWIN*)
    ASSET="cpython-${PY_VERSION}+${PB_TAG}-x86_64-pc-windows-msvc-install_only.tar.gz"
    DEST="$ROOT/src-tauri/bin/windows-x64/python"
    MARKER="$DEST/python.exe"
    ;;
  *)
    echo "unsupported platform: ${HOST_OS}-${HOST_ARCH} (Intel Mac 不受支持)" >&2
    exit 1
    ;;
esac

if [ -x "$MARKER" ] || [ -f "$MARKER" ]; then
  echo "[python] already present: $MARKER"
  "$MARKER" --version || echo "[python] warning: cannot execute on this host (expected on Intel/cross-build machines)" >&2
  exit 0
fi

URL="https://github.com/astral-sh/python-build-standalone/releases/download/${PB_TAG}/${ASSET//+/%2B}"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "[python] downloading ${ASSET}"
curl -fL --retry 3 --retry-delay 2 "$URL" -o "$TMP/python.tar.gz"

mkdir -p "$TMP/out"
tar -xzf "$TMP/python.tar.gz" -C "$TMP/out"

if [ ! -d "$TMP/out/python" ]; then
  echo "[python] archive does not contain python/ directory" >&2
  exit 1
fi

mkdir -p "$(dirname "$DEST")"
rm -rf "$DEST"
mv "$TMP/out/python" "$DEST"

echo "[python] installed to $DEST"
"$MARKER" --version || echo "[python] warning: cannot execute on this host (expected on Intel/cross-build machines)" >&2
