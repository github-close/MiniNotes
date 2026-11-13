//! 自绘弹出菜单：替代 TrackPopupMenu 原生菜单。
//!
//! 为什么不用原生菜单：Win10 上菜单配色跟随**系统**的「应用深色」设置
//! （AppsUseLightTheme），进程级 uxtheme 暗色开关（theme.rs）管不到它——
//! 应用设为暗色而系统为亮色时，原生菜单仍是白色，与整体设计割裂。
//! 自绘菜单与应用同一色板、同一绘制基元（GDI+ 圆角），任何系统设置下一致。
//!
//! 交互模型：菜单窗口带 WS_EX_NOACTIVATE 弹出（不抢焦点，主窗口保持前台、
//! 不会触发失焦自动隐藏），自身 SetCapture 接管鼠标——悬停高亮、点击选中、
//! 点击菜单外/右键即取消。show() 内跑消息循环直到菜单销毁，返回选中 id
//! （语义等同 TrackPopupMenu 的 TPM_RETURNCMD；0 = 取消）。
//!
//! 二级子菜单（Item.sub）：hover 到子菜单宿主行就地弹出、移开即收起（原生
//! 菜单行为）。子菜单是 owned 展示窗口——无捕获、不激活（SW_SHOWNA），主菜单
//! 全程持有捕获，子菜单上的所有鼠标事件仍先到达主菜单，由主菜单跨窗口命中
//! 测试代管 hover/按压/选中（见 WM_MOUSEMOVE 等分支）。子项被点击时其 id
//! 直接成为 show() 的返回值，与一级菜单项无区别。

use windows_sys::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, WAIT_FAILED, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreatePen, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, GetDC, GetTextExtentPoint32W,
    InvalidateRect, LineTo, MoveToEx, ReleaseDC, SelectObject, SetBkMode, SetTextColor,
    DT_LEFT, DT_SINGLELINE, DT_VCENTER, PAINTSTRUCT, PS_SOLID, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::INFINITE;
use windows_sys::Win32::UI::Controls::WM_MOUSELEAVE;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCursorPos, GetClientRect,
    GetForegroundWindow, GetWindowRect, IsWindow, IsWindowVisible, LoadCursorW,
    MsgWaitForMultipleObjectsEx, PeekMessageW, PM_REMOVE, PostQuitMessage, RegisterClassW,
    SetCursor, SetForegroundWindow, ShowWindow, SystemParametersInfoW, TranslateMessage,
    CS_DROPSHADOW, IDC_ARROW, MSG, MWMO_INPUTAVAILABLE, QS_ALLINPUT, SW_SHOWNA, SW_SHOW,
    WNDCLASSW, WM_CAPTURECHANGED, WM_ERASEBKGND, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP,
    WM_MOUSEMOVE, WM_NULL, WM_PAINT, WM_QUIT, WM_RBUTTONDOWN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_POPUP, SPI_GETWORKAREA,
};

use crate::theme;
use crate::util::wide;

#[derive(Clone)]
pub struct Item {
    pub id: u32,
    pub text: String,
    pub sep: bool,
    pub checked: bool,
    /// 行尾小色块（填充底色子菜单用）：直接展示实际色值，None = 不画
    pub swatch: Option<COLORREF>,
    /// 二级子菜单：hover 本行自动弹出（原生行为），移开/收起见模块注释。
    /// 子菜单不再嵌套（只支持一层）
    pub sub: Option<Vec<Item>>,
}

const MENU_CLASS: &str = "MiniNotesMenuWnd";
const ITEM_H: i32 = 30;
const SEP_H: i32 = 9;
const TEXT_X: i32 = 40; // 文字左缘（勾选圆占位 14..34）
const PAD_R: i32 = 18;
const MIN_W: i32 = 140;
const BORDER: i32 = 1; // 1px 外框

