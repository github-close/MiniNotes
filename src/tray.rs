use windows_sys::Win32::Foundation::{HWND, POINT};
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, PostMessageW, SetForegroundWindow,
    TrackPopupMenu, MF_CHECKED, MF_SEPARATOR, MF_STRING, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_APP,
    WM_NULL,
};

use crate::util::{from_wide, wide};

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

/// 在光标处弹出托盘菜单，返回所选命令（0 = 取消）
pub fn show_menu(hwnd: HWND, autostart_on: bool) -> u32 {
    unsafe {
        let menu = CreatePopupMenu();
        AppendMenuW(menu, MF_STRING, CMD_OPEN as usize, wide("打开主界面").as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_NEW_NOTE as usize, wide("新建笔记").as_ptr());
        AppendMenuW(menu, MF_STRING, CMD_SETTINGS as usize, wide("设置…").as_ptr());
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(
            menu,
            MF_STRING | if autostart_on { MF_CHECKED } else { 0 },
            CMD_AUTOSTART as usize,
            wide("开机自启").as_ptr(),
        );
        AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
        AppendMenuW(menu, MF_STRING, CMD_QUIT as usize, wide("退出").as_ptr());

        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        // 经典套路：弹菜单前激活自己，否则菜单不消失
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(
            menu,
            TPM_RIGHTBUTTON | TPM_RETURNCMD,
            pt.x,
            pt.y,
            0,
            hwnd,
            std::ptr::null(),
        ) as u32;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd
    }
}

/// 忽略：预留给后续扩展（读取提示文本）
#[allow(dead_code)]
pub fn tip_of(nid: &NOTIFYICONDATAW) -> String {
    from_wide(&nid.szTip)
}
