use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{GetSysColorBrush, HFONT, COLOR_WINDOW};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcessId;
use windows_sys::Win32::UI::Controls::{
    ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, ICC_STANDARD_CLASSES, ICC_TAB_CLASSES,
    INITCOMMONCONTROLSEX, LVCFMT_LEFT, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVIF_STATE,
    LVM_INSERTCOLUMNW, LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE, LVS_EX_CHECKBOXES,
    LVS_EX_FULLROWSELECT, LVS_EDITLABELS, LVS_NOCOLUMNHEADER, LVS_REPORT, LVS_SHOWSELALWAYS,
    LVIS_SELECTED, NMHDR, NMLVDISPINFOW, NMLISTVIEW, NMLVCUSTOMDRAW, TCIF_TEXT, TCITEMW,
    TCM_GETCURSEL, TCM_INSERTITEMW, TCM_SETCURSEL, TCN_SELCHANGE,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, SetFocus, VK_CONTROL, VK_DELETE, VK_ESCAPE, VK_RETURN,
};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetAncestor, GetClientRect, GetCursorPos, GetForegroundWindow, GetWindowRect,
    GetWindowThreadProcessId, IDC_ARROW, IsWindowVisible, KillTimer, LoadCursorW, MINMAXINFO,
    MoveWindow, PostMessageW, PostQuitMessage, RegisterClassW, SendMessageW, SetForegroundWindow,
    SetTimer, ShowWindow, SystemParametersInfoW, SW_HIDE, SW_SHOW, TPM_RETURNCMD, TPM_RIGHTBUTTON,
    TrackPopupMenu, WNDCLASSW, WM_ACTIVATE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_GETMINMAXINFO, WM_HOTKEY, WM_KEYDOWN, WM_NOTIFY, WM_NULL, WM_SIZE, WM_SYSKEYDOWN, WM_TIMER,
    WS_BORDER, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_CLIENTEDGE, WS_OVERLAPPEDWINDOW, WS_TABSTOP,
    WS_VISIBLE, WS_VSCROLL, BS_PUSHBUTTON, ES_AUTOHSCROLL, ES_AUTOVSCROLL, ES_MULTILINE, GA_ROOT,
    MF_SEPARATOR, MF_STRING, SIZE_MINIMIZED, SPI_GETWORKAREA, WM_LBUTTONUP, WM_RBUTTONUP,
};

use crate::config::Config;
use crate::{hotkey, notes, settings, todos, tray, util};

// ---------- 常量 ----------

pub const MAIN_CLASS: &str = "MiniNotesMainWnd";

pub const MSG_SHOW: u32 = WM_APP + 2;
pub const MSG_ESC_HIDE: u32 = WM_APP + 3;
pub const MSG_NEW_NOTE: u32 = WM_APP + 4;
pub const MSG_DEL_NOTE: u32 = WM_APP + 5;
pub const MSG_DEL_TODO: u32 = WM_APP + 6;
pub const MSG_ADD_TODO: u32 = WM_APP + 7;

pub const ID_TAB: u32 = 100;
pub const ID_TODO_INPUT: u32 = 201;
pub const ID_TODO_ADD: u32 = 202;
pub const ID_TODO_LIST: u32 = 203;
pub const ID_TODO_CLEAR: u32 = 204;
pub const ID_NOTE_LIST: u32 = 301;
pub const ID_NOTE_NEW: u32 = 302;
pub const ID_EDITOR: u32 = 303;
pub const TIMER_SAVE: usize = 1;

pub const TAB_TODO: usize = 0;

const SUB_EDITOR: usize = 1;
const SUB_TODO_INPUT: usize = 2;
const SUB_NOTE_LIST: usize = 3;
const SUB_TODO_LIST: usize = 4;

// 列表右键菜单命令
const M_NOTE_NEW: u32 = 2001;
const M_NOTE_RENAME: u32 = 2002;
const M_NOTE_DELETE: u32 = 2003;
const M_TODO_EDIT: u32 = 2101;
const M_TODO_DELETE: u32 = 2102;
const M_TODO_CLEAR: u32 = 2103;

