"""生成待办列表勾选框图标资源 (todo-icons-light.bin / todo-icons-dark.bin)。

图标在 4 倍超采样画布上以几何原语绘制（正圆 + 折线），LANCZOS 降采样得到
平滑抗锯齿边缘，替代旧的逐像素距离数学烘焙（无抗锯齿、边缘毛糙）。
颜色取自 theme.rs 两套色板常量（改色后需重跑本脚本）：
  LIGHT: window_bg=FFFFFF text_disabled=A3AAB3 accent=3478F6 on_accent=FFFFFF
  DARK : window_bg=1E1F23 text_disabled=60656B accent=6B9FFF on_accent=10141A

输出格式（window.rs make_checkbox_imagelist 直接 include_bytes 使用）：
  72×26 图标条（3 格横排：0=纯行底色，1=未完成圆环，2=完成对勾），
  自底向上，每像素 4 字节预乘 BGRA（BYTE B,G,R,A）。
  运行时 CreateDIBSection(72,26) 整块 memcpy 后 ImageList_Add 一次入列，
  ImageList 按 cx=24 自动切成 3 张图。图标底不透明=行背景色（ImageList
  不支持 alpha，见架构说明），仅格 1/2 有图形、其余为纯底色。
"""
from PIL import Image, ImageDraw

CELL_W, CELL_H, CX, CY = 24, 26, 12, 13
SS = 4  # 超采样倍率

PALETTES = {
    "light": {
        "bg": (0xFF, 0xFF, 0xFF),
        "ring": (0xA3, 0xAA, 0xB3),   # text_disabled
        "fill": (0x34, 0x78, 0xF6),   # accent
        "check": (0xFF, 0xFF, 0xFF),  # on_accent
    },
    "dark": {
        "bg": (0x1E, 0x1F, 0x23),
        "ring": (0x60, 0x65, 0x6B),
        "fill": (0x6B, 0x9F, 0xFF),
        "check": (0x10, 0x14, 0x1A),
    },
}

RING_R, RING_W = 7.0, 1.4     # 未完成：圆环外径半径 / 描边宽
DISC_R, CHECK_W = 7.5, 1.9    # 完成：实心圆半径 / 对勾笔画宽
# 对勾折线端点（1x 单元坐标）
CHECK_PTS = [(8.5, 13.3), (11.0, 15.9), (15.5, 9.3)]


def cell_blank(s: dict) -> Image.Image:
    img = Image.new("RGB", (CELL_W * SS, CELL_H * SS), s["bg"])
    return img

def cell_ring(s: dict) -> Image.Image:
    img = cell_blank(s)
    d = ImageDraw.Draw(img)
    d.ellipse(
        [(CX - RING_R) * SS, (CY - RING_R) * SS, (CX + RING_R) * SS, (CY + RING_R) * SS],
        outline=s["ring"], width=round(RING_W * SS),
    )
    return img

def cell_done(s: dict) -> Image.Image:
    img = cell_blank(s)
    d = ImageDraw.Draw(img)
    d.ellipse(
        [(CX - DISC_R) * SS, (CY - DISC_R) * SS, (CX + DISC_R) * SS, (CY + DISC_R) * SS],
        fill=s["fill"],
    )
    pts = [(x * SS, y * SS) for x, y in CHECK_PTS]
    d.line(pts, fill=s["check"], width=round(CHECK_W * SS), joint="curve")
    for p in (pts[0], pts[-1]):  # PIL 无圆头线帽，端点补小圆
        r = CHECK_W * SS / 2
        d.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=s["check"])
    return img


def bake_strip(s: dict) -> bytes:
    """3 格合成 72×26 自底向上预乘 BGRA 字节流"""
    strip = Image.new("RGB", (CELL_W * 3 * SS, CELL_H * SS))
    for i, cell in enumerate((cell_blank(s), cell_ring(s), cell_done(s))):
        strip.paste(cell.resize((CELL_W * SS, CELL_H * SS), Image.LANCZOS), (CELL_W * SS * i, 0))
    strip = strip.resize((CELL_W * 3, CELL_H), Image.LANCZOS)
    px = strip.load()
    rows = []
    for y in range(CELL_H - 1, -1, -1):  # DIB 正高度 = 自底向上
        row = bytearray()
        for x in range(CELL_W * 3):
            r, g, b = px[x, y]
            row += bytes((b, g, r, 255))  # 不透明，预乘后 RGB 不变
        rows.append(bytes(row))
    return b"".join(rows)


for name, s in PALETTES.items():
    data = bake_strip(s)
    path = f"todo-icons-{name}.bin"
    with open(path, "wb") as f:
        f.write(data)
    print(f"wrote {path}: 72x26 (3 cells 24x26) bottom-up BGRA, {len(data)} bytes")

# 预览：两主题各一行 [底色 | 未完成 | 完成]，1x 与 8x
def load_strip(path: str) -> Image.Image:
    with open(path, "rb") as f:
        data = f.read()
    img = Image.new("RGB", (CELL_W * 3, CELL_H))
    px = img.load()
    i = 0
    for y in range(CELL_H - 1, -1, -1):
        for x in range(CELL_W * 3):
            b, g, r, a = data[i:i + 4]; i += 4
            px[x, y] = (r, g, b)
    return img

row_h = CELL_H * 2 + 6
prev = Image.new("RGB", (CELL_W * 6 + 6, row_h), (90, 90, 90))
for i, name in enumerate(("light", "dark")):
    strip = load_strip(f"todo-icons-{name}.bin")
    prev.paste(strip, (i * (CELL_W * 3 + 6), CELL_H // 2))
prev.save("todo-preview-1x.png")

Z = 8
zoom = Image.new("RGB", ((CELL_W * 3) * Z * 2 + 24, CELL_H * Z), (24, 24, 24))
for i, name in enumerate(("light", "dark")):
    strip = load_strip(f"todo-icons-{name}.bin")
    z = strip.resize((CELL_W * 3 * Z, CELL_H * Z), Image.NEAREST)
    zoom.paste(z, (i * ((CELL_W * 3) * Z + 24), 0))
zoom.save("todo-preview-zoom.png")
print("wrote todo-preview-1x.png / todo-preview-zoom.png")
