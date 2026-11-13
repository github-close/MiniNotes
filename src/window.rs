use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, InvalidateRect, LineTo, MoveToEx,
    SelectObject, SetBkMode, SetTextColor, DT_CENTER, DT_LEFT, DT_RIGHT, DT_SINGLELINE, DT_VCENTER,
    HDC, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Controls::{
    ImageList_Add, ImageList_Create, LVM_SETIMAGELIST,
    LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETTEXTCOLOR, LVM_SETTEXTBKCOLOR, ILC_COLOR32,
    LVS_EX_FULLROWSELECT, LVS_NOCOLUMNHEADER, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVIF_STATE, LVIS_SELECTED, LVSIL_SMALL, WM_MOUSELEAVE, DRAWITEMSTRUCT, NMHDR, ODS_SELECTED,
    NMLISTVIEW, NMITEMACTIVATE, NMLVCUSTOMDRAW,
    LVM_GETITEMRECT, LVM_HITTEST, LVHITTESTINFO,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetFocus, GetKeyState, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VK_CONTROL,
    VK_DELETE, VK_ESCAPE, VK_RETURN, VK_SPACE,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow,
    DrawIconEx, EN_KILLFOCUS, EN_SETFOCUS, GetAncestor, GetClientRect,
    GetForegroundWindow, GetSystemMetrics, GetWindowRect, GetWindowThreadProcessId, GA_ROOT, HTBOTTOM,
    HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT,
    HTTOPRIGHT, IDC_ARROW, IsIconic, IsWindowVisible, IsZoomed, KillTimer, LoadCursorW,
    MINMAXINFO, MoveWindow, NCCALCSIZE_PARAMS, PostMessageW, PostQuitMessage, RegisterClassW,
    SendMessageW, SetForegroundWindow, SetTimer, ShowWindow, SystemParametersInfoW,
    SIZE_MINIMIZED, SW_HIDE, SW_RESTORE, SW_SHOW, SPI_GETWORKAREA, SM_CXSIZEFRAME,
    SM_CXPADDEDBORDER, SM_CYSIZEFRAME,
    BS_OWNERDRAW, DI_NORMAL, WNDCLASSW,
    WM_ACTIVATE, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CTLCOLORSTATIC, WM_MOUSEWHEEL,
    WM_CTLCOLOREDIT, WM_DESTROY, WM_DRAWITEM, WM_ERASEBKGND, WM_GETMINMAXINFO, WM_HOTKEY,
    WM_KEYDOWN, WM_KILLFOCUS, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCCALCSIZE, WM_PASTE,
    WM_NCHITTEST,
    WM_NOTIFY, WM_PAINT, WM_RBUTTONUP, WM_SETTINGCHANGE, WM_SIZE, WM_SYSKEYDOWN,
    WM_TIMER, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL, WS_MAXIMIZEBOX, WS_MINIMIZEBOX, WS_SYSMENU, WS_THICKFRAME, WS_CLIPCHILDREN,
};

use crate::config::Config;
use crate::{hotkey, notes, settings, todos, tray, theme, util};

// ---------- 布局常量 ----------

pub const MAIN_CLASS: &str = "MiniNotesMainWnd";

const TITLE_H: i32 = 36;   // 自绘标题栏高度
const CONTENT_Y: i32 = 84; // 内容起始 y = 标题栏 36 + Tab 条 48
const PILL_W: i32 = 74;
const PILL_H: i32 = 32;
const PILL_GAP: i32 = 8;
const BORDER_RESIZE: i32 = 8; // 边缘可拖拽调整大小的宽度

pub const MSG_SHOW: u32 = WM_APP + 2;
pub const MSG_ESC_HIDE: u32 = WM_APP + 3;
pub const MSG_NEW_NOTE: u32 = WM_APP + 4;
pub const MSG_DEL_NOTE: u32 = WM_APP + 5;
pub const MSG_DEL_TODO: u32 = WM_APP + 6;
pub const MSG_ADD_TODO: u32 = WM_APP + 7;
pub const MSG_TOGGLE_TODO: u32 = WM_APP + 8;

pub const ID_TODO_INPUT: u32 = 201;
pub const ID_TODO_ADD: u32 = 202;
pub const ID_TODO_LIST: u32 = 203;
pub const ID_TODO_CLEAR: u32 = 204;
pub const ID_SETTINGS_BTN: u32 = 210;
pub const ID_NOTE_LIST: u32 = 301;
pub const ID_NOTE_NEW: u32 = 302;
pub const ID_EDITOR: u32 = 303;
pub const TIMER_SAVE: usize = 1;

pub const TAB_TODO: usize = 0;
const TAB_NOTE: usize = 1;

const SUB_EDITOR: usize = 1;
const SUB_TODO_INPUT: usize = 2;
const SUB_NOTE_LIST: usize = 3;
const SUB_TODO_LIST: usize = 4;
const SUB_RENAME_EDIT: usize = 5;
const SUB_BTN: usize = 10; // owner-draw 按钮的悬停跟踪子类

// 列表右键菜单命令
const M_NOTE_NEW: u32 = 2001;
const M_NOTE_RENAME: u32 = 2002;
const M_NOTE_DELETE: u32 = 2003;
const M_TODO_EDIT: u32 = 2101;
const M_TODO_DELETE: u32 = 2102;
const M_TODO_CLEAR: u32 = 2103;

const VK_N: u32 = 0x4E;

// EDIT 控件常用消息（windows-sys 0.52 缺失）
const EM_GETRECT: u32 = 0x00B2;
const EM_SETRECT: u32 = 0x00B3;
const EM_SETLIMITTEXT: u32 = 0x00C5;
const EM_SETMARGINS: u32 = 0x00D3;
const CLR_NONE: COLORREF = 0xFFFF_FFFF;
const EN_CHANGE: u32 = 0x0300;

// ---------- 状态（仅 UI 线程） ----------

pub struct App {
    pub hwnd: HWND,
    pub hicon: isize,
    pub cfg: Config,

    pub todo_input: HWND,
    pub todo_add: HWND,
    pub todo_list: HWND,
    pub todo_clear: HWND,
    pub settings_btn: HWND,
    pub note_list: HWND,
    pub note_new: HWND,
    pub editor: HWND,

    pub font: isize,
    pub font_strike: isize,
    pub font_title: isize,

    pub notes: Vec<notes::NoteMeta>,
    pub cur_note: Option<usize>,
    pub note_dirty: bool,
    pub suppress_notify: bool,
    pub todos: Vec<todos::TodoItem>,
    pub settings_open: bool,
    pub initialized: bool,

    // 主题
    pub dark: bool,
    pub brushes: theme::Brushes,
    // 自绘状态
    pub cur_tab: usize,
    pub hover_tab: i32, // -1 = 无
    pub hover_close: bool,
    pub mouse_tracked: bool,
    pub himl: isize,
    pub btn_states: Vec<(isize, bool, bool)>, // (hwnd, hover, down)
    pub todo_slot: RECT,                      // 待办输入框的视觉槽（父窗口画描边用）
    pub input_line_h: i32,                    // UI 字体行高（输入框控件高度 = 行高）
    pub rename: Option<crate::rename::State>, // 进行中的内联重命名（自管理编辑框）
    pub note_drag: Option<NoteDrag>,          // 笔记列表拖拽排序状态
}

/// 笔记列表拖拽：from = 按下时的行；sx/sy = 按下点；cx/cy = 当前光标（列表客户区
/// 坐标，供自绘插入线取位）；line = 当前插入线 y（-1 = 未画）；active = 拖拽中
#[derive(Clone, Copy)]
pub struct NoteDrag {
    pub from: i32,
    pub sx: i32,
    pub sy: i32,
    pub cx: i32,
    pub cy: i32,
    pub line: i32,
    pub active: bool,
}

// 拖拽排序：插入指示线由列表自绘管线（notes::on_custom_draw POSTPAINT）画，
// 不用 comctl 插入标记——LVM_SETINSERTMARK 在本列表配置下实测不生效

// Win32 消息天然重入（SendMessage 会同步回调 wndproc），RefCell 会撞借用 panic，
// 因此用静态裸指针保存 App（UI 单线程，等同 C 的 GWLP_USERDATA 模式）。
// 注意：不能对 Option<Box> 用 ptr::read()——会按值拷贝出临时 Box 导致误 drop（double-free）
static mut APP: Option<Box<App>> = None;
static mut GDIP: usize = 0;

