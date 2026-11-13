use windows_sys::Win32::Foundation::HWND;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::WM_APP;

use crate::menu::{self, Item};
use crate::util::wide;

/// 托盘回调消息（老式版本：lParam = 鼠标消息）
pub const TRAY_CB: u32 = WM_APP + 1;
pub const TRAY_UID: u32 = 1;

// 托盘菜单命令 ID
pub const CMD_OPEN: u32 = 1001;
pub const CMD_NEW_NOTE: u32 = 1002;
pub const CMD_SETTINGS: u32 = 1003;
pub const CMD_AUTOSTART: u32 = 1004;
pub const CMD_QUIT: u32 = 1005;

pub fn add(hwnd: HWND, hicon: isize, tip: &str) {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = TRAY_CB;
    nid.hIcon = hicon;
    let t = wide(tip);
    let n = t.len().min(127);
    nid.szTip[..n].copy_from_slice(&t[..n]);
    unsafe { Shell_NotifyIconW(NIM_ADD, &mut nid) };
}

pub fn remove(hwnd: HWND) {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    unsafe { Shell_NotifyIconW(NIM_DELETE, &mut nid) };
}

/// 托盘气泡通知（用于热键降级等非阻塞提示）
pub fn notify(hwnd: HWND, title: &str, text: &str) {
    let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    nid.hWnd = hwnd;
    nid.uID = TRAY_UID;
    nid.uFlags = NIF_INFO;
    let t = wide(title);
    let n = t.len().min(63);
    nid.szInfoTitle[..n].copy_from_slice(&t[..n]);
    let s = wide(text);
    let n = s.len().min(255);
    nid.szInfo[..n].copy_from_slice(&s[..n]);
    unsafe { Shell_NotifyIconW(NIM_MODIFY, &mut nid) };
}

/// 在光标处弹出托盘菜单（自绘，配色跟随应用主题），返回所选命令（0 = 取消）
pub fn show_menu(hwnd: HWND, autostart_on: bool) -> u32 {
    let item = |id: u32, text: &str, checked: bool| Item {
        id,
        text: text.to_string(),
        sep: false,
        checked,
        swatch: None,
        sub: None,
    };
    let sep = || Item {
        id: 0,
        text: String::new(),
        sep: true,
        checked: false,
        swatch: None,
        sub: None,
    };
    let items = vec![
        item(CMD_OPEN, "打开主界面", false),
        item(CMD_NEW_NOTE, "新建笔记", false),
        item(CMD_SETTINGS, "设置…", false),
        sep(),
        item(CMD_AUTOSTART, "开机自启", autostart_on),
        sep(),
        item(CMD_QUIT, "退出", false),
    ];
    let theme = crate::window::with_app(|app| app.theme);
    menu::show(hwnd, theme, &items)
}
