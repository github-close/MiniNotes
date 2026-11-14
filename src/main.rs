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
    MessageBoxW, MSG, SendMessageW, TranslateMessage, IDI_APPLICATION, MB_ICONWARNING,
    MB_SETFOREGROUND, MB_TOPMOST,
};

use crate::util::wide;

fn main() {
    
    unsafe {
        // 单实例（整机唯一）：先枚举已有主窗口并按所属进程 exe 路径区分——
        // 其他目录的副本在跑 → 弹窗拒绝并指路（并存会争夺全局热键、数据也
        // 各自独立，见 README「下载」）；同一 exe 在跑 → 唤起已有窗口。
        // 枚举竞态（两份同时启动）由随后的互斥体兜底
        let me = util::exe_path();
        let instances = all_instances();
        let others: Vec<&(HWND, String)> =
            instances.iter().filter(|(_, p)| !p.eq_ignore_ascii_case(&me)).collect();
        if let Some((_, path)) = others.first() {
            let msg = format!(
                "检测到另一份 MiniNotes 正在运行：\n{}\n\n两份副本并存会争夺全局热键、数据也各自独立，不做并存。\n请先退出已运行的实例（托盘右键 → 退出），再运行本程序。",
                path
            );
            MessageBoxW(
                0,
                wide(&msg).as_ptr(),
                wide("MiniNotes").as_ptr(),
                MB_ICONWARNING | MB_TOPMOST | MB_SETFOREGROUND,
            );
            return;
        }
        if let Some((h, _)) = instances.iter().find(|(_, p)| p.eq_ignore_ascii_case(&me)) {
            SendMessageW(*h, window::MSG_SHOW, 0, 0);
            return;
        }
        let _mutex = CreateMutexW(std::ptr::null(), 0, wide(&mutex_name()).as_ptr());
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // 极端竞态：另一份同路径实例刚启动、窗口还没建出来（枚举没看到），
            // 它很快会自行展示界面，这里直接退出即可
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

/// 互斥体名 = exe 路径（小写化）的 FNV-1a 哈希。主流程的枚举先于互斥体
/// 完成"其他目录拒绝 / 同路径唤起"的判定，互斥体只兜两份同路径实例
/// 同时启动、枚举都看不到对方窗口的竞态
fn mutex_name() -> String {
    let path = util::exe_path().to_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in path.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("MiniNotes_SI_{h:016X}")
}

/// 枚举命中结果暂存（回调与调用同线程，写入后即取走，无并发）
static mut FOUND: Vec<(isize, String)> = Vec::new();

/// 枚举所有 MiniNotes 主窗口（含本进程），按窗口所属进程的 exe 路径区分
/// 副本——FindWindowW 只认类名，无法区分不同目录的副本
unsafe extern "system" fn enum_instance(hwnd: HWND, _lp: LPARAM) -> i32 {
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
    if ok != 0 {
        (*std::ptr::addr_of_mut!(FOUND)).push((hwnd, util::from_wide(&buf[..len as usize])));
    }
    1
}

/// 当前机器上所有 MiniNotes 主窗口（含本进程）
fn all_instances() -> Vec<(HWND, String)> {
    unsafe {
        std::ptr::addr_of_mut!(FOUND).write(Vec::new());
        EnumWindows(Some(enum_instance), 0);
        std::ptr::replace(std::ptr::addr_of_mut!(FOUND), Vec::new())
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
