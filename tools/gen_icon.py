#!/usr/bin/env python3
"""生成 Voxril 品牌图标（零第三方依赖）。

设计：圆角方形 + 对角三色渐变（紫 → 靛 → 青）+ 居中白色首字母 V。
圆角方形只占画布 80.5%（四周留白），符合 macOS Big Sur 图标规范，
避免在 Dock / 桌面上视觉尺寸大于系统应用图标。

产出（相对仓库根目录）：
- src-tauri/icons/icon.png   1024x1024 RGBA，用于 dev 窗口 / 非 macOS 打包
- src-tauri/icons/icon.icns  macOS 应用与 DMG 图标（通过 sips + iconutil 生成）

可重复执行，直接覆盖旧图标。用法：python3 tools/gen_icon.py
"""
import math
import os
import shutil
import struct
import subprocess
import tempfile
import time
import zlib

SIZE = 1024
CONTENT_RATIO = 0.805         # 内容（圆角方形）边长 / 画布边长，四周留白
CORNER_RATIO = 0.2255         # 圆角半径 / 内容边长
STOPS = [                     # 对角渐变色标
    (0.00, (124, 58, 237)),   # #7C3AED
    (0.55, (99, 102, 241)),   # #6366F1
    (1.00, (34, 211, 238)),   # #22D3EE
]

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON_PNG = os.path.join(ROOT, "src-tauri", "icons", "icon.png")
ICON_ICNS = os.path.join(ROOT, "src-tauri", "icons", "icon.icns")
ICON_ICO = os.path.join(ROOT, "src-tauri", "icons", "icon.ico")


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def gradient(t: float):
    t = 0.0 if t < 0 else 1.0 if t > 1 else t
    for i in range(len(STOPS) - 1):
        t0, c0 = STOPS[i]
        t1, c1 = STOPS[i + 1]
        if t <= t1:
            u = 0.0 if t1 <= t0 else (t - t0) / (t1 - t0)
            return (lerp(c0[0], c1[0], u), lerp(c0[1], c1[1], u), lerp(c0[2], c1[2], u))
    return STOPS[-1][1]


def rounded_rect_sdf(px: float, py: float, half: float, r: float) -> float:
    qx = abs(px) - half + r
    qy = abs(py) - half + r
    ax = qx if qx > 0 else 0.0
    ay = qy if qy > 0 else 0.0
    return math.sqrt(ax * ax + ay * ay) + min(max(qx, qy), 0.0) - r


def segment_sdf(px: float, py: float, ax: float, ay: float, bx: float, by: float) -> float:
    vx = bx - ax
    vy = by - ay
    denom = vx * vx + vy * vy
    if denom == 0.0:
        t = 0.0
    else:
        t = ((px - ax) * vx + (py - ay) * vy) / denom
        t = 0.0 if t < 0 else 1.0 if t > 1 else t
    dx = px - (ax + vx * t)
    dy = py - (ay + vy * t)
    return math.sqrt(dx * dx + dy * dy)


def render(size: int) -> bytearray:
    # 内容（圆角方形）只占画布 80.5%，四周留白，贴合 macOS 图标视觉尺寸
    content = size * CONTENT_RATIO
    pad = (size - content) / 2.0
    center = size / 2.0
    half = content / 2.0
    radius = CORNER_RATIO * content
    stroke = 0.088 * content

    # V 字的三个顶点（以图像中心为原点，y 轴向下）
    left = (-0.255 * content, -0.205 * content)
    right = (0.255 * content, -0.205 * content)
    bottom = (0.0, 0.235 * content)

    # 顶部柔光中心
    lx, ly = 0.0, -0.30 * content
    light_r = 0.95 * content

    rows = bytearray()
    px_scale = 1.0 / (2.0 * (content - 1))
    inv_content = 1.0 / content

    for y in range(size):
        rows.append(0)  # PNG filter type: None
        cy = y + 0.5 - center
        yn = ((y + 0.5) - pad) * inv_content
        yn = 0.0 if yn < 0.0 else 1.0 if yn > 1.0 else yn
        for x in range(size):
            cx = x + 0.5 - center

            sd = rounded_rect_sdf(cx, cy, half - 0.5, radius)
            acov = 0.5 - sd
            if acov <= 0.0:
                rows.extend((0, 0, 0, 0))
                continue
            if acov > 1.0:
                acov = 1.0

            # 对角渐变（按内容区域归一化）
            r, g, b = gradient((x - pad + y - pad) * px_scale)

            # 左上柔光（提升高级质感）
            dx = cx - lx
            dy = cy - ly
            light = 1.0 - math.sqrt(dx * dx + dy * dy) / light_r
            if light > 0.0:
                light = light * light * 0.16
                r += (255.0 - r) * light
                g += (255.0 - g) * light
                b += (255.0 - b) * light

            # V 字形（两条圆头线段）
            d = segment_sdf(cx, cy, left[0], left[1], bottom[0], bottom[1])
            d2 = segment_sdf(cx, cy, right[0], right[1], bottom[0], bottom[1])
            if d2 < d:
                d = d2
            vcov = 0.5 + stroke - d
            if vcov > 0.0:
                if vcov > 1.0:
                    vcov = 1.0
                # 字母白色 → 底部略偏淡紫，避免死白
                vr = lerp(255.0, 226.0, yn)
                vg = lerp(255.0, 232.0, yn)
                vb = 255.0
                r = r * (1.0 - vcov) + vr * vcov
                g = g * (1.0 - vcov) + vg * vcov
                b = b * (1.0 - vcov) + vb * vcov

            rows.extend((
                int(r + 0.5) if r < 255 else 255,
                int(g + 0.5) if g < 255 else 255,
                int(b + 0.5) if b < 255 else 255,
                int(acov * 255.0 + 0.5),
            ))
    return rows


