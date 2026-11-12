use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BeginPaint, BLENDFUNCTION, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CreateCompatibleDC, CreateDIBSection, CreateFontW, DIB_RGB_COLORS,
    DeleteDC, EndPaint, FillRect, GetDC, InvalidateRect, CreateSolidBrush, DeleteObject,
    PAINTSTRUCT, ReleaseDC, SelectObject,
};
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
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowRect,
    LoadCursorW, MessageBoxW, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetWindowTextW, ShowWindow, SW_SHOW, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE,
    WM_CTLCOLORSTATIC, WM_CTLCOLOREDIT, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND, WM_PAINT,
    BM_CLICK, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SYSKEYDOWN, WS_GROUP,
    WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    ES_MULTILINE, WM_PASTE,
    LoadImageW, GetSystemMetrics, IMAGE_ICON, LR_SHARED, WM_SETICON,
    SM_CXICON, SM_CYICON, SM_CXSMICON, SM_CYSMICON,
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
const ST_TITLE: u32 = 13;
const ST_TAG: u32 = 14;
// 单选钮的 STATIC 文字标签（点击 = 切换对应单选钮）
const ST_RDL: u32 = 15;
const ST_RDD: u32 = 16;
const ST_RDS: u32 = 17;
const SUB_CAPTURE: usize = 10;
const SUB_SBTN: usize = 11;
const SS_NOTIFY: u32 = 0x0100; // windows-sys 0.52 缺失
const EM_SETRECT: u32 = 0x00B3; // windows-sys 0.52 缺失：设置 EDIT 格式矩形

// 品牌 banner：设置窗口顶部的 logo（assets/brand/settings-logo.bin，
// 由 make_settings_logo.py 从 symbol-color.svg 离线生成——预乘 BGRA、自底向上，
// 运行时 CreateDIBSection 后 AlphaBlend 直接合成，无解码、零运行时依赖）
const LOGO_SIZE: i32 = 42;
const BANNER_SHIFT: i32 = 56;
static LOGO_BIN: &[u8] = include_bytes!("../assets/brand/settings-logo.bin");

