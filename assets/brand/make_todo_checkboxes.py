"""生成待办列表勾选框图标资源 (todo-icons-light.bin / todo-icons-dark.bin)。

图标在 4 倍超采样画布上以几何原语绘制（正圆 + 折线），LANCZOS 降采样得到
平滑抗锯齿边缘，替代旧的逐像素距离数学烘焙（无抗锯齿、边缘毛糙）。
底色为全透明（预乘 alpha=0）：消费方 todos.rs 用 AlphaBlend(AC_SRC_ALPHA)
按格贴图，选中行的胶囊底色可以从图标四周透出来；笔记列表同 bin 只用于
imagelist 撑行高、内容从不渲染，透明与否无影响。
颜色取自 theme.rs 两套色板常量（改色后需重跑本脚本）：
  LIGHT: text_disabled=A3AAB3 accent=3478F6 on_accent=FFFFFF
  DARK : text_disabled=60656B accent=6B9FFF on_accent=10141A

输出格式（window.rs make_checkbox_imagelist / todos.rs 直接 include_bytes）：
  72×26 图标条（3 格横排：0=全透明占位，1=未完成圆环，2=完成对勾），
  自底向上，每像素 4 字节预乘 BGRA（BYTE B,G,R,A）。
  todos.rs：CreateDIBSection(72,26) 整块 memcpy 后按格 AlphaBlend；
  window.rs：同尺寸 DIB + ImageList_Add（仅撑行高）。
"""
from PIL import Image, ImageDraw

CELL_W, CELL_H, CX, CY = 24, 26, 12, 13
SS = 4  # 超采样倍率

PALETTES = {
    "light": {
        "bg": (255, 255, 255),
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
    return Image.new("RGBA", (CELL_W * SS, CELL_H * SS), (0, 0, 0, 0))

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
        fill=s["fill"] + (255,),
    )
    pts = [(x * SS, y * SS) for x, y in CHECK_PTS]
    d.line(pts, fill=s["check"] + (255,), width=round(CHECK_W * SS), joint="curve")
    for p in (pts[0], pts[-1]):  # PIL 无圆头线帽，端点补小圆
        r = CHECK_W * SS / 2
        d.ellipse([p[0] - r, p[1] - r, p[0] + r, p[1] + r], fill=s["check"] + (255,))
    return img


def bake_strip(s: dict) -> bytes:
    """3 格合成 72×26 自底向上预乘 BGRA 字节流（底透明）"""
    strip = Image.new("RGBA", (CELL_W * 3 * SS, CELL_H * SS), (0, 0, 0, 0))
    for i, cell in enumerate((cell_blank(s), cell_ring(s), cell_done(s))):
        strip.paste(cell.resize((CELL_W * SS, CELL_H * SS), Image.LANCZOS), (CELL_W * SS * i, 0))
    strip = strip.resize((CELL_W * 3, CELL_H), Image.LANCZOS)
    px = strip.load()
    rows = []
    for y in range(CELL_H - 1, -1, -1):  # DIB 正高度 = 自底向上
        row = bytearray()
        for x in range(CELL_W * 3):
            r, g, b, a = px[x, y]
            row += bytes(((b * a) // 255, (g * a) // 255, (r * a) // 255, a))
        rows.append(bytes(row))
    return b"".join(rows)


BG_OF = {"light": (0xFF, 0xFF, 0xFF), "dark": (0x1E, 0x1F, 0x23)}  # 预览底=window_bg

for name, s in PALETTES.items():
    data = bake_strip(s)
    path = f"todo-icons-{name}.bin"
    with open(path, "wb") as f:
        f.write(data)
    print(f"wrote {path}: 72x26 (3 cells 24x26) bottom-up premultiplied BGRA, {len(data)} bytes")

# 预览：两主题各一行 [占位 | 未完成 | 完成] 合成到各自 window_bg 上，1x 与 8x
def load_strip(path: str) -> Image.Image:
    with open(path, "rb") as f:
        data = f.read()
    img = Image.new("RGBA", (CELL_W * 3, CELL_H))
    px = img.load()
    i = 0
    for y in range(CELL_H - 1, -1, -1):
        for x in range(CELL_W * 3):
            b, g, r, a = data[i:i + 4]; i += 4
            px[x, y] = (r, g, b, a)
    return img

prev = Image.new("RGB", (CELL_W * 6 + 6, CELL_H), (90, 90, 90))
strips = {}
for i, name in enumerate(("light", "dark")):
    strip = load_strip(f"todo-icons-{name}.bin")
    strips[name] = strip
    bg = Image.new("RGB", (CELL_W * 3, CELL_H), BG_OF[name])
    bg.paste(strip, (0, 0), strip)
    prev.paste(bg, (i * (CELL_W * 3 + 6), 0))
prev.save("todo-preview-1x.png")

Z = 8
zoom = Image.new("RGB", ((CELL_W * 3) * Z * 2 + 24, CELL_H * Z), (24, 24, 24))
for i, name in enumerate(("light", "dark")):
    strip = strips[name]
    big = strip.resize((CELL_W * 3 * Z, CELL_H * Z), Image.NEAREST)
    bg = Image.new("RGB", (CELL_W * 3 * Z, CELL_H * Z), BG_OF[name])
    bg.paste(big, (0, 0), big)
    zoom.paste(bg, (i * ((CELL_W * 3) * Z + 24), 0))
zoom.save("todo-preview-zoom.png")
print("wrote todo-preview-1x.png / todo-preview-zoom.png")