def png_bytes(size: int, raw: bytes) -> bytes:
    def chunk(tag: bytes, data: bytes) -> bytes:
        return (struct.pack(">I", len(data)) + tag + data
                + struct.pack(">I", zlib.crc32(tag + data) & 0xFFFFFFFF))

    ihdr = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (b"\x89PNG\r\n\x1a\n"
            + chunk(b"IHDR", ihdr)
            + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
            + chunk(b"IEND", b""))


def write_png(path: str, size: int, raw: bytes) -> None:
    with open(path, "wb") as f:
        f.write(png_bytes(size, raw))


def write_ico(path: str, size: int, png: bytes) -> None:
    """写出单尺寸 ICO（内嵌 PNG，Windows Vista+ 支持）。"""
    dim = 0 if size >= 256 else size  # 0 表示 256
    header = struct.pack("<HHH", 0, 1, 1)
    entry = struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(png), 22)
    with open(path, "wb") as f:
        f.write(header)
        f.write(entry)
        f.write(png)


def make_icns(png_path: str, icns_path: str) -> bool:
    """用 macOS 自带 sips + iconutil 由 1024 PNG 生成 icns。"""
    if not (shutil.which("sips") and shutil.which("iconutil")):
        print("[icon] 未找到 sips/iconutil，跳过 icns 生成（仅 macOS 可用）")
        return False

    specs = [
        (16, "icon_16x16.png"), (32, "icon_16x16@2x.png"),
        (32, "icon_32x32.png"), (64, "icon_32x32@2x.png"),
        (128, "icon_128x128.png"), (256, "icon_128x128@2x.png"),
        (256, "icon_256x256.png"), (512, "icon_256x256@2x.png"),
        (512, "icon_512x512.png"), (1024, "icon_512x512@2x.png"),
    ]
    tmp = tempfile.mkdtemp(prefix="voxril-iconset-")
    iconset = os.path.join(tmp, "icon.iconset")
    os.makedirs(iconset, exist_ok=True)
    try:
        for px, name in specs:
            subprocess.run(
                ["sips", "-z", str(px), str(px), png_path, "--out", os.path.join(iconset, name)],
                check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
            )
        subprocess.run(
            ["iconutil", "-c", "icns", iconset, "-o", icns_path],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        return True
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def main() -> None:
    os.makedirs(os.path.dirname(ICON_PNG), exist_ok=True)
    t0 = time.time()
    raw = render(SIZE)
    write_png(ICON_PNG, SIZE, raw)
    print(f"[icon] 已生成 {ICON_PNG} ({SIZE}x{SIZE}) 用时 {time.time() - t0:.1f}s")

    # Windows 打包需要多分辨率 ICO（这里输出 256x256，足够 NSIS/任务栏使用）
    ico_size = 256
    write_ico(ICON_ICO, ico_size, png_bytes(ico_size, render(ico_size)))
    print(f"[icon] 已生成 {ICON_ICO} ({ico_size}x{ico_size})")

    if make_icns(ICON_PNG, ICON_ICNS):
        print(f"[icon] 已生成 {ICON_ICNS}")


if __name__ == "__main__":
    main()
