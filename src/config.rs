//! Einstellungen in `%APPDATA%\Vorleser\settings.ini` (einfache `key=value`-Zeilen).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

/// Im Testlauf werden Einstellungen gelesen, aber nie geschrieben.
static READ_ONLY: AtomicBool = AtomicBool::new(false);

pub fn set_read_only() {
    READ_ONLY.store(true, Relaxed);
}

pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;

#[derive(Clone, Debug, PartialEq)]
pub struct Config {
    /// SAPI-Token-ID der Stimme; leer = Standardstimme des Systems.
    pub voice: String,
    /// SAPI-Geschwindigkeit, -10 bis +10.
    pub rate: i32,
    pub hotkey_read: String,
    pub hotkey_pause: String,
    pub hotkey_menu: String,
    /// Mausauslöser für das Menü, z. B. "Strg+Rechtsklick"; leer = aus.
    pub mouse_menu: String,
    /// Lautstärke in Prozent.
    pub volume: u32,
    /// Zeilenumbrüche, Silbentrennung und Links vor dem Vorlesen bereinigen.
    pub clean_text: bool,
    /// Das Fenster bleibt über anderen Fenstern.
    pub always_on_top: bool,
    /// Einmal am Tag auf GitHub nach einer neueren Version suchen.
    pub update_check: bool,
    /// Zeitpunkt der letzten erfolgreichen Suche, Sekunden seit 1970.
    pub last_update_check: u64,
    /// Sprache der Oberfläche: "auto", "de" oder "en".
    pub language: String,
    /// Proxy: leer = wie Windows, sonst Adresse eines Proxy-Skripts (wpad.dat) oder `host:port`.
    pub proxy: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            voice: String::new(),
            rate: 0,
            hotkey_read: "Strg+Alt+V".into(),
            hotkey_pause: "Strg+Alt+P".into(),
            hotkey_menu: "Strg+Alt+M".into(),
            mouse_menu: "Strg+Rechtsklick".into(),
            volume: 100,
            clean_text: true,
            always_on_top: false,
            update_check: true,
            last_update_check: 0,
            language: "auto".into(),
            proxy: String::new(),
        }
    }
}

impl Config {
    pub fn parse(text: &str) -> Config {
        let mut cfg = Config::default();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else { continue };
            let value = value.trim();
            match key.trim() {
                "voice" => cfg.voice = value.to_string(),
                "rate" => cfg.rate = value.parse().unwrap_or(0).clamp(-10, 10),
                "hotkey_read" => cfg.hotkey_read = value.to_string(),
                "hotkey_pause" => cfg.hotkey_pause = value.to_string(),
                "hotkey_menu" => cfg.hotkey_menu = value.to_string(),
                "mouse_menu" => cfg.mouse_menu = value.to_string(),
                "volume" => cfg.volume = value.parse().unwrap_or(100).min(100),
                "clean_text" => cfg.clean_text = value != "0",
                "always_on_top" => cfg.always_on_top = value == "1",
                "update_check" => cfg.update_check = value != "0",
                "last_update_check" => cfg.last_update_check = value.parse().unwrap_or(0),
                "language" => cfg.language = value.to_string(),
                "proxy" => cfg.proxy = value.to_string(),
                _ => {}
            }
        }
        cfg
    }

    pub fn serialize(&self) -> String {
        format!(
            "# Vorleser-Einstellungen. Tastenkombinationen z. B. Strg+Alt+V, Ctrl+Shift+F9, Win+Alt+R; leer = aus.\r\n\
             # mouse_menu: Zusatztaste(n) + Rechtsklick, z. B. Strg+Rechtsklick oder Strg+Shift+Rechtsklick.\r\n\
             voice={}\r\nrate={}\r\nhotkey_read={}\r\nhotkey_pause={}\r\nhotkey_menu={}\r\nmouse_menu={}\r\n\
             volume={}\r\nclean_text={}\r\nalways_on_top={}\r\n\
             update_check={}\r\nlast_update_check={}\r\nlanguage={}\r\nproxy={}\r\n",
            self.voice,
            self.rate,
            self.hotkey_read,
            self.hotkey_pause,
            self.hotkey_menu,
            self.mouse_menu,
            self.volume,
            self.clean_text as u8,
            self.always_on_top as u8,
            self.update_check as u8,
            self.last_update_check,
            self.language,
            self.proxy
        )
    }

    pub fn load() -> Config {
        match path().and_then(|p| std::fs::read_to_string(p).ok()) {
            Some(text) => Config::parse(&text),
            None => Config::default(),
        }
    }

    pub fn save(&self) {
        if READ_ONLY.load(Relaxed) {
            return;
        }
        if let Some(p) = path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let _ = std::fs::write(p, self.serialize());
        }
    }
}

fn path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Vorleser").join("settings.ini"))
}

/// Wandelt z. B. "Strg+Alt+V" in (Modifikatoren, virtueller Tastencode) um.
pub fn parse_hotkey(text: &str) -> Option<(u32, u32)> {
    let mut mods = 0;
    let mut key = None;
    for part in text.split('+') {
        let part = part.trim().to_ascii_uppercase();
        match parse_modifier(&part) {
            Some(m) => mods |= m,
            None if key.is_none() => key = Some(parse_key(&part)?),
            None => return None,
        }
    }
    match key {
        Some(vk) if mods != 0 => Some((mods, vk)),
        _ => None,
    }
}

