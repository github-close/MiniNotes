use std::cell::RefCell;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BeginPaint, BLENDFUNCTION, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, CreateCompatibleDC, CreateDIBSection, CreateFontW, DIB_RGB_COLORS,
    DeleteDC, EndPaint, FillRect, GetDC, InvalidateRect, CreateSolidBrush, DeleteObject,
    PAINTSTRUCT, ReleaseDC, SelectObject, DEFAULT_CHARSET, DrawTextW, EnumFontFamiliesExW,
    GetTextFaceW, LOGFONTW, SetBkMode, SetTextColor, TEXTMETRICW, TRANSPARENT, DT_END_ELLIPSIS,
    DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Registry::{
    RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
};
use windows_sys::Win32::UI::Controls::{
    SetWindowTheme, BST_CHECKED, DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODT_COMBOBOX,
    ODS_COMBOBOXEDIT, ODS_SELECTED, WM_MOUSELEAVE,
};
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
    WM_CTLCOLORSTATIC, WM_CTLCOLOREDIT, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND, WM_MEASUREITEM,
    WM_PAINT,
    BM_CLICK, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_SYSKEYDOWN, WS_GROUP,
    WS_CAPTION, WS_CHILD, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    ES_MULTILINE, WM_PASTE,
    LoadImageW, GetSystemMetrics, IMAGE_ICON, LR_SHARED, WM_SETICON,
    SM_CXICON, SM_CYICON, SM_CXSMICON, SM_CYSMICON,
    WNDCLASSW, IDC_ARROW,
    CBS_DROPDOWNLIST, CBS_OWNERDRAWFIXED, CBS_HASSTRINGS,
    CB_ADDSTRING, CB_SETCURSEL, CB_GETCURSEL,
    CB_GETLBTEXT, CB_GETLBTEXTLEN, CB_ERR, WM_CTLCOLORLISTBOX, WS_VSCROLL,
    CBN_DROPDOWN,
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
// 编辑器字体行（笔记输入区独立字体）
const ST_EFONT: u32 = 18; // 「编辑器字体：」标签
const CB_EFONT: u32 = 19; // 字体族下拉
const CB_ESIZE: u32 = 20; // 字号下拉（可输入）
// 第四档主题（浅黄）：单选钮 + 其 STATIC 文字标签
const RD_YELLOW: u32 = 21;
const ST_RDY: u32 = 22;
const SUB_CAPTURE: usize = 10;
const SUB_SBTN: usize = 11;
const SUB_COMBO: usize = 12;
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
    // 字体/字号下拉的已提交选中行（-1 = 无）。owner-draw 列表的 ODS_SELECTED
    // 会跟随鼠标/键盘移动（原生高亮），"当前值"需要自行记录才能画成深色
    sel_face: i32,
    sel_size: i32,
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
        // 宽 384：主题行要放下四个单选钮（浅色/暗色/浅黄/跟随系统）
        let mut rc = RECT { left: 0, top: 0, right: 384, bottom: 258 + BANNER_SHIFT };
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
            let dark = with_app(|app| app.theme.is_dark());
            let (m, v, auto_hide, theme_mode) = with_app(|app| {
                (app.cfg.hotkey_mods, app.cfg.hotkey_vk, app.cfg.auto_hide, app.cfg.theme.clone())
            });
            let (ed_face, ed_size) =
                with_app(|app| (app.cfg.ed_font.clone(), app.cfg.ed_size));
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
                14, 42 + BANNER_SHIFT, 360, 36,
                ST_HINT,
            );
            let st_theme = mk("STATIC", "主题：", WS_CHILD | WS_VISIBLE, 14, 88 + BANNER_SHIFT, 62, 18, ST_LABEL + 100);
            // 单选钮只留圆圈、文字由 STATIC 承担：系统自绘的单选/复选文字颜色
            // 不受 WM_CTLCOLORSTATIC 控制，暗色下会隐入背景（与下方复选框同理）；
            // SS_NOTIFY 让点击文字也能切换对应单选钮
            let rd_light = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32 | WS_GROUP as u32, 80, 86 + BANNER_SHIFT, 18, 18, RD_LIGHT);
            let rd_dark = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 148, 86 + BANNER_SHIFT, 18, 18, RD_DARK);
            let rd_yellow = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 216, 86 + BANNER_SHIFT, 18, 18, RD_YELLOW);
            let rd_system = mk("BUTTON", "", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_AUTORADIOBUTTON as u32, 284, 86 + BANNER_SHIFT, 18, 18, RD_SYSTEM);
            let st_rdl = mk("STATIC", "浅色", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 102, 87 + BANNER_SHIFT, 40, 18, ST_RDL);
            let st_rdd = mk("STATIC", "暗色", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 170, 87 + BANNER_SHIFT, 40, 18, ST_RDD);
            let st_rdy = mk("STATIC", "浅黄", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 238, 87 + BANNER_SHIFT, 40, 18, ST_RDY);
            let st_rds = mk("STATIC", "跟随系统", WS_CHILD | WS_VISIBLE | SS_NOTIFY, 306, 87 + BANNER_SHIFT, 62, 18, ST_RDS);
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
            // 编辑器字体行：owner-draw（DarkMode_Explorer 在 Win10 上画不黑 combo
            // 选字段与列表底，字段/条目改由对话框用色板自绘，见 WM_DRAWITEM；
            // HASSTRINGS 让 CB_GETLBTEXT 拿回文本）；下拉列表高度参数 = 展开后的总高
            let st_edfont = mk("STATIC", "编辑器字体：", WS_CHILD | WS_VISIBLE, 14, 176 + BANNER_SHIFT, 78, 18, ST_EFONT);
            let cb_face = mk(
                "COMBOBOX",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL as u32
                    | CBS_OWNERDRAWFIXED as u32 | CBS_HASSTRINGS as u32,
                94, 174 + BANNER_SHIFT, 204, 240,
                CB_EFONT,
            );
            let cb_size = mk(
                "COMBOBOX",
                "",
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | CBS_DROPDOWNLIST as u32 | WS_VSCROLL as u32
                    | CBS_OWNERDRAWFIXED as u32 | CBS_HASSTRINGS as u32,
                304, 174 + BANNER_SHIFT, 62, 240,
                CB_ESIZE,
            );
            let btn_save = mk("BUTTON", "保存", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 204, 214 + BANNER_SHIFT, 80, 30, BTN_SAVE);
            let btn_cancel = mk("BUTTON", "取消", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 292, 214 + BANNER_SHIFT, 80, 30, BTN_CANCEL);

            util::apply_font(st_title, font_title);
            util::apply_font(st_tag, font);
            for h in [st_label, ed, st_theme, rd_light, st_rdl, rd_dark, st_rdd, rd_yellow, st_rdy, rd_system, st_rds, chk_hide, st_hide, chk_auto, st_auto, st_hint, btn_save, btn_cancel, st_edfont, cb_face, cb_size] {
                util::apply_font(h, font);
            }

            // 复选框/单选钮的图形与热键输入框的光标在暗色下用系统暗色主题绘制（文字由 STATIC 承担）；
            // ComboBox 挂同款主题，下拉滚动条/箭头跟随暗色（列表配色见 WM_CTLCOLORLISTBOX）
            let theme_name = if dark { "DarkMode_Explorer" } else { "Explorer" };
            for h in [rd_light, rd_dark, rd_yellow, rd_system, chk_hide, chk_auto, ed, cb_face, cb_size] {
                SetWindowTheme(h, wide(theme_name).as_ptr(), std::ptr::null());
            }

            // 字体下拉：枚举全部系统字体并预选当前值；字号下拉：预设 + 手输
            fill_face_combo(cb_face, &ed_face);
            for s in ED_SIZES {
                SendMessageW(cb_size, CB_ADDSTRING, 0, wide(&s.to_string()).as_ptr() as isize);
            }
            // 预选当前字号（列表里找不到就不预选，应用时保留原值）
            if let Some(i) = ED_SIZES.iter().position(|&s| s == ed_size) {
                SendMessageW(cb_size, CB_SETCURSEL, i as usize, 0);
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
            send_checked(rd_yellow, theme_mode == "yellow");
            send_checked(rd_system, theme_mode == "system");
            send_checked(chk_hide, auto_hide);
            send_checked(chk_auto, autost);
            SetWindowSubclass(ed, Some(capture_proc), SUB_CAPTURE, 0);
            // ComboBox 的编辑子控件（CBS_DROPDOWN）与下拉列表发出的 CTLCOLOR 消息
            // 发给 combo 本身：DarkMode_Explorer 主题下 combo 不再转发给对话框，
            // 对话框的配色臂收不到 → 编辑区留白底。子类转发给对话框取暗色画刷
            SetWindowSubclass(cb_face, Some(combo_proc), SUB_COMBO, 0);
            SetWindowSubclass(cb_size, Some(combo_proc), SUB_COMBO, 0);
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
                    sel_face: -1,
                    sel_size: -1,
                    hk_dismissed: false,
                })
            });
            0
        }
        WM_ERASEBKGND => {
            with_app(|app| unsafe {
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut rc);
                let b = CreateSolidBrush(theme::palette(app.theme).window_bg);
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
                let p = theme::palette(app.theme);
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
                let p = theme::palette(app.theme);
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, p.surface2);
                windows_sys::Win32::Graphics::Gdi::SetTextColor(hdc, p.text);
                app.brushes.surface2
            })
        }
        WM_CTLCOLORLISTBOX => {
            // ComboBox 的下拉列表与选中字段（dropdownlist 的静态区也走这条链路）
            // 着色：底色与输入框一致，暗色下不露白
            let hdc = wp as isize;
            with_app(|app| unsafe {
                let p = theme::palette(app.theme);
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, p.surface2);
                windows_sys::Win32::Graphics::Gdi::SetTextColor(hdc, p.text);
                app.brushes.surface2
            })
        }
        WM_MEASUREITEM => {
            // owner-draw combo 的条目高度（combo 创建期间就会重入此消息，
            // 不触碰 ST 状态）
            let mi = lp as *mut MEASUREITEMSTRUCT;
            if !mi.is_null() && (*mi).CtlType as u32 == ODT_COMBOBOX {
                unsafe { (*mi).itemHeight = 24 };
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DRAWITEM => {
            let di = lp as *const DRAWITEMSTRUCT;
            if !di.is_null() {
                if (*di).CtlType as u32 == ODT_COMBOBOX {
                    with_st(|s| {
                        let committed = if (*di).CtlID == CB_EFONT { s.sel_face } else { s.sel_size };
                        with_app(|app| unsafe {
                            draw_combo_item(di, app.theme, s.font, committed);
                        });
                    });
                    return 1;
                }
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
                            app.theme,
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
            } else if id == ST_RDY {
                unsafe { SendMessageW(find_child(hwnd, RD_YELLOW), BM_CLICK, 0, 0) };
            } else if id == CB_EFONT || id == CB_ESIZE {
                // 下拉列表通知：展开瞬间记录已提交选中行，供 owner-draw 画"当前值"深色。
                // 原生高亮（ODS_SELECTED）会跟随鼠标/键盘移动，画成浅色；点击/Enter
                // 提交后列表关闭，下次展开重新读回——所以只在 DROPDOWN 时读一次，
                // 不能用 SELCHANGE（鼠标划过列表时它也会连续触发，会跟着鼠标变）
                let code = ((wp >> 16) & 0xFFFF) as u16 as u32;
                if code == CBN_DROPDOWN {
                    with_st(|s| unsafe {
                        let cb = find_child(s.hwnd, id);
                        let sel = SendMessageW(cb, CB_GETCURSEL, 0, 0);
                        let sel = if sel as i32 == CB_ERR { -1 } else { sel as i32 };
                        if id == CB_EFONT {
                            s.sel_face = sel;
                        } else {
                            s.sel_size = sel;
                        }
                    });
                }
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

/// owner-draw combo 条目/选字段两档高亮：已提交选中（当前值）accent_soft（深）、
/// 原生高亮（ODS_SELECTED，跟随鼠标/键盘移动）accent_tint（浅）、普通 surface2。
/// 深色优先于浅色（悬停停在被选中的行上时仍是深色）。DarkMode_Explorer 在
/// Win10 上不会把 combo 选字段/列表底画成暗色，改由这里自绘。
/// itemID = u32::MAX 是空选字段（只铺底）；ODS_COMBOBOXEDIT 标记的是编辑/字段区
/// 绘制，不参与任何高亮
unsafe fn draw_combo_item(di: *const DRAWITEMSTRUCT, t: theme::Theme, font: isize, committed: i32) {
    let p = theme::palette(t);
    let rc = (*di).rcItem;
    let in_field = ((*di).itemState & ODS_COMBOBOXEDIT) != 0;
    let highlight = !in_field && ((*di).itemState & ODS_SELECTED) != 0;
    let committed_hit = !in_field && committed >= 0 && (*di).itemID as i32 == committed;
    let bg = if committed_hit {
        p.accent_soft
    } else if highlight {
        p.accent_tint
    } else {
        p.surface2
    };
    let b = CreateSolidBrush(bg);
    FillRect((*di).hDC, &rc, b);
    DeleteObject(b);
    let id = (*di).itemID;
    if id == u32::MAX {
        return;
    }
    // CBS_HASSTRINGS：CB_GETLBTEXT 取回条目文本（返回字符数，出错为 -1）
    let mut buf = [0u16; 64];
    let n = SendMessageW((*di).hwndItem, CB_GETLBTEXT, id as usize, buf.as_mut_ptr() as isize);
    if n <= 0 {
        return;
    }
    SetBkMode((*di).hDC, TRANSPARENT as i32);
    SetTextColor((*di).hDC, p.text);
    SelectObject((*di).hDC, font);
    let mut r = rc;
    r.left += 6;
    DrawTextW(
        (*di).hDC,
        buf.as_ptr(),
        n as i32,
        &mut r,
        DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS | DT_NOPREFIX,
    );
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

/// ComboBox 子类：把编辑子控件/下拉列表发到 combo 的 CTLCOLOR 消息转发给
/// 对话框（combo 挂 DarkMode_Explorer 后不再自动转发，见安装处注释），
/// 返回值照抄对话框给的画刷
unsafe extern "system" fn combo_proc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _d: usize,
) -> LRESULT {
    if msg == WM_CTLCOLOREDIT || msg == WM_CTLCOLORLISTBOX {
        let parent = windows_sys::Win32::UI::WindowsAndMessaging::GetParent(hwnd);
        return SendMessageW(parent, msg, wp, lp);
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

// ---------- 编辑器字体下拉 ----------

// 字号下拉的预设值（8~72 覆盖常用范围；不做可输入字号——CBS_DROPDOWN 的
// 编辑子控件在 Win10 暗色 comctl 下自身底色画不黑，见 draw_combo_item 注释）
const ED_SIZES: [i32; 23] = [
    8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 20, 22, 24, 26, 28, 32, 36, 40, 48, 56, 64, 72,
];

/// 枚举系统全部字体族名。DEFAULT_CHARSET 覆盖所有字符集；@ 开头的是东亚竖排
/// 变体（横排文本用不上）；同名族按字符集重复出现，去重后大小写不敏感排序
/// （CJK 名按码点自然排后）。回调只做字符串收集，无消息泵，裸指针传参安全
fn enum_font_faces() -> Vec<String> {
    unsafe extern "system" fn collect(
        lf: *const LOGFONTW,
        _tm: *const TEXTMETRICW,
        _ty: u32,
        lp: LPARAM,
    ) -> i32 {
        let out = unsafe { &mut *(lp as *mut Vec<String>) };
        let face = util::from_wide(&unsafe { &*lf }.lfFaceName);
        if !face.is_empty() && !face.starts_with('@') && !out.contains(&face) {
            out.push(face);
        }
        1 // 非零 = 继续枚举
    }
    let mut faces: Vec<String> = Vec::new();
    unsafe {
        let hdc = GetDC(0);
        if hdc != 0 {
            let mut lf: LOGFONTW = std::mem::zeroed();
            lf.lfCharSet = DEFAULT_CHARSET;
            EnumFontFamiliesExW(hdc, &lf, Some(collect), &mut faces as *mut _ as LPARAM, 0);
            ReleaseDC(0, hdc);
        }
    }
    faces.sort_by_key(|f| f.to_lowercase());
    faces
}

/// 列表里找字体名（大小写不敏感的精确匹配）
fn face_index(faces: &[String], name: &str) -> Option<usize> {
    if name.is_empty() {
        return None;
    }
    faces.iter().position(|f| f.eq_ignore_ascii_case(name))
}

/// 填充字体下拉并预选当前值。配置名精确匹配不到时（字体有本地化别名，
/// 枚举名与英文名不一致），以配置名建字体后 GetTextFace 取 GDI 解析出的
/// 真实族名再匹配；仍找不到则不预选——应用保存时保留原配置不被静默改写
unsafe fn fill_face_combo(cb: HWND, current: &str) {
    let faces = enum_font_faces();
    for f in &faces {
        SendMessageW(cb, CB_ADDSTRING, 0, wide(f).as_ptr() as isize);
    }
    let mut pick = face_index(&faces, current);
    if pick.is_none() {
        let mem = CreateCompatibleDC(0);
        if mem != 0 {
            let hf = util::create_font_face_px(current, -12, 400, false);
            if hf != 0 {
                let old = SelectObject(mem, hf);
                let mut buf = [0u16; 64];
                let n = GetTextFaceW(mem, buf.len() as i32 - 1, buf.as_mut_ptr()).max(0) as usize;
                SelectObject(mem, old);
                DeleteObject(hf);
                let resolved = util::from_wide(&buf[..n.min(buf.len())]);
                pick = face_index(&faces, resolved.trim());
            }
            DeleteDC(mem);
        }
    }
    if let Some(i) = pick {
        SendMessageW(cb, CB_SETCURSEL, i as usize, 0);
    }
}

/// 读回下拉当前项文本（无选中 = 保留原配置）。字体与字号共用：
/// 两者都是 CBS_DROPDOWNLIST，条目即文本
unsafe fn combo_selection(parent: HWND, id: u32) -> Option<String> {
    let cb = find_child(parent, id);
    let sel = SendMessageW(cb, CB_GETCURSEL, 0, 0);
    if sel as i32 == CB_ERR {
        return None;
    }
    let len = SendMessageW(cb, CB_GETLBTEXTLEN, sel as usize, 0) as usize;
    if len == 0 || len > 512 {
        return None;
    }
    let mut buf = vec![0u16; len + 1];
    SendMessageW(cb, CB_GETLBTEXT, sel as usize, buf.as_mut_ptr() as isize);
    let s = util::from_wide(&buf);
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

fn apply(hwnd: HWND) {
    let (m, v) = with_st(|s| (s.hk_mods, s.hk_vk));
    let auto_hide = with_st(|s| is_checked(s.chk_autohide));
    let autost = with_st(|s| is_checked(s.chk_autostart));
    let theme_mode = with_st(|s| {
        if is_checked(find_child(s.hwnd, RD_DARK)) {
            "dark"
        } else if is_checked(find_child(s.hwnd, RD_YELLOW)) {
            "yellow"
        } else if is_checked(find_child(s.hwnd, RD_SYSTEM)) {
            "system"
        } else {
            "light"
        }
    });
    // 编辑器字体：下拉无选中 / 字号不在预设列表都保留原配置（字体被卸载等边界不静默改写）
    let (ed_face, ed_size) = with_st(|s| unsafe {
        let face = combo_selection(s.hwnd, CB_EFONT);
        let size = combo_selection(s.hwnd, CB_ESIZE).and_then(|t| t.trim().parse::<i32>().ok());
        (face, size)
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
        if let Some(f) = ed_face {
            app.cfg.ed_font = f;
        }
        if let Some(n) = ed_size {
            app.cfg.ed_size = n.clamp(8, 72);
        }
        crate::config::save(&app.cfg);
        crate::window::apply_editor_font(app);
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
