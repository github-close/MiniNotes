"""生成笔记列表行尾「置顶图钉」像素资源 (pin-light.bin / pin-dark.bin / pin-yellow.bin)。

图标来源：Windows 系统自带图标字体 Segoe MDL2 Assets 的 Pin 字形 (U+E718)，
逆时针旋转 90° 使针尖朝下（系统字形横向朝左），不用手绘矢量。
颜色烘焙三份，分别对应 theme.rs 三套色板的 accent 常量：
  LIGHT.accent  = rgb(0x34, 0x78, 0xF6)
  DARK.accent   = rgb(0x6B, 0x9F, 0xFF)
  YELLOW.accent = rgb(0xE0, 0x9A, 0x00)
（theme.rs 改色时需重跑本脚本）

用法：python make_pin_icon.py
输出：
  pin-light.bin / pin-dark.bin / pin-yellow.bin — 12×16，逐行自底向上，
  每像素 4 字节预乘 BGRA，源码 notes.rs 的 PIN_*_BIN 直接 include_bytes 使用，
  运行时 CreateDIBSection + AlphaBlend(AC_SRC_OVER) 合成，无解码器。
  pin-preview-*.png — 实际 1x 效果预览（放大图）。
"""
import struct
from PIL import Image, ImageFont, ImageDraw

BOX_W, BOX_H = 12, 16          # 位图尺寸（含 1px 边距）
GLYPH_H = 14                   # 字形目标高度（宽度按比例）
LIGHT_BG, LIGHT_ACCENT = (255, 255, 255), (0x34, 0x78, 0xF6)
DARK_BG, DARK_ACCENT = (0x1E, 0x1F, 0x23), (0x6B, 0x9F, 0xFF)
YELLOW_BG, YELLOW_ACCENT = (0xFF, 0xF6, 0xD8), (0xE0, 0x9A, 0x00)

def glyph_alpha() -> Image.Image:
    """E718 字形的灰度 alpha 遮罩：高分辨率渲染 → 旋转 → 裁切 → 缩到目标高"""
    font = ImageFont.truetype(r"C:\Windows\Fonts\SegMDL2.ttf", 128)
    big = Image.new("L", (256, 256), 0)
    d = ImageDraw.Draw(big)
    d.text((128, 128), "\uE718", font=font, fill=255, anchor="mm")
    big = big.rotate(90, expand=True)  # 逆时针 90°：针尖朝左 → 朝下
    bbox = big.getbbox()
    glyph = big.crop(bbox)
    w = round(glyph.width * GLYPH_H / glyph.height)
    return glyph.resize((w, GLYPH_H), Image.LANCZOS)

def bake(alpha_img, accent):
    """灰度遮罩 → 指定强调色 → 预乘 BGRA 自底向上字节流（settings-logo.bin 同格式）"""
    out = Image.new("RGBA", (BOX_W, BOX_H), (0, 0, 0, 0))
    ox, oy = (BOX_W - alpha_img.width) // 2, (BOX_H - alpha_img.height) // 2
    px, src = out.load(), alpha_img.load()
    for y in range(alpha_img.height):
        for x in range(alpha_img.width):
            a = src[x, y]
            px[ox + x, oy + y] = (*accent, a)
    rows = []
    for y in range(BOX_H - 1, -1, -1):  # DIB 正高度 = 自底向上
        row = bytearray()
        for x in range(BOX_W):
            r, g, b, a = px[x, y]
            row += bytes(((b * a) // 255, (g * a) // 255, (r * a) // 255, a))
        rows.append(bytes(row))
    return b"".join(rows)

def compose(preview_path_alpha: Image.Image, accent, bg, path: str, zoom_img=None):
    img = Image.new("RGB", (BOX_W, BOX_H), bg)
    px = img.load()
    for y in range(BOX_H):
        for x in range(BOX_W):
            a = preview_path_alpha.getpixel((x, y))
            r, g, b = accent
            px[x, y] = tuple((c * a + bg[i] * (255 - a)) // 255 for i, c in enumerate((r, g, b)))
    img.save(path)
    return img

alpha = glyph_alpha()
light = bake(alpha, LIGHT_ACCENT)
dark = bake(alpha, DARK_ACCENT)
yellow = bake(alpha, YELLOW_ACCENT)
for name, data in (("pin-light.bin", light), ("pin-dark.bin", dark), ("pin-yellow.bin", yellow)):
    with open(name, "wb") as f:
        f.write(data)
    print(f"wrote {name}: {BOX_W}x{BOX_H} premultiplied BGRA bottom-up, {len(data)} bytes")

# 预览：左浅色右暗色 1x 原尺寸，下方 8 倍放大
def debin(data: bytes) -> Image.Image:
    img = Image.new("RGBA", (BOX_W, BOX_H))
    px = img.load()
    i = 0
    for y in range(BOX_H - 1, -1, -1):
        for x in range(BOX_W):
            b, g, r, a = data[i:i + 4]; i += 4
            px[x, y] = (r, g, b, a)
    return img

la, da, ya = debin(light), debin(dark), debin(yellow)
strip = Image.new("RGB", (BOX_W * 4, BOX_H), LIGHT_BG)
strip.paste(la, (BOX_W, 0), la)
strip.save("pin-preview-1x.png")
zoom_w, zoom_h = BOX_W * 10, BOX_H * 10
zoom = Image.new("RGB", (zoom_w * 3 + 80, zoom_h), (24, 24, 24))
zoom.paste(la.resize((zoom_w, zoom_h), Image.NEAREST), (0, 0), la.resize((zoom_w, zoom_h), Image.NEAREST))
zoom.paste(da.resize((zoom_w, zoom_h), Image.NEAREST), (zoom_w + 40, 0), da.resize((zoom_w, zoom_h), Image.NEAREST))
zoom.paste(ya.resize((zoom_w, zoom_h), Image.NEAREST), (zoom_w * 2 + 80, 0), ya.resize((zoom_w, zoom_h), Image.NEAREST))
zoom.save("pin-preview-zoom.png")
print("wrote pin-preview-1x.png / pin-preview-zoom.png")
