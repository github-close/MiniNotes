//! 设计系统：浅色/暗色双主题色板、GDI+ 圆角绘制、扁平按钮绘制。
//!
//! 设计原则（贯穿所有界面）：
//! - 一套强调色 + 中性灰阶，克制用色；选中/主操作用强调色，其余一律中性
//! - 层次靠背景深浅和 1px 细线表达，不用立体边框
//! - 完成态降级（置灰 + 删除线），不抢视觉

use windows_sys::Win32::Foundation::{COLORREF, RECT};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor,
    DT_CENTER, DT_LEFT, DT_SINGLELINE, DT_VCENTER, HBRUSH, HDC, TRANSPARENT,
};
use windows_sys::Win32::Graphics::GdiPlus::{
    GdipAddPathArc, GdipClosePathFigure, GdipCreateFromHDC, GdipCreatePath, GdipCreatePen1,
    GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath, GdipDeletePen,
    GdipDrawPath, GdipFillPath, GdipSetSmoothingMode, FillModeAlternate, GpBrush, GpGraphics,
    GpPath, GpPen, SmoothingModeAntiAlias, UnitPixel,
};

pub type GdipToken = usize;

/// 启动 GDI+（进程一次即可）
pub fn gdiplus_init() -> GdipToken {
    unsafe {
        let input = windows_sys::Win32::Graphics::GdiPlus::GdiplusStartupInput {
            GdiplusVersion: 1,
            DebugEventCallback: 0,
            SuppressBackgroundThread: 0,
            SuppressExternalCodecs: 0,
        };
        let mut token: GdipToken = 0;
        let mut out: windows_sys::Win32::Graphics::GdiPlus::GdiplusStartupOutput =
            std::mem::zeroed();
        windows_sys::Win32::Graphics::GdiPlus::GdiplusStartup(&mut token, &input, &mut out);
        token
    }
}

pub fn gdiplus_shutdown(token: GdipToken) {
    unsafe {
        windows_sys::Win32::Graphics::GdiPlus::GdiplusShutdown(token);
    }
}

// ---------- 色板 ----------

pub struct Palette {
    pub window_bg: COLORREF,     // 主背景（窗口/编辑器/列表）
    pub surface2: COLORREF,      // 输入框/次级按钮底
    pub hover: COLORREF,         // 悬停底
    pub hairline: COLORREF,      // 1px 分隔线
    pub text: COLORREF,          // 主文字
    pub text_sec: COLORREF,      // 次级文字
    pub text_disabled: COLORREF, // 完成态文字
    pub accent: COLORREF,        // 强调色（选中/主按钮）
    pub accent_hover: COLORREF,
    pub accent_press: COLORREF,
    pub accent_soft: COLORREF,   // 输入框聚焦描边
    pub accent_tint: COLORREF,   // 选中行底（预混的不透明色）
    pub on_accent: COLORREF,     // 强调色上的文字
    pub danger: COLORREF,        // 标题栏关闭悬停
    pub on_danger: COLORREF,
}

/// COLORREF 是 0x00BBGGRR
const fn rgb(r: u32, g: u32, b: u32) -> COLORREF {
    (b << 16) | (g << 8) | r
}

pub const LIGHT: Palette = Palette {
    window_bg: rgb(0xFF, 0xFF, 0xFF),
    surface2: rgb(0xF2, 0xF4, 0xF7),
    hover: rgb(0xEC, 0xEF, 0xF3),
    hairline: rgb(0xE4, 0xE7, 0xEB),
    text: rgb(0x1A, 0x1D, 0x21),
    text_sec: rgb(0x6B, 0x72, 0x80),
    text_disabled: rgb(0xA3, 0xAA, 0xB3),
    accent: rgb(0x34, 0x78, 0xF6),
    accent_hover: rgb(0x4B, 0x89, 0xFF),
    accent_press: rgb(0x2B, 0x66, 0xD9),
    accent_soft: rgb(0x8F, 0xB4, 0xFA),
    accent_tint: rgb(0xE3, 0xED, 0xFE),
    on_accent: rgb(0xFF, 0xFF, 0xFF),
    danger: rgb(0xE8, 0x11, 0x23),
    on_danger: rgb(0xFF, 0xFF, 0xFF),
};