pub fn set_app(a: App) {
    unsafe {
        let p = std::ptr::addr_of_mut!(APP);
        *p = Some(Box::new(a));
    }
}

pub fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    unsafe {
        let p = std::ptr::addr_of_mut!(APP);
        match (*p).as_mut() {
            Some(b) => f(b.as_mut()),
            None => panic!("APP not initialized"),
        }
    }
}

fn app_ready() -> bool {
    unsafe { (*std::ptr::addr_of_mut!(APP)).is_some() }
}

/// 重命名编辑框安装子类（rename.rs 调用，避免把子类常量扩散出去）
pub fn set_rename_subclass(edit: HWND) {
    unsafe { SetWindowSubclass(edit, Some(child_subclass), SUB_RENAME_EDIT, 0) };
}

pub fn init_gdiplus() {
    unsafe {
        let p = std::ptr::addr_of_mut!(GDIP);
        *p = theme::gdiplus_init();
    }
}

// ---------- 创建 ----------

pub fn init_common_controls() {
    let icc = windows_sys::Win32::UI::Controls::INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<windows_sys::Win32::UI::Controls::INITCOMMONCONTROLSEX>() as u32,
        dwICC: windows_sys::Win32::UI::Controls::ICC_LISTVIEW_CLASSES
            | windows_sys::Win32::UI::Controls::ICC_STANDARD_CLASSES,
    };
    unsafe { windows_sys::Win32::UI::Controls::InitCommonControlsEx(&icc) };
}

pub fn create(hicon: isize) -> HWND {
    unsafe {
        let class = util::wide(MAIN_CLASS);
        let wc: WNDCLASSW = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: class.as_ptr(),
            hIcon: hicon,
            hCursor: LoadCursorW(0, IDC_ARROW),
            hbrBackground: 0, // 自己负责全部绘制，避免闪烁
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);

        let mut wa = RECT { left: 0, top: 0, right: 1280, bottom: 720 };
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut core::ffi::c_void, 0);
        // 默认 480x360（720x540 的三分之二）：无滚动可容纳列表+编辑器，
        // 字号不变，只是内容区收窄；用户拉伸后仍按保存尺寸恢复
        let (cx, cy) = (480i32, 360i32);
        let x = wa.left + ((wa.right - wa.left - cx) / 2).max(0);
        let y = wa.top + ((wa.bottom - wa.top - cy) / 2).max(0);

        // 无标题栏但保留厚边框：DWM 阴影、边缘拖拽调整大小、Win+方向键贴靠都还在
        // WS_CLIPCHILDREN：父窗口绘制永远不覆盖子控件区域——没有它，
        // 每次父窗口重绘都会先盖住子控件再等子控件重画，表现为整个页面持续闪烁
        let style = WS_THICKFRAME | WS_SYSMENU | WS_MINIMIZEBOX | WS_MAXIMIZEBOX | WS_CLIPCHILDREN;
        CreateWindowExW(
            0,
            class.as_ptr(),
            util::wide("MiniNotes").as_ptr(),
            style,
            x,
            y,
            cx,
            cy,
            0,
            0,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    }
}