struct MenuState {
    hwnd: HWND,
    owner: HWND,
    /// 菜单打开时 owner 是否可见（可见 → 消失即隐藏流程，需连带关菜单）
    owner_visible: bool,
    items: Vec<Item>,
    rows: Vec<RECT>,
    font: isize,
    theme: crate::theme::Theme,
    hover: i32,  // -1 = 无
    pressed: bool,
    result: u32, // 选中 id（0 = 取消）
    /// 销毁中：DestroyWindow 会触发 WM_CAPTURECHANGED（释放捕获），
    /// 置位防重入（否则在销毁流程里再销毁一次）
    closing: bool,
    // ----- 以下字段仅子菜单 / 仅主菜单使用，单结构体共用省一套类型 -----
    /// 子菜单窗口屏幕矩形（子菜单：主菜单命中测试用；创建后不动）
    screen: RECT,
    /// 宿主行在主菜单中的行号（子菜单）
    anchor: usize,
    /// 打开的二级子菜单（主菜单）
    sub: Option<Box<MenuState>>,
    /// 是否二级子菜单：字体借用主菜单的（font 字段是拷贝），清理时不销毁；
    /// 无捕获，纯展示
    is_sub: bool,
}

// UI 单线程，与 window.rs 的 APP 相同的静态裸指针模式
static mut MENU: Option<Box<MenuState>> = None;

/// 带状态处理菜单消息；state 不在（重入替换/清理的窗口期收到残余消息）时
/// 返回 default 而不是 panic——窗口消息与 static 生命周期必须解耦
fn menu_with<R>(f: impl FnOnce(&mut MenuState) -> R, default: R) -> R {
    unsafe {
        let p = std::ptr::addr_of_mut!(MENU);
        match (*p).as_mut() {
            Some(m) => f(m),
            None => default,
        }
    }
}

fn menu_ready() -> bool {
    unsafe { (*std::ptr::addr_of_mut!(MENU)).is_some() }
}

/// wndproc 收到的 hwnd 可能是主菜单或它打开的子菜单窗口，定位对应状态
fn state_for(root: &mut MenuState, hwnd: HWND) -> *mut MenuState {
    if root.hwnd == hwnd {
        return root;
    }
    match root.sub.as_mut() {
        Some(s) if s.hwnd == hwnd => s.as_mut() as *mut MenuState,
        _ => std::ptr::null_mut(),
    }
}

/// 量宽 + 行布局：窗口宽由最长文字决定，返回 (宽, 各行客户区矩形, 总高)。
/// 主菜单与子菜单共用（子菜单宽度独立量自己的文字）
fn layout(font: isize, items: &[Item]) -> (i32, Vec<RECT>, i32) {
    let mut w = MIN_W;
    for it in items {
        if !it.sep {
            let tw = text_width(font, &it.text);
            w = w.max(TEXT_X + tw + PAD_R);
        }
    }
    let mut cy = 0i32; // 行布局累加器（内容总高），勿与窗口锚点 y 混用
    let mut rows = Vec::with_capacity(items.len());
    for it in items {
        let rh = if it.sep { SEP_H } else { ITEM_H };
        rows.push(RECT {
            left: BORDER,
            top: cy + BORDER,
            right: w - BORDER,
            bottom: cy + BORDER + rh,
        });
        cy += rh;
    }
    (w, rows, cy + BORDER)
}

