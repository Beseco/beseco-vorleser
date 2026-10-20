//! Sprachsystem: Alle Texte der Oberfläche stehen in Sprachdateien (`lang/*.lang`), der Code
//! nennt nur Schlüssel: `t("menu.file")`. Deutsch und Englisch sind eingebaut; weitere Sprachen
//! (oder verbesserte Übersetzungen) lädt Vorleser zusätzlich aus `lang\` neben der EXE und aus
//! `%APPDATA%\Vorleser\lang\`. Fehlt ein Text, gilt der englische.
//!
//! Aufbau einer Sprachdatei: `@code = fr`, `@name = Français`, danach `schlüssel = Text`.
//! `\n` ist ein Zeilenumbruch, `\t` trennt im Menü die Tastenkombination ab, `&` markiert den
//! Buchstaben für die Alt-Taste, `{}` wird durch einen Wert ersetzt.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
use std::sync::OnceLock;

use windows::Win32::Globalization::{GetUserDefaultUILanguage, LCIDToLocaleName};

/// Wert der Einstellung `language` für „wie Windows“.
pub const AUTO: &str = "auto";

const BUILT_IN: [&str; 2] = [include_str!("../lang/de.lang"), include_str!("../lang/en.lang")];
const FALLBACK: &str = "en";

pub struct Language {
    pub code: String,
    pub name: String,
    texts: HashMap<String, String>,
}

static LANGUAGES: OnceLock<Vec<Language>> = OnceLock::new();
static CURRENT: AtomicUsize = AtomicUsize::new(0);

fn parse(source: &str) -> Option<Language> {
    let mut code = None;
    let mut name = None;
    let mut texts = HashMap::new();
    for line in source.lines() {
        let line = line.trim_start_matches('\u{feff}').trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else { continue };
        let (key, value) = (key.trim(), unescape(value.trim()));
        match key {
            "@code" => code = Some(value.to_lowercase()),
            "@name" => name = Some(value),
            _ => {
                texts.insert(key.to_string(), value);
            }
        }
    }
    let code = code?;
    Some(Language { name: name.unwrap_or_else(|| code.clone()), code, texts })
}

fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// Ordner, aus denen zusätzliche Sprachdateien geladen werden.
fn external_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|p| p.join("lang"))) {
        dirs.push(dir);
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        dirs.push(PathBuf::from(appdata).join("Vorleser").join("lang"));
    }
    dirs
}

fn load() -> Vec<Language> {
    let mut languages: Vec<Language> = BUILT_IN.iter().filter_map(|source| parse(source)).collect();
    for dir in external_dirs() {
        let Ok(entries) = std::fs::read_dir(dir) else { continue };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|ext| !ext.eq_ignore_ascii_case("lang")) {
                continue;
            }
            let Some(language) = std::fs::read_to_string(&path).ok().as_deref().and_then(parse) else { continue };
            // Eine Datei für eine vorhandene Sprache ergänzt oder verbessert deren Texte.
            match languages.iter_mut().find(|l| l.code == language.code) {
                Some(existing) => {
                    existing.texts.extend(language.texts);
                    existing.name = language.name;
                }
                None => languages.push(language),
            }
        }
    }
    languages
}

fn languages_ref() -> &'static [Language] {
    LANGUAGES.get_or_init(load)
}

/// Alle verfügbaren Sprachen als (Code, Name).
pub fn languages() -> Vec<(&'static str, &'static str)> {
    languages_ref().iter().map(|l| (l.code.as_str(), l.name.as_str())).collect()
}

/// Sprachcode der Windows-Oberfläche, z. B. "de".
pub fn windows_language() -> String {
    let mut name = [0u16; 85];
    let len = unsafe { LCIDToLocaleName(GetUserDefaultUILanguage() as u32, Some(&mut name), 0) };
    let tag = String::from_utf16_lossy(&name[..(len.max(1) - 1) as usize]);
    tag.split('-').next().unwrap_or("").to_lowercase()
}

/// Legt die Sprache fest: ein Sprachcode wie "de" oder [`AUTO`].
pub fn init(setting: &str) {
    let languages = languages_ref();
    let find = |code: &str| languages.iter().position(|l| l.code == code);
    let wanted = if setting == AUTO || setting.is_empty() { windows_language() } else { setting.to_lowercase() };
    let index = find(&wanted).or_else(|| find(FALLBACK)).unwrap_or(0);
    CURRENT.store(index, Relaxed);
}

/// Code der eingestellten Sprache.
pub fn current() -> &'static str {
    languages_ref().get(CURRENT.load(Relaxed)).map_or(FALLBACK, |l| l.code.as_str())
}

/// Text zum Schlüssel in der eingestellten Sprache, sonst auf Englisch, sonst der Schlüssel.
pub fn t(key: &'static str) -> &'static str {
    lookup(key).unwrap_or(key)
}

/// Wie [`t`], für zusammengesetzte Schlüssel; `None`, wenn es den Text in keiner Sprache gibt.
pub fn lookup(key: &str) -> Option<&'static str> {
    let languages = languages_ref();
    let find = |language: Option<&'static Language>| language.and_then(|l| l.texts.get(key)).map(String::as_str);
    find(languages.get(CURRENT.load(Relaxed))).or_else(|| find(languages.iter().find(|l| l.code == FALLBACK)))
}

/// Zeigt eine gespeicherte Tastenkombination wie "Strg+Alt+V" in der Sprache der Oberfläche.
pub fn hotkey(text: &str) -> String {
    text.split('+')
        .map(|part| match part.trim().to_ascii_uppercase().as_str() {
            "STRG" | "CTRL" | "CONTROL" => t("key.ctrl").to_string(),
            "UMSCHALT" | "SHIFT" => t("key.shift").to_string(),
            "LEERTASTE" | "SPACE" => t("key.space").to_string(),
            "RECHTSKLICK" | "RIGHTCLICK" => t("key.rightclick").to_string(),
            _ => part.trim().to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(source: &str) -> Vec<String> {
        let mut keys: Vec<String> = parse(source).unwrap().texts.into_keys().collect();
        keys.sort();
        keys
    }

    #[test]
    fn built_in_languages_have_the_same_keys() {
        assert_eq!(keys(BUILT_IN[0]), keys(BUILT_IN[1]));
    }

    /// Jeder Schlüssel, den der Code verwendet, muss in den Sprachdateien stehen.
    #[test]
    fn every_used_key_exists() {
        let english = parse(BUILT_IN[1]).unwrap();
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut missing = Vec::new();
        for entry in std::fs::read_dir(dir).unwrap().flatten() {
            let source = std::fs::read_to_string(entry.path()).unwrap();
            for part in source.split("t(\"").skip(1) {
                let key = &part[..part.find('"').unwrap()];
                let looks_like_key = key.contains('.') && !key.contains([' ', '/']) && !key.ends_with('.');
                if looks_like_key && !english.texts.contains_key(key) {
                    missing.push(key.to_string());
                }
            }
        }
        assert!(missing.is_empty(), "fehlende Schlüssel: {missing:?}");
    }

    #[test]
    fn parses_language_files() {
        let language = parse("# Kommentar\n@code = FR\n@name = Français\nmenu.file = &Fichier\nx = a\\nb\\tc\n").unwrap();
        assert_eq!(language.code, "fr");
        assert_eq!(language.name, "Français");
        assert_eq!(language.texts["menu.file"], "&Fichier");
        assert_eq!(language.texts["x"], "a\nb\tc");
        assert!(parse("ohne = code").is_none());
    }
}
