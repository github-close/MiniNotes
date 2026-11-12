#![windows_subsystem = "windows"]

mod config;
mod hotkey;
mod notes;
mod rename;
mod settings;
mod theme;
mod todos;
mod tray;
mod util;
mod window;

use windows_sys::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HWND};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, FindWindowW, GetMessageW, LoadIconW, MSG, SendMessageW, TranslateMessage,
    IDI_APPLICATION,
};

use crate::util::wide;

const MUTEX_NAME: &str = "MiniNotes_SingleInstance_v1";

fn main() {
    
    unsafe {
        // 单实例：已有实例则唤起它并退出
        let _mutex = CreateMutexW(std::ptr::null(), 0, wide(MUTEX_NAME).as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Some(h) = find_main_window() {
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

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, 0, 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn find_main_window() -> Option<HWND> {
    unsafe {
        let h = FindWindowW(wide(window::MAIN_CLASS).as_ptr(), std::ptr::null());
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
