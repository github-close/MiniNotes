//! 自管理内联重命名：不用系统的 LVM_EDITLABELS / LVS_EDITLABELS。
//! 本机实测（Win10 19045，comctl32 6.10.19041.6456）：系统的内联编辑
//! 在 LVM_EDITLABELW 时于 comctl32 内部访问违例导致进程闪退
//! （最小复现程序确认，裸列表即崩，与应用代码无关）。
//! 因此自己创建/定位/提交编辑框：行为一致，且位置与主题完全可控。

use windows_sys::Win32::Foundation::{HWND, LPARAM, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::InvalidateRect;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{LVM_GETCOLUMNWIDTH, LVM_GETITEMRECT};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, SendMessageW, ES_AUTOHSCROLL, WS_CHILD, WS_VISIBLE,
};

/// EC_LEFTMARGIN | EC_RIGHTMARGIN 的设边距消息（windows-sys 未导出）
const EM_SETMARGINS: u32 = 0x00D3;

use crate::util::{apply_font, get_text, wide};
use crate::window::App;

/// 重命名编辑框的控件 ID（其 EN_* 通知在 WM_COMMAND 中被忽略）
pub const ID_RENAME_EDIT: u32 = 401;

#[derive(Clone, Copy, PartialEq)]
pub enum Target {
    Note(usize),
    Todo(usize),
}

pub struct State {
    pub edit: HWND,
    pub target: Target,
}

impl Target {
    fn lv(self, app: &App) -> HWND {
        match self {
            Target::Note(_) => app.note_list,
            Target::Todo(_) => app.todo_list,
        }
    }

    /// 编辑框 x（listview 客户区坐标）。编辑框文字自带 ~2px 边距，
    /// 减 2 后其文字与自绘标题文字同列：
    /// 笔记标题在 x=12（行首无图标），待办标题在图标槽后 x=54。
    fn edit_x(self) -> i32 {
        match self {
            Target::Note(_) => 10,
            Target::Todo(_) => 52,
        }
    }
}

fn item_rect(lv: HWND, idx: usize) -> RECT {
    let mut rc = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    unsafe { SendMessageW(lv, LVM_GETITEMRECT, idx as WPARAM, &mut rc as *mut RECT as LPARAM) };
    rc
}

/// 在列表第 idx 行上打开重命名编辑框（已存在则先取消旧的）
pub fn start(app: &mut App, target: Target, text: &str) {
    cancel(app);
    let lv = target.lv(app);
    let idx = match target {
        Target::Note(i) => i,
        Target::Todo(i) => i,
    };
    let rc = item_rect(lv, idx);
    if rc.bottom <= rc.top {
        return;
    }
    let colw = unsafe { SendMessageW(lv, LVM_GETCOLUMNWIDTH, 0, 0) as i32 };
    let x = target.edit_x();
    let edit = unsafe {
        CreateWindowExW(
            0,
            wide("EDIT").as_ptr(),
            wide(text).as_ptr(),
            WS_CHILD | WS_VISIBLE | ES_AUTOHSCROLL as u32,
            x,
            rc.top + 2,
            colw - x - 8,
            rc.bottom - rc.top - 3,
            lv,
            ID_RENAME_EDIT as isize,
            GetModuleHandleW(std::ptr::null()),
            std::ptr::null(),
        )
    };
    if edit == 0 {
        return;
    }
    apply_font(edit, app.font);
    // 挂系统主题类：暗色下光标用浅色（默认 EDIT 光标近黑，深底上不可见）；
    // 背景文字配色由列表子类的 WM_CTLCOLOREDIT 接管（见 window.rs child_subclass）
    let theme_name = if app.dark { "DarkMode_Explorer" } else { "Explorer" };
    unsafe {
        windows_sys::Win32::UI::Controls::SetWindowTheme(
            edit,
            wide(theme_name).as_ptr(),
            std::ptr::null(),
        );
    }
    // EC_LEFTMARGIN | EC_RIGHTMARGIN，左右 2/4px
    unsafe { SendMessageW(edit, EM_SETMARGINS, 3, (2 | (4 << 16)) as LPARAM) };
    crate::window::set_rename_subclass(edit);
    unsafe { SetFocus(edit) };
    app.rename = Some(State { edit, target });
}

/// 提交（仅当 hwnd 是当前重命名编辑框时）。返回是否应用了修改。
pub fn commit(app: &mut App, edit: HWND) -> bool {
    let Some(state) = app.rename.take() else { return false };
    if state.edit != edit {
        // 不是当前编辑框（旧句柄的 KILLFOCUS 残余）
        app.rename = Some(state);
        return false;
    }
    let text = get_text(edit);
    unsafe { DestroyWindow(edit) };
    let lv = state.target.lv(app);
    let applied = match state.target {
        Target::Note(idx) => crate::notes::apply_rename(app, idx, &text),
        Target::Todo(idx) => crate::todos::apply_edit(app, idx, &text),
    };
    unsafe { InvalidateRect(lv, std::ptr::null(), 0) };
    applied
}

/// 取消：销毁编辑框，不应用修改
pub fn cancel(app: &mut App) {
    if let Some(state) = app.rename.take() {
        unsafe { DestroyWindow(state.edit) };
    }
}

/// 焦点切走时提交（点击列表其他位置、切换 Tab 等）
pub fn commit_if(app: &mut App, edit: HWND) {
    if app.rename.as_ref().map(|s| s.edit) == Some(edit) {
        commit(app, edit);
    }
}
