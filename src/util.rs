use std::path::PathBuf;

use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::Graphics::Gdi::{CreateFontW, DeleteObject, HFONT};
use windows_sys::Win32::System::LibraryLoader::GetModuleFileNameW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    SendMessageW, WM_GETTEXT, WM_GETTEXTLENGTH, WM_SETFONT, WM_SETTEXT,
};

pub const UI_FONT_FACE: &str = "Microsoft YaHei UI";

/// Rust &str -> UTF-16（含结尾 NUL），供所有 W 系列 API 使用
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// UTF-16 缓冲 -> Rust String（截到首个 NUL）
pub fn from_wide(buf: &[u16]) -> String {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..end])
}

pub fn set_text(hwnd: HWND, s: &str) {
    let w = wide(s);
    unsafe { SendMessageW(hwnd, WM_SETTEXT, 0, w.as_ptr() as isize) };
}

pub fn get_text(hwnd: HWND) -> String {
    unsafe {
        let len = SendMessageW(hwnd, WM_GETTEXTLENGTH, 0, 0) as usize;
        let mut buf = vec![0u16; len + 1];
        SendMessageW(hwnd, WM_GETTEXT, buf.len(), buf.as_mut_ptr() as isize);
        from_wide(&buf)
    }
}

pub fn apply_font(hwnd: HWND, font: HFONT) {
    unsafe { SendMessageW(hwnd, WM_SETFONT, font as usize, 1) };
}

/// 创建界面字体（12px 像素高）。strike=true 时带删除线（待办完成态）。
pub fn create_ui_font(strike: bool) -> HFONT {
    create_font_px(-12, 400, strike)
}

/// 指定像素高度与字重创建界面字体（高度取负值 = 字符高度，不含行距）
pub fn create_font_px(height: i32, weight: i32, strike: bool) -> HFONT {
    create_font_face_px(UI_FONT_FACE, height, weight, strike)
}

/// 指定字体名 + 像素高度与字重创建字体（笔记编辑区独立字体设置用，
/// 其余界面仍走 create_font_px 固定 YaHei UI）
pub fn create_font_face_px(face: &str, height: i32, weight: i32, strike: bool) -> HFONT {
    let face = wide(face);
    unsafe {
        CreateFontW(
            height,
            0,
            0,
            0,
            weight,
            0,
            0,
            strike as u32,
            0, // DEFAULT_CHARSET
            0, // OUT_DEFAULT_PRECIS
            0, // CLIP_DEFAULT_PRECIS
            5, // CLEARTYPE_QUALITY
            0, // DEFAULT_PITCH
            face.as_ptr(),
        )
    }
}

/// 标题/Tab 用小号半粗字体
pub fn create_font_title() -> HFONT {
    create_font_px(-13, 600, false)
}

/// 字体一行的高度（tmHeight + tmExternalLeading），供布局按行高定位控件
pub fn font_line_height(hfont: HFONT) -> i32 {
    use windows_sys::Win32::Graphics::Gdi::{
        GetDC, GetTextMetricsW, ReleaseDC, SelectObject, TEXTMETRICW,
    };
    unsafe {
        let hdc = GetDC(0);
        let old = SelectObject(hdc, hfont);
        let mut tm: TEXTMETRICW = std::mem::zeroed();
        GetTextMetricsW(hdc, &mut tm);
        SelectObject(hdc, old);
        ReleaseDC(0, hdc);
        (tm.tmHeight + tm.tmExternalLeading) as i32
    }
}

/// 在矩形中央绘制次级提示文字（列表空状态）
pub fn draw_hint_centered(hdc: isize, rc: &windows_sys::Win32::Foundation::RECT, text: &str, color: u32, hfont: isize) {
    use windows_sys::Win32::Graphics::Gdi::{
        DrawTextW, SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_SINGLELINE, DT_VCENTER,
        TRANSPARENT,
    };
    unsafe {
        SetBkMode(hdc, TRANSPARENT as i32);
        SetTextColor(hdc, color);
        SelectObject(hdc, hfont);
        let mut r = *rc;
        DrawTextW(hdc, wide(text).as_ptr(), -1, &mut r, DT_SINGLELINE | DT_CENTER | DT_VCENTER);
    }
}

pub fn destroy_font(font: HFONT) {
    if font != 0 {
        unsafe { DeleteObject(font) };
    }
}

/// exe 完整路径
pub fn exe_path() -> String {
    unsafe {
        let mut buf = [0u16; 1024];
        let n = GetModuleFileNameW(0, buf.as_mut_ptr(), 1024);
        from_wide(&buf[..n as usize])
    }
}

/// 数据目录：exe 旁 data/（便携式）
pub fn data_dir() -> PathBuf {
    let exe = std::path::Path::new(&exe_path())
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let dir = exe.join("data");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
