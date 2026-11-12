use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, DrawTextW, FillRect, SelectObject, SetBkMode, SetTextColor,
    DT_END_ELLIPSIS, DT_SINGLELINE, DT_VCENTER, TRANSPARENT,
};
use windows_sys::Win32::UI::Controls::{
    ImageList_Draw, LVM_GETCOLUMNWIDTH, LVM_GETITEMCOUNT, LVM_GETITEMRECT,
    LVM_GETITEMSTATE, LVM_REDRAWITEMS, LVM_SETITEMW, LVIF_IMAGE, LVIF_TEXT, LVITEMW,
    LVM_DELETEALLITEMS, LVM_GETNEXTITEM, LVM_INSERTITEMW, LVIS_SELECTED,
    LVNI_SELECTED, NMITEMACTIVATE, NMLVCUSTOMDRAW, CDDS_ITEMPREPAINT,
    CDDS_POSTPAINT, CDDS_PREPAINT, CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYPOSTPAINT,
    CDRF_SKIPDEFAULT,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

use crate::theme;
use crate::util::{get_text, set_text, wide};
use crate::window::App;

#[derive(Serialize, Deserialize, Clone)]
pub struct TodoItem {
    pub text: String,
    pub done: bool,
    pub created: u64,
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

// ---------- ListView 裸操作 ----------

fn lv_insert(lv: HWND, idx: i32, text: &str, done: bool) {
    let mut w = wide(text);
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    // iImage：1=未完成圆圈，2=完成勾选（图标即勾选框，见 window.rs make_checkbox_imagelist）
    item.mask = LVIF_TEXT | LVIF_IMAGE;
    item.iItem = idx;
    item.iImage = if done { 2 } else { 1 };
    item.pszText = w.as_mut_ptr();
    unsafe { SendMessageW(lv, LVM_INSERTITEMW, 0, &mut item as *mut LVITEMW as LPARAM) };
}

fn lv_set_image(lv: HWND, idx: i32, done: bool) {
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.mask = LVIF_IMAGE;
    item.iItem = idx;
    item.iImage = if done { 2 } else { 1 };
    unsafe { SendMessageW(lv, LVM_SETITEMW, idx as WPARAM, &mut item as *mut LVITEMW as LPARAM) };
}

fn lv_selected(lv: HWND) -> i32 {
    unsafe { SendMessageW(lv, LVM_GETNEXTITEM, (-1isize) as usize, LVNI_SELECTED as LPARAM) as i32 }
}

fn lv_count(lv: HWND) -> i32 {
    unsafe { SendMessageW(lv, LVM_GETITEMCOUNT, 0, 0) as i32 }
}

/// 取某行的包围矩形
fn lv_item_rect(lv: HWND, idx: i32) -> RECT {
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 }; // left=0 → LVIR_BOUNDS
    unsafe {
        SendMessageW(lv, LVM_GETITEMRECT, idx as WPARAM, &mut rc as *mut RECT as LPARAM);
    }
    rc
}

fn lv_redraw(lv: HWND, first: i32, last: i32) {
    unsafe { SendMessageW(lv, LVM_REDRAWITEMS, first as WPARAM, last as LPARAM) };
}

// ---------- 业务 ----------

pub fn rebuild(app: &mut App) {
    crate::rename::cancel(app);
    app.suppress_notify = true;
    let lv = app.todo_list;
    unsafe { SendMessageW(lv, LVM_DELETEALLITEMS, 0, 0) };
    for (i, it) in app.todos.iter().enumerate() {
        lv_insert(lv, i as i32, &it.text, it.done);
    }
    app.suppress_notify = false;
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
    let idx = lv_selected(app.todo_list);
    if idx < 0 || idx as usize >= app.todos.len() {
        return;
    }
    app.todos.remove(idx as usize);
    save(&app.todos);
    rebuild(app);
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
    let idx = lv_selected(app.todo_list);
    if idx < 0 {
        return;
    }
    let idx = idx as usize;
    if idx >= app.todos.len() {
        return;
    }
    let text = app.todos[idx].text.clone();
    crate::rename::start(app, crate::rename::Target::Todo(idx), &text);
}

/// 切换第 idx 条完成状态
fn toggle_at(app: &mut App, idx: usize) {
    if idx >= app.todos.len() {
        return;
    }
    app.todos[idx].done = !app.todos[idx].done;
    save(&app.todos);
    lv_set_image(app.todo_list, idx as i32, app.todos[idx].done);
    lv_redraw(app.todo_list, idx as i32, idx as i32);
}

pub fn toggle_selected(app: &mut App) {
    let idx = lv_selected(app.todo_list);
    if idx >= 0 {
        toggle_at(app, idx as usize);
    }
}

/// 单击：点在行首勾选框区域（左 28px）切换完成状态，返回 true 表示已处理
pub fn on_click(app: &mut App, nm: &NMITEMACTIVATE) -> bool {
    if nm.iItem < 0 {
        return false;
    }
    let idx = nm.iItem;
    if idx as usize >= app.todos.len() {
        return false;
    }
    let pt = nm.ptAction;
    let rc = lv_item_rect(app.todo_list, idx);
    if pt.x >= rc.left + 2 && pt.x <= rc.left + 30 && pt.y >= rc.top && pt.y <= rc.bottom {
        toggle_at(app, idx as usize);
        true
    } else {
        false
    }
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
    true
}

/// 自绘整行：选中淡底 + 勾选框图标 + 文字（完成态置灰删除线）+ 空状态提示
pub fn on_custom_draw(app: &App, nm: &NMLVCUSTOMDRAW) -> LRESULT {
    unsafe {
        let lv = nm.nmcd.hdr.hwndFrom;
        match nm.nmcd.dwDrawStage {
            CDDS_PREPAINT => {
                if lv_count(lv) == 0 {
                    CDRF_NOTIFYPOSTPAINT as LRESULT
                } else {
                    (CDRF_NOTIFYITEMDRAW | CDRF_NOTIFYPOSTPAINT) as LRESULT
                }
            }
            CDDS_POSTPAINT => {
                let p = theme::palette(app.dark);
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
                // 空状态提示
                if lv_count(lv) == 0 {
                    let hint = "还没有待办。在上方输入，回车添加";
                    crate::util::draw_hint_centered(nm.nmcd.hdc, &rc, hint, p.text_sec, app.font);
                }
                CDRF_DODEFAULT as LRESULT
            }
            CDDS_ITEMPREPAINT => {
                let idx = nm.nmcd.dwItemSpec as usize;
                let p = theme::palette(app.dark);
                let i = nm.nmcd.dwItemSpec as i32;
                let rc = lv_item_rect(lv, i);
                let done = app.todos.get(idx).map(|t| t.done).unwrap_or(false);

                let selected = {
                    SendMessageW(lv, LVM_GETITEMSTATE, i as WPARAM, LVIS_SELECTED as LPARAM) as u32
                        & LVIS_SELECTED
                        != 0
                };
                if selected {
                    theme::fill_round(
                        nm.nmcd.hdc,
                        RECT { left: rc.left + 2, top: rc.top, right: rc.right - 2, bottom: rc.bottom - 1 },
                        6.0,
                        Some(p.accent_tint),
                        None,
                    );
                }
                // 勾选框图标（不透明底=行背景，见 make_checkbox_imagelist）
                if app.himl != 0 {
                    ImageList_Draw(
                        app.himl,
                        if done { 2 } else { 1 },
                        nm.nmcd.hdc,
                        rc.left,
                        rc.top + ((rc.bottom - rc.top) - 26) / 2,
                        0,
                    );
                }
                // 文字
                let text = app.todos.get(idx).map(|t| t.text.clone()).unwrap_or_default();
                let mut tr = RECT { left: rc.left + 30, top: rc.top, right: rc.right - 8, bottom: rc.bottom };
                if done {
                    SelectObject(nm.nmcd.hdc, app.font_strike);
                    SetTextColor(nm.nmcd.hdc, p.text_disabled);
                } else {
                    SelectObject(nm.nmcd.hdc, app.font);
                    SetTextColor(nm.nmcd.hdc, p.text);
                }
                SetBkMode(nm.nmcd.hdc, TRANSPARENT as i32);
                DrawTextW(
                    nm.nmcd.hdc,
                    crate::util::wide(&text).as_ptr(),
                    -1,
                    &mut tr,
                    DT_SINGLELINE | DT_VCENTER | DT_END_ELLIPSIS,
                );
                CDRF_SKIPDEFAULT as LRESULT
            }
            _ => CDRF_DODEFAULT as LRESULT,
        }
    }
}
