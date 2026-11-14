#![windows_subsystem = "windows"]

mod config;
mod hotkey;
mod menu;
mod notes;
mod rename;
mod settings;
mod theme;
mod todos;
mod tray;
mod util;
mod window;

use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HWND, LPARAM};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::{
    CreateMutexW, OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32,
    PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetClassNameW, GetMessageW, GetWindowThreadProcessId, LoadIconW,
    MSG, SendMessageW, TranslateMessage, IDI_APPLICATION,
};

use crate::util::wide;

fn main() {
    
    unsafe {
        // 单实例（按 exe 路径限定）：同一 exe 重复启动 → 唤起已有窗口并退出；
        // 不同目录的副本各自独立运行。互斥体全局命名会让"下载新版点开只看到
        // 旧实例窗口"（v0.1.0 发布实测踩到），故名字带 exe 路径哈希
        let _mutex = CreateMutexW(std::ptr::null(), 0, wide(&mutex_name()).as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Some(h) = find_own_window() {
                SendMessageW(h, window::MSG_SHOW, 0, 0);
            }
            return;
        }

        window::init_common_controls();
        window::init_gdiplus();

        let cfg = config::load();
        let hicon = app_icon();
        let hwnd = window::create(hicon);
        window::init_app(hwnd, cfg, hicon);

        tray::add(hwnd, hicon, "MiniNotes");

        // 全局热键：配置的组合失败时自动降级尝试备选组合，并气泡告知（不阻塞）
        // 备选：Ctrl+Alt+N → Alt+Q → Ctrl+Alt+Q
        const FALLBACKS: [(u32, u32); 3] = [
            (2 | 1, 0x4E), // Ctrl+Alt+N
            (1, 0x51),     // Alt+Q
            (2 | 1, 0x51), // Ctrl+Alt+Q
        ];
        window::with_app(|app| {
            let (m, v) = (app.cfg.hotkey_mods, app.cfg.hotkey_vk);
            if hotkey::register(hwnd, m, v) {
                return;
            }
            let mut applied = false;
            for (fm, fv) in FALLBACKS {
                if hotkey::register(hwnd, fm, fv) {
                    let old = hotkey::text(m, v);
                    let new = hotkey::text(fm, fv);
                    app.cfg.hotkey_mods = fm;
                    app.cfg.hotkey_vk = fv;
                    config::save(&app.cfg);
                    tray::notify(
                        hwnd,
                        "MiniNotes 热键已调整",
                        &format!("热键 {old} 被其他程序占用，已改用 {new}。可在 托盘右键 → 设置 中修改。"),
                    );
                    applied = true;
                    break;
                }
            }
            if !applied {
                tray::notify(hwnd, "MiniNotes 热键不可用", "所有候选热键均被占用，可左键点击托盘图标唤起。建议在设置中换一个热键。");
            }
        });

        // 旧版本自启注册表写的是裸路径（无 --hidden）：静默改写迁移，
        // 否则旧条目每次开机都会把主窗口弹到桌面
        if crate::settings::autostart_stale() {
            crate::settings::autostart_set(true);
        }
        // 手动启动（双击/快捷方式，无 --hidden 参数）直接把窗口展示到桌面；
        // 开机自启（注册表命令带 --hidden）保持托盘常驻不打扰
        let launched_hidden = std::env::args().any(|a| a == crate::settings::LAUNCH_FLAG);
        if !launched_hidden {
            window::show_main();
        }

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 互斥体名 = exe 路径（小写化）的 FNV-1a 哈希
fn mutex_name() -> String {
    let path = util::exe_path().to_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("MiniNotes_SI_{h:016X}")
}

/// 命中结果暂存（EnumWindows 回调与调用同线程，写入后即读，无并发）
static mut FOUND_HWND: isize = 0;

/// 唤醒已有的本副本实例：按窗口类枚举顶层窗口，再匹配窗口所属进程的
/// exe 路径——FindWindowW 只认类名，多实例并存时会唤醒到别的目录副本
unsafe extern "system" fn enum_main(hwnd: HWND, _lp: LPARAM) -> i32 {
    let mut cls = [0u16; 64];
    GetClassNameW(hwnd, cls.as_mut_ptr(), 64);
    if util::from_wide(&cls) != window::MAIN_CLASS {
        return 1;
    }
    let mut pid = 0u32;
    GetWindowThreadProcessId(hwnd, &mut pid);
    if pid == 0 {
        return 1;
    }
    let proc = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
    if proc == 0 {
        return 1;
    }
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, buf.as_mut_ptr(), &mut len);
    CloseHandle(proc);
    if ok != 0 && util::from_wide(&buf[..len as usize]).eq_ignore_ascii_case(&util::exe_path()) {
        std::ptr::addr_of_mut!(FOUND_HWND).write(hwnd);
        return 0;
    }
    1
}

fn find_own_window() -> Option<HWND> {
    unsafe {
        std::ptr::addr_of_mut!(FOUND_HWND).write(0);
        EnumWindows(Some(enum_main), 0);
        let h = std::ptr::addr_of!(FOUND_HWND).read();
        if h != 0 {
            Some(h)
        } else {
            None
        }
    }
}

/// 资源图标（winres 嵌入，ID=1）；失败回退系统图标
fn app_icon() -> isize {
    unsafe {
        let h = LoadIconW(GetModuleHandleW(std::ptr::null()), 1usize as *const u16);
        if h != 0 {
            h
        } else {
            LoadIconW(0, IDI_APPLICATION)
        }
    }
}
