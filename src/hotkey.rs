use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN,
};

pub const HOTKEY_ID: i32 = 1;

pub fn register(hwnd: HWND, mods: u32, vk: u32) -> bool {
    unsafe { RegisterHotKey(hwnd, HOTKEY_ID, mods | MOD_NOREPEAT, vk) != 0 }
}

pub fn unregister(hwnd: HWND) {
    unsafe { UnregisterHotKey(hwnd, HOTKEY_ID) };
}

pub fn text(mods: u32, vk: u32) -> String {
    let mut parts: Vec<String> = Vec::new();
    if mods & MOD_CONTROL != 0 {
        parts.push("Ctrl".into());
    }
    if mods & MOD_ALT != 0 {
        parts.push("Alt".into());
    }
    if mods & MOD_SHIFT != 0 {
        parts.push("Shift".into());
    }
    if mods & MOD_WIN != 0 {
        parts.push("Win".into());
    }
    parts.push(vk_name(vk));
    parts.join(" + ")
}

/// 校验：至少一个修饰键，且主键不能是修饰键本身
pub fn valid(mods: u32, vk: u32) -> bool {
    mods != 0 && !is_modifier(vk)
}

pub fn is_modifier(vk: u32) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0xA0..=0xA5 | 0x5B | 0x5C)
}

pub fn vk_name(vk: u32) -> String {
    match vk {
        0x08 => "Backspace".into(),
        0x09 => "Tab".into(),
        0x0D => "Enter".into(),
        0x13 => "Pause".into(),
        0x14 => "CapsLock".into(),
        0x20 => "Space".into(),
        0x21 => "PageUp".into(),
        0x22 => "PageDown".into(),
        0x23 => "End".into(),
        0x24 => "Home".into(),
        0x25 => "Left".into(),
        0x26 => "Up".into(),
        0x27 => "Right".into(),
        0x28 => "Down".into(),
        0x2C => "PrintScreen".into(),
        0x2D => "Insert".into(),
        0x2E => "Delete".into(),
        0x30..=0x39 => ((vk as u8 + b'0' - 0x30) as char).to_string(),
        0x41..=0x5A => ((vk as u8 - 0x41 + b'A') as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        0xA0 => "LShift".into(),
        0xA1 => "RShift".into(),
        0xA2 => "LCtrl".into(),
        0xA3 => "RCtrl".into(),
        0xA4 => "LAlt".into(),
        0xA5 => "RAlt".into(),
        _ => format!("0x{:02X}", vk),
    }
}
