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

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WAIT_FAILED, WPARAM};
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
    GetForegroundWindow, IsWindow, IsWindowVisible, LoadCursorW, MsgWaitForMultipleObjectsEx,
    PeekMessageW, PM_REMOVE, PostQuitMessage, RegisterClassW, SetCursor, SetForegroundWindow,
    ShowWindow, SystemParametersInfoW, TranslateMessage, CS_DROPSHADOW, IDC_ARROW, MSG,
    MWMO_INPUTAVAILABLE, QS_ALLINPUT, SW_SHOW, WNDCLASSW, WM_CAPTURECHANGED, WM_ERASEBKGND,
    WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NULL, WM_PAINT, WM_QUIT,
    WM_RBUTTONDOWN, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, SPI_GETWORKAREA,
};

use crate::theme;
use crate::util::wide;

#[derive(Clone)]
pub struct Item {
    pub id: u32,
    pub text: String,
    pub sep: bool,
    pub checked: bool,
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
    dark: bool,
    hover: i32,  // -1 = 无
    pressed: bool,
    result: u32, // 选中 id（0 = 取消）
    /// 销毁中：DestroyWindow 会触发 WM_CAPTURECHANGED（释放捕获），
    /// 置位防重入（否则在销毁流程里再销毁一次）
    closing: bool,
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

/// 在光标处弹出菜单并阻塞到关闭，返回选中命令 id（0 = 取消）
pub fn show(owner: HWND, dark: bool, items: &[Item]) -> u32 {
    unsafe {
        // 不可重入：菜单开着时再次 show（连续两次托盘/列表右键）会覆盖
        // static 状态，旧模态循环退出时的清理会误删新菜单的状态（unwrap panic）。
        // 先销毁旧菜单；旧循环醒来按 hwnd 识别归属，不会动新菜单的状态
        {
            let p = std::ptr::addr_of_mut!(MENU);
            if let Some(old) = (*p).take() {
                DestroyWindow(old.hwnd); // 销毁即释放捕获
                crate::util::destroy_font(old.font);
            }
        }
        register_class();
        let font = crate::util::create_ui_font(false);

        // 量宽：文字最大宽决定窗口宽
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
        let h = cy + BORDER;

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
                dark,
                hover: -1,
                pressed: false,
                result: 0,
                closing: false,
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
                    dismiss(m);
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

fn hit_row(m: &MenuState, x: i32, y: i32) -> i32 {
    for (i, rc) in m.rows.iter().enumerate() {
        if x >= rc.left && x < rc.right && y >= rc.top && y < rc.bottom && !m.items[i].sep {
            return i as i32;
        }
    }
    -1
}

fn dismiss(m: &mut MenuState) {
    if m.closing {
        return;
    }
    m.closing = true;
    unsafe {
        // 销毁窗口即释放捕获（无需显式 ReleaseCapture）；释放过程产生的
        // WM_CAPTURECHANGED 由 closing 守卫跳过
        DestroyWindow(m.hwnd);
        // 捕获期间 owner 的鼠标消息被菜单吞掉；销毁后补一个空消息让队列醒转
        windows_sys::Win32::UI::WindowsAndMessaging::PostMessageW(m.owner, WM_NULL, 0, 0);
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

unsafe extern "system" fn menu_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_PAINT => {
            if !menu_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            menu_with(|m| unsafe {
                let mut ps: PAINTSTRUCT = std::mem::zeroed();
                let hdc = BeginPaint(hwnd, &mut ps);
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                GetClientRect(hwnd, &mut rc);
                let (w, h) = (rc.right, rc.bottom);

                // 双缓冲防闪烁
                let mem = CreateCompatibleDC(hdc);
                let bmp = CreateCompatibleBitmap(hdc, w, h);
                let old_bmp = SelectObject(mem, bmp);
                let p = theme::palette(m.dark);
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
            }, ());
            0
        }
        WM_ERASEBKGND => 1,
        WM_MOUSEMOVE => {
            if !menu_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            // 捕获期间系统不发 WM_SETCURSOR（该消息仅限未捕获状态），光标会停留
            // 为上一个窗口的形状（如文本工字形）——每次移动自己设回箭头。
            // 捕获下菜单能收到屏幕任何位置的移动，效果等同原生菜单：菜单开着时光标全屏都是箭头
            unsafe { SetCursor(LoadCursorW(0, IDC_ARROW)) };
            menu_with(|m| unsafe {
                let (x, y) = client_from_lparam(lp);
                let hov = hit_row(m, x, y);
                if hov != m.hover {
                    m.hover = hov;
                    if !m.pressed {
                        InvalidateRect(hwnd, std::ptr::null(), 0);
                    }
                }
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut tme);
            }, ());
            0
        }
        WM_MOUSELEAVE => {
            if menu_ready() {
                menu_with(|m| unsafe {
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
            menu_with(|m| unsafe {
                let (x, y) = client_from_lparam(lp);
                if hit_row(m, x, y) >= 0 {
                    m.pressed = true;
                    InvalidateRect(hwnd, std::ptr::null(), 0);
                } else {
                    // 点击菜单外：取消（SetCapture 下菜单窗口能收到所有点击）
                    m.result = 0;
                    dismiss(m);
                }
            }, ());
            0
        }
        WM_LBUTTONUP => {
            if !menu_ready() {
                return DefWindowProcW(hwnd, msg, wp, lp);
            }
            menu_with(|m| unsafe {
                let (x, y) = client_from_lparam(lp);
                let hit = hit_row(m, x, y);
                if hit >= 0 {
                    m.result = m.items[hit as usize].id;
                    dismiss(m);
                } else if m.pressed {
                    // 按下后移出行外松开：回弹，不关闭
                    m.pressed = false;
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
                    dismiss(m);
                }, ());
                return 0;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        WM_KEYDOWN => {
            // 菜单激活时持有焦点，Esc 关闭（原生菜单行为）
            if wp as u32 == 0x1B && menu_ready() {
                menu_with(|m| {
                    m.result = 0;
                    dismiss(m);
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
                    dismiss(m);
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
