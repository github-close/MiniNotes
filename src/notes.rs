use std::path::PathBuf;

use windows_sys::Win32::Foundation::{HWND, LRESULT, LPARAM, RECT, WPARAM};
use windows_sys::Win32::UI::Controls::{
    CDDS_ITEMPREPAINT, CDDS_POSTPAINT, CDDS_PREPAINT, CDRF_DODEFAULT, CDRF_NOTIFYITEMDRAW,
    CDRF_NOTIFYPOSTPAINT, LVIF_TEXT, LVITEMW, LVM_DELETEALLITEMS,
    LVM_ENSUREVISIBLE, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMSTATE, LVM_GETNEXTITEM,
    LVM_INSERTITEMW, LVM_SETITEMSTATE, LVIS_SELECTED, LVNI_SELECTED, NMLVCUSTOMDRAW,
};
use windows_sys::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows_sys::Win32::UI::WindowsAndMessaging::{MessageBoxW, SendMessageW, SetWindowTextW, MB_OKCANCEL};

use crate::theme;
use crate::util::{get_text, set_text, wide};
use crate::window::App;

pub struct NoteMeta {
    pub filename: String,
    pub mtime: u64,
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
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn is_note_file(name: &str) -> bool {
    matches!(ext_of(name).as_str(), "md" | "txt")
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

fn lv_select(lv: HWND, idx: i32) {
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
                if SendMessageW(lv, LVM_GETITEMCOUNT, 0, 0) == 0 {
                    crate::util::draw_hint_centered(nm.nmcd.hdc, &rc, "点「＋ 新建」写第一篇笔记", p.text_sec, app.font);
                }
                CDRF_DODEFAULT as LRESULT
            }
            CDDS_ITEMPREPAINT => {
                let idx = nm.nmcd.dwItemSpec as usize;
                let i = nm.nmcd.dwItemSpec as i32;
                let p = theme::palette(app.dark);
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
                // 标题文字（行首无图标槽，左缩进 10 与「＋ 新建」文字对齐）
                let title = app
                    .notes
                    .get(idx)
                    .map(|m| title_of(&m.filename))
                    .unwrap_or_default();
                let mut tr = RECT { left: rc.left + 10, top: rc.top, right: rc.right - 8, bottom: rc.bottom };
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
                CDRF_SKIPDEFAULT as LRESULT
            }
            _ => CDRF_DODEFAULT as LRESULT,
        }
    }
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

    let mut metas: Vec<NoteMeta> = std::fs::read_dir(dir())
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| is_note_file(&e.file_name().to_string_lossy()))
                .map(|e| {
                    let filename = e.file_name().to_string_lossy().into_owned();
                    NoteMeta {
                        filename,
                        mtime: mtime_of(&e.path()),
                    }
                })
                .collect()
        })
        .unwrap_or_default();
    metas.sort_by(|a, b| b.mtime.cmp(&a.mtime));

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
        .map(|d| d.as_secs())
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
    let path = dir().join(&app.notes[idx].filename);
    let text = std::fs::read_to_string(path).unwrap_or_default();
    // 文件是 LF；EDIT 控件只认 CRLF，否则换行丢失
    let text = text.replace("\r\n", "\n").replace('\n', "\r\n");
    app.suppress_notify = true;
    set_text(app.editor, &text);
    app.suppress_notify = false;
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
    refresh_keep(app, &filename);
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
        app.cur_note = None;
        app.note_dirty = false;
    }
    // 选中替代项：先记下文件名，refresh 会按 mtime 重排列表，用索引会指错行
    let next_name = if app.notes.is_empty() {
        None
    } else {
        let i = (idx as usize).min(app.notes.len() - 1);
        Some(app.notes[i].filename.clone())
    };
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
    if app.cur_note == Some(idx) {
        app.cfg.last_note = new_name;
        update_title(app);
    }
    true
}
