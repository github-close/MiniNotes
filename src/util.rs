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

/// 创建界面字体（9pt）。strike=true 时带删除线（待办完成态）。
pub fn create_ui_font(strike: bool) -> HFONT {
    let face = wide(UI_FONT_FACE);
    unsafe {
        CreateFontW(
            -12,
            0,
            0,
            0,
            400,
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
