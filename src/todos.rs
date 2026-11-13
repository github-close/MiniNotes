//! 待办页：自绘列表窗口 + todos.json 读写。
//!
//! 为什么不用 ListView：报表视图只支持统一行高（行高由挂载的图标列表决定），
//! 无法「长条目换行、短条目单行」地变高。超长待办要求换行显示、一眼看全，
//! 因此换成自绘子窗口：条目高度按文字换行测量（上限 3 行，超出省略号），
//! 滚轮滚动 + 细滚动条指示，勾选框/选中/编辑/右键交互与原列表一致。

use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BeginPaint, BLENDFUNCTION, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateDIBSection, CreateSolidBrush,
    DeleteDC, DeleteObject, DIB_RGB_COLORS, DrawTextW, EndPaint, FillRect, GetDC,
    GetTextExtentPoint32W, InvalidateRect, ReleaseDC, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, DT_NOPREFIX, DT_SINGLELINE, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    SetFocus, VK_DELETE, VK_ESCAPE, VK_SPACE,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CS_DBLCLKS, DefWindowProcW, GetAncestor, GetClientRect, LoadCursorW, PostMessageW,
    RegisterClassW, GA_ROOT, IDC_ARROW, WM_COMMAND, WM_CTLCOLOREDIT, WM_ERASEBKGND, WM_KEYDOWN,
    WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SIZE, WNDCLASSW,
};

use crate::theme;
use crate::util::{get_text, set_text, wide};
use crate::window::{self, App};

#[derive(Serialize, Deserialize, Clone)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub created: u64,
}

// ---------- 自绘列表的布局常量 ----------

pub const TODO_LIST_CLASS: &str = "MiniNotesTodoList";

const TEXT_X: i32 = 30;      // 文字列（勾选框右侧），续行同列对齐
const TEXT_R_PAD: i32 = 8;   // 文字区右留白
const ITEM_VPAD: i32 = 4;    // 条目上下内边距
const CHECKBOX_W: i32 = 30;  // 勾选框点击区宽（首行图标位）
const MAX_LINES: i32 = 3;    // 换行上限，超出省略号
const SCROLLBAR_W: i32 = 4;  // 滚动条指示宽

/// EN_CHANGE 通知码（windows-sys 未导出该常量，与 rename.rs 的私有定义同值）
fn rename_en_change() -> u32 {
    0x0300
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn file_path() -> std::path::PathBuf {
    crate::util::data_dir().join("todos.json")
}

pub fn load() -> Vec<TodoItem> {
    let path = file_path();
    if !path.exists() {
        return Vec::new();
    }
    // 读取/解析失败时先把原文件改名备份：否则下一次保存会直接用空列表覆盖掉用户数据
    let parsed = std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str::<Vec<TodoItem>>(&s).ok());
    match parsed {
        Some(items) => items,
        None => {
            let _ = std::fs::rename(&path, path.with_extension("json.bak"));
            Vec::new()
        }
    }
}

pub fn save(items: &[TodoItem]) {
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(file_path(), json);
    }
}

// ---------- 自绘列表窗口 ----------

/// 勾选框图标条像素：与笔记列表 imagelist 同源（make_todo_checkboxes.py 烘焙，
/// 3 格 24x26 横排：0 纯底 / 1 未完成圆环 / 2 完成对勾，浅暗主题各一份）。
/// 这里不再走 ImageList，直接建 memory DC 后按格 AlphaBlend 贴图
static TODO_ICONS_LIGHT: &[u8] = include_bytes!("../assets/brand/todo-icons-light.bin");
static TODO_ICONS_DARK: &[u8] = include_bytes!("../assets/brand/todo-icons-dark.bin");

// 图标条 DC 缓存（每主题一份），WM_DESTROY 时 cleanup_assets 释放
static mut STRIP_DC: [isize; 2] = [0; 2];
static mut STRIP_BMP: [isize; 2] = [0; 2];

