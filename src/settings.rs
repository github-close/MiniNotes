use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{FillRect, InvalidateRect, CreateSolidBrush, DeleteObject};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::Controls::{SetWindowTheme, BST_CHECKED, DRAWITEMSTRUCT, ODS_SELECTED, WM_MOUSELEAVE};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, MOD_ALT, MOD_CONTROL, MOD_SHIFT, VK_CONTROL, VK_LWIN, VK_MENU,
    VK_RWIN, VK_SHIFT,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRect, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_AUTORADIOBUTTON, BS_OWNERDRAW,
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect, LoadCursorW, MessageBoxW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow, SW_SHOW,
    WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLORSTATIC, WM_CTLCOLOREDIT, WM_DESTROY,
    WM_DRAWITEM, WM_ERASEBKGND,
    BM_CLICK, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SYSKEYDOWN, WS_GROUP,
    WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WNDCLASSW, IDC_ARROW,
};

use crate::theme::{self, BtnAlign, BtnKind};
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
const RD_LIGHT: u32 = 8;
const RD_DARK: u32 = 9;
const RD_SYSTEM: u32 = 10;
const ST_HIDE: u32 = 11;
const ST_AUTO: u32 = 12;
const SUB_CAPTURE: usize = 10;
const SUB_SBTN: usize = 11;
const SS_NOTIFY: u32 = 0x0100; // windows-sys 0.52 缺失

struct SettingsState {
    hwnd: HWND,
    owner: HWND,
    chk_autohide: HWND,
    chk_autostart: HWND,
    hk_mods: u32,
    hk_vk: u32,
    font: isize,
    hover_save: bool,
    hover_cancel: bool,
    down_save: bool,
    down_cancel: bool,
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

        let style = (WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU) as u32;
        let mut rc = RECT { left: 0, top: 0, right: 360, bottom: 252 };
        AdjustWindowRect(&mut rc, style, 0);
        let (w, h) = (rc.right - rc.left, rc.bottom - rc.top);

        let mut base = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        if GetWindowRect(owner, &mut base) == 0 {
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
        // 控件在 WM_CREATE 里就绪；此处只做模态化与显示
        EnableWindow(owner, 0);
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
            let dark = with_app(|app| app.dark);
            let (m, v, auto_hide, theme_mode) = with_app(|app| {
                (app.cfg.hotkey_mods, app.cfg.hotkey_vk, app.cfg.auto_hide, app.cfg.theme.clone())
            });
            let autost = autostart_on();

            // 暗色时标题栏跟随（Win10 19045+）
            if dark {
                let on: i32 = 1;
                windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
                    hwnd,
                    20, // DWMWA_USE_IMMERSIVE_DARK_MODE
                    &on as *const i32 as *const core::ffi::c_void,
                    4,
                );
            }

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
            let st_label = mk("STATIC", "全局热键：", WS_CHILD | WS_VISIBLE, 14, 16, 62, 18, ST_LABEL);
            let ed = mk("EDIT", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP, 80, 12, 172, 24, ED_HOTKEY);
            let st_theme = mk("STATIC", "主题：", WS_CHILD | WS_VISIBLE, 14, 50, 62, 18, ST_LABEL + 100);
            let rd_light = mk("BUTTON", "浅色", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32 | WS_GROUP as u32, 80, 48, 56, 20, RD_LIGHT);
            let rd_dark = mk("BUTTON", "暗色", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 140, 48, 56, 20, RD_DARK);
            let rd_system = mk("BUTTON", "跟随系统", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 200, 48, 84, 20, RD_SYSTEM);
            // checkbox 文字由 STATIC 承担：DarkMode_Explorer 主题下系统自绘的
            // checkbox 文字颜色不受 WM_CTLCOLORSTATIC 控制，会隐入深色背景
            let chk_hide = mk(
                "BUTTON",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                14, 82, 18, 18,
                CHK_AUTOHIDE,
            );
            let st_hide = mk(
                "STATIC",
                "失焦时自动隐藏窗口",
                WS_CHILD | WS_VISIBLE | SS_NOTIFY,
                38, 83, 220, 18,
                ST_HIDE,
            );
            let chk_auto = mk(
                "BUTTON",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                14, 108, 18, 18,
                CHK_AUTOSTART,
            );
            let st_auto = mk(
                "STATIC",
                "开机时自动启动 MiniNotes",
                WS_CHILD | WS_VISIBLE | SS_NOTIFY,
                38, 109, 220, 18,
                ST_AUTO,
            );
            let st_hint = mk(
                "STATIC",
                "点击热键输入框后按下组合键即可录入（如 Alt+N）。\n热键需包含 Ctrl / Alt / Shift / Win 至少一个修饰键。",
                WS_CHILD | WS_VISIBLE,
                14, 136, 336, 36,
                ST_HINT,
            );
            let btn_save = mk("BUTTON", "保存", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 180, 208, 80, 30, BTN_SAVE);
            let btn_cancel = mk("BUTTON", "取消", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 268, 208, 80, 30, BTN_CANCEL);