struct SettingsState {
    hwnd: HWND,
    owner: HWND,
    chk_autohide: HWND,
    chk_autostart: HWND,
    hk_mods: u32,
    hk_vk: u32,
    font: isize,
    font_title: isize,
    logo_dc: isize,
    logo_bmp: isize,
    hover_save: bool,
    hover_cancel: bool,
    down_save: bool,
    down_cancel: bool,
    // 打开设置时停用了全局热键（否则当前已注册的组合键会被系统拦截去切换主窗口，
    // 输入框永远捕获不到最后那个主键）。关闭时按此标记恢复注册
    hk_dismissed: bool,
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
/// 自启注册表命令带的参数：开机自启保持托盘常驻，不带此参数的启动（手动双击）
/// 则把主窗口展示到桌面
pub const LAUNCH_FLAG: &str = "--hidden";

/// 自启注册表应写入的完整命令
fn autostart_cmd() -> String {
    format!("\"{}\" {}", util::exe_path(), LAUNCH_FLAG)
}

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

/// on=true 写入带引号的 exe 完整路径 + --hidden 参数；on=false 删除
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
            let data = wide(&autostart_cmd());
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

/// 注册表里已有自启值但不是当前期望命令（旧版本写入的不带 --hidden 的裸路径）
/// → 需要静默改写，否则旧条目开机自启时会把主窗口弹到桌面
pub fn autostart_stale() -> bool {
    unsafe {
        let mut hk: HKEY = 0;
        let sub = wide(RUN_SUBKEY);
        if RegOpenKeyExW(HKEY_CURRENT_USER, sub.as_ptr(), 0, KEY_READ, &mut hk) != 0 {
            return false;
        }
        let name = wide(RUN_VAL);
        let mut buf = [0u16; 512];
        let mut len = (buf.len() * 2) as u32;
        let mut typ = 0u32;
        let r = RegQueryValueExW(
            hk,
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut typ,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        RegCloseKey(hk);
        if r != 0 || typ != REG_SZ {
            return false;
        }
        let n = (len as usize / 2).min(buf.len());
        let cur = String::from_utf16_lossy(&buf[..n]);
        cur.trim_end_matches('\0') != autostart_cmd()
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
        let mut rc = RECT { left: 0, top: 0, right: 360, bottom: 220 + BANNER_SHIFT };
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
        if hwnd == 0 {
            return; // 创建失败：不要禁用主窗口，否则再也点不动
        }
        // 控件在 WM_CREATE 里就绪；此处只做模态化与显示。
        // 守卫必须先于 EnableWindow：禁用前台的主窗口会让系统立刻收回前台、
        // 主窗口收到 WM_ACTIVATE(0)，此时若 settings_open 还没置位，失焦隐藏
        // 会用旧配置把主窗口藏回托盘（关掉设置后看起来就是"窗口自己消失了"）
        with_app(|app| {
            app.settings_open = true;
            // 录入期间停用全局热键：已注册的组合键会被 RegisterHotKey 系统级拦截，
            // 最后的主键到不了输入框（捕获停在半截），还会触发 toggle_main 把主窗口切走
            hotkey::unregister(app.hwnd);
            with_st(|s| s.hk_dismissed = true);
        });
        EnableWindow(owner, 0);
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
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

            // 暗色时标题栏跟随（uxtheme 序号 + DWM 属性，老版本自动回退属性 19）
            if dark {
                crate::theme::allow_dark_chrome(hwnd);
            }

            // 标题栏图标：与托盘/主窗口同源的 logo（资源 ID=1）。小图标按系统
            // 标题栏精确尺寸加载，避免大图缩放发虚；大图标供 Alt-Tab 等场景。
            // LR_SHARED 共享句柄，无需 DestroyIcon
            let hinst = GetModuleHandleW(std::ptr::null());
            let icon_sm = LoadImageW(
                hinst, 1usize as *const u16, IMAGE_ICON,
                GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON), LR_SHARED,
            );
            let icon_big = LoadImageW(
                hinst, 1usize as *const u16, IMAGE_ICON,
                GetSystemMetrics(SM_CXICON), GetSystemMetrics(SM_CYICON), LR_SHARED,
            );
            SendMessageW(hwnd, WM_SETICON, 0, icon_sm);
            SendMessageW(hwnd, WM_SETICON, 1, icon_big);

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

            // 品牌 banner：logo 位图（DIB）+ 标题/标语
            let face = wide(crate::util::UI_FONT_FACE);
            let font_title = CreateFontW(
                -17, 0, 0, 0, 600, 0, 0, 0, 0, 0, 0, 5, 0,
                face.as_ptr(),
            );
            let mut logo_dc = CreateCompatibleDC(0);
            let mut logo_bmp: isize = 0;
            if logo_dc != 0 {
                let mut bmi: BITMAPINFO = std::mem::zeroed();
                bmi.bmiHeader = BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: LOGO_SIZE,
                    biHeight: LOGO_SIZE, // 正高度 = 自底向上，与 .bin 排布一致
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..std::mem::zeroed()
                };
                let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
                let screen_dc = GetDC(hwnd);
                logo_bmp = CreateDIBSection(
                    screen_dc,
                    &bmi,
                    DIB_RGB_COLORS,
                    &mut bits,
                    0,
                    0,
                );
                ReleaseDC(hwnd, screen_dc);
                if logo_bmp != 0 && !bits.is_null() && LOGO_BIN.len() == (LOGO_SIZE * LOGO_SIZE * 4) as usize {
                    std::ptr::copy_nonoverlapping(LOGO_BIN.as_ptr(), bits as *mut u8, LOGO_BIN.len());
                    SelectObject(logo_dc, logo_bmp);
                } else {
                    DeleteDC(logo_dc);
                    logo_dc = 0;
                }
            }

            let st_title = mk("STATIC", "MiniNotes", WS_CHILD | WS_VISIBLE, 66, 12, 160, 24, ST_TITLE);
            let tag_text = format!("托盘常驻的极简便签 · v{}", env!("CARGO_PKG_VERSION"));
            let st_tag = mk("STATIC", &tag_text, WS_CHILD | WS_VISIBLE, 66, 38, 270, 16, ST_TAG);

            let st_label = mk("STATIC", "全局热键：", WS_CHILD | WS_VISIBLE, 14, 16 + BANNER_SHIFT, 62, 18, ST_LABEL);
            let ed = mk("EDIT", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_MULTILINE as u32, 80, 12 + BANNER_SHIFT, 172, 24, ED_HOTKEY);
            // 输入说明紧跟热键输入框，而不是挂在最底部
            let st_hint = mk(
                "STATIC",
                "点击热键输入框后按下组合键即可录入（如 Alt+N）。\n热键需包含 Ctrl / Alt / Shift / Win 至少一个修饰键。",
                WS_CHILD | WS_VISIBLE,
                14, 42 + BANNER_SHIFT, 336, 36,
                ST_HINT,
            );
            let st_theme = mk("STATIC", "主题：", WS_CHILD | WS_VISIBLE, 14, 88 + BANNER_SHIFT, 62, 18, ST_LABEL + 100);
            // 单选钮只留圆圈、文字由 STATIC 承担：系统自绘的单选/复选文字颜色
            // 不受 WM_CTLCOLORSTATIC 控制，暗色下会隐入背景（与下方复选框同理）；
            // SS_NOTIFY 让点击文字也能切换对应单选钮
            let rd_light = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32 | WS_GROUP as u32, 80, 86 + BANNER_SHIFT, 18, 18, RD_LIGHT);
            let rd_dark = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 148, 86 + BANNER_SHIFT, 18, 18, RD_DARK);
            let rd_system = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 216, 86 + BANNER_SHIFT, 18, 18, RD_SYSTEM);
            let st_rdl = mk("STATIC", "浅色", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 102, 87 + BANNER_SHIFT, 40, 18, ST_RDL);
            let st_rdd = mk("STATIC", "暗色", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 170, 87 + BANNER_SHIFT, 40, 18, ST_RDD);
            let st_rds = mk("STATIC", "跟随系统", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 238, 87 + BANNER_SHIFT, 84, 18, ST_RDS);
            // checkbox 文字由 STATIC 承担：DarkMode_Explorer 主题下系统自绘的
            // checkbox 文字颜色不受 WM_CTLCOLORSTATIC 控制，会隐入深色背景
            let chk_hide = mk(
                "BUTTON",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                14, 118 + BANNER_SHIFT, 18, 18,
                CHK_AUTOHIDE,
            );
            let st_hide = mk(
                "STATIC",
                "失焦时自动隐藏窗口",
                WS_CHILD | WS_VISIBLE | SS_NOTIFY,
                38, 119 + BANNER_SHIFT, 220, 18,
                ST_HIDE,
            );
            let chk_auto = mk(
                "BUTTON",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
                14, 144 + BANNER_SHIFT, 18, 18,
                CHK_AUTOSTART,
            );
            let st_auto = mk(
                "STATIC",
                "开机时自动启动 MiniNotes",
                WS_CHILD | WS_VISIBLE | SS_NOTIFY,
                38, 145 + BANNER_SHIFT, 220, 18,
                ST_AUTO,
            );
            let btn_save = mk("BUTTON", "保存", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 180, 176 + BANNER_SHIFT, 80, 30, BTN_SAVE);
            let btn_cancel = mk("BUTTON", "取消", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 268, 176 + BANNER_SHIFT, 80, 30, BTN_CANCEL);

            util::apply_font(st_title, font_title);
            util::apply_font(st_tag, font);
            for h in [st_label, ed, st_theme, rd_light, st_rdl, rd_dark, st_rdd, rd_system, st_rds, chk_hide, st_hide, chk_auto, st_auto, st_hint, btn_save, btn_cancel] {
                util::apply_font(h, font);
            }

            // 复选框/单选钮的图形与热键输入框的光标在暗色下用系统暗色主题绘制（文字由 STATIC 承担）
            let theme_name = if dark { "DarkMode_Explorer" } else { "Explorer" };
            for h in [rd_light, rd_dark, rd_system, chk_hide, chk_auto, ed] {
                SetWindowTheme(h, wide(theme_name).as_ptr(), std::ptr::null());
            }

            SetWindowTextW(ed, wide(&hotkey::text(m, v)).as_ptr());
            // 输入框文字垂直居中：单行 EDIT 永远在顶部画文本，框体越高下方空得越多；
            // EM_SETRECT 只对多行 EDIT 生效，故以 ES_MULTILINE 创建后把格式矩形
            // 垂直居中（capture_proc 已吞掉全部字符输入与粘贴，不会真的成为多行）。
            // 注意必须在 WM_SETFONT 之后发送：设置字体会重置格式矩形
            unsafe {
                let mut crc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(ed, &mut crc);
                let line_h = util::font_line_height(font);
                let fr = RECT {
                    left: 2,
                    top: (crc.bottom - line_h) / 2,
                    right: crc.right - 2,
                    bottom: crc.bottom,
                };
                SendMessageW(ed, EM_SETRECT, 0, &fr as *const RECT as isize);
            }
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
                    font_title,
                    logo_dc,
                    logo_bmp,
                    hover_save: false,
                    hover_cancel: false,
                    down_save: false,
                    down_cancel: false,
                    hk_dismissed: false,
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
        WM_PAINT => {
            // 窗口本体只画 banner 的 logo；其余区域由 WM_ERASEBKGND 铺底、控件自绘
            if !st_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            with_st(|s| unsafe {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                if s.logo_dc != 0 {
                    let bf = BLENDFUNCTION {
                        BlendOp: AC_SRC_OVER as u8,
                        BlendFlags: 0,
                        SourceConstantAlpha: 255,
                        AlphaFormat: AC_SRC_ALPHA as u8,
                    };
                    AlphaBlend(hdc, 14, 10, LOGO_SIZE, LOGO_SIZE, s.logo_dc, 0, 0, LOGO_SIZE, LOGO_SIZE, bf);
                }
                EndPaint(hwnd, &ps);
            });
            0
        }
        WM_CTLCOLORSTATIC => {
            let hdc = wp as isize;
            with_app(|app| unsafe {
                let p = theme::palette(app.dark);
                let ctl = lp as HWND;
                let id = windows_sys::Win32::UI::WindowsAndMessaging::GetDlgCtrlID(ctl);
                // 提示语与 banner 标语用次级文字色
                let sec = id == ST_HINT as i32 || id == ST_TAG as i32;
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, p.window_bg);
                windows_sys::Win32::Graphics::Gdi::SetTextColor(hdc, if sec { p.text_sec } else { p.text });
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
            } else if id == ST_RDL {
                unsafe { SendMessageW(find_child(hwnd, RD_LIGHT), BM_CLICK, 0, 0) };
            } else if id == ST_RDD {
                unsafe { SendMessageW(find_child(hwnd, RD_DARK), BM_CLICK, 0, 0) };
            } else if id == ST_RDS {
                unsafe { SendMessageW(find_child(hwnd, RD_SYSTEM), BM_CLICK, 0, 0) };
            }
            0
        }
        WM_CLOSE => {
            DestroyWindow(hwnd);
            0
        }
        WM_DESTROY => {
            with_st(|s| unsafe {
                // 恢复全局热键（录入期间已停用，见 open）。取消 / 保存同组合键 /
                // 保存新组合键三条路统一按 cfg 恢复：保存新组合时 apply 已把 cfg
                // 更新为新值并注册，这里重注册等价于幂等恢复
                if s.hk_dismissed {
                    with_app(|app| {
                        hotkey::unregister(app.hwnd);
                        if !hotkey::register(app.hwnd, app.cfg.hotkey_mods, app.cfg.hotkey_vk) {
                            let hk = hotkey::text(app.cfg.hotkey_mods, app.cfg.hotkey_vk);
                            crate::tray::notify(
                                app.hwnd,
                                "MiniNotes 热键恢复失败",
                                &format!("热键 {} 重新注册失败，可能已被其他程序占用，可左键点击托盘图标唤起。", hk),
                            );
                        }
                    });
                }
                EnableWindow(s.owner, 1);
                if windows_sys::Win32::UI::WindowsAndMessaging::IsWindowVisible(s.owner) != 0 {
                    SetForegroundWindow(s.owner);
                } else {
                    // 主窗口在设置期间被隐藏（如旧入口路径的失焦隐藏）：
                    // 关闭设置应回到主界面，而不是只剩托盘
                    crate::window::show_main();
                }
                util::destroy_font(s.font);
                util::destroy_font(s.font_title);
                if s.logo_bmp != 0 {
                    DeleteObject(s.logo_bmp);
                }
                if s.logo_dc != 0 {
                    DeleteDC(s.logo_dc);
                }
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
    if msg == WM_PASTE {
        return 1; // 阻止右键粘贴：热键只允许键盘录入，且换行会破坏多行化的显示
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
    // 无论热键是否注册成功，其余设置都照常保存：一次热键冲突不该把主题/自启等一起丢掉
    autostart_set(autost);
    with_app(|app| {
        app.cfg.auto_hide = auto_hide;
        app.cfg.autostart = autost;
        app.cfg.theme = theme_mode.to_string();
        crate::config::save(&app.cfg);
        crate::window::apply_theme(app);
    });
    if !hk_ok {
        unsafe {
            MessageBoxW(
                hwnd,
                wide(&format!(
                    "热键 {} 注册失败，可能已被其他程序占用，已保留原热键；其余设置已保存。",
                    hotkey::text(m, v)
                ))
                .as_ptr(),
                wide("热键冲突").as_ptr(),
                0x30,
            );
        };
    }
    unsafe { DestroyWindow(hwnd) };
}

fn find_child(parent: HWND, id: u32) -> HWND {
    unsafe {
        windows_sys::Win32::UI::WindowsAndMessaging::GetDlgItem(parent, id as i32)
    }
}
