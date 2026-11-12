"""生成设置窗口 banner 用的 logo 像素数据 (settings-logo.bin)。

用法：
  1. 先把 symbol-color.svg 渲染成 42×42 透明 PNG（任意工具，如 Chrome 无头截图）：
     chrome --headless --screenshot=settings-logo.png --window-size=42,42 symbol-color.svg
  2. python make_settings_logo.py settings-logo.png

输出格式（源码 settings.rs 的 LOGO_BIN 直接 include_bytes 使用）：
  42×42，逐行自底向上，每像素 4 字节预乘 BGRA（BYTE B,G,R,A）。
  运行时 CreateDIBSection(42, 42, 32bpp BI_RGB, bottom-up) 后整块 memcpy 即可，
  GDI AlphaBlend(AC_SRC_OVER) 直接带 alpha 合成，无需解码器。
"""
import struct, sys
from PIL import Image

SIZE = 42

def png_to_bgra_bin(png_path: str, out_path: str) -> None:
    img = Image.open(png_path).convert('RGBA')
    if img.size != (SIZE, SIZE):
        img = img.resize((SIZE, SIZE), Image.LANCZOS)
    px = img.load()
    rows = []
    for y in range(SIZE - 1, -1, -1):  # DIB 正高度 = 自底向上
        row = bytearray()
        for x in range(SIZE):
            r, g, b, a = px[x, y]
            # 预乘 alpha（AlphaBlend AC_SRC_ALPHA 要求），COLORREF 风格 BGRA 字节序
            row += bytes(((b * a) // 255, (g * a) // 255, (r * a) // 255, a))
        rows.append(bytes(row))
    data = b''.join(rows)
    with open(out_path, 'wb') as f:
        f.write(data)
    print(f'wrote {out_path}: {SIZE}x{SIZE} premultiplied BGRA bottom-up, {len(data)} bytes')

if __name__ == '__main__':
    png_to_bgra_bin(sys.argv[1], sys.argv[2] if len(sys.argv) > 2 else 'settings-logo.bin')
