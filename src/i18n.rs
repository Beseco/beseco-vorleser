//! Sprache der Oberfläche: Deutsch oder Englisch. Die Texte stehen direkt an der Stelle, an der
//! sie gebraucht werden, als `tr("Deutsch", "English")`.

use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use windows::Win32::Globalization::GetUserDefaultUILanguage;

static ENGLISH: AtomicBool = AtomicBool::new(false);

/// Werte für die Einstellung `language`.
pub const AUTO: &str = "auto";
pub const GERMAN: &str = "de";
pub const ENGLISH_CODE: &str = "en";

/// Legt die Sprache fest: "de", "en" oder "auto" (Deutsch, wenn Windows auf Deutsch läuft).
pub fn init(setting: &str) {
    let english = match setting {
        GERMAN => false,
        ENGLISH_CODE => true,
        _ => (unsafe { GetUserDefaultUILanguage() } & 0x3ff) != 0x07,
    };
    ENGLISH.store(english, Relaxed);
}

pub fn is_english() -> bool {
    ENGLISH.load(Relaxed)
}

pub fn tr(german: &'static str, english: &'static str) -> &'static str {
    if is_english() {
        english
    } else {
        german
    }
}

/// Zeigt eine gespeicherte Tastenkombination wie "Strg+Alt+V" in der Sprache der Oberfläche.
pub fn hotkey(text: &str) -> String {
    if !is_english() {
        return text.to_string();
    }
    text.split('+')
        .map(|part| match part.trim().to_ascii_uppercase().as_str() {
            "STRG" | "CTRL" | "CONTROL" => "Ctrl".to_string(),
            "UMSCHALT" | "SHIFT" => "Shift".to_string(),
            "LEERTASTE" | "SPACE" => "Space".to_string(),
            "RECHTSKLICK" | "RIGHTCLICK" => "Right-click".to_string(),
            _ => part.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_hotkeys() {
        init(ENGLISH_CODE);
        assert_eq!(tr("Stopp", "Stop"), "Stop");
        assert_eq!(hotkey("Strg+Alt+V"), "Ctrl+Alt+V");
        assert_eq!(hotkey("Strg+Shift+Leertaste"), "Ctrl+Shift+Space");
        init(GERMAN);
        assert_eq!(tr("Stopp", "Stop"), "Stopp");
        assert_eq!(hotkey("Strg+Alt+V"), "Strg+Alt+V");
    }
}