/// WM_DESTROY 时释放图标条资源
pub fn cleanup_assets() {
    unsafe {
        let (dc, bmp) = (std::ptr::addr_of_mut!(STRIP_DC), std::ptr::addr_of_mut!(STRIP_BMP));
        for i in 0..2 {
            if (*dc)[i] != 0 {
                DeleteDC((*dc)[i]);
                (*dc)[i] = 0;
            }
            if (*bmp)[i] != 0 {
                DeleteObject((*bmp)[i]);
                (*bmp)[i] = 0;
            }
        }
    }
}

/// 惰性创建 idx 主题的图标条 memory DC（72x26 DIB，内容 = bin 整块拷贝）
unsafe fn strip_dc(hdc: isize, dark: bool) -> isize {
    let idx = dark as usize;
    let bin = if dark { TODO_ICONS_DARK } else { TODO_ICONS_LIGHT };
    let (dc, bmp) = (std::ptr::addr_of_mut!(STRIP_DC), std::ptr::addr_of_mut!(STRIP_BMP));
    if (*dc)[idx] == 0 {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: 72,
            biHeight: 26, // 正高度 = 自底向上，与 .bin 排布一致
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let mem = CreateCompatibleDC(hdc);
        let hbmp = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0);
        if mem != 0 && hbmp != 0 && !bits.is_null() && bin.len() == 72 * 26 * 4 {
            std::ptr::copy_nonoverlapping(bin.as_ptr(), bits as *mut u8, bin.len());
            DeleteObject(SelectObject(mem, hbmp));
            (*dc)[idx] = mem;
            (*bmp)[idx] = hbmp;
        } else {
            if hbmp != 0 {
                DeleteObject(hbmp);
            }
            if mem != 0 {
                DeleteDC(mem);
            }
        }
    }
    (*dc)[idx]
}

/// 注册待办列表窗口类（init_app 创建窗口前调用一次）。
/// CS_DBLCLKS 必须显式声明：默认 0 时系统不把二次按下翻译成
/// WM_LBUTTONDBLCLK，双击编辑静默失效（旧 ListView 自带该样式）
pub fn register_list_class() {
    unsafe {
        let class = wide(TODO_LIST_CLASS);
        let wc: WNDCLASSW = WNDCLASSW {
            lpfnWndProc: Some(todo_list_wndproc),
            hInstance: GetModuleHandleW(std::ptr::null()),
            lpszClassName: class.as_ptr(),
            hCursor: LoadCursorW(0, IDC_ARROW),
            style: CS_DBLCLKS,
            ..std::mem::zeroed()
        };
        RegisterClassW(&wc);
    }
}

// ---------- 条目几何（换行测量） ----------

