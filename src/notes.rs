use std::path::PathBuf;

use windows_sys::Win32::Foundation::{HWND, LRESULT, LPARAM, POINT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BLENDFUNCTION, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, DIB_RGB_COLORS,
};
use windows_sys::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_POSTPAINT, CDDS_PREPAINT, CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW,
    CDRF_NOTIFYPOSTPAINT, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS,
    LVM_ENSUREVISIBLE, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMSTATE, LVM_GETNEXTITEM,
    LVM_HITTEST, LVHITTESTINFO,
    LVM_INSERTITEMW, LVM_SETITEMSTATE, LVM_SETITEMTEXTW, LVIS_SELECTED, LVNI_SELECTED,
    NMLVCUSTOMDRAW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    MessageBoxW, SendMessageW, SetWindowTextW, MB_OKCANCEL, SB_TOP, WM_VSCROLL,
};

use crate::theme;
use crate::util::{get_text, set_text, wide};
use crate::window::App;

pub struct NoteMeta {
    pub filename: String,
    pub mtime: u64,
    pub pinned: bool,
}

pub fn dir() -> PathBuf {
    let d = crate::util::data_dir().join("notes");
    let _ = std::fs::create_dir_all(&d);
    d
}

/// 标题 = 文件名去扩展名
pub fn title_of(filename: &str) -> String {
    std::path::Path::new(filename)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| filename.to_string())
}

fn ext_of(filename: &str) -> String {
    std::path::Path::new(filename)
        .extension()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_else(|| "md".into())
}

/// 过滤 Windows 文件名非法字符
pub fn sanitize(title: &str) -> String {
    let t: String = title
        .trim()
        .chars()
        .map(|c| match c {
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            _ => c,
        })
        .collect();
    if t.is_empty() {
        "未命名笔记".into()
    } else {
        t
    }
}

