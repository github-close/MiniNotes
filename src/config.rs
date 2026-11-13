use std::path::PathBuf;

use crate::util;

#[derive(Clone)]
pub struct Config {
    pub hotkey_mods: u32, // MOD_ALT/MOD_CONTROL/MOD_SHIFT/MOD_WIN 组合，不含 MOD_NOREPEAT
    pub hotkey_vk: u32,
    pub auto_hide: bool,
    pub autostart: bool,
    pub theme: String, // "light" / "dark" / "system"
    pub win_x: i32,
    pub win_y: i32,
    pub win_cx: i32,
    pub win_cy: i32,
    pub last_tab: usize,
    pub last_note: String,
    /// 笔记编辑区字体名（仅编辑器生效，其余界面固定 YaHei UI）
    pub ed_font: String,
    /// 笔记编辑区字号（像素字符高度，与界面字号的 px 口径一致）
    pub ed_size: i32,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            hotkey_mods: 1, // MOD_ALT
            hotkey_vk: 0x4E, // 'N'
            auto_hide: true,
            autostart: false,
            theme: "system".into(),
            win_x: -1, // -1 = 首次居中
            win_y: -1,
            win_cx: 720,
            win_cy: 540,
            last_tab: 1,
            last_note: String::new(),
            ed_font: "Microsoft YaHei UI".into(),
            ed_size: 12,
        }
    }
}

fn config_path() -> PathBuf {
    util::data_dir().join("config.ini")
}

/// 极简 key=value 解析，UTF-8
pub fn load() -> Config {
    let mut cfg = Config::default();
    let Ok(text) = std::fs::read_to_string(config_path()) else {
        return cfg;
    };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        match k {
            "hotkey_mods" => cfg.hotkey_mods = v.parse().unwrap_or(cfg.hotkey_mods),
            "hotkey_vk" => cfg.hotkey_vk = v.parse().unwrap_or(cfg.hotkey_vk),
            "auto_hide" => cfg.auto_hide = v != "0",
            "autostart" => cfg.autostart = v != "0",
            "theme" => cfg.theme = v.to_string(),
            "win_x" => cfg.win_x = v.parse().unwrap_or(cfg.win_x),
            "win_y" => cfg.win_y = v.parse().unwrap_or(cfg.win_y),
            "win_cx" => cfg.win_cx = v.parse().unwrap_or(cfg.win_cx),
            "win_cy" => cfg.win_cy = v.parse().unwrap_or(cfg.win_cy),
            "last_tab" => cfg.last_tab = v.parse().unwrap_or(cfg.last_tab),
            "last_note" => cfg.last_note = v.to_string(),
            "ed_font" => {
                if !v.is_empty() {
                    cfg.ed_font = v.to_string();
                }
            }
            "ed_size" => {
                if let Ok(n) = v.parse::<i32>() {
                    cfg.ed_size = n.clamp(8, 72);
                }
            }
            _ => {}
        }
    }
    cfg
}

pub fn save(cfg: &Config) {
    let text = format!(
        "hotkey_mods={}\nhotkey_vk={}\nauto_hide={}\nautostart={}\ntheme={}\nwin_x={}\nwin_y={}\nwin_cx={}\nwin_cy={}\nlast_tab={}\nlast_note={}\ned_font={}\ned_size={}\n",
        cfg.hotkey_mods,
        cfg.hotkey_vk,
        cfg.auto_hide as u8,
        cfg.autostart as u8,
        cfg.theme,
        cfg.win_x,
        cfg.win_y,
        cfg.win_cx,
        cfg.win_cy,
        cfg.last_tab,
        cfg.last_note,
        cfg.ed_font,
        cfg.ed_size,
    );
    let _ = std::fs::write(config_path(), text);
}