const VK_N: u32 = 0x4E;

// ---------- 状态（仅 UI 线程） ----------

pub struct App {
    pub hwnd: HWND,
    pub cfg: Config,

    pub tab: HWND,
    pub todo_input: HWND,
    pub todo_add: HWND,
    pub todo_list: HWND,
    pub todo_clear: HWND,
    pub note_list: HWND,
    pub note_new: HWND,
    pub editor: HWND,

    pub font: HFONT,
    pub font_strike: HFONT,

    pub notes: Vec<notes::NoteMeta>,
    pub cur_note: Option<usize>,
    pub note_dirty: bool,
    pub suppress_notify: bool,
    pub todos: Vec<todos::TodoItem>,
    pub settings_open: bool,
    pub initialized: bool,
}

// Win32 消息天然重入（SendMessage 会同步回调 wndproc），RefCell 会撞借用panic，
// 因此用静态裸指针保存 App（UI 单线程，等同 C 的 GWLP_USERDATA 模式）
static mut APP: Option<Box<App>> = None;

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
    // 注意：不能用 .read() —— 会按值拷贝 Option<Box> 导致临时 Box 被误 drop（double-free）
    unsafe { (*std::ptr::addr_of_mut!(APP)).is_some() }
}

// ---------- 创建 ----------

pub fn init_common_controls() {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_TAB_CLASSES | ICC_BAR_CLASSES | ICC_STANDARD_CLASSES,
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
            hbrBackground: GetSysColorBrush(COLOR_WINDOW),
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);

        let mut wa = RECT { left: 0, top: 0, right: 1280, bottom: 720 };
        SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            &mut wa as *mut RECT as *mut core::ffi::c_void,
            0,
        );
        let (cx, cy) = (720i32, 540i32);
        let x = wa.left + ((wa.right - wa.left - cx) / 2).max(0);
        let y = wa.top + ((wa.bottom - wa.top - cy) / 2).max(0);

        CreateWindowExW(
            0,
            class.as_ptr(),
            util::wide("MiniNotes").as_ptr(),
            WS_OVERLAPPEDWINDOW,
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
            id as isize, // HMENU = 子控件 ID
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    }
}