pub const DARK: Palette = Palette {
    window_bg: rgb(0x1E, 0x1F, 0x23),
    surface2: rgb(0x2A, 0x2D, 0x32),
    hover: rgb(0x31, 0x35, 0x3B),
    hairline: rgb(0x34, 0x38, 0x3D),
    text: rgb(0xE8, 0xEA, 0xED),
    text_sec: rgb(0x9A, 0xA0, 0xA6),
    text_disabled: rgb(0x60, 0x65, 0x6B),
    accent: rgb(0x6B, 0x9F, 0xFF),
    accent_hover: rgb(0x85, 0xAF, 0xFF),
    accent_press: rgb(0x5A, 0x8A, 0xE0),
    accent_soft: rgb(0x3E, 0x5A, 0x8C),
    accent_tint: rgb(0x31, 0x3F, 0x5A),
    on_accent: rgb(0x10, 0x14, 0x1A),
    danger: rgb(0xE8, 0x11, 0x23),
    on_danger: rgb(0xFF, 0xFF, 0xFF),
};

pub fn palette(dark: bool) -> &'static Palette {
    if dark {
        &DARK
    } else {
        &LIGHT
    }
}

/// "light"/"dark"/"system" -> 是否暗色。system 读注册表（无该键的老系统按浅色）。
pub fn resolve_dark(mode: &str) -> bool {
    match mode {
        "dark" => true,
        "light" => false,
        _ => registry_prefers_dark(),
    }
}

/// 读 HKCU\...\Themes\Personalize\AppsUseLightTheme（0 = 暗色）
fn registry_prefers_dark() -> bool {
    unsafe {
        use windows_sys::Win32::System::Registry::{
            RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ,
        };
        let sub = crate::util::wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
        let mut hk: HKEY = 0;
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut hk) != 0 {
            return false;
        }
        let name = crate::util::wide("AppsUseLightTheme");
        let mut val: u32 = 1;
        let mut size = 4u32;
        let r = RegQueryValueExW(
            hk,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            (&mut val as *mut u32) as *mut u8,
            &mut size,
        );
        RegCloseKey(hk);
        r == 0 && val == 0
    }
}

pub fn invalidate(hwnd: isize) {
    unsafe {
        windows_sys::Win32::Graphics::Gdi::InvalidateRect(hwnd, std::ptr::null(), 0);
    }
}

// ---------- 缓存画刷（WM_CTLCOLOR* 用） ----------

pub struct Brushes {
    pub window_bg: HBRUSH,
    pub surface2: HBRUSH,
}

impl Brushes {
    pub const fn new() -> Brushes {
        Brushes { window_bg: 0, surface2: 0 }
    }

    /// 按（暗色与否）重建；旧画刷销毁
    pub fn rebuild(&mut self, dark: bool) {
        unsafe {
            if self.window_bg != 0 {
                DeleteObject(self.window_bg);
            }
            if self.surface2 != 0 {
                DeleteObject(self.surface2);
            }
            let pal = palette(dark);
            self.window_bg = CreateSolidBrush(pal.window_bg);
            self.surface2 = CreateSolidBrush(pal.surface2);
        }
    }
}

impl Drop for Brushes {
    fn drop(&mut self) {
        unsafe {
            if self.window_bg != 0 {
                DeleteObject(self.window_bg);
            }
            if self.surface2 != 0 {
                DeleteObject(self.surface2);
            }
        }
    }
}

// ---------- GDI+ 绘制基元 ----------

/// COLORREF -> GDI+ ARGB（不透明）
pub fn argb(c: COLORREF) -> u32 {
    0xFF00_0000 | ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF)
}

fn path_round(path: *mut GpPath, x: f32, y: f32, w: f32, h: f32, r: f32) -> i32 {
    let r = r.min(w / 2.0).min(h / 2.0);
    unsafe {
        let mut s = GdipAddPathArc(path, x, y, 2.0 * r, 2.0 * r, 180.0, 90.0);
        s |= GdipAddPathArc(path, x + w - 2.0 * r, y, 2.0 * r, 2.0 * r, 270.0, 90.0);
        s |= GdipAddPathArc(path, x + w - 2.0 * r, y + h - 2.0 * r, 2.0 * r, 2.0 * r, 0.0, 90.0);
        s |= GdipAddPathArc(path, x, y + h - 2.0 * r, 2.0 * r, 2.0 * r, 90.0, 90.0);
        s | GdipClosePathFigure(path)
    }
}