fn child(parent: HWND, class: &str, text: &str, style: u32, exstyle: u32, id: u32) -> HWND {
    unsafe {
        CreateWindowExW(
            exstyle,
            util::wide(class).as_ptr(),
            util::wide(text).as_ptr(),
            style,
            0,
            0,
            0,
            0,
            parent,
            id as isize,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    }
}

/// 26px 高的透明小图列表：挂到列表上把行高撑起来，给自绘勾选框留空间
/// 勾选框图标列表：3 张不透明 24x26 图（0=纯行背景，1=未完成圆圈，2=完成勾选）。
/// 这台 Win10 的 ImageList 不支持 alpha/mask 透明（全画成黑色不透明块），
/// 因此图标自带行背景色底——与列表底色相同，视觉无缝；选中行的高亮从图标槽右侧开始。
fn make_checkbox_imagelist(dark: bool) -> isize {
    let p = theme::palette(dark);
    unsafe {
        let himl = ImageList_Create(24, 26, ILC_COLOR32, 3, 1);
        // 0 = 纯背景
        add_icon(himl, |_, _| p.window_bg);
        // 1 = 未完成：圆圈描边
        add_icon(himl, |x, y| {
            let d = ((x as f32 + 0.5) - 12.0).hypot((y as f32 + 0.5) - 13.0);
            if d > 5.2 && d < 6.4 {
                p.text_disabled
            } else {
                p.window_bg
            }
        });
        // 2 = 完成：实心圆 + 白色对勾
        add_icon(himl, |x, y| {
            let d = ((x as f32 + 0.5) - 12.0).hypot((y as f32 + 0.5) - 13.0);
            if d < 6.8 {
                // 对勾两段线：像素接近线段即命中
                let da = seg_dist(x as f32 + 0.5, y as f32 + 0.5, 8.8, 13.2, 11.0, 16.0);
                let db = seg_dist(x as f32 + 0.5, y as f32 + 0.5, 11.0, 16.0, 15.4, 9.4);
                if da.min(db) < 1.05 {
                    p.on_accent
                } else {
                    p.accent
                }
            } else {
                p.window_bg
            }
        });
        himl
    }
}

/// 生成一张图标并挂进 ImageList。ImageList_Add 只拷贝位图，DIB 用完必须自己释放
fn add_icon(himl: isize, fill: impl Fn(i32, i32) -> COLORREF) {
    unsafe {
        let hb = make_icon_dib(fill);
        ImageList_Add(himl, hb, 0);
        DeleteObject(hb);
    }
}

/// 创建 24x26 的 32bpp top-down DIB 并按像素填充（COLORREF -> 0xFFBBGGRR 像素值）
unsafe fn make_icon_dib(fill: impl Fn(i32, i32) -> COLORREF) -> windows_sys::Win32::Graphics::Gdi::HBITMAP {
    let mut bi: windows_sys::Win32::Graphics::Gdi::BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<windows_sys::Win32::Graphics::Gdi::BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = 24;
    bi.bmiHeader.biHeight = -26;
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = 0; // BI_RGB
    let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
    let hdib = windows_sys::Win32::Graphics::Gdi::CreateDIBSection(0, &bi, 0, &mut bits, 0, 0);
    if hdib != 0 && !bits.is_null() {
        let px = bits as *mut u32;
        for y in 0..26 {
            for x in 0..24 {
                let c = fill(x, y);
                // COLORREF 是 0x00BBGGRR，32bpp DIB 像素的 u32 是 0xAARRGGBB：需要交换 R/B
                *px.add((y * 24 + x) as usize) =
                    0xFF00_0000 | ((c & 0xFF) << 16) | (c & 0xFF00) | ((c >> 16) & 0xFF);
            }
        }
    }
    hdib
}

/// 点到线段的最短距离
fn seg_dist(px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32) -> f32 {
    let abx = bx - ax;
    let aby = by - ay;
    let len2 = abx * abx + aby * aby;
    let t = (((px - ax) * abx + (py - ay) * aby) / len2).clamp(0.0, 1.0);
    let cx = ax + t * abx;
    let cy = ay + t * aby;
    ((px - cx).powi(2) + (py - cy).powi(2)).sqrt()
}

/// 创建 App 状态与全部子控件，加载持久化数据并恢复上次状态
pub fn init_app(hwnd: HWND, cfg: Config, hicon: isize) {
    let todos_data = todos::load();
    let dark = theme::resolve_dark(&cfg.theme);
    let mut brushes = theme::Brushes::new();
    brushes.rebuild(dark);
    // 弹出菜单（列表右键/托盘）暗色除进程级 PreferredAppMode 外，
    // 还要求 owner 窗口允许暗色，缺一则菜单保持系统浅色
    theme::allow_dark_chrome(hwnd);
    set_app(App {
        hwnd,
        hicon,
        cfg,
        todo_input: 0,
        todo_add: 0,
        todo_list: 0,
        todo_clear: 0,
        settings_btn: 0,
        note_list: 0,
        note_new: 0,
        editor: 0,
        font: 0,
        font_strike: 0,
        font_title: 0,
        notes: Vec::new(),
        cur_note: None,
        note_dirty: false,
        suppress_notify: false,
        todos: todos_data,
        settings_open: false,
        initialized: false,
        dark,
        brushes,
        cur_tab: 1,
        hover_tab: -1,
        hover_close: false,
        mouse_tracked: false,
        himl: 0,
        btn_states: Vec::new(),
        todo_slot: RECT { left: 0, top: 0, right: 0, bottom: 0 },
        input_line_h: 20,
        rename: None,
        note_drag: None,
    });

    unsafe {
        // 无边框平面输入框（边框由主窗口绘制）
        // 待办输入框：单行 EDIT，控件高度取字体行高、垂直嵌进 30px 槽内——
        // 单行控件的光标与文字同层、撑满控件高度，槽内居中后光标随文字居中
        // （多行 EDIT 的光标只有一行字高、锚在格式区顶部，表现为短光标悬空偏高）
        let todo_input = child(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_AUTOHSCROLL as u32,
            0,
            ID_TODO_INPUT,
        );
        let todo_add = child(hwnd, "BUTTON", "添加", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 0, ID_TODO_ADD);
        let todo_list = child(
            hwnd,
            "SysListView32",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | LVS_REPORT | LVS_SHOWSELALWAYS
                | LVS_NOCOLUMNHEADER,
            0,
            ID_TODO_LIST,
        );
        let todo_clear = child(hwnd, "BUTTON", "清除已完成", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 0, ID_TODO_CLEAR);
        let settings_btn = child(hwnd, "BUTTON", "⚙ 设置", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 0, ID_SETTINGS_BTN);
        let note_list = child(
            hwnd,
            "SysListView32",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | LVS_REPORT | LVS_SHOWSELALWAYS
                | LVS_NOCOLUMNHEADER,
            0,
            ID_NOTE_LIST,
        );
        let note_new = child(hwnd, "BUTTON", "＋ 新建", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32, 0, ID_NOTE_NEW);
        let editor = child(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
            0,
            ID_EDITOR,
        );
        // 放开文字上限（默认 32K），便签长文也够用
        SendMessageW(editor, EM_SETLIMITTEXT, 0x7FFF_FFFE as usize, 0);

        // 勾选框图标随主题在 apply_theme 中重建并挂载
        // 整行选择（不再用系统勾选框）
        let ex = LVS_EX_FULLROWSELECT;
        SendMessageW(todo_list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex as LPARAM);
        SendMessageW(note_list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex as usize, ex as LPARAM);

        insert_column(todo_list, 2000);
        insert_column(note_list, 176);

        // 字体
        let font = util::create_ui_font(false);
        let font_strike = util::create_ui_font(true);
        let font_title = util::create_font_title();
        let input_line_h = util::font_line_height(font);
        for h in [todo_input, todo_add, todo_list, todo_clear, settings_btn, note_list, note_new, editor] {
            util::apply_font(h, font);
        }
        // 输入类控件加左右文字边距，光标/文字不贴边
        for h in [todo_input, editor] {
            SendMessageW(h, EM_SETMARGINS, 3, (10 | (10 << 16)) as LPARAM); // EC_LEFT|RIGHT, 各 10px
        }

        SetWindowSubclass(editor, Some(child_subclass), SUB_EDITOR, 0);
        SetWindowSubclass(todo_input, Some(child_subclass), SUB_TODO_INPUT, 0);
        SetWindowSubclass(note_list, Some(child_subclass), SUB_NOTE_LIST, 0);
        SetWindowSubclass(todo_list, Some(child_subclass), SUB_TODO_LIST, 0);
        for b in [todo_add, todo_clear, settings_btn, note_new] {
            SetWindowSubclass(b, Some(button_subclass), SUB_BTN, 0);
        }

        with_app(|app| {
            app.todo_input = todo_input;
            app.todo_add = todo_add;
            app.todo_list = todo_list;
            app.todo_clear = todo_clear;
            app.settings_btn = settings_btn;
            app.note_list = note_list;
            app.note_new = note_new;
            app.editor = editor;
            app.font = font;
            app.font_strike = font_strike;
            app.font_title = font_title;
            app.btn_states = vec![
                (todo_add, false, false),
                (todo_clear, false, false),
                (settings_btn, false, false),
                (note_new, false, false),
            ];
            app.input_line_h = input_line_h;
        });

        with_app(todos::rebuild);
        notes::ensure_first_run();
        with_app(notes::refresh);
        with_app(|app| {
            if app.cfg.win_x >= 0 && app.cfg.win_cx >= 400 && app.cfg.win_cy >= 300 {
                MoveWindow(app.hwnd, app.cfg.win_x, app.cfg.win_y, app.cfg.win_cx, app.cfg.win_cy, 0);
            }
            apply_theme(app);
            if let Some(idx) = app.cur_note {
                notes::load_by_index(app, idx);
            }
            let t = app.cfg.last_tab.min(1);
            app.initialized = true;
            switch_tab(app, t);
            layout(app);
        });
    }
}

fn insert_column(lv: HWND, cx: i32) {
    let mut w = util::wide("");
    let mut col: windows_sys::Win32::UI::Controls::LVCOLUMNW = unsafe { std::mem::zeroed() };
    col.mask = windows_sys::Win32::UI::Controls::LVCF_TEXT | windows_sys::Win32::UI::Controls::LVCF_WIDTH;
    col.fmt = windows_sys::Win32::UI::Controls::LVCFMT_LEFT;
    col.cx = cx;
    col.pszText = w.as_mut_ptr();
    unsafe { SendMessageW(lv, windows_sys::Win32::UI::Controls::LVM_INSERTCOLUMNW, 0, &mut col as *mut _ as LPARAM) };
}

// ---------- 主题 ----------

/// 解析主题并应用到全部控件（画刷、图标列表、系统主题字符串）
pub fn apply_theme(app: &mut App) {
    let dark = theme::resolve_dark(&app.cfg.theme);
    app.dark = dark;
    app.brushes.rebuild(dark);
    // 总开关先行：不开它，DarkMode_Explorer 的部件（列表底/滚动条/光标/菜单）
    // 仍按浅色渲染（见 theme.rs set_system_dark 说明）
    theme::set_system_dark(dark);
    unsafe {
        editor_set_margin(app.editor, 10, 8);

        // 列表：背景 / 文字 / 文字底透明；滚动条跟随系统暗色主题
        // EDIT（编辑器/待办输入框）：暗色滚动条与光标
        let theme_name = if dark { "DarkMode_Explorer" } else { "Explorer" };
        for h in [app.editor, app.todo_input] {
            windows_sys::Win32::UI::Controls::SetWindowTheme(
                h,
                util::wide(theme_name).as_ptr(),
                std::ptr::null(),
            );
        }
        let old_himl = app.himl;
        let himl = make_checkbox_imagelist(dark);
        app.himl = himl;
        SendMessageW(app.todo_list, LVM_SETIMAGELIST, LVSIL_SMALL as usize, himl);
        SendMessageW(app.note_list, LVM_SETIMAGELIST, LVSIL_SMALL as usize, himl);
        // 两个列表都换上新图列表后旧的就不再被引用，销毁掉，避免每次切主题漏一份
        if old_himl != 0 {
            windows_sys::Win32::UI::Controls::ImageList_Destroy(old_himl);
        }
        for lv in [app.todo_list, app.note_list] {
            // 不再发送 LVM_SETBKCOLOR：实测（Win10 19045 comctl32 6.10）发送过该消息后，
            // 内联重命名 LVM_EDITLABELW 会在 comctl32 内部触发访问违例（进程闪退）。
            // 列表背景由主题绘制与自绘覆盖，这条消息没有实际作用。
            SendMessageW(lv, LVM_SETTEXTCOLOR, 0, theme::palette(dark).text as LPARAM);
            SendMessageW(lv, LVM_SETTEXTBKCOLOR, 0, CLR_NONE as LPARAM);
            windows_sys::Win32::UI::Controls::SetWindowTheme(
                lv,
                util::wide(theme_name).as_ptr(),
                std::ptr::null(),
            );
        }
    }
    theme::invalidate(app.hwnd);
    for c in [app.todo_input, app.todo_add, app.todo_list, app.todo_clear, app.settings_btn, app.note_list, app.note_new, app.editor] {
        theme::invalidate(c);
    }
}

/// 编辑器内边距（文字不贴边）
fn editor_set_margin(editor: HWND, x: i32, y: i32) {
    unsafe {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        SendMessageW(editor, EM_GETRECT, 0, &mut rc as *mut RECT as LPARAM);
        let mut wrc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(editor, &mut wrc);
        let new = RECT {
            left: rc.left.max(x),
            top: rc.top.max(y),
            right: wrc.right - x,
            bottom: wrc.bottom - y,
        };
        SendMessageW(editor, EM_SETRECT, 0, &new as *const RECT as LPARAM);
    }
}

// ---------- 布局与页面 ----------

fn layout(app: &mut App) {
    unsafe {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(app.hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        if w <= 0 || h <= 0 {
            return;
        }
        let py = CONTENT_Y;
        let todo = app.cur_tab == TAB_TODO;

        MoveWindow(app.settings_btn, w - 70, TITLE_H + 8, 58, 32, 1);

        // 待办页
        ShowWindow(app.todo_input, if todo { SW_SHOW } else { SW_HIDE });
        ShowWindow(app.todo_add, if todo { SW_SHOW } else { SW_HIDE });
        ShowWindow(app.todo_list, if todo { SW_SHOW } else { SW_HIDE });
        ShowWindow(app.todo_clear, if todo { SW_SHOW } else { SW_HIDE });
        // 输入框视觉槽 30px 高；EDIT 控件本身取一行高，槽内垂直居中
        // （父窗口在槽外圈画描边，槽底色与控件底色同为 surface2，衔接无缝）
        let slot = RECT { left: 12, top: py + 10, right: w - 112, bottom: py + 40 };
        app.todo_slot = slot;
        let ih = app.input_line_h;
        MoveWindow(app.todo_input, slot.left + 1, slot.top + (30 - ih) / 2, slot.right - slot.left - 2, ih, 1);
        MoveWindow(app.todo_add, w - 104, py + 10, 92, 30, 1);
        MoveWindow(app.todo_list, 12, py + 50, w - 24, h - py - 50 - 46, 1);
        MoveWindow(app.todo_clear, w - 108, h - 38, 96, 28, 1);
        SendMessageW(app.todo_list, windows_sys::Win32::UI::Controls::LVM_SETCOLUMNWIDTH, 0, (w - 44).max(60) as LPARAM);

        // 笔记页
        ShowWindow(app.note_new, if todo { SW_HIDE } else { SW_SHOW });
        ShowWindow(app.note_list, if todo { SW_HIDE } else { SW_SHOW });
        ShowWindow(app.editor, if todo { SW_HIDE } else { SW_SHOW });
        MoveWindow(app.note_new, 12, py + 10, 80, 30, 1);
        MoveWindow(app.note_list, 12, py + 50, 176, h - py - 64, 1);
        MoveWindow(app.editor, 200, py + 10, w - 212, h - py - 18, 1);
        SendMessageW(app.note_list, windows_sys::Win32::UI::Controls::LVM_SETCOLUMNWIDTH, 0, 156);
        editor_set_margin(app.editor, 10, 8);
    }
}

/// 从任意位置新建笔记：先切到笔记页，否则用户看不到新建结果
fn show_notes_and_create() {
    show_main();
    with_app(|app| {
        if app.cur_tab != TAB_NOTE {
            switch_tab(app, TAB_NOTE);
        }
        notes::create_note(app);
    });
}

fn switch_tab(app: &mut App, idx: usize) {
    app.cur_tab = idx;
    app.cfg.last_tab = idx;
    layout(app);
    focus_page(app);
}

fn focus_page(app: &App) {
    unsafe {
        if app.cur_tab == TAB_TODO {
            SetFocus(app.todo_input);
        } else {
            SetFocus(app.editor);
        }
    }
}

// ---------- 显示 / 隐藏 ----------

pub fn show_main() {
    with_app(|app| unsafe {
        // 最小化过的窗口不能靠 SW_SHOW 还原：SW_SHOW 只是按当前（极小化）状态显示，
        // 屏幕上仍然什么都看不到，必须走 SW_RESTORE
        if IsIconic(app.hwnd) != 0 {
            ShowWindow(app.hwnd, SW_RESTORE);
        } else if IsWindowVisible(app.hwnd) == 0 {
            ShowWindow(app.hwnd, SW_SHOW);
        }
        SetForegroundWindow(app.hwnd);
        focus_page(app);
    });
}

pub fn hide_main() {
    with_app(|app| {
        notes::save_current(app);
        save_window_rect(app);
        app.cfg.last_tab = app.cur_tab;
        if let Some(i) = app.cur_note {
            if let Some(m) = app.notes.get(i) {
                app.cfg.last_note = m.filename.clone();
            }
        }
        crate::config::save(&app.cfg);
        unsafe { ShowWindow(app.hwnd, SW_HIDE) };
    });
}

pub fn toggle_main() {
    let visible = with_app(|app| unsafe { IsWindowVisible(app.hwnd) != 0 });
    if visible {
        hide_main();
    } else {
        show_main();
    }
}

fn save_window_rect(app: &mut App) {
    unsafe {
        // 最小化/最大化时窗口矩形不是用户摆放的“正常尺寸”（最大化时甚至是屏幕外的一圈），
        // 都不落盘：保留上一次正常状态的位置尺寸
        if IsIconic(app.hwnd) != 0 || IsZoomed(app.hwnd) != 0 {
            return;
        }
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(app.hwnd, &mut rc);
        app.cfg.win_x = rc.left;
        app.cfg.win_y = rc.top;
        app.cfg.win_cx = rc.right - rc.left;
        app.cfg.win_cy = rc.bottom - rc.top;
    }
}

/// 焦点跑到自己进程的其他窗口（设置框、弹出菜单）时不要自动隐藏
fn should_skip_auto_hide() -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg != 0 {
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(fg, &mut pid);
            if pid == GetCurrentProcessId() {
                return true;
            }
        }
        false
    }
}

