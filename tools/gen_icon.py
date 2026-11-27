#!/usr/bin/env python3
"""生成 Voxril 品牌图标（零第三方依赖）。

设计：圆角方形 + 对角三色渐变（紫 → 靛 → 青）+ 居中白色首字母 V。
- 大尺寸：圆角方形占画布 80.5%（四周留白），符合 macOS Big Sur 图标规范，
  避免在 Dock / 桌面上视觉尺寸大于系统应用图标。
- 小尺寸：留白收紧、笔画加粗，否则 16px 下 V 会缩成一团灰块。

每个尺寸都按矢量独立重绘（SDF 抗锯齿），而不是从 1024 缩放，
所以 Windows 任务栏 / 开始菜单 / 高 DPI 下都保持锐利。

产出（相对仓库根目录）：
- src-tauri/icons/icon.png   1024x1024 RGBA，用于 dev 窗口 / 非 macOS 打包
- src-tauri/icons/icon.ico   16/24/32/48/64/128/256 多尺寸，Windows exe 与安装包
- src-tauri/icons/icon.icns  macOS 应用与 DMG 图标（16~1024 全尺寸，sips + iconutil）

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
STROKE_RATIO = 0.088          # V 笔画半宽 / 内容边长

# 小尺寸专用版式：留白收紧 + 笔画加粗。阈值按「图标实际显示像素」定，
# 48px 起留白就足够，不需要再收紧。
SMALL_CONTENT_RATIO = 0.94
SMALL_STROKE_RATIO = 0.105
SMALL_SIZE_LIMIT = 47

STOPS = [                     # 对角渐变色标
    (0.00, (124, 58, 237)),   # #7C3AED
    (0.55, (99, 102, 241)),   # #6366F1
    (1.00, (34, 211, 238)),   # #22D3EE
]

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
ICON_PNG = os.path.join(ROOT, "src-tauri", "icons", "icon.png")
ICON_ICNS = os.path.join(ROOT, "src-tauri", "icons", "icon.icns")
ICON_ICO = os.path.join(ROOT, "src-tauri", "icons", "icon.ico")

# ICO 内各尺寸独立渲染；256 用 PNG 内嵌，其余用 BMP 兼容旧 shell
ICO_SIZES = [16, 24, 32, 48, 64, 128, 256]


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


def render(size: int, small=None) -> bytearray:
    # 小尺寸收紧留白、加粗笔画，保证 16px 下 V 仍可辨认。
    # small 默认按画布像素判断；生成 icns 的 @2x 档时会显式按逻辑 pt 传入，
    # 避免同一个逻辑尺寸的 retina 档与普通档版式不一致。
    if small is None:
        small = size <= SMALL_SIZE_LIMIT
    if small:
        content_ratio, stroke_ratio = SMALL_CONTENT_RATIO, SMALL_STROKE_RATIO
    else:
        content_ratio, stroke_ratio = CONTENT_RATIO, STROKE_RATIO

    # 内容（圆角方形）占画布 content_ratio，四周留白，贴合 macOS 图标视觉尺寸
    content = size * content_ratio
    pad = (size - content) / 2.0
    center = size / 2.0
    half = content / 2.0
    radius = CORNER_RATIO * content
    stroke = stroke_ratio * content

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


def bmp_image(size: int, raw: bytes) -> bytes:
    """把 render() 的 RGBA 行数据转成 ICO 用的 32bpp DIB（bottom-up + AND mask）。

    小尺寸用 BMP 而不是内嵌 PNG：资源管理器、快捷方式、部分 shell 组件
    对 24/48px 的内嵌 PNG 支持不齐，BMP 是所有 Windows 版本都认的格式。
    """
    stride = size * 4 + 1  # render() 每行首字节是 PNG filter 标记
    pixels = bytearray()
    for y in range(size - 1, -1, -1):  # DIB 行序自下而上
        row = raw[y * stride + 1:(y + 1) * stride]
        for x in range(size):
            r, g, b, a = row[x * 4:x * 4 + 4]
            pixels += bytes((b, g, r, a))  # BGRA
    # biHeight 为实际高度两倍：XOR（像素）+ AND（透明掩码）两部分
    header = struct.pack("<IiiHHIIiiII", 40, size, size * 2, 1, 32, 0, len(pixels), 0, 0, 0, 0)
    mask_stride = ((size + 31) // 32) * 4  # 1bpp，按 32 位对齐；alpha 已够用，掩码全 0
    return header + bytes(pixels) + bytes(mask_stride * size)


def write_ico(path: str, sizes=ICO_SIZES) -> None:
    """写出多尺寸 ICO：每个尺寸独立矢量渲染，256 用 PNG 内嵌，其余用 BMP。"""
    images = []
    for size in sizes:
        raw = render(size)
        data = png_bytes(size, raw) if size >= 256 else bmp_image(size, raw)
        images.append((size, data))

    header = struct.pack("<HHH", 0, 1, len(images))
    offset = 6 + 16 * len(images)
    entries = bytearray()
    blob = bytearray()
    for size, data in images:
        dim = 0 if size >= 256 else size  # 0 表示 256
        entries += struct.pack("<BBBBHHII", dim, dim, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        blob += data

    with open(path, "wb") as f:
        f.write(header)
        f.write(bytes(entries))
        f.write(bytes(blob))


def make_icns(png_path: str, icns_path: str) -> bool:
    """生成 macOS icns（16~1024 全档）。

    ≤47pt 的小档按紧凑版式矢量重绘：直接缩放 1024 会让留白吃掉一半画幅，
    16/32pt 下的 V 细成一团灰——这正是「图标看着糊」的主因。
    大档用 sips 重采样，从 1024 缩到 128pt 以上足够锐利，
    而纯 Python 逐像素重绘大图耗时不成比例。
    """
    if not (shutil.which("sips") and shutil.which("iconutil")):
        print("[icon] 未找到 sips/iconutil，跳过 icns 生成（仅 macOS 可用）")
        return False

    # (像素边长, 逻辑 pt, iconset 文件名)
    specs = [
        (16, 16, "icon_16x16.png"), (32, 16, "icon_16x16@2x.png"),
        (32, 32, "icon_32x32.png"), (64, 32, "icon_32x32@2x.png"),
        (128, 128, "icon_128x128.png"), (256, 128, "icon_128x128@2x.png"),
        (256, 256, "icon_256x256.png"), (512, 256, "icon_256x256@2x.png"),
        (512, 512, "icon_512x512.png"), (1024, 512, "icon_512x512@2x.png"),
    ]
    tmp = tempfile.mkdtemp(prefix="voxril-iconset-")
    iconset = os.path.join(tmp, "icon.iconset")
    os.makedirs(iconset, exist_ok=True)
    try:
        for px, pt, name in specs:
            out = os.path.join(iconset, name)
            if pt <= SMALL_SIZE_LIMIT:
                write_png(out, px, render(px, small=True))
            else:
                subprocess.run(
                    ["sips", "-z", str(px), str(px), png_path, "--out", out],
                    check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                )
        subprocess.run(
            ["iconutil", "-c", "icns", iconset, "-o", icns_path],
            check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
        )
        return True
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


def write_favicon_svg(path: str) -> None:
    """输出浏览器 favicon（矢量）。

    与 PNG 图标同一套几何参数（渐变、圆角比例、V 位置与笔画），
    唯一区别是满版不留意白——浏览器标签页本身很小，留白会让图形缩得看不清。
    """
    view = 64.0
    center = view / 2.0
    radius = CORNER_RATIO * view
    stroke = STROKE_RATIO * view
    lx, ly = center - 0.255 * view, center - 0.205 * view
    rx, ry = center + 0.255 * view, center - 0.205 * view
    bx, by = center, center + 0.235 * view
    stops = "\n".join(
        '      <stop offset="{:.2f}" stop-color="#{:02X}{:02X}{:02X}"/>'.format(off, *rgb)
        for off, rgb in STOPS
    )
    svg = f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {view:g} {view:g}" fill="none">
  <!-- 由 tools/gen_icon.py 生成，与打包图标同源，请勿手改 -->
  <defs>
    <linearGradient id="vox-g" x1="0" y1="0" x2="{view:g}" y2="{view:g}" gradientUnits="userSpaceOnUse">
{stops}
    </linearGradient>
  </defs>
  <rect width="{view:g}" height="{view:g}" rx="{radius:.2f}" fill="url(#vox-g)"/>
  <path d="M{lx:.2f} {ly:.2f} L{bx:.2f} {by:.2f} L{rx:.2f} {ry:.2f}" stroke="#FFFFFF" stroke-width="{stroke:.2f}" stroke-linecap="round" stroke-linejoin="round"/>
</svg>
"""
    with open(path, "w", encoding="utf-8") as f:
        f.write(svg)


def main() -> None:
    os.makedirs(os.path.dirname(ICON_PNG), exist_ok=True)
    t0 = time.time()
    raw = render(SIZE)
    write_png(ICON_PNG, SIZE, raw)
    print(f"[icon] 已生成 {ICON_PNG} ({SIZE}x{SIZE}) 用时 {time.time() - t0:.1f}s")

    # Windows exe / 安装包图标：多尺寸，避免系统把 256 缩放导致小图标发糊
    write_ico(ICON_ICO)
    print(f"[icon] 已生成 {ICON_ICO} ({'/'.join(str(s) for s in ICO_SIZES)})")

    if make_icns(ICON_PNG, ICON_ICNS):
        print(f"[icon] 已生成 {ICON_ICNS}")

    # 网页 favicon 同源生成，避免手抄参数漂移
    write_favicon_svg(os.path.join(ROOT, "web", "public", "favicon.svg"))
    print("[icon] 已生成 web/public/favicon.svg")


if __name__ == "__main__":
    main()