/// 在光标处弹出菜单并阻塞到关闭，返回选中命令 id（0 = 取消）；
/// 子菜单项被点选时返回的是子项的 id
pub fn show(owner: HWND, theme: crate::theme::Theme, items: &[Item]) -> u32 {
    unsafe {
        // 不可重入：菜单开着时再次 show（连续两次托盘/列表右键）会覆盖
        // static 状态，旧模态循环退出时的清理会误删新菜单的状态（unwrap panic）。
        // 先销毁旧菜单；旧循环醒来按 hwnd 识别归属，不会动新菜单的状态
        {
            let p = std::ptr::addr_of_mut!(MENU);
            if let Some(old) = (*p).take() {
                DestroyWindow(old.hwnd); // 销毁即释放捕获；子菜单是 owned 窗口，级联销毁
                if !old.is_sub {
                    crate::util::destroy_font(old.font);
                }
            }
        }
        register_class();
        let font = crate::util::create_ui_font(false);
        let (w, rows, h) = layout(font, items);

        // 锚点 = 光标；按工作区翻转/收拢，保证完整可见（近似原生 TrackPopupMenu 行为）
        let mut pt = POINT { x: 0, y: 0 };
        GetCursorPos(&mut pt);
        let mut wa = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut core::ffi::c_void, 0);
        let mut x = pt.x;
        if x + w > wa.right {
            x = wa.right - w;
        }
        let mut y = pt.y;
        if y + h > wa.bottom {
            // 底部放不下：向上翻转贴住光标上缘（托盘菜单的正确位置）
            y = (pt.y - h).max(wa.top);
        }

        let hwnd = CreateWindowExW(
            (WS_EX_TOOLWINDOW | WS_EX_TOPMOST) as u32,
            wide(MENU_CLASS).as_ptr(),
            std::ptr::null(),
            WS_POPUP as u32,
            x,
            y,
            w,
            h,
            owner, // WS_POPUP 时此参数为 owner：随主窗口销毁而销毁
            0,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        );
        if hwnd == 0 {
            crate::util::destroy_font(font);
            return 0;
        }
        {
            let p = std::ptr::addr_of_mut!(MENU);
            *p = Some(Box::new(MenuState {
                hwnd,
                owner,
                owner_visible: IsWindowVisible(owner) != 0,
                items: items.to_vec(),
                rows,
                font,
                theme,
                hover: -1,
                pressed: false,
                result: 0,
                closing: false,
                screen: RECT { left: x, top: y, right: x + w, bottom: y + h },
                anchor: 0,
                sub: None,
                is_sub: false,
            }));
        }

        // 菜单必须成为前台窗口：SetCapture 只对前台窗口完全生效（MSDN），
        // 后台窗口的捕获仅覆盖自己可视区上方的事件——点其他程序时消息直接给
        // 对方，菜单收不到任何通知，永远不消失。托盘点击会给本进程前台权限，
        // 列表右键时主窗口本就是前台，两种场景下这里的激活都会成功。
        // owner 因激活转移收到 WM_ACTIVATE(0) 不会误触失焦隐藏：
        // should_skip_auto_hide 按 PID 判定（菜单同属本进程）。
        ShowWindow(hwnd, SW_SHOW);
        SetForegroundWindow(hwnd);
        SetCapture(hwnd);

        // 模态循环：等同 TrackPopupMenu(TPM_RETURNCMD)，期间 owner 照常收消息。
        // 用 MsgWait+Peek 而非 GetMessage：菜单销毁后队列可能为空，GetMessage
        // 会永久阻塞；MWMO_INPUTAVAILABLE 兼顾「先有消息后等待」的竞态
        loop {
            menu_with(|m| {
                // owner 在菜单期间被隐藏（失焦自动隐藏）→ 连带取消关闭
                if m.owner_visible && IsWindowVisible(m.owner) == 0 {
                    m.result = 0;
                    dismiss_all(m);
                }
            }, ());
            if IsWindow(hwnd) == 0 {
                break;
            }
            let wres =
                MsgWaitForMultipleObjectsEx(0, std::ptr::null(), INFINITE, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
            if wres == WAIT_FAILED {
                break;
            }
            let mut quit_seen = false;
            let mut msg: MSG = std::mem::zeroed();
            while PeekMessageW(&mut msg, 0, 0, 0, PM_REMOVE) != 0 {
                if msg.message == WM_QUIT {
                    // 与原生菜单一致：WM_QUIT 回贴给主循环
                    PostQuitMessage(msg.wParam as i32);
                    quit_seen = true;
                    break;
                }
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if quit_seen {
                break;
            }
        }

        {
            if IsWindow(hwnd) != 0 {
                DestroyWindow(hwnd);
            }
            // 菜单是激活方且 owner 可见时把前台还给 owner（点选菜单项的正常路径；
            // 若用户点击了其他程序，前台已是对方，不再抢回）
            if GetForegroundWindow() == hwnd && IsWindowVisible(owner) != 0 {
                SetForegroundWindow(owner);
            }
            let p = std::ptr::addr_of_mut!(MENU);
            if let Some(m) = (*p).take() {
                if m.hwnd == hwnd {
                    // 自己的菜单：正常清理并返回选中结果
                    let result = m.result;
                    crate::util::destroy_font(m.font);
                    return result;
                }
                // 被重入的新菜单替换：把新菜单的状态放回（它还在自己的模态
                // 循环里跑），旧循环直接退出，不返回任何结果
                *p = Some(m);
                return 0;
            }
        }
        0
    }
}

fn register_class() {
    unsafe {
        let class = wide(MENU_CLASS);
        let wc: WNDCLASSW = WNDCLASSW {
            lpfnWndProc: Some(menu_proc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(0, IDC_ARROW),
            style: CS_DROPSHADOW, // 系统给弹出窗加淡阴影，贴近原生菜单观感
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
    }
}

// ---------- 二级子菜单（主菜单代管交互） ----------

/// hover 进入带子菜单的行：在该行右侧弹出子菜单（右侧放不下翻到左侧），
/// 竖直与宿主行对齐并夹回工作区。已在该行打开时不重建——鼠标在宿主行与
/// 子菜单间往返时窗口不闪烁。子菜单 SW_SHOWNA 不激活不抢捕获：
/// 前台与鼠标捕获都留在主菜单，交互由主菜单跨窗口命中测试代管
unsafe fn open_sub(m: &mut MenuState, row: usize) {
    if m.closing || m.sub.as_ref().map_or(false, |s| s.anchor == row) {
        return;
    }
    close_sub(m);
    let Some(items) = m.items.get(row).and_then(|it| it.sub.clone()) else {
        return;
    };
    let (w, rows, h) = layout(m.font, &items);
    let mut prc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetWindowRect(m.hwnd, &mut prc);
    let mut wa = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    SystemParametersInfoW(SPI_GETWORKAREA, 0, &mut wa as *mut RECT as *mut core::ffi::c_void, 0);
    let hr = m.rows[row];
    let mut x = prc.left + hr.right;
    let mut y = prc.top + hr.top;
    if x + w > wa.right {
        // 右侧放不下：翻到主菜单左侧
        x = (prc.left + hr.left - w).max(wa.left);
    }
    y = y.max(wa.top);
    if y + h > wa.bottom {
        y = wa.bottom - h;
    }
    let hwnd = CreateWindowExW(
        (WS_EX_TOOLWINDOW | WS_EX_TOPMOST) as u32,
        wide(MENU_CLASS).as_ptr(),
        std::ptr::null(),
        WS_POPUP as u32,
        x,
        y,
        w,
        h,
        m.hwnd, // owner = 主菜单窗口：级联销毁、保持在其上层
        0,
        GetModuleHandleW(std::ptr::null()),
        std::ptr::null(),
    );
    if hwnd == 0 {
        return;
    }
    // 状态先入 static 再显示：WM_PAINT 要能按 hwnd 找到状态
    m.sub = Some(Box::new(MenuState {
        hwnd,
        owner: m.hwnd,
        owner_visible: true,
        items,
        rows,
        font: m.font, // 借用主菜单字体；is_sub 保证清理不销毁
        theme: m.theme,
        hover: -1,
        pressed: false,
        result: 0,
        closing: false,
        screen: RECT { left: x, top: y, right: x + w, bottom: y + h },
        anchor: row,
        sub: None,
        is_sub: true,
    }));
    ShowWindow(hwnd, SW_SHOWNA);
}

fn close_sub(m: &mut MenuState) {
    if let Some(s) = m.sub.take() {
        unsafe { DestroyWindow(s.hwnd) };
    }
}

/// 整组关闭（主菜单 + 子菜单）：子菜单先销毁（状态随 Box 同步摘除），
/// 随后主菜单走标准销毁流程。子菜单是 owned 窗口，销毁主菜单系统也会
/// 级联销毁它，显式先关只是让状态摘除与窗口销毁的顺序可控
fn dismiss_all(m: &mut MenuState) {
    if m.closing {
        return;
    }
    m.closing = true;
    close_sub(m);
    unsafe {
        // 销毁窗口即释放捕获（无需显式 ReleaseCapture）；释放过程产生的
        // WM_CAPTURECHANGED 由 closing 守卫跳过
        DestroyWindow(m.hwnd);
        // 捕获期间 owner 的鼠标消息被菜单吞掉；销毁后补一个空消息让队列醒转
        windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(m.owner, WM_NULL, 0, 0);
    }
}

/// 主菜单捕获期间的鼠标客户坐标 → 屏幕坐标（无边框 popup 客户区原点 == 窗口原点，
/// 与行布局同基准）
fn screen_point(m: &MenuState, x: i32, y: i32) -> (i32, i32) {
    unsafe {
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetWindowRect(m.hwnd, &mut rc);
        (rc.left + x, rc.top + y)
    }
}

fn in_rect(p: (i32, i32), r: RECT) -> bool {
    p.0 >= r.left && p.0 < r.right && p.1 >= r.top && p.1 < r.bottom
}

/// 命中测试：客户区 (x, y) 所在的行（分隔行不算），-1 = 无
fn hit_row(m: &MenuState, x: i32, y: i32) -> i32 {
    for (i, rc) in m.rows.iter().enumerate() {
        if x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom && !m.items[i].sep {
            return i as i32;
        }
    }
    -1
}

fn track_leave(hwnd: HWND) {
    unsafe {
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        TrackMouseEvent(&mut tme);
    }
}

/// 勾选圆：实心强调圆 + 对勾（两段线段，画法与待办勾选框一致）
unsafe fn draw_check(hdc: isize, cx: i32, cy: i32, p: &theme::Palette) {
    let r = 6;
    theme::fill_round(
        hdc,
        RECT { left: cx - r, top: cy - r, right: cx + r, bottom: cy + r },
        r as f32,
        Some(p.accent),
        None,
    );
    let pen = CreatePen(PS_SOLID, 2, p.on_accent);
    let old = SelectObject(hdc, pen);
    let mut pt = POINT { x: 0, y: 0 };
    MoveToEx(hdc, cx - 3, cy, &mut pt);
    LineTo(hdc, cx - 1, cy + 2);
    // LineTo 从上一终点续画：第二段 (cx-1, cy+2) → (cx+3, cy-3)
    LineTo(hdc, cx + 3, cy - 3);
    SelectObject(hdc, old);
    DeleteObject(pen);
}

/// 绘制整个菜单（主菜单/子菜单共用；双缓冲防闪烁）
unsafe fn paint_state(hwnd: HWND, m: &MenuState) {
    unsafe {
        let mut ps: PAINTSTRUCT = std::mem::zeroed();
        let hdc = BeginPaint(hwnd, &mut ps);
        let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(hwnd, &mut rc);
        let (w, h) = (rc.right, rc.bottom);

        let mem = CreateCompatibleDC(hdc);
        let bmp = CreateCompatibleBitmap(hdc, w, h);
        let old_bmp = SelectObject(mem, bmp);
        let p = theme::palette(m.theme);
        let bg = CreateSolidBrush(p.window_bg);
        FillRect(mem, &rc, bg);
        DeleteObject(bg);

        // 1px 外框
        let line = CreatePen(PS_SOLID, 1, p.hairline);
        let old_pen = SelectObject(mem, line);
        let mut pt0 = POINT { x: 0, y: 0 };
        MoveToEx(mem, 0, h - 1, &mut pt0);
        LineTo(mem, 0, 0);
        LineTo(mem, w - 1, 0);
        LineTo(mem, w - 1, h - 1);
        LineTo(mem, 0, h - 1);
        SelectObject(mem, old_pen);
        DeleteObject(line);

        SetBkMode(mem, TRANSPARENT as i32);
        SelectObject(mem, m.font);
        for (i, it) in m.items.iter().enumerate() {
            let row = m.rows[i];
            if it.sep {
                // 分隔线：1px 细线，左右留边
                let cy = (row.top + row.bottom) / 2;
                let b = CreateSolidBrush(p.hairline);
                FillRect(mem, &RECT { left: row.left + 10, top: cy, right: row.right - 10, bottom: cy + 1 }, b);
                DeleteObject(b);
                continue;
            }
            if m.hover == i as i32 {
                theme::fill_round(
                    mem,
                    RECT {
                        left: row.left + 2,
                        top: row.top + 2,
                        right: row.right - 2,
                        bottom: row.bottom - 2,
                    },
                    6.0,
                    Some(if m.pressed { p.accent_tint } else { p.hover }),
                    None,
                );
            }
            if it.checked {
                draw_check(mem, TEXT_X - 13, (row.top + row.bottom) / 2, p);
            }
            if let Some(sc) = it.swatch {
                // 行尾色块：12x14 圆角色片 + 1px 描边，浅粉彩不至于隐形
                let cy = (row.top + row.bottom) / 2;
                theme::fill_round(
                    mem,
                    RECT {
                        left: row.right - PAD_R - 12,
                        top: cy - 7,
                        right: row.right - PAD_R,
                        bottom: cy + 7,
                    },
                    3.0,
                    Some(sc),
                    Some((p.edge, 1.0)),
                );
            }
            SetTextColor(mem, p.text);
            let mut tr = RECT {
                left: row.left + TEXT_X,
                top: row.top,
                right: row.right - PAD_R,
                bottom: row.bottom,
            };
            DrawTextW(mem, wide(&it.text).as_ptr(), -1, &mut tr, DT_SINGLELINE | DT_VCENTER | DT_LEFT);
        }

        BitBlt(hdc, 0, 0, w, h, mem, 0, 0, SRCCOPY);
        SelectObject(mem, old_bmp);
        DeleteObject(bmp);
        DeleteDC(mem);
        EndPaint(hwnd, &ps);
    }
}

unsafe extern "system" fn menu_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            if menu_ready() {
                menu_with(|root| unsafe {
                    let target = state_for(root, hwnd);
                    if !target.is_null() {
                        paint_state(hwnd, &*target);
                    }
                }, ());
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            // 鼠标事件只可能到达持有捕获的主菜单窗口（子菜单无捕获）；
            // 收到非主菜单 hwnd 的残余消息时直接忽略
            if !menu_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            menu_with(|root| unsafe {
                if root.hwnd != hwnd || root.closing {
                    return;
                }
                // 捕获期间系统不发 WM_SETCURSOR（该消息仅限未捕获状态），光标会停留
                // 为上一个窗口的形状（如文本工字形）——每次移动自己设回箭头。
                // 捕获下菜单能收到屏幕任何位置的移动，效果等同原生菜单：菜单开着时光标全屏都是箭头
                SetCursor(LoadCursorW(0, IDC_ARROW));
                let (x, y) = client_from_lparam(lp);
                let sp = screen_point(root, x, y);
                // 子菜单区域内：代管子菜单 hover（命中子菜单行高亮；进入
                // 子菜单不算「移开宿主行」，子菜单保持打开）
                {
                    let sub_in = root.sub.as_ref().map_or(false, |s| in_rect(sp, s.screen));
                    if sub_in {
                        let s = root.sub.as_mut().unwrap();
                        let hov = hit_row(s, sp.0 - s.screen.left, sp.1 - s.screen.top);
                        if hov != s.hover {
                            s.hover = hov;
                            if hov < 0 {
                                s.pressed = false;
                            }
                            InvalidateRect(s.hwnd, std::ptr::null(), 0);
                        }
                        track_leave(hwnd);
                        return;
                    }
                }
                // 主菜单区域内：hover 变化 + 子菜单开/关
                let hov = hit_row(root, x, y);
                if hov != root.hover {
                    root.hover = hov;
                    match hov {
                        h if h >= 0 && root.items[h as usize].sub.is_some() => open_sub(root, h as usize),
                        _ => close_sub(root),
                    }
                    if !root.pressed {
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                    }
                }
                track_leave(hwnd);
            }, ());
            0
        }
        WM_MOUSELEAVE => {
            if menu_ready() {
                menu_with(|root| unsafe {
                    let target = state_for(root, hwnd);
                    if target.is_null() {
                        return;
                    }
                    let m = &mut *target;
                    if m.hover != -1 {
                        m.hover = -1;
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                    }
                }, ());
            }
            0
        }
        WM_LBUTTONDOWN => {
            if !menu_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            menu_with(|root| unsafe {
                if root.hwnd != hwnd || root.closing {
                    return;
                }
                let (x, y) = client_from_lparam(lp);
                let sp = screen_point(root, x, y);
                // 按在子菜单上：子菜单行进入按压态（高亮加深）
                let sub_in = root.sub.as_ref().map_or(false, |s| in_rect(sp, s.screen));
                if sub_in {
                    let s = root.sub.as_mut().unwrap();
                    let hov = hit_row(s, sp.0 - s.screen.left, sp.1 - s.screen.top);
                    if hov >= 0 {
                        s.pressed = true;
                        InvalidateRect(s.hwnd, std::ptr::null(), 0);
                    }
                    return;
                }
                let hit = hit_row(root, x, y);
                if hit >= 0 {
                    root.pressed = true;
                    // 子菜单宿主行：按下只确保子菜单打开（不选中不关闭）
                    if root.items[hit as usize].sub.is_some() {
                        open_sub(root, hit as usize);
                    }
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                } else {
                    // 点击菜单外：取消（SetCapture 下菜单窗口能收到所有点击）
                    root.result = 0;
                    dismiss_all(root);
                }
            }, ());
            0
        }
        WM_LBUTTONUP => {
            if !menu_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            menu_with(|root| unsafe {
                if root.hwnd != hwnd || root.closing {
                    return;
                }
                let (x, y) = client_from_lparam(lp);
                let sp = screen_point(root, x, y);
                // 抬在子菜单上：选中子项（id 直接作为 show() 返回值）
                let sub_in = root.sub.as_ref().map_or(false, |s| in_rect(sp, s.screen));
                if sub_in {
                    let (hov, sid) = {
                        let s = root.sub.as_mut().unwrap();
                        let hov = hit_row(s, sp.0 - s.screen.left, sp.1 - s.screen.top);
                        (hov, if hov >= 0 { s.items[hov as usize].id } else { 0 })
                    };
                    if hov >= 0 {
                        root.result = sid;
                        dismiss_all(root);
                    } else if root.sub.as_ref().map_or(false, |s| s.pressed) {
                        // 按下后移出行外松开：回弹，不关闭
                        let s = root.sub.as_mut().unwrap();
                        s.pressed = false;
                        InvalidateRect(s.hwnd, std::ptr::null(), 0);
                    }
                    return;
                }
                let hit = hit_row(root, x, y);
                if hit >= 0 {
                    // 子菜单宿主行：点击不产生命令也不关闭（原生菜单行为，
                    // 子菜单保持打开）
                    if root.items[hit as usize].sub.is_some() {
                        root.pressed = false;
                        open_sub(root, hit as usize);
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                        return;
                    }
                    root.result = root.items[hit as usize].id;
                    dismiss_all(root);
                } else if root.pressed {
                    // 按下后移出行外松开：回弹，不关闭
                    root.pressed = false;
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                }
            }, ());
            0
        }
        WM_RBUTTONDOWN => {
            // 右键一律取消（与原生菜单一致：右键其他位置 = 关闭）
            if menu_ready() {
                menu_with(|m| {
                    m.result = 0;
                    dismiss_all(m);
                }, ());
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_KEYDOWN => {
            // 菜单激活时持有焦点，Esc 关闭（原生菜单行为）；子菜单开着时
            // 先只收起子菜单，再按一次才关主菜单（原生层级语义）
            if wp as u32 == 0x1B && menu_ready() {
                menu_with(|m| {
                    if m.hwnd != hwnd {
                        return;
                    }
                    if m.sub.is_some() {
                        close_sub(m);
                        unsafe { InvalidateRect(m.hwnd, std::ptr::null(), 0) };
                        return;
                    }
                    m.result = 0;
                    dismiss_all(m);
                }, ());
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_CAPTURECHANGED => {
            // 用户点击了其他线程的窗口：系统强制收回捕获（点击照常送达对方窗口），
            // 我们从此收不到任何鼠标消息——必须自行退出，否则菜单永远留在屏幕上。
            // 注意 closing 守卫：自己 dismiss（点中菜单项）的销毁流程也会同步
            // 产生这条消息，此时 result 已是选中命令，绝不能清零
            if menu_ready() {
                menu_with(|m| {
                    if m.closing {
                        return;
                    }
                    m.result = 0;
                    dismiss_all(m);
                }, ());
            }
            0 // 不再要回捕获
        }
        // 光标：不处理，走 DefWindowProc 应用类光标（IDC_ARROW）。自己返回 1
        // 而不调 SetCursor 会让光标停留为上一个窗口设置的形状（如文本工字形）
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn client_from_lparam(lp: LPARAM) -> (i32, i32) {
    let x = (lp & 0xFFFF) as u16 as i16 as i32;
    let y = ((lp >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

fn text_width(font: isize, s: &str) -> i32 {
    unsafe {
        let hdc = GetDC(0);
        let old = SelectObject(hdc, font);
        let mut sz = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
        let w = wide(s);
        GetTextExtentPoint32W(hdc, w.as_ptr(), (w.len() - 1) as i32, &mut sz);
        SelectObject(hdc, old);
        ReleaseDC(0, hdc);
        sz.cx
    }
}