// ---------- 坐标 / 命中区 ----------

fn client_from_lparam(lp: LPARAM) -> (i32, i32) {
    let x = (lp & 0xFFFF) as u16 as i16 as i32;
    let y = ((lp >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

fn close_rect(w: i32) -> RECT {
    RECT { left: w - 36, top: 4, right: w - 8, bottom: 32 }
}

fn pill_rect(i: usize) -> RECT {
    RECT {
        left: 12 + i as i32 * (PILL_W + PILL_GAP),
        top: TITLE_H + 8,
        right: 12 + i as i32 * (PILL_W + PILL_GAP) + PILL_W,
        bottom: TITLE_H + 8 + PILL_H,
    }
}

fn pt_in(rc: RECT, x: i32, y: i32) -> bool {
    x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom
}

// ---------- 绘制 ----------

unsafe fn paint(hwnd: HWND) {
    let ready = app_ready();
    let mut ps: PAINTSTRUCT = std::mem::zeroed();
    let hdc = BeginPaint(hwnd, &mut ps);
    if !ready {
        EndPaint(hwnd, &ps);
        return;
    }
    with_app(|app| {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        if w <= 0 || h <= 0 {
            return;
        }

        // 双缓冲，避免自绘闪烁
        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old_bmp = SelectObject(mem, bmp);

        let p = theme::palette(app.dark);
        let bg_brush = CreateSolidBrush(p.window_bg);
        FillRect(mem, &RECT { left: 0, top: 0, right: w, bottom: h }, bg_brush);
        DeleteObject(bg_brush);

        // 窗口外缘 1px 描边：无边框窗的浅色底浮在浅色内容（或暗色浮深色）上时，
        // 仅靠 DWM 阴影分不出边界，用一圈细线勾出窗口轮廓
        let edge_brush = CreateSolidBrush(p.edge);
        FillRect(mem, &RECT { left: 0, top: 0, right: w, bottom: 1 }, edge_brush);
        FillRect(mem, &RECT { left: 0, top: h - 1, right: w, bottom: h }, edge_brush);
        FillRect(mem, &RECT { left: 0, top: 0, right: 1, bottom: h }, edge_brush);
        FillRect(mem, &RECT { left: w - 1, top: 0, right: w, bottom: h }, edge_brush);
        DeleteObject(edge_brush);

        // -- 标题栏 --
        DrawIconEx(mem, 12, 10, app.hicon, 16, 16, 0, 0, DI_NORMAL);
        SetBkMode(mem, TRANSPARENT as i32);
        SelectObject(mem, app.font_title);
        SetTextColor(mem, p.text);
        let mut tr = RECT { left: 38, top: 0, right: 220, bottom: TITLE_H };
        DrawTextW(mem, util::wide("MiniNotes").as_ptr(), -1, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_LEFT);

        // 热键提示（空间足够才画）
        if w > 460 {
            SelectObject(mem, app.font);
            SetTextColor(mem, p.text_sec);
            let hint = format!("{}  呼起 / 隐藏", hotkey::text(app.cfg.hotkey_mods, app.cfg.hotkey_vk));
            let mut hr = RECT { left: w - 300, top: 0, right: w - 48, bottom: TITLE_H };
            DrawTextW(mem, util::wide(&hint).as_ptr(), -1, &mut hr, DT_SINGLELINE | DT_VCENTER | DT_RIGHT);
        }

        // 关闭按钮
        let cr = close_rect(w);
        if app.hover_close {
            fill_solid(mem, cr, p.danger);
            draw_x(mem, cr, p.on_danger);
        } else {
            draw_x(mem, cr, p.text_sec);
        }

        // 标题栏下的细线
        let hl_brush = CreateSolidBrush(p.hairline);
        FillRect(mem, &RECT { left: 0, top: TITLE_H - 1, right: w, bottom: TITLE_H }, hl_brush);
        DeleteObject(hl_brush);

        // -- Tab 药丸 --
        for (i, label) in ["待办", "笔记"].iter().enumerate() {
            let mut pr = pill_rect(i);
            let sel = app.cur_tab == i;
            let hov = app.hover_tab == i as i32;
            if sel {
                theme::fill_round(mem, pr, 16.0, Some(p.accent), None);
            } else if hov {
                theme::fill_round(mem, pr, 16.0, Some(p.surface2), None);
            }
            SelectObject(mem, app.font_title);
            SetTextColor(
                mem,
                if sel {
                    p.on_accent
                } else if hov {
                    p.text
                } else {
                    p.text_sec
                },
            );
            DrawTextW(mem, util::wide(label).as_ptr(), -1, &mut pr, DT_SINGLELINE | DT_VCENTER | DT_CENTER);
        }

        // -- 待办输入框描边：两次填充画出 1px 圆角环带 --
        // （不能用 GDI+ 描边路径：笔刷中心线贴控件边缘时，笔宽的一半落在 EDIT
        //   子控件区域里，会被 WS_CLIPCHILDREN 裁掉；圆角斜对角处整个笔宽都没入
        //   子控件，表现为圆角断线。改为「外圈填描边色 + 内圈填底色」，
        //   环带完整落在父窗口可见区，圆角连贯，且内外弧同心。）
        if app.cur_tab == TAB_TODO && IsWindowVisible(app.todo_input) != 0 {
            let slot = app.todo_slot;
            let focused = GetFocus() == app.todo_input;
            let ring_color = if focused { p.accent_soft } else { p.hairline };
            let ring = RECT {
                left: slot.left - 1,
                top: slot.top - 1,
                right: slot.right + 1,
                bottom: slot.bottom + 1,
            };
            theme::fill_round(mem, ring, 7.0, Some(ring_color), None);
            theme::fill_round(mem, slot, 6.0, Some(p.surface2), None);
        }

        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        DeleteObject(bmp);
        DeleteDC(mem);
    });
    EndPaint(hwnd, &ps);
}

unsafe fn fill_solid(hdc: HDC, rc: RECT, color: COLORREF) {
    let b = CreateSolidBrush(color);
    FillRect(hdc, &rc, b);
    DeleteObject(b);
}

unsafe fn draw_x(hdc: HDC, rc: RECT, color: COLORREF) {
    let pen = CreatePen(PS_SOLID, 2, color);
    let old = SelectObject(hdc, pen);
    let cx = (rc.left + rc.right) / 2;
    let cy = (rc.top + rc.bottom) / 2;
    let mut pt = POINT { x: 0, y: 0 };
    MoveToEx(hdc, cx - 5, cy - 5, &mut pt);
    LineTo(hdc, cx + 5, cy + 5);
    MoveToEx(hdc, cx + 5, cy - 5, &mut pt);
    LineTo(hdc, cx - 5, cy + 5);
    SelectObject(hdc, old);
    DeleteObject(pen);
}

// ---------- 窗口过程 ----------

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    // WM_NCCALCSIZE 必须在 App 就绪前也生效（窗口创建期间就会收到，且 wParam 可能为 0）：
    // 返回 0 = 客户区覆盖整个窗口，系统边框/标题栏全部让位（阴影与贴靠由 DWM 承担）。
    // 但最大化时系统给的窗口矩形是「工作区四周各大一圈边框」（左/上为负坐标、右/下压出
    // 屏幕）：带边框窗口会把这圈当非客户区，我们返回 0 就会把它变成客户区，边缘内容被裁。
    // 因此最大化时把客户区按边框宽度内缩回去。
    if msg == WM_NCCALCSIZE {
        if wp != 0 && IsZoomed(hwnd) != 0 {
            let p = lp as *mut NCCALCSIZE_PARAMS;
            if !p.is_null() {
                let fx = GetSystemMetrics(SM_CXSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
                let fy = GetSystemMetrics(SM_CYSIZEFRAME) + GetSystemMetrics(SM_CXPADDEDBORDER);
                let r = &mut (*p).rgrc[0];
                r.left += fx;
                r.top += fy;
                r.right -= fx;
                r.bottom -= fy;
            }
        }
        return 0;
    }
    if !app_ready() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_NCHITTEST => {
            // 边缘可调整大小；标题条可拖动；关闭按钮保持 HTCLIENT 以接收点击
            let mut wr = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetWindowRect(hwnd, &mut wr);
            let gx = (lp & 0xFFFF) as u16 as i16 as i32;
            let gy = ((lp >> 16) & 0xFFFF) as u16 as i16 as i32;
            let x = gx - wr.left;
            let y = gy - wr.top;
            let w = wr.right - wr.left;
            let h = wr.bottom - wr.top;
            let m = BORDER_RESIZE;
            if x < m && y < m {
                return HTTOPLEFT as LRESULT;
            }
            if x >= w - m && y < m {
                return HTTOPRIGHT as LRESULT;
            }
            if x < m && y >= h - m {
                return HTBOTTOMLEFT as LRESULT;
            }
            if x >= w - m && y >= h - m {
                return HTBOTTOMRIGHT as LRESULT;
            }
            if y < m {
                return HTTOP as LRESULT;
            }
            if y >= h - m {
                return HTBOTTOM as LRESULT;
            }
            if x < m {
                return HTLEFT as LRESULT;
            }
            if x >= w - m {
                return HTRIGHT as LRESULT;
            }
            if y < TITLE_H {
                if pt_in(close_rect(w), x, y) {
                    return HTCLIENT as LRESULT;
                }
                return HTCAPTION as LRESULT;
            }
            HTCLIENT as LRESULT
        }
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
        }
        WM_MOUSEMOVE => {
            let (x, y) = client_from_lparam(lp);
            with_app(|app| {
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetClientRect(app.hwnd, &mut rc);
                let w = rc.right;
                let mut ht = -1;
                for i in 0..2usize {
                    if pt_in(pill_rect(i), x, y) {
                        ht = i as i32;
                    }
                }
                let hc = pt_in(close_rect(w), x, y);
                if ht != app.hover_tab || hc != app.hover_close {
                    app.hover_tab = ht;
                    app.hover_close = hc;
                    InvalidateRect(app.hwnd, std::ptr::null(), 0);
                }
                if !app.mouse_tracked {
                    app.mouse_tracked = true;
                    let mut tme = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE,
                        hwndTrack: app.hwnd,
                        dwHoverTime: 0,
                    };
                    TrackMouseEvent(&mut tme);
                }
            });
            0
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                app.mouse_tracked = false;
                app.hover_tab = -1;
                app.hover_close = false;
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_LBUTTONDOWN => {
            let (x, y) = client_from_lparam(lp);
            with_app(|app| {
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetClientRect(app.hwnd, &mut rc);
                let w = rc.right;
                if y < TITLE_H && pt_in(close_rect(w), x, y) {
                    hide_main();
                    return;
                }
                for i in 0..2usize {
                    if pt_in(pill_rect(i), x, y) {
                        switch_tab(app, i);
                        return;
                    }
                }
                // 待办页点击输入框槽位（含控件上下的留白）都聚焦输入框
                if app.cur_tab == TAB_TODO && pt_in(app.todo_slot, x, y) {
                    SetFocus(app.todo_input);
                }
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_DRAWITEM => {
            let di = lp as *const DRAWITEMSTRUCT;
            if !di.is_null() {
                let id = (*di).CtlID;
                let text = match id {
                    ID_TODO_ADD => "添加",
                    ID_TODO_CLEAR => "清除已完成",
                    ID_SETTINGS_BTN => "⚙ 设置",
                    ID_NOTE_NEW => "＋ 新建",
                    _ => "",
                };
                let kind = match id {
                    ID_TODO_ADD => theme::BtnKind::Primary,
                    // 列表头部按钮：幽灵样式，无边框不突出，文字与列表标题对齐
                    ID_NOTE_NEW => theme::BtnKind::Ghost,
                    _ => theme::BtnKind::Tonal,
                };
                // ＋新建 文字与下方笔记标题对齐（同一左缘 x=24）
                let align = if id == ID_NOTE_NEW {
                    theme::BtnAlign::Left(12)
                } else {
                    theme::BtnAlign::Center
                };
                with_app(|app| {
                    let (hov, down) = btn_flags(app, (*di).hwndItem);
                    theme::draw_button(
                        (*di).hDC,
                        (*di).rcItem,
                        text,
                        kind,
                        hov,
                        down || ((*di).itemState & ODS_SELECTED != 0),
                        app.dark,
                        app.font,
                        align,
                    );
                });
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CTLCOLOREDIT => {
            // 待办输入框 / 列表内联编辑：主题色背景与文字
            let hdc = wp as HDC;
            with_app(|app| unsafe {
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, theme::palette(app.dark).text);
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, theme::palette(app.dark).surface2);
                app.brushes.surface2
            })
        }
        WM_CTLCOLORSTATIC => {
            let hdc = wp as HDC;
            with_app(|app| unsafe {
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, theme::palette(app.dark).text_sec);
                windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, theme::palette(app.dark).window_bg);
                app.brushes.window_bg
            })
        }
        WM_SETTINGCHANGE => {
            // 系统"跟随"主题：设置变化时重新解析
            with_app(|app| {
                if app.cfg.theme == "system" {
                    let dark = theme::resolve_dark("system");
                    if dark != app.dark {
                        apply_theme(app);
                    }
                }
            });
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_SIZE => {
            if wp == SIZE_MINIMIZED as usize {
                hide_main();
                return 0;
            }
            with_app(|app| {
                if app.initialized {
                    layout(app);
                }
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
            0
        }
        WM_ACTIVATE => {
            if wp & 0xFFFF == 0 {
                let skip = with_app(|app| app.settings_open || !app.cfg.auto_hide)
                    || should_skip_auto_hide();
                if !skip && IsWindowVisible(hwnd) != 0 {
                    hide_main();
                }
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CLOSE => {
            hide_main();
            0
        }
        WM_HOTKEY => {
            if wp as i32 == hotkey::HOTKEY_ID {
                toggle_main();
            }
            0
        }
        WM_TIMER => {
            if wp == TIMER_SAVE {
                KillTimer(hwnd, TIMER_SAVE);
                with_app(notes::save_current);
            }
            0
        }
        WM_GETMINMAXINFO => {
            let mmi = lp as *mut MINMAXINFO;
            if !mmi.is_null() {
                (*mmi).ptMinTrackSize = POINT { x: 560, y: 420 };
            }
            0
        }
        WM_DESTROY => {
            with_app(|app| {
                notes::save_current(app);
                save_window_rect(app);
                app.cfg.last_tab = app.cur_tab;
                crate::config::save(&app.cfg);
                util::destroy_font(app.font);
                util::destroy_font(app.font_strike);
                util::destroy_font(app.font_title);
                app.font = 0;
                app.font_strike = 0;
                app.font_title = 0;
            });
            tray::remove(hwnd);
            unsafe {
                let p = std::ptr::addr_of_mut!(GDIP);
                if *p != 0 {
                    theme::gdiplus_shutdown(*p);
                    *p = 0;
                }
            }
            PostQuitMessage(0);
            0
        }
        WM_COMMAND => on_command(wp),
        WM_NOTIFY => on_notify(hwnd, wp, lp),
        tray::TRAY_CB => on_tray(hwnd, lp),
        MSG_SHOW => {
            show_main();
            0
        }
        MSG_ESC_HIDE => {
            hide_main();
            0
        }
        MSG_NEW_NOTE => {
            show_notes_and_create();
            0
        }
        MSG_ADD_TODO => {
            with_app(todos::add_from_input);
            0
        }
        MSG_TOGGLE_TODO => {
            with_app(todos::toggle_selected);
            0
        }
        MSG_DEL_NOTE => {
            with_app(notes::delete_selected);
            0
        }
        MSG_DEL_TODO => {
            with_app(todos::delete_selected);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn btn_flags(app: &App, hwnd: HWND) -> (bool, bool) {
    for (h, hov, down) in &app.btn_states {
        if *h == hwnd {
            return (*hov, *down);
        }
    }
    (false, false)
}

fn btn_set(app: &mut App, hwnd: HWND, hov: bool, down: bool) -> bool {
    for e in app.btn_states.iter_mut() {
        if e.0 == hwnd {
            let changed = e.1 != hov || e.2 != down;
            e.1 = hov;
            e.2 = down;
            return changed;
        }
    }
    false
}

fn on_command(wp: WPARAM) -> LRESULT {
    let id = (wp & 0xFFFF) as u32;
    let code = ((wp >> 16) & 0xFFFF) as u32;
    match (code, id) {
        (EN_CHANGE, ID_EDITOR) => {
            with_app(|app| {
                notes::notify_dirty(app);
                if app.note_dirty {
                    unsafe { SetTimer(app.hwnd, TIMER_SAVE, 600, None) };
                }
                // 输入法组词上屏后，EDIT 对后续删除的增量失效区会漏算首字符区域，
                // 被删字形残留在屏上（要等下一次全量重绘才消失）——内容一变就让
                // 编辑器整体失效重绘，屏幕永远与内容同步
                unsafe { InvalidateRect(app.editor, std::ptr::null(), 0) };
            });
        }
        (EN_SETFOCUS, ID_TODO_INPUT) | (EN_KILLFOCUS, ID_TODO_INPUT) => {
            with_app(|app| unsafe { InvalidateRect(app.hwnd, std::ptr::null(), 0) });
        }
        (0, ID_TODO_ADD) => with_app(todos::add_from_input),
        (0, ID_TODO_CLEAR) => with_app(todos::clear_done),
        (0, ID_NOTE_NEW) => with_app(notes::create_note),
        (0, ID_SETTINGS_BTN) => with_app(|app| settings::open(app.hwnd)),
        _ => {}
    }
    0
}

fn on_notify(hwnd: HWND, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows_sys::Win32::UI::Controls::{
        LVN_ITEMCHANGED, NM_CLICK, NM_CUSTOMDRAW, NM_DBLCLK, NM_RCLICK,
    };
    let hdr = lp as *const NMHDR;
    unsafe {
        let (id_from, code) = ((*hdr).idFrom, (*hdr).code);
        match id_from as u32 {
            ID_TODO_LIST => match code {
                NM_CUSTOMDRAW => with_app(|app| todos::on_custom_draw(app, &*(lp as *const NMLVCUSTOMDRAW))),
                NM_CLICK => {
                    with_app(|app| {
                        todos::on_click(app, &*(lp as *const NMITEMACTIVATE));
                    });
                    0
                }
                NM_DBLCLK => {
                    with_app(todos::begin_edit);
                    0
                }
                NM_RCLICK => {
                    todo_ctx_menu(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, WM_NOTIFY, wp, lp),
            },
            ID_NOTE_LIST => match code {
                NM_CUSTOMDRAW => with_app(|app| notes::on_custom_draw(app, &*(lp as *const NMLVCUSTOMDRAW))),
                LVN_ITEMCHANGED => {
                    let nm = &*(lp as *const NMLISTVIEW);
                    if (nm.uChanged & LVIF_STATE) != 0
                        && (nm.uNewState & LVIS_SELECTED) != 0
                        && (nm.uOldState & LVIS_SELECTED) == 0
                    {
                        with_app(|app| {
                            if app.suppress_notify {
                                return;
                            }
                            let idx = nm.iItem as usize;
                            if app.cur_note != Some(idx) {
                                notes::load_by_index(app, idx);
                            }
                        });
                    }
                    0
                }
                NM_DBLCLK => {
                    with_app(notes::begin_rename);
                    0
                }
                NM_RCLICK => {
                    note_ctx_menu(hwnd);
                    0
                }
                _ => DefWindowProcW(hwnd, WM_NOTIFY, wp, lp),
            },
            _ => DefWindowProcW(hwnd, WM_NOTIFY, wp, lp),
        }
    }
}

fn on_tray(hwnd: HWND, lp: LPARAM) -> LRESULT {
    unsafe {
        match lp as u32 {
            WM_LBUTTONUP => {
                if IsWindowVisible(hwnd) == 0 {
                    show_main();
                } else {
                    SetForegroundWindow(hwnd);
                }
            }
            WM_RBUTTONUP => {
                let auto = settings::autostart_on();
                match tray::show_menu(hwnd, auto) {
                    tray::CMD_OPEN => show_main(),
                    tray::CMD_NEW_NOTE => {
                        show_notes_and_create();
                    }
                    tray::CMD_SETTINGS => settings::open(hwnd),
                    tray::CMD_AUTOSTART => {
                        with_app(|app| {
                            let on = !settings::autostart_on();
                            settings::autostart_set(on);
                            app.cfg.autostart = on;
                            crate::config::save(&app.cfg);
                        });
                    }
                    tray::CMD_QUIT => {
                        DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    0
}

// ---------- 右键菜单 ----------

fn popup_at(hwnd: HWND, items: &[(u32, &str, bool)]) -> u32 {
    // 自绘菜单替代原生 TrackPopupMenu：配色跟随应用主题（原因见 menu.rs 模块注释）
    let list: Vec<crate::menu::Item> = items
        .iter()
        .map(|(id, text, sep)| crate::menu::Item {
            id: *id,
            text: (*text).to_string(),
            sep: *sep,
            checked: false,
        })
        .collect();
    let dark = with_app(|app| app.dark);
    crate::menu::show(hwnd, dark, &list)
}

fn note_ctx_menu(hwnd: HWND) {
    match popup_at(
        hwnd,
        &[
            (M_NOTE_NEW, "新建笔记", false),
            (M_NOTE_RENAME, "重命名", false),
            (0, "", true),
            (M_NOTE_DELETE, "删除", false),
        ],
    ) {
        M_NOTE_NEW => with_app(notes::create_note),
        M_NOTE_RENAME => with_app(notes::begin_rename),
        M_NOTE_DELETE => with_app(notes::delete_selected),
        _ => {}
    }
}

fn todo_ctx_menu(hwnd: HWND) {
    match popup_at(
        hwnd,
        &[
            (M_TODO_EDIT, "编辑", false),
            (M_TODO_DELETE, "删除", false),
            (0, "", true),
            (M_TODO_CLEAR, "清除已完成", false),
        ],
    ) {
        M_TODO_EDIT => with_app(todos::begin_edit),
        M_TODO_DELETE => with_app(todos::delete_selected),
        M_TODO_CLEAR => with_app(todos::clear_done),
        _ => {}
    }
}

// ---------- 子控件子类 ----------

/// owner-draw 按钮的悬停/按下跟踪
unsafe extern "system" fn button_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    _id: usize,
    _d: usize,
) -> LRESULT {
    // Esc 在按钮上同样隐藏窗口（与编辑器/输入框/列表保持一致）
    if (msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN) && wp as u32 == VK_ESCAPE as u32 {
        let root = GetAncestor(hwnd, GA_ROOT);
        PostMessageW(root, MSG_ESC_HIDE, 0, 0);
        return 1;
    }
    match msg {
        // 禁止系统在 DRAWITEM 前擦底（暗色主题下表现为白色闪烁）
        WM_ERASEBKGND => return 1,
        WM_MOUSEMOVE => {
            let mut entered = false;
            with_app(|app| {
                let (hov, _) = btn_flags(app, hwnd);
                if !hov {
                    btn_set(app, hwnd, true, false);
                    entered = true;
                }
            });
            if entered {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            }
        }
        WM_MOUSELEAVE => {
            with_app(|app| {
                btn_set(app, hwnd, false, false);
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        WM_LBUTTONDOWN => {
            with_app(|app| {
                btn_set(app, hwnd, true, true);
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        WM_LBUTTONUP => {
            with_app(|app| {
                btn_set(app, hwnd, true, false);
            });
            InvalidateRect(hwnd, std::ptr::null(), 0);
        }
        _ => {}
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}

// ---------- 笔记列表拖拽排序 ----------

fn lv_hit_item(lv: HWND, x: i32, y: i32) -> i32 {
    let mut hti: LVHITTESTINFO = unsafe { std::mem::zeroed() };
    hti.pt = POINT { x, y };
    unsafe { SendMessageW(lv, LVM_HITTEST, 0, &mut hti as *mut _ as LPARAM) };
    hti.iItem
}

fn lv_item_rect_bounds(lv: HWND, idx: i32) -> RECT {
    // RECT.left 输入时是 LVIR_* 代码：0 = LVIR_BOUNDS（整行）
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { SendMessageW(lv, LVM_GETITEMRECT, idx as WPARAM, &mut rc as *mut _ as LPARAM) };
    rc
}

/// 拖拽落点：光标所在行上半 → 插到行前，下半 → 行后；返回已按移出位置
/// 校正的插入索引（原位放下返回 None）。插入指示线由 notes 自绘管线画
fn drag_drop_index(lv: HWND, from: i32, x: i32, y: i32) -> Option<usize> {
    let item = lv_hit_item(lv, x, y);
    if item < 0 {
        return None;
    }
    let rc = lv_item_rect_bounds(lv, item);
    let mut to = if y > (rc.top + rc.bottom) / 2 { item + 1 } else { item };
    // 移出位置在落点之前时，落点索引左移一格（先删后插）
    if from < to {
        to -= 1;
    }
    if to == from || to == from + 1 {
        return None; // 原位放下
    }
    Some(to as usize)
}

/// 子控件键盘行为
unsafe extern "system" fn child_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _d: usize,
) -> LRESULT {
    // 重命名编辑框：Enter 提交、Esc 取消、失焦提交
    if id == SUB_RENAME_EDIT {
        if msg == WM_KILLFOCUS {
            with_app(|app| crate::rename::commit_if(app, hwnd));
            return 0;
        }
        if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
            match wp as u32 {
                13 => {
                    // VK_RETURN
                    with_app(|app| crate::rename::commit(app, hwnd));
                    return 1;
                }
                27 => {
                    // VK_ESCAPE
                    with_app(|app| crate::rename::cancel(app));
                    return 1;
                }
                // Shift+Insert 系统粘贴快捷键（无对应字符消息，须在 keydown 拦）
                0x2D if (windows_sys::Win32::UI::Input::KeyboardAndMouse::GetKeyState(
                    windows_sys::Win32::UI::Input::KeyboardAndMouse::VK_SHIFT as i32,
                ) as i16) < 0 =>
                {
                    crate::rename::paste_flattened(hwnd);
                    return 1;
                }
                _ => {}
            }
        }
        if msg == WM_CHAR && wp as u32 == 0x16 {
            // Ctrl+V 的字符消息（SYN）：标准 EDIT 在此触发粘贴（内部再转 WM_PASTE），
            // 多行化后会带入换行，这里统一替换为过滤后的粘贴。
            // 注意勿在 keydown 拦截 Ctrl+V——字符消息仍会到达，会变成双重插入
            crate::rename::paste_flattened(hwnd);
            return 1;
        }
        if msg == WM_PASTE {
            // 右键菜单 / EDIT 内部转发的粘贴，同样过滤换行
            crate::rename::paste_flattened(hwnd);
            return 1;
        }
        return DefSubclassProc(hwnd, msg, wp, lp);
    }
    // 列表的两个暗色补丁（内联重命名编辑框挂在列表下，消息由列表收到）：
    // WM_ERASEBKGND：擦除用窗口底色，避免主题底色与自绘底色在滚动时闪色差
    // WM_CTLCOLOREDIT：重命名编辑框默认白底黑字（DefWindowProc 兜底），
    //   在这里主题化成与待办输入框一致的 surface2 底
    if id == SUB_NOTE_LIST || id == SUB_TODO_LIST {
        match msg {
            WM_ERASEBKGND => {
                with_app(|app| unsafe {
                    let p = theme::palette(app.dark);
                    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                    GetClientRect(hwnd, &mut rc);
                    let b = CreateSolidBrush(p.window_bg);
                    FillRect(wp as HDC, &rc, b);
                    DeleteObject(b);
                });
                return 1;
            }
            WM_CTLCOLOREDIT => {
                let hdc = wp as HDC;
                let brush = with_app(|app| unsafe {
                    let p = theme::palette(app.dark);
                    SetBkMode(hdc, TRANSPARENT as i32);
                    SetTextColor(hdc, p.text);
                    windows_sys::Win32::Graphics::Gdi::SetBkColor(hdc, p.surface2);
                    app.brushes.surface2
                });
                return brush;
            }
            WM_COMMAND => {
                // 重命名编辑框的 EN_CHANGE：多行 EDIT 改文本只失效插入跨度，
                // 行首/行中键入（含 IME 上屏、退格）时右侧被推移的旧文字像素
                // 不重绘，与新文字叠成花屏——粘贴路径已在 rename::paste_flattened
                // 兜底，其余所有改动路径统一在此整框强制重绘
                let code = ((wp as u32) >> 16) & 0xffff;
                let cid = (wp as u32) & 0xffff;
                if code == EN_CHANGE && cid == crate::rename::ID_RENAME_EDIT {
                    unsafe { InvalidateRect(lp as HWND, std::ptr::null(), 0) };
                }
            }
            // 拖拽中滚动滚轮：原生滚动是位块搬移，会把已画的插入线像素一起
            // 搬到新位置，产生一串幽灵线；滚轮事件后整表失效一次全部清掉
            // （滚轮事件稀疏，整表重绘开销可忽略；拖拽中光标不动时幽灵线
            // 靠这个清除——增量失效只在光标移动时触发，管不到滚动）
            WM_MOUSEWHEEL => {
                let r = unsafe { DefSubclassProc(hwnd, msg, wp, lp) };
                with_app(|app| {
                    if id == SUB_NOTE_LIST && app.note_drag.map(|d| d.active) == Some(true) {
                        unsafe { InvalidateRect(hwnd, std::ptr::null(), 1) };
                    }
                });
                return r;
            }
            // 拖拽中屏蔽全部按键：Delete 触发删笔记、Esc 隐藏窗口、方向键移动
            // 选中并滚动——拖到一半任何一条都会乱套
            WM_KEYDOWN | WM_SYSKEYDOWN => {
                let swallow = with_app(|app| {
                    id == SUB_NOTE_LIST && app.note_drag.map(|d| d.active) == Some(true)
                });
                if swallow {
                    return 1;
                }
            }
            // 拖拽排序：按下记录候选行，移动越阈值后自绘管线画插入线，
            // 抬起时落地重排。处理完仍走 DefSubclassProc，不影响原生选中
            WM_LBUTTONDOWN => {
                with_app(|app| {
                    if id == SUB_NOTE_LIST && app.rename.is_none() {
                        let x = ((lp as usize) & 0xffff) as u16 as i16 as i32;
                        let y = (((lp as usize) >> 16) & 0xffff) as u16 as i16 as i32;
                        let item = lv_hit_item(hwnd, x, y);
                        app.note_drag = if item >= 0 {
                            Some(NoteDrag {
                                from: item,
                                sx: x,
                                sy: y,
                                cx: x,
                                cy: y,
                                line: -1,
                                active: false,
                            })
                        } else {
                            None
                        };
                    }
                });
            }
            WM_MOUSEMOVE => {
                with_app(|app| {
                    if id == SUB_NOTE_LIST {
                        if let Some(d) = &mut app.note_drag {
                            let x = ((lp as usize) & 0xffff) as u16 as i16 as i32;
                            let y = (((lp as usize) >> 16) & 0xffff) as u16 as i16 as i32;
                            d.cx = x;
                            d.cy = y;
                            if !d.active && (x - d.sx).abs() + (y - d.sy).abs() > 6 {
                                d.active = true;
                            }
                            if d.active {
                                // 只失效新旧插入线两条细带：整表失效每 move 全量重绘
                                // （擦底+重画所有行）肉眼就是持续闪烁
                                let new_line = notes::drag_line_y(hwnd, d.from, d.cx, d.cy);
                                if new_line.unwrap_or(-1) != d.line {
                                    let mut crc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                                    unsafe { GetClientRect(hwnd, &mut crc) };
                                    let mut strip = |y: i32| unsafe {
                                        InvalidateRect(
                                            hwnd,
                                            &RECT { left: 0, top: y - 2, right: crc.right, bottom: y + 2 },
                                            1,
                                        )
                                    };
                                    if d.line >= 0 {
                                        strip(d.line);
                                    }
                                    if let Some(yy) = new_line {
                                        strip(yy);
                                    }
                                    d.line = new_line.unwrap_or(-1);
                                }
                            }
                        }
                    }
                });
            }
            WM_LBUTTONUP => {
                with_app(|app| {
                    if id == SUB_NOTE_LIST {
                        if let Some(d) = app.note_drag.take() {
                            if d.active {
                                let x = ((lp as usize) & 0xffff) as u16 as i16 as i32;
                                let y = (((lp as usize) >> 16) & 0xffff) as u16 as i16 as i32;
                                if let Some(to) = drag_drop_index(hwnd, d.from, x, y) {
                                    // reorder 内部 refresh 全表重绘，顺带抹掉插入线
                                    notes::reorder(app, d.from as usize, to);
                                } else {
                                    // 落点无效只抹掉插入线
                                    if d.line >= 0 {
                                        let mut crc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                                        unsafe { GetClientRect(hwnd, &mut crc) };
                                        unsafe {
                                            InvalidateRect(
                                                hwnd,
                                                &RECT { left: 0, top: d.line - 2, right: crc.right, bottom: d.line + 2 },
                                                1,
                                            )
                                        };
                                    }
                                }
                            }
                        }
                    }
                });
            }
            // 注意：不处理 WM_CAPTURECHANGED——comctl 列表按下后自行 SetCapture，
            // 检测到拖拽发出 LVN_BEGINDRAG 后会 ReleaseCapture（约定由应用接管拖拽
            // 循环），此时捕获丢失消息必然到达，但我们的拖拽不依赖捕获：光标仍
            // 在列表上就仍能收到 WM_MOUSEMOVE/WM_LBUTTONUP，状态由 BUTTONUP 收尾。
            // 若在此清状态，拖拽永远活不过 BEGINDRAG
            WM_KILLFOCUS => {
                // 失焦兜底：拖拽中途点别处/Alt-Away，残留的插入线随状态清除
                with_app(|app| {
                    if let Some(d) = app.note_drag.take() {
                        if d.line >= 0 {
                            let mut crc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                            unsafe { GetClientRect(hwnd, &mut crc) };
                            unsafe {
                                InvalidateRect(
                                    hwnd,
                                    &RECT { left: 0, top: d.line - 2, right: crc.right, bottom: d.line + 2 },
                                    1,
                                )
                            };
                        }
                    }
                });
            }
            _ => {}
        }
    }
    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
        let vk = wp as u32;
        let root = GetAncestor(hwnd, GA_ROOT);
        match id {
            SUB_EDITOR => {
                if vk == VK_ESCAPE as u32 {
                    PostMessageW(root, MSG_ESC_HIDE, 0, 0);
                    return 1;
                }
                if vk == VK_N && (GetKeyState(VK_CONTROL as i32) as i16) < 0 {
                    PostMessageW(root, MSG_NEW_NOTE, 0, 0);
                    return 1;
                }
            }
            SUB_TODO_INPUT => {
                if vk == VK_ESCAPE as u32 {
                    PostMessageW(root, MSG_ESC_HIDE, 0, 0);
                    return 1;
                }
                if vk == VK_RETURN as u32 {
                    PostMessageW(root, MSG_ADD_TODO, 0, 0);
                    return 1;
                }
            }
            SUB_NOTE_LIST => {
                if vk == VK_ESCAPE as u32 {
                    PostMessageW(root, MSG_ESC_HIDE, 0, 0);
                    return 1;
                }
                if vk == VK_DELETE as u32 {
                    PostMessageW(root, MSG_DEL_NOTE, 0, 0);
                    return 1;
                }
            }
            SUB_TODO_LIST => {
                if vk == VK_ESCAPE as u32 {
                    PostMessageW(root, MSG_ESC_HIDE, 0, 0);
                    return 1;
                }
                if vk == VK_DELETE as u32 {
                    PostMessageW(root, MSG_DEL_TODO, 0, 0);
                    return 1;
                }
                if vk == VK_SPACE as u32 {
                    PostMessageW(root, MSG_TOGGLE_TODO, 0, 0);
                    return 1;
                }
            }
            _ => {}
        }
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}