/// 创建 App 状态与全部子控件，加载持久化数据并恢复上次状态
pub fn init_app(hwnd: HWND, cfg: Config) {
    let todos_data = todos::load();
    set_app(App {
        hwnd,
        cfg,
        tab: 0,
        todo_input: 0,
        todo_add: 0,
        todo_list: 0,
        todo_clear: 0,
        note_list: 0,
        note_new: 0,
        editor: 0,
        font: 0,
        font_strike: 0,
        notes: Vec::new(),
        cur_note: None,
        note_dirty: false,
        suppress_notify: false,
        todos: todos_data,
        settings_open: false,
        initialized: false,
    });

    unsafe {
        let tab = child(hwnd, "SysTabControl32", "", WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS, 0, ID_TAB);
        let todo_input = child(
            hwnd,
            "EDIT",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | ES_AUTOHSCROLL as u32,
            0,
            ID_TODO_INPUT,
        );
        let todo_add = child(hwnd, "BUTTON", "添加", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, 0, ID_TODO_ADD);
        let todo_list = child(
            hwnd,
            "SysListView32",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | LVS_REPORT | LVS_SHOWSELALWAYS
                | LVS_NOCOLUMNHEADER | LVS_EDITLABELS,
            0,
            ID_TODO_LIST,
        );
        let todo_clear = child(hwnd, "BUTTON", "清除已完成", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, 0, ID_TODO_CLEAR);
        let note_list = child(
            hwnd,
            "SysListView32",
            "",
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_BORDER | LVS_REPORT | LVS_SHOWSELALWAYS
                | LVS_NOCOLUMNHEADER | LVS_EDITLABELS,
            0,
            ID_NOTE_LIST,
        );
        let note_new = child(hwnd, "BUTTON", "＋ 新建", WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_PUSHBUTTON as u32, 0, ID_NOTE_NEW);
        let editor = child(
            hwnd,
            "RichEdit50W",
            "",
            WS_CHILD | WS_VISIBLE | WS_VSCROLL | ES_MULTILINE as u32 | ES_AUTOVSCROLL as u32,
            WS_EX_CLIENTEDGE,
            ID_EDITOR,
        );

        // 列表扩展样式：待办带勾选框；两个列表都整行选择
        let ex_todo = LVS_EX_CHECKBOXES | LVS_EX_FULLROWSELECT;
        SendMessageW(todo_list, LVM_SETEXTENDEDLISTVIEWSTYLE, ex_todo as WPARAM, ex_todo as LPARAM);
        SendMessageW(note_list, LVM_SETEXTENDEDLISTVIEWSTYLE, LVS_EX_FULLROWSELECT as WPARAM, LVS_EX_FULLROWSELECT as LPARAM);

        // 各插一列（无表头模式仍需列）
        insert_column(todo_list, 2000);
        insert_column(note_list, 146);

        // Tab：待办 / 笔记
        for (i, t) in ["待办", "笔记"].iter().enumerate() {
            let mut w = util::wide(t);
            let mut ti: TCITEMW = std::mem::zeroed();
            ti.mask = TCIF_TEXT;
            ti.pszText = w.as_mut_ptr();
            SendMessageW(tab, TCM_INSERTITEMW, i, &mut ti as *mut TCITEMW as LPARAM);
        }

        // 字体
        let font = util::create_ui_font(false);
        let font_strike = util::create_ui_font(true);
        for h in [tab, todo_input, todo_add, todo_list, todo_clear, note_list, note_new, editor] {
            util::apply_font(h, font);
        }

        // 子类化：Esc 隐藏 / Ctrl+N 新建 / Enter 加待办 / Del 删除
        SetWindowSubclass(editor, Some(child_subclass), SUB_EDITOR, 0);
        SetWindowSubclass(todo_input, Some(child_subclass), SUB_TODO_INPUT, 0);
        SetWindowSubclass(note_list, Some(child_subclass), SUB_NOTE_LIST, 0);
        SetWindowSubclass(todo_list, Some(child_subclass), SUB_TODO_LIST, 0);

        with_app(|app| {
            app.tab = tab;
            app.todo_input = todo_input;
            app.todo_add = todo_add;
            app.todo_list = todo_list;
            app.todo_clear = todo_clear;
            app.note_list = note_list;
            app.note_new = note_new;
            app.editor = editor;
            app.font = font;
            app.font_strike = font_strike;
        });

        // 数据加载与状态恢复
        with_app(todos::rebuild);
        notes::ensure_first_run();
        with_app(notes::refresh);
        with_app(|app| {
            // 恢复窗口位置尺寸
            if app.cfg.win_x >= 0 && app.cfg.win_cx >= 400 && app.cfg.win_cy >= 300 {
                MoveWindow(app.hwnd, app.cfg.win_x, app.cfg.win_y, app.cfg.win_cx, app.cfg.win_cy, 0);
            }
            // 加载上次的笔记内容到编辑器
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
    let mut col: LVCOLUMNW = unsafe { std::mem::zeroed() };
    col.mask = LVCF_TEXT | LVCF_WIDTH;
    col.fmt = LVCFMT_LEFT;
    col.cx = cx;
    col.pszText = w.as_mut_ptr();
    unsafe { SendMessageW(lv, LVM_INSERTCOLUMNW, 0, &mut col as *mut LVCOLUMNW as LPARAM) };
}

// ---------- 布局与页面 ----------

fn current_tab(app: &App) -> usize {
    unsafe { SendMessageW(app.tab, TCM_GETCURSEL, 0, 0).max(0) as usize }
}

fn layout(app: &App) {
    unsafe {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(app.hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);
        if w <= 0 || h <= 0 {
            return;
        }
        let tab_h = 30;
        MoveWindow(app.tab, 0, 0, w, tab_h, 1);
        let py = tab_h;
        let ph = h - tab_h;
        let cur = current_tab(app);
        let todo = cur == TAB_TODO;

        // 待办页
        ShowWindow(app.todo_input, if todo { SW_SHOW } else { SW_HIDE });
        ShowWindow(app.todo_add, if todo { SW_SHOW } else { SW_HIDE });
        ShowWindow(app.todo_list, if todo { SW_SHOW } else { SW_HIDE });
        ShowWindow(app.todo_clear, if todo { SW_SHOW } else { SW_HIDE });
        MoveWindow(app.todo_input, 8, py + 8, w - 118, 24, 1);
        MoveWindow(app.todo_add, w - 104, py + 8, 96, 24, 1);
        MoveWindow(app.todo_list, 8, py + 40, w - 16, ph - 40 - 42, 1);
        MoveWindow(app.todo_clear, w - 104, h - 34, 96, 26, 1);
        SendMessageW(app.todo_list, LVM_SETCOLUMNWIDTH, 0, (w - 44).max(60) as LPARAM);

        // 笔记页
        ShowWindow(app.note_new, if todo { SW_HIDE } else { SW_SHOW });
        ShowWindow(app.note_list, if todo { SW_HIDE } else { SW_SHOW });
        ShowWindow(app.editor, if todo { SW_HIDE } else { SW_SHOW });
        MoveWindow(app.note_new, 8, py + 8, 96, 24, 1);
        MoveWindow(app.note_list, 8, py + 40, 170, ph - 48, 1);
        MoveWindow(app.editor, 186, py + 8, w - 194, ph - 16, 1);
        SendMessageW(app.note_list, LVM_SETCOLUMNWIDTH, 0, 146);
    }
}

fn switch_tab(app: &mut App, idx: usize) {
    unsafe { SendMessageW(app.tab, TCM_SETCURSEL, idx, 0) };
    app.cfg.last_tab = idx;
    layout(app);
    focus_page(app);
}

fn focus_page(app: &App) {
    unsafe {
        if current_tab(app) == TAB_TODO {
            SetFocus(app.todo_input);
        } else {
            SetFocus(app.editor);
        }
    }
}

// ---------- 显示 / 隐藏 ----------

pub fn show_main() {
    with_app(|app| unsafe {
        if IsWindowVisible(app.hwnd) == 0 {
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
        app.cfg.last_tab = current_tab(app);
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
        if windows_sys::Win32::UI::WindowsAndMessaging::IsIconic(app.hwnd) != 0 {
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

// ---------- 窗口过程 ----------

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if !app_ready() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_SIZE => {
            if wp == SIZE_MINIMIZED as usize {
                hide_main();
                return 0;
            }
            with_app(|app| {
                if app.initialized {
                    layout(app)
                }
            });
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
                (*mmi).ptMinTrackSize = POINT { x: 560, y: 380 };
            }
            0
        }
        WM_DESTROY => {
            with_app(|app| {
                notes::save_current(app);
                save_window_rect(app);
                app.cfg.last_tab = current_tab(app);
                crate::config::save(&app.cfg);
                util::destroy_font(app.font);
                util::destroy_font(app.font_strike);
                app.font = 0;
                app.font_strike = 0;
            });
            tray::remove(hwnd);
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
            show_main();
            with_app(notes::create_note);
            0
        }
        MSG_ADD_TODO => {
            with_app(todos::add_from_input);
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

fn on_command(wp: WPARAM) -> LRESULT {
    let id = (wp & 0xFFFF) as u32;
    let code = ((wp >> 16) & 0xFFFF) as u32;
    match (code, id) {
        (windows_sys::Win32::UI::WindowsAndMessaging::EN_CHANGE, ID_EDITOR) => {
            with_app(|app| {
                notes::notify_dirty(app);
                if app.note_dirty {
                    unsafe { SetTimer(app.hwnd, TIMER_SAVE, 600, None) };
                }
            });
        }
        (0, ID_TODO_ADD) => with_app(todos::add_from_input),
        (0, ID_TODO_CLEAR) => with_app(todos::clear_done),
        (0, ID_NOTE_NEW) => with_app(notes::create_note),
        _ => {}
    }
    0
}

fn on_notify(hwnd: HWND, wp: WPARAM, lp: LPARAM) -> LRESULT {
    use windows_sys::Win32::UI::Controls::{
        LVN_ENDLABELEDIT, LVN_ITEMCHANGED, NM_CUSTOMDRAW, NM_DBLCLK, NM_RCLICK,
    };
    let hdr = lp as *const NMHDR;
    unsafe {
        let (id_from, code) = ((*hdr).idFrom, (*hdr).code);
        match id_from as u32 {
            ID_TAB if code == TCN_SELCHANGE => {
                with_app(|app| {
                    let cur = current_tab(app);
                    switch_tab(app, cur);
                });
                0
            }
            ID_TODO_LIST => match code {
                NM_CUSTOMDRAW => with_app(|app| todos::on_custom_draw(app, &*(lp as *const NMLVCUSTOMDRAW))),
                LVN_ITEMCHANGED => {
                    with_app(|app| todos::on_item_changed(app, &*(lp as *const NMLISTVIEW)));
                    0
                }
                LVN_ENDLABELEDIT => {
                    with_app(|app| todos::on_end_label_edit(app, &*(lp as *const NMLVDISPINFOW)) as LRESULT)
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
                LVN_ENDLABELEDIT => {
                    with_app(|app| notes::on_end_label_edit(app, &*(lp as *const NMLVDISPINFOW)) as LRESULT)
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
                        show_main();
                        with_app(notes::create_note);
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
    unsafe {
        let menu = CreatePopupMenu();
        for (id, text, sep) in items {
            if *sep {
                AppendMenuW(menu, MF_SEPARATOR, 0, std::ptr::null());
            } else {
                AppendMenuW(menu, MF_STRING, *id as usize, util::wide(text).as_ptr());
            }
        }
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RIGHTBUTTON | TPM_RETURNCMD, pt.x, pt.y, 0, hwnd, std::ptr::null()) as u32;
        PostMessageW(hwnd, WM_NULL, 0, 0);
        DestroyMenu(menu);
        cmd
    }
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

// ---------- 子控件子类：键盘快捷行为 ----------

unsafe extern "system" fn child_subclass(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
    id: usize,
    _d: usize,
) -> LRESULT {
    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
        let vk = wp as u32;
        let root = GetAncestor(hwnd, GA_ROOT);
        let esc = vk == VK_ESCAPE as u32;
        let del = vk == VK_DELETE as u32;
        match id {
            SUB_EDITOR => {
                if esc {
                    PostMessageW(root, MSG_ESC_HIDE, 0, 0);
                    return 1;
                }
                if vk == VK_N && (GetKeyState(VK_CONTROL as i32) as i16) < 0 {
                    PostMessageW(root, MSG_NEW_NOTE, 0, 0);
                    return 1;
                }
            }
            SUB_TODO_INPUT => {
                if esc {
                    PostMessageW(root, MSG_ESC_HIDE, 0, 0);
                    return 1;
                }
                if vk == VK_RETURN as u32 {
                    PostMessageW(root, MSG_ADD_TODO, 0, 0);
                    return 1;
                }
            }
            SUB_NOTE_LIST => {
                if del {
                    PostMessageW(root, MSG_DEL_NOTE, 0, 0);
                    return 1;
                }
            }
            SUB_TODO_LIST => {
                if del {
                    PostMessageW(root, MSG_DEL_TODO, 0, 0);
                    return 1;
                }
            }
            _ => {}
        }
    }
    DefSubclassProc(hwnd, msg, wp, lp)
}
