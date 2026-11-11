use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::Controls::BST_CHECKED;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRect, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, CreateWindowExW, DefWindowProcW,
    DestroyWindow, GetWindowRect, LoadCursorW, MessageBoxW, RegisterClassW, SetForegroundWindow,
    SetWindowTextW, ShowWindow, SW_SHOW, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_DESTROY,
    WM_KEYDOWN, WM_SYSKEYDOWN, WS_BORDER, WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE, WNDCLASSW, IDC_ARROW,
};

use crate::util::{self, wide};
use crate::window::with_app;
use crate::hotkey;

const SET_CLASS: &str = "MiniNotesSettingsWnd";

const ST_LABEL: u32 = 1;
const ED_HOTKEY: u32 = 2;
const CHK_AUTOHIDE: u32 = 3;
const CHK_AUTOSTART: u32 = 4;
const ST_HINT: u32 = 5;
const BTN_SAVE: u32 = 6;
const BTN_CANCEL: u32 = 7;
const SUB_CAPTURE: usize = 10;

struct SettingsState {
    hwnd: HWND,
    owner: HWND,
    chk_autohide: HWND,
    chk_autostart: HWND,
    hk_mods: u32,
    hk_vk: u32,
    font: isize,
}

thread_local! {
    static ST: RefCell<Option<SettingsState>> = const { RefCell::new(None) };
}

fn with_st<R>(f: impl FnOnce(&mut SettingsState) -> R) -> R {
    ST.with(|s| f(s.borrow_mut().as_mut().unwrap()))
}

fn st_ready() -> bool {
    ST.with(|s| s.borrow().is_some())
}

// ---------- 开机自启（HKCU Run） ----------

const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VAL: &str = "MiniNotes";