fn mtime_of(path: &std::path::Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        // 毫秒精度：秒精度下“保存上一笔记”与“写入新文件”几乎必落在同一秒，
        // 排序并列会退回目录读取顺序（近似随机），新建笔记插不到列表最顶上
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn is_note_file(name: &str) -> bool {
    matches!(ext_of(name).as_str(), "md" | "txt")
}

// ---------- 手动排序（拖拽顺序持久化） ----------

/// 顺序文件：一行一个文件名。列表以它为准排序，不参与 mtime；
/// 没记录到的新文件（外部新增）按 mtime 倒序追加到末尾
fn order_file() -> std::path::PathBuf {
    crate::util::data_dir().join("notes.order")
}

fn load_order() -> Vec<String> {
    std::fs::read_to_string(order_file())
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn write_order(names: &[String]) {
    let mut s = names.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    let _ = std::fs::write(order_file(), s);
}

/// 应用当前列表顺序到顺序文件（拖拽/删除等内存已改好的场景）
pub fn save_order_from_app(app: &App) {
    let names: Vec<String> = app.notes.iter().map(|m| m.filename.clone()).collect();
    write_order(&names);
}

// ---------- 置顶 ----------

/// 置顶文件：一行一个文件名。置顶的固定排在列表最前（组内仍按手动顺序）；
/// 没记录到的一律视为未置顶
fn pinned_file() -> std::path::PathBuf {
    crate::util::data_dir().join("notes.pinned")
}

fn load_pinned() -> Vec<String> {
    std::fs::read_to_string(pinned_file())
        .map(|s| s.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
        .unwrap_or_default()
}

fn write_pinned(names: &[String]) {
    let mut s = names.join("\n");
    if !s.is_empty() {
        s.push('\n');
    }
    let _ = std::fs::write(pinned_file(), s);
}

/// 置顶区边界：app.notes 前缀里连续置顶的数量
/// （refresh 的排序保证置顶笔记总在前缀，拖拽落点 clamp 与插入线取位都用它）
pub fn pinned_count(app: &App) -> usize {
    app.notes.iter().take_while(|m| m.pinned).count()
}

/// 当前列表选中项是否置顶（右键菜单文案用）
pub fn selected_pinned(app: &App) -> bool {
    let idx = lv_selected(app.note_list);
    idx >= 0 && app.notes.get(idx as usize).is_some_and(|m| m.pinned)
}

/// 置顶 / 取消置顶当前选中的笔记。置顶：文件名插入 pinned 与顺序文件首位
/// （跳到列表最顶，且「最新置顶的排最上」）；取消：仅移出 pinned，顺序位置
/// 保留（自然落回未置顶区首位）
pub fn toggle_pin(app: &mut App) {
    let idx = lv_selected(app.note_list);
    if idx < 0 || idx as usize >= app.notes.len() {
        return;
    }
    let idx = idx as usize;
    let filename = app.notes[idx].filename.clone();
    let mut pins = load_pinned();
    if app.notes[idx].pinned {
        pins.retain(|n| n != &filename);
    } else {
        pins.retain(|n| n != &filename);
        pins.insert(0, filename.clone());
        let mut order = load_order();
        order.retain(|n| n != &filename);
        order.insert(0, filename.clone());
        write_order(&order);
    }
    write_pinned(&pins);
    refresh_keep(app, &filename);
}

/// 拖拽落地：把列表项 from 移动到 to，持久化顺序。编辑器与选中保持不变
/// （拖拽只是挪行，不切换正在编辑的笔记；拖的恰好是当前笔记时选中自然跟随）
pub fn reorder(app: &mut App, from: usize, to: usize) {
    if from >= app.notes.len() || to > app.notes.len() || from == to {
        return;
    }
    // 置顶的只能在置顶区内拖，未置顶的只能在未置顶区内拖：
    // to 是 remove 之后的插入索引，落点越界时收拢回本区边界
    let boundary = pinned_count(app);
    let to = if from < boundary {
        to.clamp(0, boundary - 1)
    } else {
        to.clamp(boundary, app.notes.len())
    };
    // to 是 remove 后的插入索引：to == from 才是原位；to == from + 1 是
    // 真实的"向下移一格"（删掉自己后补插一格），不能当 no-op 吞掉
    if to == from {
        return;
    }
    // 移动前先记下当前笔记名：remove/insert 会平移索引，按名字重定位
    let cur_name = app.cur_note.and_then(|i| app.notes.get(i)).map(|m| m.filename.clone());
    let moved = app.notes.remove(from);
    app.notes.insert(to, moved);
    save_order_from_app(app);
    if let Some(n) = &cur_name {
        app.cur_note = app.notes.iter().position(|m| m.filename == *n);
        app.cfg.last_note = n.clone();
    }
    refresh(app);
}

// ---------- ListView 裸操作 ----------

fn lv_insert(lv: HWND, idx: i32, text: &str) {
    let mut w = wide(text);
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    // 笔记无图标（待办列表的勾选圈才需要）：不声明图标位，
    // 行首不预留 24px 图标槽，标题与「＋ 新建」文字对齐同一左缘
    // （挂载的小图列表仍负责把行高撑到 26px）
    item.mask = LVIF_TEXT;
    item.iItem = idx;
    item.pszText = w.as_mut_ptr();
    unsafe { SendMessageW(lv, LVM_INSERTITEMW, 0, &mut item as *mut _ as LPARAM) };
}

/// 原地更新列表项标题（重命名提交用）：位置不重排，只改文字，
/// 与资源管理器一致；否则提交后列表还挂着旧标题直到下一次 refresh
fn lv_set_text(lv: HWND, idx: i32, text: &str) {
    let mut w = wide(text);
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.mask = LVIF_TEXT;
    item.iItem = idx;
    item.pszText = w.as_mut_ptr();
    unsafe { SendMessageW(lv, LVM_SETITEMTEXTW, idx as WPARAM, &mut item as *mut _ as LPARAM) };
}

fn lv_select(lv: HWND, idx: i32) {
    // 先清空全部再选目标行：列表未挂 LVS_SINGLESEL 时 SETITEMSTATE 对新行
    // 是"追加"选中，不清旧会留下两行同时高亮（刷新后补选新笔记的场景）
    let mut clear: LVITEMW = unsafe { std::mem::zeroed() };
    clear.stateMask = LVIS_SELECTED; // state = 0 → 去选中；wParam = -1 作用于全部项
    unsafe { SendMessageW(lv, LVM_SETITEMSTATE, (-1isize) as usize, &mut clear as *mut _ as LPARAM) };
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.stateMask = LVIS_SELECTED;
    item.state = LVIS_SELECTED;
    unsafe {
        SendMessageW(lv, LVM_SETITEMSTATE, idx as WPARAM, &mut item as *mut _ as LPARAM);
    };
    if idx >= 0 {
        unsafe { SendMessageW(lv, LVM_ENSUREVISIBLE, idx as WPARAM, 0) };
    }
}

fn lv_selected(lv: HWND) -> i32 {
    unsafe { SendMessageW(lv, LVM_GETNEXTITEM, (-1isize) as usize, LVNI_SELECTED as LPARAM) as i32 }
}

fn lv_item_rect(lv: HWND, idx: i32) -> RECT {
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 }; // left=0 → LVIR_BOUNDS
    unsafe { SendMessageW(lv, LVM_GETITEMRECT, idx as WPARAM, &mut rc as *mut RECT as LPARAM) };
    rc
}

/// 拖拽插入线的纵向位置（列表客户区 y）：光标所在行上/下边界；
/// 行外光标或原位放置（拖了等于没拖）返回 None 不画线。
/// pin_boundary = 置顶区边界（前缀里置顶笔记数）：落点越界时插入线
/// 收拢画在置顶区底缘，不指向对方区域内部（与 reorder 的 clamp 同规则）
pub fn drag_line_y(pin_boundary: usize, lv: HWND, from: i32, x: i32, y: i32) -> Option<i32> {
    let mut hti: LVHITTESTINFO = unsafe { std::mem::zeroed() };
    hti.pt = POINT { x, y };
    unsafe { SendMessageW(lv, LVM_HITTEST, 0, &mut hti as *mut _ as LPARAM) };
    if hti.iItem < 0 {
        return None;
    }
    let rc = lv_item_rect(lv, hti.iItem);
    let after = y > (rc.top + rc.bottom) / 2;
    let mut to = if after { hti.iItem + 1 } else { hti.iItem };
    if from < to {
        to -= 1;
    }
    let pb = pin_boundary as i32;
    let raw = to;
    let to = if from < pb {
        to.clamp(0, pb - 1)
    } else {
        to.clamp(pb, i32::MAX)
    };
    // to 是 remove 后的插入索引：to == from 才是原位放置（自己行的下半/
    // 下一行上半经校正后都折叠回 to == from）；to == from + 1 是真实的
    // 向下移一格，要有线
    if to == from {
        return None;
    }
    if to == raw {
        Some(if after { rc.bottom } else { rc.top })
    } else {
        Some(lv_item_rect(lv, pb - 1).bottom)
    }
}

/// 自绘整行：选中淡底 + 标题文字 + 空状态提示
pub fn on_custom_draw(app: &crate::window::App, nm: &NMLVCUSTOMDRAW) -> LRESULT {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateSolidBrush, DeleteObject, DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor,
        DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, TRANSPARENT,
    };
    use windows_sys::Win32::UI::Controls::{LVM_GETCOLUMNWIDTH, CDRF_SKIPDEFAULT};
    unsafe {
        let lv = nm.nmcd.hdr.hwndFrom;
        match nm.nmcd.dwDrawStage {
            CDDS_PREPAINT => {
                // 不在这里铺底！局部重绘（如点击选中只重绘受影响行）时此 DC 无裁剪，
                // 整客户区填充会把未重绘的行全部擦没（行被擦掉不显示的根源）。
                // 底色匹配交给：WM_ERASEBKGND（子类接管，BeginPaint DC 带裁剪）
                // + 每行 ITEM_PREPAINT 的行底填充
                if SendMessageW(lv, LVM_GETITEMCOUNT, 0, 0) == 0 {
                    CDRF_NOTIFYPOSTPAINT as LRESULT
                } else {
                    (CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as LRESULT
                }
            }
            CDDS_POSTPAINT => {
                let p = theme::palette(app.theme);
                let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
                windows_sys::Win32::UI::WindowsAndMessaging::GetClientRect(lv, &mut rc);
                // Explorer 主题会在最右列边缘画一条贯穿整个列表的 1px 装饰竖线，
                // 延伸到没有内容的空白区，看着像一块凸出的空面板——用背景色盖掉
                let colw = SendMessageW(lv, LVM_GETCOLUMNWIDTH, 0, 0) as i32;
                if colw > 0 && colw <= rc.right {
                    let b = CreateSolidBrush(p.window_bg);
                    FillRect(
                        nm.nmcd.hdc,
                        &RECT { left: colw - 1, top: 0, right: colw, bottom: rc.bottom },
                        b,
                    );
                    DeleteObject(b);
                }
                if SendMessageW(lv, LVM_GETITEMCOUNT, 0, 0) == 0 {
                    crate::util::draw_hint_centered(nm.nmcd.hdc, &rc, "点「＋ 新建」写第一篇笔记", p.text_sec, app.font);
                }
                // 拖拽排序插入线：2px accent 横线画在目标行边界上
                if let Some(d) = &app.note_drag {
                    if d.active {
                        if let Some(y) = drag_line_y(pinned_count(app), lv, d.from, d.cx, d.cy) {
                            let colw2 = SendMessageW(lv, LVM_GETCOLUMNWIDTH, 0, 0) as i32;
                            let b = CreateSolidBrush(p.accent);
                            FillRect(
                                nm.nmcd.hdc,
                                &RECT { left: 0, top: y - 1, right: colw2.max(rc.right), bottom: y + 1 },
                                b,
                            );
                            DeleteObject(b);
                        }
                    }
                }
                CDRF_DODEFAULT as LRESULT
            }
            CDDS_ITEMPREPAINT => {
                let idx = nm.nmcd.dwItemSpec as usize;
                let i = nm.nmcd.dwItemSpec as i32;
                let p = theme::palette(app.theme);
                let rc = lv_item_rect(lv, i);
                // 行底铺窗口底色：主题控件在 ITEM_PREPAINT 前画的主题行底与窗口底
                // 存在色差（暗色下成"凸面板"），行内自填即可，且局部重绘自洽
                let bg = CreateSolidBrush(p.window_bg);
                FillRect(nm.nmcd.hdc, &rc, bg);
                DeleteObject(bg);
                let selected = SendMessageW(lv, LVM_GETITEMSTATE, i as WPARAM, LVIS_SELECTED as LPARAM) as u32
                    & LVIS_SELECTED
                    != 0;
                if selected {
                    theme::fill_round(
                        nm.nmcd.hdc,
                        RECT { left: rc.left + 2, top: rc.top, right: rc.right - 2, bottom: rc.bottom - 1 },
                        6.0,
                        Some(p.accent_tint),
                        None,
                    );
                }
                // 标题文字（行首无图标槽，左缩进 10 与「＋ 新建」文字对齐；
                // 置顶行右界内缩给行尾图钉让位，长标题省略号照常）
                let pinned = app.notes.get(idx).is_some_and(|m| m.pinned);
                let title = app
                    .notes
                    .get(idx)
                    .map(|m| title_of(&m.filename))
                    .unwrap_or_default();
                let mut tr = RECT {
                    left: rc.left + 10,
                    top: rc.top,
                    right: rc.right - if pinned { 26 } else { 8 },
                    bottom: rc.bottom,
                };
                SelectObject(nm.nmcd.hdc, app.font);
                SetTextColor(
                    nm.nmcd.hdc,
                    if selected { p.accent } else { p.text },
                );
                SetBkMode(nm.nmcd.hdc, TRANSPARENT as i32);
                DrawTextW(
                    nm.nmcd.hdc,
                    crate::util::wide(&title).as_ptr(),
                    -1,
                    &mut tr,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
                if pinned {
                    draw_pin(nm.nmcd.hdc, rc.right - 17, (rc.top + rc.bottom) / 2, app.theme);
                }
                CDRF_SKIPDEFAULT as LRESULT
            }
            _ => CDRF_DODEFAULT as LRESULT,
        }
    }
}

// ---------- 行尾置顶图钉 ----------

/// 置顶图钉 = 系统图标字体 Segoe MDL2 Assets 的 Pin 字形（U+E718，旋转为针尖
/// 朝下）烘焙成的像素资源，由 assets/brand/make_pin_icon.py 生成（三主题
/// 强调色各一份，theme.rs 改色后需重跑脚本）。运行时 CreateDIBSection +
/// AlphaBlend 直接合成——不用运行时画矢量，视觉与设计稿逐像素一致
static PIN_LIGHT_BIN: &[u8] = include_bytes!("../assets/brand/pin-light.bin");
static PIN_DARK_BIN: &[u8] = include_bytes!("../assets/brand/pin-dark.bin");
static PIN_YELLOW_BIN: &[u8] = include_bytes!("../assets/brand/pin-yellow.bin");

const PIN_W: i32 = 12;
const PIN_H: i32 = 16;

// 三个主题的图钉位图（memory DC + DIB），首次使用时按需创建。
// 索引 0 = 浅色 1 = 暗色 2 = 浅黄；进程退出统一在 cleanup_pin_assets 释放
static mut PIN_DC: [isize; 3] = [0; 3];
static mut PIN_BMP: [isize; 3] = [0; 3];

/// WM_DESTROY 时释放图钉位图资源
pub fn cleanup_pin_assets() {
    unsafe {
        let (dc, bmp) = (std::ptr::addr_of_mut!(PIN_DC), std::ptr::addr_of_mut!(PIN_BMP));
        for i in 0..3 {
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

/// 惰性创建 idx 主题的图钉位图（memory DC，选入预乘 BGRA 的 DIB）
unsafe fn pin_dc(hdc: isize, idx: usize) -> isize {
    let bin = match idx {
        1 => PIN_DARK_BIN,
        2 => PIN_YELLOW_BIN,
        _ => PIN_LIGHT_BIN,
    };
    let (dc, bmp) = (std::ptr::addr_of_mut!(PIN_DC), std::ptr::addr_of_mut!(PIN_BMP));
    if (*dc)[idx] == 0 {
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: PIN_W,
            biHeight: PIN_H, // 正高度 = 自底向上，与 .bin 排布一致
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            ..std::mem::zeroed()
        };
        let mut bits: *mut core::ffi::c_void = std::ptr::null_mut();
        let mem = CreateCompatibleDC(hdc);
        let hbmp = CreateDIBSection(mem, &bmi, DIB_RGB_COLORS, &mut bits, 0, 0);
        if mem != 0 && hbmp != 0 && !bits.is_null() && bin.len() == (PIN_W * PIN_H * 4) as usize {
            std::ptr::copy_nonoverlapping(bin.as_ptr(), bits as *mut u8, bin.len());
            DeleteObject(windows_sys::Win32::Graphics::Gdi::SelectObject(mem, hbmp));
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

/// 行尾画置顶图钉（cx, cy = 图标中心；t = 当前主题，取对应强调色的烘焙份）
unsafe fn draw_pin(hdc: isize, cx: i32, cy: i32, t: theme::Theme) {
    let idx = match t {
        theme::Theme::Dark => 1,
        theme::Theme::Yellow => 2,
        _ => 0,
    };
    let src = pin_dc(hdc, idx);
    if src == 0 {
        return;
    }
    let bf = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: 255,
        AlphaFormat: AC_SRC_ALPHA as u8,
    };
    AlphaBlend(
        hdc,
        cx - PIN_W / 2,
        cy - PIN_H / 2,
        PIN_W,
        PIN_H,
        src,
        0,
        0,
        PIN_W,
        PIN_H,
        bf,
    );
}

// ---------- 业务 ----------

/// 首次运行：目录为空时创建欢迎笔记
pub fn ensure_first_run() {
    let d = dir();
    let has_any = std::fs::read_dir(&d)
        .map(|rd| rd.filter_map(|e| e.ok()).any(|e| is_note_file(&e.file_name().to_string_lossy())))
        .unwrap_or(false);
    if !has_any {
        let _ = std::fs::write(
            d.join("欢迎使用 MiniNotes.md"),
            "# 欢迎使用 MiniNotes\n\n\
             - 全局热键（默认 Alt + N，可在设置中修改）或左键点托盘图标：唤起 / 隐藏\n\
             - 失焦自动隐藏（可在设置中关闭），Esc 也可隐藏\n\
             - 「待办」页：输入后回车添加，勾选完成，双击编辑，Del 删除\n\
             - 「笔记」页：自动保存为 exe 旁 data/notes/ 下的 .md 文件\n\
             - 数据便携，可直接用其他编辑器打开\n\n\
             删掉这段说明，开始写你的第一条笔记吧。\n",
        );
    }
}

/// 重扫目录并重建列表（按修改时间倒序），尽量保持/恢复选中
pub fn refresh(app: &mut App) {
    crate::rename::cancel(app);
    app.suppress_notify = true;
    let keep = app
        .cur_note
        .and_then(|i| app.notes.get(i))
        .map(|m| m.filename.clone())
        .or_else(|| Some(app.cfg.last_note.clone()))
        .unwrap_or_default();
    let lv = app.note_list;
    unsafe { SendMessageW(lv, LVM_DELETEALLITEMS, 0, 0) };

    let pinned = load_pinned();
    let mut metas: Vec<NoteMeta> = std::fs::read_dir(dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| is_note_file(&e.file_name().to_string_lossy()))
                .map(|e| {
                    let filename = e.file_name().to_string_lossy().into_owned();
                    NoteMeta {
                        pinned: pinned.contains(&filename),
                        filename,
                        mtime: mtime_of(&e.path()),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    // 置顶组固定在最前（Reverse：pinned=true 排前），组内仍按手动排序：
    // 按顺序文件的记录排；未记录的（外部新增/首次运行）按 mtime 倒序追加到
    // 组末尾。编辑笔记不再让行浮上来
    let order = load_order();
    metas.sort_by(|a, b| {
        let ka = (
            std::cmp::Reverse(a.pinned),
            order.iter().position(|n| n == &a.filename).unwrap_or(usize::MAX),
            std::cmp::Reverse(a.mtime),
        );
        let kb = (
            std::cmp::Reverse(b.pinned),
            order.iter().position(|n| n == &b.filename).unwrap_or(usize::MAX),
            std::cmp::Reverse(b.mtime),
        );
        ka.cmp(&kb)
    });
    // 回写：清理已删文件的残留条目，把外部新增的文件记录进去
    let names: Vec<String> = metas.iter().map(|m| m.filename.clone()).collect();
    write_order(&names);
    // 置顶文件同样回写：清掉已删文件的残留条目（置顶名单不新增条目）
    let pins: Vec<String> = load_pinned().into_iter().filter(|n| names.contains(n)).collect();
    write_pinned(&pins);

    app.cur_note = None;
    for (i, m) in metas.iter().enumerate() {
        lv_insert(lv, i as i32, &title_of(&m.filename));
        if app.cur_note.is_none() && m.filename == keep {
            app.cur_note = Some(i);
        }
    }
    app.notes = metas;

    let sel = app.cur_note.unwrap_or(0);
    if !app.notes.is_empty() {
        lv_select(lv, sel as i32);
        if app.cur_note.is_none() {
            app.cur_note = Some(sel);
        }
    }
    app.suppress_notify = false;
}

/// 保存当前笔记（若有未保存修改）
pub fn save_current(app: &mut App) {
    let Some(idx) = app.cur_note else { return };
    if !app.note_dirty || idx >= app.notes.len() {
        return;
    }
    let path = dir().join(&app.notes[idx].filename);
    // EDIT 控件内部用 CRLF；写盘统一回 LF（跨工具友好）
    let text = get_text(app.editor).replace("\r\n", "\n");
    let _ = std::fs::write(path, text);
    app.notes[idx].mtime = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    app.note_dirty = false;
}

/// 加载列表项 idx 对应笔记到编辑器
pub fn load_by_index(app: &mut App, idx: usize) {
    if idx >= app.notes.len() {
        return;
    }
    save_current(app);
    app.cur_note = Some(idx);
    // 高亮必须跟内容一起切：refresh 的 keep 解析优先 cur_note（仍是切换前
    // 的旧笔记），新建/置顶这类 refresh 后紧跟 load 的路径，列表高亮会留在
    // 旧笔记上——内容与高亮分家，看着像在另一篇笔记里打字。这里按 idx 补选
    app.suppress_notify = true;
    lv_select(app.note_list, idx as i32);
    app.suppress_notify = false;
    let path = dir().join(&app.notes[idx].filename);
    let text = std::fs::read_to_string(path).unwrap_or_default();
    // 文件是 LF；EDIT 控件只认 CRLF，否则换行丢失
    let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    app.suppress_notify = true;
    set_text(app.editor, &text);
    app.suppress_notify = false;
    // 载入一篇笔记 = 全新查看：滚回顶部。SB_TOP 只滚动视图不动光标/选区，
    // 紧接着的打字仍落在原光标位置（EM_SCROLLCARET 会把视图带回光标处）
    unsafe { SendMessageW(app.editor, WM_VSCROLL, SB_TOP as usize, 0) };
    app.note_dirty = false;
    app.cfg.last_note = app.notes[idx].filename.clone();
    update_title(app);
}

fn update_title(app: &App) {
    let t = app
        .cur_note
        .and_then(|i| app.notes.get(i))
        .map(|m| format!("MiniNotes — {}", title_of(&m.filename)))
        .unwrap_or_else(|| "MiniNotes".into());
    unsafe { SetWindowTextW(app.hwnd, wide(&t).as_ptr()) };
}

pub fn notify_dirty(app: &mut App) {
    if app.suppress_notify {
        return;
    }
    if app.cur_note.is_some() {
        app.note_dirty = true;
    }
}

/// 新建笔记并打开
pub fn create_note(app: &mut App) {
    save_current(app);
    // 找不重名的文件名
    let mut title = "未命名笔记".to_string();
    let mut n = 1;
    while dir().join(format!("{title}.md")).exists() {
        n += 1;
        title = format!("未命名笔记 {n}");
    }
    let filename = format!("{title}.md");
    let _ = std::fs::write(dir().join(&filename), "");
    // 新笔记固定置顶：插到顺序文件最前面
    let mut order = load_order();
    order.retain(|n| n != &filename);
    order.insert(0, filename.clone());
    write_order(&order);
    refresh_keep(app, &filename);
    // 直接在列表项上进入命名：编辑框全选默认名，键入即整体替换
    // （Enter 提交新名；Esc 取消则保留默认名，与资源管理器一致）
    if let Some(idx) = app.notes.iter().position(|m| m.filename == filename) {
        crate::rename::start(app, crate::rename::Target::Note(idx), &title);
        return;
    }
    unsafe { SetFocus(app.editor) };
}

/// 重建列表并选中指定文件
fn refresh_keep(app: &mut App, filename: &str) {
    app.cfg.last_note = filename.to_string();
    refresh(app);
    // refresh 通过 last_note 恢复选中；若没选上则手动加载
    if let Some(idx) = app.notes.iter().position(|m| m.filename == filename) {
        if app.cur_note != Some(idx) {
            load_by_index(app, idx);
        } else {
            update_title(app);
        }
    }
    update_title(app);
}

/// 删除当前选中的笔记（列表选中项）
pub fn delete_selected(app: &mut App) {
    let idx = lv_selected(app.note_list);
    if idx < 0 || idx as usize >= app.notes.len() {
        return;
    }
    let filename = app.notes[idx as usize].filename.clone();
    let msg = wide(&format!("确定删除笔记「{}」吗？文件也会被删除。", title_of(&filename)));
    let ret = unsafe {
        MessageBoxW(
            app.hwnd,
            msg.as_ptr(),
            wide("删除笔记").as_ptr(),
            MB_OKCANCEL,
        )
    };
    if ret != 1 {
        return; // IDOK = 1
    }
    let _ = std::fs::remove_file(dir().join(&filename));
    let was_cur = app.cur_note == Some(idx as usize);
    app.notes.remove(idx as usize);
    if was_cur {
        app.note_dirty = false;
    }
    // 选中替代项：先记下文件名（手动排序下位置是稳定的，但 refresh 的
    // keep 解析按文件名走，用索引指行依旧不可靠）
    let next_name = if app.notes.is_empty() {
        None
    } else {
        let i = (idx as usize).min(app.notes.len() - 1);
        Some(app.notes[i].filename.clone())
    };
    // 关键：把 cur_note 预定位到邻位笔记，refresh 的 keep 解析才能选中它。
    // 否则 keep 退回 cfg.last_note（= 被删笔记名），列表里找不到 → sel=0，
    // 选中先跳到最顶（视口跟着滚顶），再靠 load_by_index 的补选拉回来
    if let Some(n) = &next_name {
        app.cur_note = app.notes.iter().position(|m| m.filename == *n);
    }
    refresh(app);
    match next_name.and_then(|n| app.notes.iter().position(|m| m.filename == n)) {
        Some(i) => load_by_index(app, i),
        None => {
            app.cur_note = None;
            app.suppress_notify = true;
            set_text(app.editor, "");
            app.suppress_notify = false;
            app.note_dirty = false;
            update_title(app);
        }
    }
}

/// 开始重命名（列表选中项）
pub fn begin_rename(app: &mut App) {
    let idx = lv_selected(app.note_list);
    if idx < 0 {
        return;
    }
    let idx = idx as usize;
    if idx >= app.notes.len() {
        return;
    }
    let title = title_of(&app.notes[idx].filename);
    crate::rename::start(app, crate::rename::Target::Note(idx), &title);
}

/// 应用重命名（自管理重命名编辑框提交时调用）。返回 true = 已改名
pub fn apply_rename(app: &mut App, idx: usize, new_text: &str) -> bool {
    if idx >= app.notes.len() {
        return false;
    }
    let new_title = sanitize(new_text);
    if new_title == title_of(&app.notes[idx].filename) {
        return false;
    }
    let old = app.notes[idx].filename.clone();
    let new_name = format!("{}.{}", new_title, ext_of(&old));
    // 仅大小写不同时目标就是自己（Windows 允许大小写改名），不能当重名处理
    if !new_name.eq_ignore_ascii_case(&old) && dir().join(&new_name).exists() {
        let msg = wide(&format!("已存在名为「{new_title}」的笔记"));
        unsafe {
            MessageBoxW(app.hwnd, msg.as_ptr(), wide("重命名失败").as_ptr(), 0);
        };
        return false;
    }
    if std::fs::rename(dir().join(&old), dir().join(&new_name)).is_err() {
        return false;
    }
    app.notes[idx].filename = new_name.clone();
    // 顺序文件同步改名：否则下次 refresh 时新名字不在记录里，会被追加到末尾
    let mut order = load_order();
    if let Some(p) = order.iter().position(|n| n == &old) {
        order[p] = new_name.clone();
        write_order(&order);
    }
    // 置顶名单同步改名：旧名不更新，下次 refresh 残留清理会把置顶弄丢
    let mut pins = load_pinned();
    if let Some(p) = pins.iter().position(|n| n == &old) {
        pins[p] = new_name.clone();
        write_pinned(&pins);
    }
    lv_set_text(app.note_list, idx as i32, &new_title);
    if app.cur_note == Some(idx) {
        app.cfg.last_note = new_name;
        update_title(app);
    }
    true
}