            for h in [st_label, ed, st_theme, rd_light, rd_dark, rd_system, chk_hide, st_hide, chk_auto, st_auto, st_hint, btn_save, btn_cancel] {
                util::apply_font(h, font);
            }

            // 复选框/单选钮的图形在暗色下用系统暗色主题绘制（文字由 STATIC 承担）
            let theme_name = if dark { "DarkMode_Explorer" } else { "Explorer" };
            for h in [rd_light, rd_dark, rd_system, chk_hide, chk_auto] {
                SetWindowTheme(h, wide(theme_name).as_ptr(), std::ptr::null());
            }

            SetWindowTextW(ed, wide(&hotkey::text(m, v)).as_ptr());
            send_checked(rd_light, theme_mode == "light");
            send_checked(rd_dark, theme_mode == "dark");
            send_checked(rd_system, theme_mode == "system");
            send_checked(chk_hide, auto_hide);
            send_checked(chk_auto, autost);
            SetWindowSubclass(ed, Some(capture_proc), SUB_CAPTURE, 0);
            SetWindowSubclass(btn_save, Some(sbtn_proc), SUB_SBTN, 0);
            SetWindowSubclass(btn_cancel, Some(sbtn_proc), SUB_SBTN + 1, 0);

            ST.with(|s| {
                *s.borrow_mut() = Some(SettingsState {
                    hwnd,
                    owner,
                    chk_autohide: chk_hide,
                    chk_autostart: chk_auto,
                    hk_mods: m,
                    hk_vk: v,
                    font,
                    hover_save: false,
                    hover_cancel: false,
                    down_save: false,
                    down_cancel: false,
                })
            });
            0
        }
        WM_ERASEBKGND => {
            with_app(|app| unsafe {
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
                let b = CreateSolidBrush(theme::palette(app.dark).window_bg);
                FillRect(wp as isize, &rc, b);
                DeleteObject(b);
            });
            1
        }
        WM_CTLCOLORSTATIC => {
            let hdc = wp as isize;
            with_app(|app| unsafe {
                let p = theme::palette(app.dark);
                let ctl = lp as HWND;
                let is_hint =
                    windows_sys::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(ctl) == ST_HINT as i32;
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, p.window_bg);
                windows_sys::Win32::Graphics::Gdi::SetTextColor(hdc, if is_hint { p.text_sec } else { p.text });
                app.brushes.window_bg
            })
        }
        WM_CTLCOLOREDIT => {
            let hdc = wp as isize;
            with_app(|app| unsafe {
                let p = theme::palette(app.dark);
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, p.surface2);
                windows_sys::Win32::Graphics::Gdi::SetTextColor(hdc, p.text);
                app.brushes.surface2
            })
        }
        WM_DRAWITEM => {
            let di = lp as *const DRAWITEMSTRUCT;
            if !di.is_null() {
                with_st(|s| {
                    let (text, kind, hov, down) = if (*di).CtlID == BTN_SAVE {
                        ("保存", BtnKind::Primary, s.hover_save, s.down_save)
                    } else {
                        ("取消", BtnKind::Tonal, s.hover_cancel, s.down_cancel)
                    };
                    with_app(|app| unsafe {
                        theme::draw_button(
                            (*di).hDC,
                            (*di).rcItem,
                            text,
                            kind,
                            hov,
                            down || ((*di).itemState & ODS_SELECTED != 0),
                            app.dark,
                            s.font,
                            BtnAlign::Center,
                        );
                    });
                });
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_COMMAND => {
            let id = (wp & 0xFFFF) as u32;
            if id == BTN_SAVE {
                apply(hwnd);
            } else if id == BTN_CANCEL {
                DestroyWindow(hwnd);
            } else if id == ST_HIDE {
                // 点击文字标签 = 切换对应勾选框
                let chk = with_st(|s| s.chk_autohide);
                unsafe { SendMessageW(chk, BM_CLICK, 0, 0) };
            } else if id == ST_AUTO {
                let chk = with_st(|s| s.chk_autostart);
                unsafe { SendMessageW(chk, BM_CLICK, 0, 0) };
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

fn send_checked(hwnd: HWND, on: bool) {
    unsafe { SendMessageW(hwnd, BM_SETCHECK, if on { BST_CHECKED as usize } else { 0 }, 0) };
}

fn is_checked(hwnd: HWND) -> bool {
    unsafe { SendMessageW(hwnd, BM_GETCHECK, 0, 0) == BST_CHECKED as isize }
}

/// 设置对话框按钮的悬停/按下跟踪
unsafe extern "system" fn sbtn_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _d: usize,
) -> LRESULT {
    let is_save = id == SUB_SBTN;
    match msg {
        // 禁止系统在 DRAWITEM 前擦底
        WM_ERASEBKGND => return 1,
        WM_MOUSEMOVE => {
            let changed = with_st(|s| {
                if is_save {
                    if !s.hover_save {
                        s.hover_save = true;
                        true
                    } else {
                        false
                    }
                } else if !s.hover_cancel {
                    s.hover_cancel = true;
                    true
                } else {
                    false
                }
            });
            if changed {
                let mut tme = windows_sys::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<windows_sys::Win32::UI::Input::KeyboardAndMouse::TRACKMOUSEEVENT>() as u32,
                    dwFlags: windows_sys::Win32::UI::Input::KeyboardAndMouse::TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                windows_sys::Win32::UI::Input::KeyboardAndMouse::TrackMouseEvent(&mut tme);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
        }
        WM_MOUSELEAVE => {
            with_st(|s| {
                if is_save {
                    s.hover_save = false;
                    s.down_save = false;
                } else {
                    s.hover_cancel = false;
                    s.down_cancel = false;
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        WM_LBUTTONDOWN => {
            with_st(|s| {
                if is_save {
                    s.down_save = true;
                } else {
                    s.down_cancel = true;
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        WM_LBUTTONUP => {
            with_st(|s| {
                if is_save {
                    s.down_save = false;
                } else {
                    s.down_cancel = false;
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
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

fn apply(hwnd: HWND) {
    let (m, v) = with_st(|s| (s.hk_mods, s.hk_vk));
    let auto_hide = with_st(|s| is_checked(s.chk_autohide));
    let autost = with_st(|s| is_checked(s.chk_autostart));
    let theme_mode = with_st(|s| {
        if is_checked(find_child(s.hwnd, RD_DARK)) {
            "dark"
        } else if is_checked(find_child(s.hwnd, RD_SYSTEM)) {
            "system"
        } else {
            "light"
        }
    });

    if !hotkey::valid(m, v) {
        unsafe {
            MessageBoxW(
                hwnd,
                wide("热键需包含 Ctrl / Alt / Shift / Win 中至少一个修饰键，请重新录入。").as_ptr(),
                wide("热键无效").as_ptr(),
                0x30,
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
        app.cfg.theme = theme_mode.to_string();
        crate::config::save(&app.cfg);
        crate::window::apply_theme(app);
    });
    unsafe { DestroyWindow(hwnd) };
}

fn find_child(parent: HWND, id: u32) -> HWND {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(parent, id as i32)
    }
}
