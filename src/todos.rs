use serde::{Deserialize, Serialize};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{SelectObject, SetTextColor};
use windows_sys::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDRF_DODEFAULT, CDRF_NEWFONT, CDRF_NOTIFYITEMDRAW,
    LVIF_STATE, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS, LVM_EDITLABELW, LVM_GETNEXTITEM,
    LVM_INSERTITEMW, LVM_SETITEMSTATE, LVIS_STATEIMAGEMASK, LVNI_SELECTED, NMLVCUSTOMDRAW,
    NMLVDISPINFOW, NMLISTVIEW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::SendMessageW;

use crate::util::{from_wide, get_text, set_text, wide};
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
    std::fs::read_to_string(file_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save(items: &[TodoItem]) {
    if let Ok(json) = serde_json::to_string_pretty(items) {
        let _ = std::fs::write(file_path(), json);
    }
}

// ---------- ListView 裸操作 ----------

fn lv_insert(lv: HWND, idx: i32, text: &str) {
    let mut w = wide(text);
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.mask = LVIF_TEXT;
    item.iItem = idx;
    item.pszText = w.as_mut_ptr();
    unsafe { SendMessageW(lv, LVM_INSERTITEMW, 0, &mut item as *mut LVITEMW as LPARAM) };
}

fn lv_selected(lv: HWND) -> i32 {
    unsafe { SendMessageW(lv, LVM_GETNEXTITEM, (-1isize) as usize, LVNI_SELECTED as LPARAM) as i32 }
}

/// INDEXTOSTATEIMAGEMASK：勾选框状态图 2 = 勾选
fn lv_set_check(lv: HWND, idx: i32, done: bool) {
    let mut item: LVITEMW = unsafe { std::mem::zeroed() };
    item.stateMask = LVIS_STATEIMAGEMASK;
    item.state = ((if done { 2 } else { 1 }) << 12) as u32;
    unsafe {
        SendMessageW(
            lv,
            LVM_SETITEMSTATE,
            idx as WPARAM,
            &mut item as *mut LVITEMW as LPARAM,
        )
    };
}

// ---------- 业务 ----------

pub fn rebuild(app: &mut App) {
    app.suppress_notify = true;
    let lv = app.todo_list;
    unsafe { SendMessageW(lv, LVM_DELETEALLITEMS, 0, 0) };
    for (i, it) in app.todos.iter().enumerate() {
        lv_insert(lv, i as i32, &it.text);
        lv_set_check(lv, i as i32, it.done);
    }
    app.suppress_notify = false;
}

pub fn add_from_input(app: &mut App) {
    let text = get_text(app.todo_input).trim().to_string();
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
    if idx >= 0 {
        unsafe { SendMessageW(app.todo_list, LVM_EDITLABELW, idx as WPARAM, 0) };
    }
}

/// 勾选状态变化（LVN_ITEMCHANGED）
pub fn on_item_changed(app: &mut App, nm: &NMLISTVIEW) {
    if app.suppress_notify || (nm.uChanged & LVIF_STATE) == 0 {
        return;
    }
    if (nm.uNewState ^ nm.uOldState) & LVIS_STATEIMAGEMASK == 0 {
        return;
    }
    let idx = nm.iItem as usize;
    if idx >= app.todos.len() {
        return;
    }
    let done = (nm.uNewState & 0x2000) != 0; // state image 2 = 勾选
    if app.todos[idx].done != done {
        app.todos[idx].done = done;
        save(&app.todos);
    }
}

/// 双击编辑结束（LVN_ENDLABELEDIT）。返回 true = 应用新文本
pub fn on_end_label_edit(app: &mut App, di: &NMLVDISPINFOW) -> bool {
    let idx = di.item.iItem as usize;
    if idx >= app.todos.len() {
        return false;
    }
    let ptr = di.item.pszText as *const u16;
    if ptr.is_null() {
        return false; // 取消编辑
    }
    let len = unsafe {
        let mut n = 0;
        while *ptr.add(n) != 0 {
            n += 1;
        }
        n
    };
    let text = from_wide(unsafe { std::slice::from_raw_parts(ptr, len) });
    if text.trim().is_empty() {
        return false;
    }
    app.todos[idx].text = text;
    save(&app.todos);
    true
}

/// 完成项置灰 + 删除线（NM_CUSTOMDRAW）
pub fn on_custom_draw(app: &App, nm: &NMLVCUSTOMDRAW) -> LRESULT {
    unsafe {
        match nm.nmcd.dwDrawStage {
            CDDS_PREPAINT => CDRF_NOTIFYITEMDRAW as LRESULT,
            CDDS_ITEMPREPAINT => {
                let idx = nm.nmcd.dwItemSpec as usize;
                if idx < app.todos.len() && app.todos[idx].done {
                    SelectObject(nm.nmcd.hdc, app.font_strike);
                    SetTextColor(nm.nmcd.hdc, 0x00_99_99_99);
                    CDRF_NEWFONT as LRESULT
                } else {
                    CDRF_DODEFAULT as LRESULT
                }
            }
            _ => CDRF_DODEFAULT as LRESULT,
        }
    }
}