pub fn autostart_on() -> bool {
    unsafe {
        let mut hk: HKEY = 0;
        let sub = wide(RUN_SUBKEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut hk) != 0 {
            return false;
        }
        let name = wide(RUN_VAL);
        let r = RegQueryValueExW(
            hk,
            name.as_ptr(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        RegCloseKey(hk);
        r == 0
    }
}

/// on=true 写入带引号的 exe 完整路径；on=false 删除
pub fn autostart_set(on: bool) -> bool {
    unsafe {
        if on {
            let mut hk: HKEY = 0;
            let sub = wide(RUN_SUBKEY);
            if RegCreateKeyExW(
                HKEY_CURRENT_USER,
                sub.as_ptr(),
                0,
                std::ptr::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                std::ptr::null(),
                &mut hk,
                std::ptr::null_mut(),
            ) != 0 {
                return false;
            }
            let path = format!("\"{}\"", util::exe_path());
            let data = wide(&path);
            let name = wide(RUN_VAL);
            let r = RegSetValueExW(
                hk,
                name.as_ptr(),
                0,
                REG_SZ,
                data.as_ptr() as *const u8,
                (data.len() * 2) as u32,
            );
            RegCloseKey(hk);
            r == 0
        } else {
            let mut hk: HKEY = 0;
            let sub = wide(RUN_SUBKEY);
            if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_SET_VALUE, &mut hk) != 0 {
                return true; // 键本来就不存在
            }
            let name = wide(RUN_VAL);
            let r = RegDeleteValueW(hk, name.as_ptr());
            RegCloseKey(hk);
            r == 0 || r == 2 // 2 = ERROR_FILE_NOT_FOUND，视为成功
        }
    }
}

// ---------- 对话框 ----------

pub fn open(owner: HWND) {
    if st_ready() {
        with_st(|s| unsafe {
            SetForegroundWindow(s.hwnd);
        });
        return;
    }

    unsafe {
        let class = wide(SET_CLASS);
        let wc: WNDCLASSW = WNDCLASSW {
            lpfnWndProc: Some(settings_proc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(0, IDC_ARROW),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);

        // 客户区 356x208，非 resizable 对话框边框
        let style = (WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU) as u32;
        let mut rc = RECT { left: 0, top: 0, right: 356, bottom: 208 };
        AdjustWindowRect(&mut rc, style, 0);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);

        // 在主窗口上方居中（主窗口可能隐藏，用工作区兜底）
        let mut or = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        let mut base = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(owner, &mut or) != 0 {
            base = or;
        } else {
            windows_sys::Win32::UI::WindowsAndMessaging::SystemParametersInfoW(
                windows_sys::Win32::UI::WindowsAndMessaging::SPI_GETWORKAREA,
                0,
                &mut base as *mut RECT as *mut core::ffi::c_void,
                0,
            );
        }
        let bw = base.right - base.left;
        let bh = base.bottom - base.top;
        let x = base.left + (bw - w) / 2;
        let y = base.top + (bh - h) / 2;

        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            wide("MiniNotes 设置").as_ptr(),
            style,
            x,
            y,
            w,
            h,
            owner,
            0,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        let _ = hwnd; // WM_CREATE 内建状态
        EnableWindow(owner, 0); // 模态感：主窗口不可点
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        with_app(|app| app.settings_open = true);
    }
}

unsafe extern "system" fn settings_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_CREATE => {
            let cs = lp as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW;
            let owner = (*cs).hwndParent;
            let (m, v, auto_hide, autost) = with_app(|app| {
                (
                    app.cfg.hotkey_mods,
                    app.cfg.hotkey_vk,
                    app.cfg.auto_hide,
                    autostart_on(),
                )
            });

            let mk = |class: &str, text: &str, style: u32, x: i32, y: i32, w: i32, h: i32, id: u32| -> HWND {
                CreateWindowExW(
                    0,
                    wide(class).as_ptr(),
                    wide(text).as_ptr(),
                    style,
                    x,
                    y,
                    w,
                    h,
                    hwnd,
                    id as isize,
                    GetModuleHandleW(std::ptr::null()),
                    std::ptr::null(),
                )
            };

            let font = util::create_ui_font(false);
            let st_label = mk("STATIC", "全局热键：", WS_CHILD | WS_VISIBLE, 12, 16, 62, 18, ST_LABEL);
            let ed = mk(
                "EDIT",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER,
                78, 12, 170, 24,
                ED_HOTKEY,
            );
            let chk_hide = mk(
                "BUTTON",
                "失焦时自动隐藏窗口",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                12, 48, 240, 20,
                CHK_AUTOHIDE,
            );
            let chk_auto = mk(
                "BUTTON",
                "开机时自动启动 MiniNotes",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                12, 74, 240, 20,
                CHK_AUTOSTART,
            );
            let st_hint = mk(
                "STATIC",
                "点击热键输入框后按下组合键即可录入（如 Alt+N）。\n热键需包含 Ctrl / Alt / Shift / Win 至少一个修饰键。",
                WS_CHILD | WS_VISIBLE,
                12, 102, 332, 36,
                ST_HINT,
            );
            let btn_save = mk("BUTTON", "保存", WS_CHILD | WS_VISIBLE | WS_TABSTOP, 180, 166, 80, 28, BTN_SAVE);
            let btn_cancel = mk("BUTTON", "取消", WS_CHILD | WS_VISIBLE | WS_TABSTOP, 268, 166, 80, 28, BTN_CANCEL);

            for h in [st_label, ed, chk_hide, chk_auto, st_hint, btn_save, btn_cancel] {
                util::apply_font(h, font);
            }
            SetWindowTextW(ed, wide(&hotkey::text(m, v)).as_ptr());
            send_msg_checked(chk_hide, BM_SETCHECK, if auto_hide { BST_CHECKED as usize } else { 0 });
            send_msg_checked(chk_auto, BM_SETCHECK, if autost { BST_CHECKED as usize } else { 0 });
            SetWindowSubclass(ed, Some(capture_proc), SUB_CAPTURE, 0);

            ST.with(|s| {
                *s.borrow_mut() = Some(SettingsState {
                    hwnd,
                    owner,
                    chk_autohide: chk_hide,
                    chk_autostart: chk_auto,
                    hk_mods: m,
                    hk_vk: v,
                    font,
                })
            });
            0
        }
        WM_COMMAND => {
            let id = (wp & 0xFFFF) as u32;
            if id == BTN_SAVE {
                apply(hwnd);
            } else if id == BTN_CANCEL {
                DestroyWindow(hwnd);
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            with_st(|s| unsafe {
                EnableWindow(s.owner, 1);
                SetForegroundWindow(s.owner);
                util::destroy_font(s.font);
            });
            ST.with(|s| *s.borrow_mut() = None);
            with_app(|app| app.settings_open = false);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

/// SendMessageW 的薄包装（BM_SETCHECK 用），减少裸 unsafe
fn send_msg_checked(hwnd: HWND, msg: u32, wp: usize) {
    unsafe { windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, msg, wp, 0) };
}

unsafe extern "system" fn capture_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _d: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
        let vk = wp as u32;
        if !st_ready() {
            return 1;
        }
        let (m0, v0) = with_st(|s| (s.hk_mods, s.hk_vk));
        if vk == windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE as u32 {
            // Esc 恢复显示
            SetWindowTextW(hwnd, wide(&hotkey::text(m0, v0)).as_ptr());
            return 1;
        }
        let ctrl = (GetKeyState(VK_CONTROL as i32) as i16) < 0;
        let alt = (GetKeyState(VK_MENU as i32) as i16) < 0;
        let shift = (GetKeyState(VK_SHIFT as i32) as i16) < 0;
        let win = (GetKeyState(VK_LWIN as i32) as i16) < 0 || (GetKeyState(VK_RWIN as i32) as i16) < 0;
        let mut mods = 0u32;
        if ctrl {
            mods |= MOD_CONTROL;
        }
        if alt {
            mods |= MOD_ALT;
        }
        if shift {
            mods |= MOD_SHIFT;
        }
        if win {
            mods |= windows_sys::Win32::UI::Input::KeyboardAndMouse::MOD_WIN;
        }
        // 只按修饰键：先显示"Ctrl + …"等待主键
        if hotkey::is_modifier(vk) {
            let mut parts = vec![];
            if ctrl {
                parts.push("Ctrl");
            }
            if alt {
                parts.push("Alt");
            }
            if shift {
                parts.push("Shift");
            }
            if win {
                parts.push("Win");
            }
            SetWindowTextW(hwnd, wide(&format!("{} + …", parts.join(" + "))).as_ptr());
            return 1;
        }
        with_st(|s| {
            s.hk_mods = mods;
            s.hk_vk = vk;
        });
        SetWindowTextW(hwnd, wide(&hotkey::text(mods, vk)).as_ptr());
        return 1;
    }
    if msg == WM_CHAR {
        return 1; // 吞掉字符输入
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

fn is_checked(hwnd: HWND) -> bool {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW(hwnd, BM_GETCHECK, 0, 0)
            == BST_CHECKED as isize
    }
}

fn apply(hwnd: HWND) {
    let (m, v) = with_st(|s| (s.hk_mods, s.hk_vk));
    let auto_hide = with_st(|s| is_checked(s.chk_autohide));
    let autost = with_st(|s| is_checked(s.chk_autostart));

    if !hotkey::valid(m, v) {
        unsafe {
            MessageBoxW(
                hwnd,
                wide("热键需包含 Ctrl / Alt / Shift / Win 中至少一个修饰键，请重新录入。").as_ptr(),
                wide("热键无效").as_ptr(),
                0x30, // MB_ICONWARNING
            );
        };
        return;
    }

    let hk_ok = with_app(|app| {
        if app.cfg.hotkey_mods == m && app.cfg.hotkey_vk == v {
            return true;
        }
        hotkey::unregister(app.hwnd);
        if hotkey::register(app.hwnd, m, v) {
            app.cfg.hotkey_mods = m;
            app.cfg.hotkey_vk = v;
            true
        } else {
            // 恢复旧热键
            let _ = hotkey::register(app.hwnd, app.cfg.hotkey_mods, app.cfg.hotkey_vk);
            false
        }
    });
    if !hk_ok {
        unsafe {
            MessageBoxW(
                hwnd,
                wide(&format!("热键 {} 注册失败，可能已被其他程序占用。", hotkey::text(m, v))).as_ptr(),
                wide("热键冲突").as_ptr(),
                0x30,
            );
        };
        return;
    }

    autostart_set(autost);
    with_app(|app| {
        app.cfg.auto_hide = auto_hide;
        app.cfg.autostart = autost;
        crate::config::save(&app.cfg);
    });
    unsafe { DestroyWindow(hwnd) };
}