/// Wandelt z. B. "Strg+Rechtsklick" in die Zusatztasten um, die beim Rechtsklick gedrückt sein müssen.
/// Ohne Zusatztaste gäbe es kein normales Kontextmenü mehr, deshalb ist das ungültig.
pub fn parse_mouse_trigger(text: &str) -> Option<u32> {
    let mut mods = 0;
    let mut click = false;
    for part in text.split('+') {
        let part = part.trim().to_ascii_uppercase();
        match parse_modifier(&part) {
            Some(m) => mods |= m,
            None if !click && matches!(part.as_str(), "RECHTSKLICK" | "RIGHTCLICK") => click = true,
            None => return None,
        }
    }
    (click && mods != 0).then_some(mods)
}

fn parse_modifier(name: &str) -> Option<u32> {
    match name {
        "STRG" | "CTRL" | "CONTROL" => Some(MOD_CONTROL),
        "ALT" => Some(MOD_ALT),
        "SHIFT" | "UMSCHALT" => Some(MOD_SHIFT),
        "WIN" => Some(MOD_WIN),
        _ => None,
    }
}

/// Gegenstück zu [`parse_hotkey`]; `None`, wenn die Taste nicht unterstützt wird.
pub fn format_hotkey(mods: u32, vk: u32) -> Option<String> {
    let key = match vk {
        0x30..=0x39 | 0x41..=0x5A => (vk as u8 as char).to_string(),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x20 => "Leertaste".to_string(),
        0x13 => "Pause".to_string(),
        _ => return None,
    };
    let names = [(MOD_CONTROL, "Strg"), (MOD_ALT, "Alt"), (MOD_SHIFT, "Shift"), (MOD_WIN, "Win")];
    let mut parts: Vec<&str> = names.iter().filter(|(m, _)| mods & m != 0).map(|(_, n)| *n).collect();
    if parts.is_empty() {
        return None;
    }
    parts.push(&key);
    Some(parts.join("+"))
}

fn parse_key(name: &str) -> Option<u32> {
    let bytes = name.as_bytes();
    if bytes.len() == 1 && (bytes[0].is_ascii_uppercase() || bytes[0].is_ascii_digit()) {
        return Some(bytes[0] as u32);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    match name {
        "SPACE" | "LEERTASTE" => Some(0x20),
        "PAUSE" => Some(0x13),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("Strg+Alt+V"), Some((MOD_CONTROL | MOD_ALT, 0x56)));
        assert_eq!(parse_hotkey("ctrl + shift + f9"), Some((MOD_CONTROL | MOD_SHIFT, 0x78)));
        assert_eq!(parse_hotkey("Win+Alt+Leertaste"), Some((MOD_WIN | MOD_ALT, 0x20)));
        assert_eq!(parse_hotkey("V"), None);
        assert_eq!(parse_hotkey("Strg+Alt"), None);
        assert_eq!(parse_hotkey("Strg+A+B"), None);
        assert_eq!(parse_hotkey(""), None);
    }

    #[test]
    fn hotkey_formatting_roundtrips() {
        for text in ["Strg+Alt+V", "Strg+Shift+F9", "Alt+Win+Leertaste", "Strg+7"] {
            let (mods, vk) = parse_hotkey(text).unwrap();
            let formatted = format_hotkey(mods, vk).unwrap();
            assert_eq!(parse_hotkey(&formatted), Some((mods, vk)));
        }
        assert_eq!(format_hotkey(MOD_CONTROL | MOD_ALT, 0x56).as_deref(), Some("Strg+Alt+V"));
        assert_eq!(format_hotkey(MOD_CONTROL, 0x0D), None);
        assert_eq!(format_hotkey(0, 0x41), None);
    }

    #[test]
    fn mouse_triggers() {
        assert_eq!(parse_mouse_trigger("Strg+Rechtsklick"), Some(MOD_CONTROL));
        assert_eq!(parse_mouse_trigger("ctrl + shift + rightclick"), Some(MOD_CONTROL | MOD_SHIFT));
        assert_eq!(parse_mouse_trigger("Rechtsklick"), None);
        assert_eq!(parse_mouse_trigger("Strg"), None);
        assert_eq!(parse_mouse_trigger("Strg+V"), None);
        assert_eq!(parse_mouse_trigger(""), None);
    }

    #[test]
    fn roundtrip() {
        let cfg = Config {
            voice: r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech\Voices\Tokens\TTS_MS_DE-DE_HEDDA_11.0".into(),
            rate: 3,
            hotkey_read: "Strg+Alt+R".into(),
            hotkey_pause: String::new(),
            hotkey_menu: "Win+Alt+M".into(),
            mouse_menu: "Strg+Shift+Rechtsklick".into(),
            volume: 70,
            clean_text: false,
            always_on_top: true,
            update_check: false,
            last_update_check: 1_760_000_000,
            language: "en".into(),
            proxy: "http://wpad.firma.local/wpad.dat".into(),
        };
        assert_eq!(Config::parse(&cfg.serialize()), cfg);
    }

    #[test]
    fn tolerant_parsing() {
        let cfg = Config::parse("rate = 99\nunbekannt=1\nkaputt\n; Kommentar\n");
        assert_eq!(cfg.rate, 10);
        assert_eq!(cfg.hotkey_read, "Strg+Alt+V");
    }
}