/// 圆角矩形绘制：填充（可选）+ 描边（可选），至少给一个
pub fn fill_round(
    hdc: HDC,
    rc: RECT,
    radius: f32,
    fill: Option<COLORREF>,
    stroke: Option<(COLORREF, f32)>,
) {
    if fill.is_none() && stroke.is_none() {
        return;
    }
    unsafe {
        let mut g: *mut GpGraphics = std::ptr::null_mut();
        if GdipCreateFromHDC(hdc, &mut g) != 0 {
            return;
        }
        GdipSetSmoothingMode(g, SmoothingModeAntiAlias);
        let mut path: *mut GpPath = std::ptr::null_mut();
        GdipCreatePath(FillModeAlternate, &mut path);
        path_round(
            path,
            rc.left as f32,
            rc.top as f32,
            (rc.right - rc.left) as f32,
            (rc.bottom - rc.top) as f32,
            radius,
        );
        if let Some(fc) = fill {
            let mut brush: *mut GpBrush = std::ptr::null_mut();
            GdipCreateSolidFill(argb(fc), &mut brush);
            GdipFillPath(g, brush, path);
            GdipDeleteBrush(brush);
        }
        if let Some((pc, pw)) = stroke {
            let mut pen: *mut GpPen = std::ptr::null_mut();
            GdipCreatePen1(argb(pc), pw, UnitPixel, &mut pen);
            GdipDrawPath(g, pen, path);
            GdipDeletePen(pen);
        }
        GdipDeletePath(path);
        GdipDeleteGraphics(g);
    }
}

// ---------- 扁平按钮 ----------

#[derive(Clone, Copy, PartialEq)]
pub enum BtnKind {
    Primary, // 强调色填充（主操作：添加）
    Tonal,   // 次级底 + 细线（其余全部按钮）
    Ghost,   // 平时只有文字（无边框），悬停浮现淡底——用于列表头部按钮
}

/// 文字对齐：居中，或左对齐 + 指定缩进（用于与列表文字对齐）
#[derive(Clone, Copy)]
pub enum BtnAlign {
    Center,
    Left(i32),
}

/// 在 owner-draw 的 hdc 上绘制按钮
pub fn draw_button(
    hdc: HDC,
    rc: RECT,
    text: &str,
    kind: BtnKind,
    hover: bool,
    down: bool,
    dark: bool,
    hfont: isize,
    align: BtnAlign,
) {
    let p = palette(dark);
    unsafe {
        // 先铺窗口背景色：圆角外的方形角落不能露出未初始化区域（黑角）。
        // 用普通 GDI 矩形填充；fill_round 半径 0 会让 GDI+ 生成零尺寸圆弧的垃圾路径
        let bg_brush = CreateSolidBrush(p.window_bg);
        FillRect(hdc, &rc, bg_brush);
        DeleteObject(bg_brush);

        let (fill, line, tcol) = match kind {
            BtnKind::Primary => {
                let f = if down {
                    p.accent_press
                } else if hover {
                    p.accent_hover
                } else {
                    p.accent
                };
                (Some(f), Some((f, 1.0)), p.on_accent)
            }
            BtnKind::Tonal => {
                if down {
                    (Some(p.hover), Some((p.accent_soft, 1.0)), p.accent)
                } else if hover {
                    (Some(p.surface2), Some((p.accent, 1.0)), p.accent)
                } else {
                    (Some(p.surface2), Some((p.hairline, 1.0)), p.text)
                }
            }
            BtnKind::Ghost => {
                if down {
                    (Some(p.hover), None, p.accent)
                } else if hover {
                    (Some(p.accent_tint), None, p.accent)
                } else {
                    (None, None, p.text_sec)
                }
            }
        };
        let r = RECT { left: rc.left, top: rc.top, right: rc.right - 1, bottom: rc.bottom - 1 };
        fill_round(hdc, r, 6.0, fill, line);
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, tcol);
        SelectObject(hdc, hfont);
        let mut tr = rc;
        let flags = match align {
            BtnAlign::Center => DT_SINGLELINE | DT_VCENTER | DT_CENTER,
            BtnAlign::Left(_) => DT_SINGLELINE | DT_VCENTER | DT_LEFT,
        };
        if let BtnAlign::Left(pad) = align {
            tr.left += pad;
        }
        DrawTextW(
            hdc,
            crate::util::wide(text).as_ptr(),
            -1,
            &mut tr,
            flags,
        );
    }
}