fn client_from_lparam(lp: LPARAM) -> (i32, i32) {
    let x = (lp & 0xFFFF) as u16 as i16 as i32;
    let y = ((lp >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

/// 列表客户区宽（滚动条指示画在右缘，文字区右留白避让它）
fn client_width(app: &App) -> i32 {
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { GetClientRect(app.todo_list, &mut rc) };
    rc.right
}

/// 文字排版区宽
fn text_w(cw: i32) -> i32 {
    (cw - TEXT_X - TEXT_R_PAD).max(40)
}

// ---------- 自定义贪心换行 ----------

// DrawTextW 的 DT_WORDBREAK 把空格分隔的连续英文/数字串当不可拆的整词：行尾
// 剩余空间装不下整词就把它推到下一行，行尾留一大截空白（"明明还有空白却提前
// 换行"）。它也不认 U+200B 零宽空格断点（实测无效），因此自己实现贪心换行：
// 逐字符按步进填行，行尾放不下时——溢出的词是长串（≥ MIN_BREAK_RUN，如哈希/
// URL/乱串）就就地拆开填满行宽；普通短词仍回退词尾断行保持正常西文排版。
// CJK 字符可任意断行。测量与绘制走同一份布局结果，条目高度天然一致。

/// CJK 全角区（含假名/谚文/扩展/全角标点）：可任意断行
fn is_cjk_wide(c: char) -> bool {
    matches!(c as u32,
        0x1100..=0x11FF       // 谚文字母
        | 0x2E80..=0x9FFF     // 部首扩展 / 假名 / CJK 统一表意
        | 0xAC00..=0xD7A3     // 谚文音节
        | 0xF900..=0xFAFF     // CJK 兼容表意
        | 0xFF00..=0xFFEF     // 全角 ASCII / 全角标点
        | 0x20000..=0x2FA1F)  // CJK 扩展 B-F
}

/// 不可断 run 达到此长度（字符数）才允许词内拆行填满行宽；
/// 普通长度的西文单词保持整词换行（正常排版），长哈希/URL/乱串填满行宽
const MIN_BREAK_RUN: usize = 12;

/// 换行布局结果：行文本 + 是否超出行数上限没放完（末行需画省略号）
struct WrapLayout {
    lines: Vec<String>,
    truncated: bool,
}

/// 换行布局：贪心填行（见上方模块注释），上限 max_lines 行
unsafe fn wrap_text(text: &str, width: i32, max_lines: usize, hfont: isize) -> WrapLayout {
    use windows_sys::Win32::Foundation::SIZE;
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    if n == 0 {
        return WrapLayout { lines: Vec::new(), truncated: false };
    }
    // 每个字符的单字符步进（YaHei 无连字，长行累计误差 ≤1-2px，可接受）
    let mut adv = vec![0i32; n];
    unsafe {
        let hdc = GetDC(0);
        let old = SelectObject(hdc, hfont);
        for (k, c) in chars.iter().enumerate() {
            let mut enc = [0u16; 2];
            let enc = c.encode_utf16(&mut enc);
            let mut sz = SIZE { cx: 0, cy: 0 };
            GetTextExtentPoint32W(hdc, enc.as_ptr(), enc.len() as i32, &mut sz);
            adv[k] = sz.cx;
        }
        SelectObject(hdc, old);
        ReleaseDC(0, hdc);
    }
    let mut lines: Vec<String> = Vec::new();
    let mut line_start = 0usize; // 当前行起始
    let mut used = 0i32;         // 当前行已用宽
    let mut last_break: Option<usize> = None; // 行内最后词尾断点（断成 [start, b)）
    let mut run_start: Option<usize> = None; // 当前不可断 run 的起点（非空白非 CJK 连续段）
    let mut i;
    // 行首空白跳过
    while line_start < n && chars[line_start].is_whitespace() {
        line_start += 1;
    }
    i = line_start;
    let mut truncated = false;
    while i < n {
        let breakable = chars[i].is_whitespace() || is_cjk_wide(chars[i]);
        if used + adv[i] <= width {
            used += adv[i];
            // 记录/复位不可断 run 起点（按已放置字符）
            if breakable {
                run_start = None;
            } else if run_start.is_none() {
                run_start = Some(i);
            }
            i += 1;
            // 词尾断点：刚放的字符是空白，或当前/下一字符是 CJK（前后皆可断）
            let k = i - 1;
            if chars[k].is_whitespace()
                || is_cjk_wide(chars[k])
                || (i < n && is_cjk_wide(chars[i]))
            {
                last_break = Some(i);
            }
            continue;
        }
        // 放不下当前字符
        if i == line_start {
            // 单字符就超行宽（极端窄窗口）：硬放进当前行，避免死循环
            used += adv[i];
            i += 1;
            continue;
        }
        // 溢出词是长 run（≥ MIN_BREAK_RUN）→ 就地拆开填满行宽；
        // 否则回退到最后词尾断点（普通整词换行）；行内无断点（行首词）也拆
        let hard = match run_start {
            Some(rs) => {
                let mut j = i;
                while j < n && !chars[j].is_whitespace() && !is_cjk_wide(chars[j]) {
                    j += 1;
                }
                j - rs >= MIN_BREAK_RUN
            }
            None => false,
        };
        let break_at = if hard {
            i
        } else {
            match last_break {
                Some(b) if b > line_start => b, // 词尾断行
                _ => i,                         // 行内无断点：按字符拆
            }
        };
        lines.push(chars[line_start..break_at].iter().collect());
        if lines.len() >= max_lines {
            // 已到行数上限还有剩余 → 末行加省略号
            truncated = break_at < n;
            break;
        }
        line_start = break_at;
        while line_start < n && chars[line_start].is_whitespace() {
            line_start += 1;
        }
        i = line_start;
        used = 0;
        last_break = None;
        run_start = None;
    }
    if !truncated && line_start < n {
        lines.push(chars[line_start..n].iter().collect());
    }
    WrapLayout { lines, truncated }
}

/// 条目换行后的自然高度（上限 MAX_LINES 行）
unsafe fn measure_height(app: &App, text: &str, cw: i32) -> i32 {
    let wrap = wrap_text(text, text_w(cw), MAX_LINES as usize, app.font_todo);
    wrap.lines.len() as i32 * app.todo_line_h
}

/// 条目高度 = 换行行数 × 行高 + 上下内边距
unsafe fn item_height(app: &App, idx: usize, cw: i32) -> i32 {
    let text = app.todos.get(idx).map(|t| t.text.as_str()).unwrap_or("");
    measure_height(app, text, cw) + ITEM_VPAD * 2
}

/// 条目顶部的客户区 y（含滚动偏移）
unsafe fn item_y(app: &App, idx: usize, cw: i32) -> i32 {
    let mut y = -app.todo_scroll;
    for i in 0..idx.min(app.todos.len()) {
        y += item_height(app, i, cw);
    }
    y
}

/// 条目的可见客户区矩形（行内编辑框定位用；滚出视口返回空矩形）
pub fn item_rect(app: &App, idx: usize) -> RECT {
    if idx >= app.todos.len() {
        return RECT { left: 0, top: 0, right: 0, bottom: 0 };
    }
    unsafe {
        let cw = client_width(app);
        let y = item_y(app, idx, cw);
        let h = item_height(app, idx, cw);
        let mut crc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
        GetClientRect(app.todo_list, &mut crc);
        // 整条滚出视口（上方或下方）时不可见：返回空矩形，
        // rename::start 按空矩形拒绝开编辑框（否则编辑框落在视口外接住键盘）
        if y + h <= 0 || y >= crc.bottom {
            return RECT { left: 0, top: 0, right: 0, bottom: 0 };
        }
        RECT { left: 0, top: y, right: cw, bottom: y + h }
    }
}

/// 内容总高与最大滚动偏移
unsafe fn scroll_extent(app: &App, cw: i32) -> (i32, i32) {
    let mut total = 0;
    for i in 0..app.todos.len() {
        total += item_height(app, i, cw);
    }
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    GetClientRect(app.todo_list, &mut rc);
    (total, (total - rc.bottom).max(0))
}

unsafe fn clamp_scroll(app: &mut App) {
    let cw = client_width(app);
    let (_, max) = scroll_extent(app, cw);
    app.todo_scroll = app.todo_scroll.clamp(0, max);
}

/// 命中测试：客户区 y 所在的条目
unsafe fn hit_item(app: &App, y: i32, cw: i32) -> Option<usize> {
    let mut acc = -app.todo_scroll;
    for (i, t) in app.todos.iter().enumerate() {
        let h = measure_height(app, &t.text, cw) + ITEM_VPAD * 2;
        if y >= acc && y < acc + h {
            return Some(i);
        }
        acc += h;
    }
    None
}

/// 待办列表重绘（全量，双缓冲防闪烁；条目数少，整表重绘开销可忽略）
fn invalidate(app: &App) {
    unsafe { InvalidateRect(app.todo_list, std::ptr::null(), 0) };
}

// ---------- 业务 ----------

/// 重建列表内容。自绘窗口的「列表内容」就是 app.todos 本身，
/// 全量失效重绘即可；选择/滚动状态随重建清零（与旧 ListView 行为一致）
pub fn rebuild(app: &mut App) {
    crate::rename::cancel(app);
    app.todo_sel = None;
    app.todo_scroll = 0;
    invalidate(app);
}

pub fn add_from_input(app: &mut App) {
    // 输入框是单行 EDIT，粘贴进来的换行压成空格
    let text = get_text(app.todo_input)
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string();
    if text.is_empty() {
        return;
    }
    app.todos.insert(
        0,
        TodoItem {
            text,
            done: false,
            created: now_secs(),
        },
    );
    save(&app.todos);
    rebuild(app);
    set_text(app.todo_input, "");
    unsafe { SetFocus(app.todo_input) };
}

pub fn delete_selected(app: &mut App) {
    match app.todo_sel {
        Some(idx) if idx < app.todos.len() => {
            app.todos.remove(idx);
            save(&app.todos);
            rebuild(app);
        }
        _ => {}
    }
}

pub fn clear_done(app: &mut App) {
    let before = app.todos.len();
    app.todos.retain(|t| !t.done);
    if app.todos.len() != before {
        save(&app.todos);
    }
    rebuild(app);
}

pub fn begin_edit(app: &mut App) {
    match app.todo_sel {
        Some(idx) if idx < app.todos.len() => {
            let text = app.todos[idx].text.clone();
            crate::rename::start(app, crate::rename::Target::Todo(idx), &text);
        }
        _ => {}
    }
}

/// 切换当前选中条目的完成状态（列表上按空格）
pub fn toggle_selected(app: &mut App) {
    match app.todo_sel {
        Some(idx) if idx < app.todos.len() => toggle_at(app, idx),
        _ => {}
    }
}

/// 切换第 idx 条完成状态（勾选区点击 / 空格）
fn toggle_at(app: &mut App, idx: usize) {
    if idx >= app.todos.len() {
        return;
    }
    app.todos[idx].done = !app.todos[idx].done;
    save(&app.todos);
    invalidate(app);
}

/// 应用待办编辑（自管理重命名编辑框提交时调用）。返回 true = 已应用
pub fn apply_edit(app: &mut App, idx: usize, new_text: &str) -> bool {
    if idx >= app.todos.len() {
        return false;
    }
    let text = new_text.trim().to_string();
    if text.is_empty() || text == app.todos[idx].text {
        return false;
    }
    app.todos[idx].text = text;
    save(&app.todos);
    invalidate(app);
    true
}

/// 绘制单个条目（勾选框 + 换行文字 + 选中胶囊），y = 条目顶部客户区 y
unsafe fn draw_item(mem: isize, app: &App, idx: usize, y: i32, cw: i32, dark: bool) {
    let p = theme::palette(dark);
    let Some(t) = app.todos.get(idx) else { return };
    let h = item_height(app, idx, cw);
    let selected = app.todo_sel == Some(idx);
    if selected {
        theme::fill_round(
            mem,
            RECT { left: 2, top: y, right: cw - 2, bottom: y + h - 1 },
            6.0,
            Some(p.accent_tint),
            None,
        );
    }
    // 勾选框：图标条 cell 1（未完成圆环）/ cell 2（完成对勾），24x26 贴在
    // 条目左侧垂直居中（cell 自带行背景色底，与列表底色一致，视觉无缝）
    let strip = strip_dc(mem, dark);
    if strip != 0 {
        let cell = if t.done { 2 } else { 1 };
        let bf = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        AlphaBlend(
            mem,
            0,
            y + (h - 26) / 2,
            24,
            26,
            strip,
            cell * 24,
            0,
            24,
            26,
            bf,
        );
    }
    // 换行文字：自定义贪心布局（见 wrap_text 注释），上限 MAX_LINES 行，
    // 截断时末行尾收窄并追省略号
    let font_sel = if t.done { app.font_todo_strike } else { app.font_todo };
    SetTextColor(mem, if t.done { p.text_disabled } else { p.text });
    SetBkMode(mem, TRANSPARENT as i32);
    SelectObject(mem, font_sel);
    let mut wrap = wrap_text(&t.text, text_w(cw), MAX_LINES as usize, font_sel);
    if wrap.truncated {
        // 末行逐字符回退到「本行 + 省略号」能放进行宽为止
        if let Some(last) = wrap.lines.last_mut() {
            let w = text_w(cw);
            loop {
                let with_dots = format!("{}\u{2026}", last);
                let w16 = wide(&with_dots);
                let mut sz = windows_sys::Win32::Foundation::SIZE { cx: 0, cy: 0 };
                GetTextExtentPoint32W(mem, w16.as_ptr(), (w16.len() - 1) as i32, &mut sz);
                if sz.cx <= w || last.is_empty() {
                    *last = with_dots;
                    break;
                }
                last.pop();
            }
        }
    }
    for (k, line) in wrap.lines.iter().enumerate() {
        let mut lr = RECT {
            left: TEXT_X,
            top: y + ITEM_VPAD + k as i32 * app.todo_line_h,
            right: cw - TEXT_R_PAD,
            bottom: y + ITEM_VPAD + (k as i32 + 1) * app.todo_line_h,
        };
        DrawTextW(mem, wide(line).as_ptr(), -1, &mut lr, DT_SINGLELINE | DT_NOPREFIX);
    }
}

/// 自绘列表窗口过程
unsafe extern "system" fn todo_list_wndproc(
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    if !window::app_ready() {
        return DefWindowProcW(hwnd, msg, wp, lp);
    }
    match msg {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(hwnd, &mut ps);
            let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            GetClientRect(hwnd, &mut rc);
            let (cw, ch) = (rc.right, rc.bottom);
            if cw <= 0 || ch <= 0 {
                EndPaint(hwnd, &ps);
                return 0;
            }
            with_app(|app| {
                // 双缓冲防闪烁
                let mem = CreateCompatibleDC(hdc);
                let bmp = CreateCompatibleBitmap(hdc, cw, ch);
                let old_bmp = SelectObject(mem, bmp);
                let p = theme::palette(app.dark);
                let bg = CreateSolidBrush(p.window_bg);
                FillRect(mem, &rc, bg);
                DeleteObject(bg);

                if app.todos.is_empty() {
                    crate::util::draw_hint_centered(
                        mem,
                        &rc,
                        "还没有待办。在上方输入，回车添加",
                        p.text_sec,
                        app.font_todo,
                    );
                } else {
                    // 可视范围内的条目逐一绘制（y 含滚动偏移，跳过视口外）
                    let mut y = -app.todo_scroll;
                    for i in 0..app.todos.len() {
                        let h = item_height(app, i, cw);
                        if y + h > 0 && y < ch {
                            draw_item(mem, app, i, y, cw, app.dark);
                        }
                        y += h;
                        if y >= ch {
                            break;
                        }
                    }
                    // 滚动条指示：内容超高时右缘细条，位置按比例
                    let (total, max) = scroll_extent(app, cw);
                    if max > 0 && cw > SCROLLBAR_W * 2 {
                        let track = ch - 4;
                        let thumb_h = ((track * ch) / total).max(24).min(track);
                        let thumb_y = 2 + (track - thumb_h) * app.todo_scroll / max;
                        theme::fill_round(
                            mem,
                            RECT {
                                left: cw - SCROLLBAR_W - 1,
                                top: thumb_y,
                                right: cw - 1,
                                bottom: thumb_y + thumb_h,
                            },
                            2.0,
                            Some(p.edge),
                            None,
                        );
                    }
                }

                BitBlt(hdc, 0, 0, cw, ch, mem, 0, 0, SRCCOPY);
                SelectObject(mem, old_bmp);
                DeleteObject(bmp);
                DeleteDC(mem);
            });
            EndPaint(hwnd, &ps);
            0
        }
        WM_ERASEBKGND => 1, // WM_PAINT 双缓冲已铺全部底色
        WM_SIZE => {
            // 尺寸变了必须全量失效重绘：宽度变 → 每条换行数/高度变（旧区域残留
            // 旧宽度的换行像素），高度变 → 滚动范围变。系统默认只失效「新增暴露
            // 的条带」，WM_PAINT 的 BeginPaint DC 又按失效区裁剪——旧区域没人重绘，
            // 表现为拉宽窗口后文字仍按旧宽度换行、右侧浮着旧像素/花屏。
            // 顺带 clamp 滚动（窗口变高内容 max 变小，避免停留越界偏移）
            with_app(|app| unsafe {
                clamp_scroll(app);
                InvalidateRect(hwnd, std::ptr::null(), 0);
            });
            0
        }
        WM_LBUTTONDOWN => {
            unsafe { SetFocus(hwnd) };
            with_app(|app| unsafe {
                let (x, y) = client_from_lparam(lp);
                let cw = client_width(app);
                let hit = hit_item(app, y, cw);
                if let Some(idx) = hit {
                    if x >= 2 && x <= CHECKBOX_W {
                        toggle_at(app, idx);
                    } else {
                        app.todo_sel = Some(idx);
                    }
                } else {
                    app.todo_sel = None;
                }
                invalidate(app);
            });
            0
        }
        WM_LBUTTONDBLCLK => {
            with_app(|app| unsafe {
                let (_, y) = client_from_lparam(lp);
                let cw = client_width(app);
                let hit = hit_item(app, y, cw);
                if hit.is_some() {
                    app.todo_sel = hit;
                    invalidate(app);
                    begin_edit(app);
                }
            });
            0
        }
        WM_RBUTTONDOWN => {
            // 与旧 ListView 行为一致：右键按下先选中光标下的条目再弹菜单，
            // 否则右键未选中的条目时，菜单的编辑/删除作用在旧选中（或空）上
            with_app(|app| unsafe {
                let (_, y) = client_from_lparam(lp);
                let cw = client_width(app);
                let hit = hit_item(app, y, cw);
                if hit.is_some() {
                    app.todo_sel = hit;
                    invalidate(app);
                }
            });
            0
        }
        WM_RBUTTONUP => {
            // 右键菜单挂在主窗口（popup 需要可见的 owner；与旧 NM_RCLICK 一致）
            let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
            window::todo_ctx_menu(root);
            0
        }
        WM_MOUSEWHEEL => {
            with_app(|app| unsafe {
                let delta = ((wp >> 16) as u16) as i16 as i32;
                if delta == 0 {
                    return;
                }
                // 每格滚 3 行
                app.todo_scroll -= delta * app.todo_line_h * 3 / 120;
                clamp_scroll(app);
                invalidate(app);
            });
            0
        }
        WM_KEYDOWN => {
            let root = unsafe { GetAncestor(hwnd, GA_ROOT) };
            let vk = wp as u32;
            if vk == VK_DELETE as u32 {
                unsafe { PostMessageW(root, window::MSG_DEL_TODO, 0, 0) };
                return 1;
            }
            if vk == VK_SPACE as u32 {
                unsafe { PostMessageW(root, window::MSG_TOGGLE_TODO, 0, 0) };
                return 1;
            }
            if vk == VK_ESCAPE as u32 {
                unsafe { PostMessageW(root, window::MSG_ESC_HIDE, 0, 0) };
                return 1;
            }
            DefWindowProcW(hwnd, msg, wp, lp)
        }
        // 行内重命名编辑框是本窗口的子控件，主题配色消息由本窗口接收
        // （与笔记列表的 child_subclass 补丁同款）
        WM_CTLCOLOREDIT => {
            let hdc = wp as isize;
            let brush = with_app(|app| unsafe {
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, theme::palette(app.dark).text);
                SetBkColor(hdc, theme::palette(app.dark).surface2);
                app.brushes.surface2
            });
            brush as LRESULT
        }
        WM_COMMAND => {
            // 重命名编辑框 EN_CHANGE：多行 EDIT 改文本只失效插入跨度，行首/行中
            // 键入（含 IME 上屏、退格）会与旧文字叠成花屏——统一整框强制重绘
            let code = ((wp as u32) >> 16) & 0xffff;
            let cid = (wp as u32) & 0xffff;
            if code == rename_en_change() && cid == crate::rename::ID_RENAME_EDIT {
                unsafe { InvalidateRect(lp as HWND, std::ptr::null(), 0) };
            }
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> R {
    window::with_app(f)
}
