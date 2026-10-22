//! Farben der Oberfläche: hell oder dunkel, wie in Windows eingestellt oder fest gewählt.

use std::sync::atomic::{AtomicU8, Ordering};

use windows::core::w;
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};

use crate::paint::{rgb, Color};

/// Gewähltes Design: 0 = wie Windows, 1 = hell, 2 = dunkel.
static CHOICE: AtomicU8 = AtomicU8::new(0);

/// Werte für die Einstellung `theme`.
pub const CHOICES: [&str; 3] = ["auto", "light", "dark"];

/// Legt das Design fest („auto“, „light“ oder „dark“; Unbekanntes gilt als „auto“).
pub fn set_choice(choice: &str) {
    let index = CHOICES.iter().position(|c| *c == choice).unwrap_or(0);
    CHOICE.store(index as u8, Ordering::Relaxed);
}

/// Die Oberfläche ist dunkel: fest gewählt oder weil Windows auf „Dunkel“ für Apps steht.
pub fn dark() -> bool {
    // Im Testmodus lässt sich das Farbschema vorgeben, ohne Windows umzustellen.
    if let Some(theme) = std::env::var_os("VORLESER_TEST").and(std::env::var("VORLESER_THEME").ok()) {
        return theme == "dark";
    }
    match CHOICE.load(Ordering::Relaxed) {
        1 => return false,
        2 => return true,
        _ => {}
    }
    let mut value = 1u32;
    let mut size = 4u32;
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut value as *mut u32 as *mut _),
            Some(&mut size),
        )
    };
    status.is_ok() && value == 0
}

#[derive(Clone, Copy)]
pub struct Palette {
    pub dark: bool,
    /// Fensterhintergrund.
    pub ground: Color,
    /// Leisten (Titelzeile, Menü, Wiedergabe).
    pub bar: Color,
    /// Flächen wie das Textblatt und Schaltflächen.
    pub surface: Color,
    /// Etwas hervorgehobene Fläche (Knopf unter der Maus).
    pub raised: Color,
    pub border: Color,
    pub text: Color,
    pub muted: Color,
    pub accent: Color,
    /// Schrift und Symbole auf der Akzentfarbe.
    pub on_accent: Color,
    /// Zart hinterlegte Auswahl (z. B. gewählte Stufe).
    pub accent_soft: Color,
    pub record: Color,
    pub warn: Color,
    pub ok: Color,
}

const LIGHT: Palette = Palette {
    dark: false,
    ground: rgb(0xF7F8FA),
    bar: rgb(0xEEF1F5),
    surface: rgb(0xFFFFFF),
    raised: rgb(0xE6EAEF),
    border: rgb(0xD5DAE0),
    text: rgb(0x1B1F24),
    muted: rgb(0x5B6470),
    accent: rgb(0x1565C0),
    on_accent: rgb(0xFFFFFF),
    accent_soft: rgb(0xDCE8F7),
    record: rgb(0xD9480F),
    warn: rgb(0xB26A00),
    ok: rgb(0x2E9E6B),
};

const DARK: Palette = Palette {
    dark: true,
    ground: rgb(0x1B1E23),
    bar: rgb(0x202329),
    surface: rgb(0x24282E),
    raised: rgb(0x30353C),
    border: rgb(0x3E454E),
    text: rgb(0xE8EAED),
    muted: rgb(0xA9B0B9),
    accent: rgb(0x6AA7F0),
    on_accent: rgb(0x0F1114),
    accent_soft: rgb(0x1E3550),
    record: rgb(0xE8590C),
    warn: rgb(0xF5C15A),
    ok: rgb(0x4CC38A),
};

pub fn palette() -> Palette {
    if dark() {
        DARK
    } else {
        LIGHT
    }
}

/// Die schwebenden Leisten sind immer dunkel und leicht durchscheinend, damit sie sich von
/// jedem Programm darunter abheben.
pub fn glass() -> Palette {
    Palette { ground: rgb(0x1B1F24).alpha(0.95), surface: rgb(0x343A42), raised: rgb(0x454C56), border: rgb(0x4A525C), ..DARK }
}
