//! Die letzten zehn Diktate, damit sie sich später noch einmal verwenden lassen (Menü „Letzte
//! Diktate“: anklicken kopiert in die Zwischenablage). Gespeichert in
//! `%APPDATA%\Vorleser\diktate.json`, also nur auf diesem PC und nur für diesen Benutzer.

use std::path::PathBuf;
use std::sync::Mutex;

use crate::json;

pub const LIMIT: usize = 10;

/// Neueste zuerst; `None`, solange die Datei noch nicht gelesen wurde.
static ENTRIES: Mutex<Option<Vec<String>>> = Mutex::new(None);

fn path() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(appdata).join("Vorleser").join("diktate.json"))
}

fn load() -> Vec<String> {
    let Some(text) = path().and_then(|p| std::fs::read_to_string(p).ok()) else { return Vec::new() };
    parse(&text)
}

fn parse(text: &str) -> Vec<String> {
    let Some(value) = json::parse(text) else { return Vec::new() };
    let items = value.as_array().unwrap_or_default();
    items.iter().filter_map(|v| v.as_str()).map(str::to_string).take(LIMIT).collect()
}

fn serialize(entries: &[String]) -> String {
    let items: Vec<String> = entries.iter().map(|e| json::string(e)).collect();
    format!("[\n  {}\n]\n", items.join(",\n  "))
}

fn save(entries: &[String]) {
    if crate::config::is_read_only() {
        return;
    }
    let Some(path) = path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(path, serialize(entries));
}

fn with<R>(f: impl FnOnce(&mut Vec<String>) -> R) -> R {
    let mut guard = ENTRIES.lock().unwrap();
    f(guard.get_or_insert_with(load))
}

/// Neueste zuerst.
pub fn entries() -> Vec<String> {
    with(|entries| entries.clone())
}

pub fn get(index: usize) -> Option<String> {
    with(|entries| entries.get(index).cloned())
}

/// Nimmt ein fertiges Diktat auf. Steht derselbe Text schon in der Liste, rückt er nach oben.
pub fn add(text: &str) {
    let text = text.trim();
    if text.is_empty() {
        return;
    }
    with(|entries| {
        insert(entries, text);
        save(entries);
    });
}

fn insert(entries: &mut Vec<String>, text: &str) {
    entries.retain(|e| e != text);
    entries.insert(0, text.to_string());
    entries.truncate(LIMIT);
}

/// Ersetzt das neueste Diktat durch seine korrigierte Fassung (nur, wenn es noch `old` ist).
pub fn replace_newest(old: &str, new: &str) {
    let (old, new) = (old.trim(), new.trim());
    with(|entries| {
        if entries.first().is_some_and(|e| e == old) && !new.is_empty() {
            entries[0] = new.to_string();
            save(entries);
        }
    });
}

pub fn clear() {
    with(|entries| {
        entries.clear();
        save(entries);
    });
}

/// Kurzer Menüeintrag: die ersten Wörter in einer Zeile. `&` würde sonst als Tastenkürzel gelesen.
pub fn label(text: &str) -> String {
    const MAX: usize = 48;
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let short = if line.chars().count() > MAX {
        let cut: String = line.chars().take(MAX).collect();
        let cut = cut.rsplit_once(' ').map_or(cut.as_str(), |(head, _)| head).to_string();
        format!("{cut} …")
    } else {
        line
    };
    short.replace('&', "&&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ten_newest_without_duplicates() {
        let mut entries = Vec::new();
        for i in 0..12 {
            insert(&mut entries, &format!("Diktat {i}"));
        }
        assert_eq!(entries.len(), LIMIT);
        assert_eq!(entries[0], "Diktat 11");
        insert(&mut entries, "Diktat 5");
        assert_eq!(entries[0], "Diktat 5");
        assert_eq!(entries.iter().filter(|e| *e == "Diktat 5").count(), 1);
    }

    #[test]
    fn round_trips_json() {
        let entries = vec!["Er sagte \"Hallo\".\nZweite Zeile".to_string(), "Pfad C:\\Daten".to_string()];
        assert_eq!(parse(&serialize(&entries)), entries);
        assert!(parse("kaputt").is_empty());
    }

    #[test]
    fn shortens_labels() {
        assert_eq!(label("Kurz &   knapp\nzwei"), "Kurz && knapp zwei");
        let long = label("Das ist ein ziemlich langer Text, der nicht ganz ins Menü passen wird.");
        assert!(long.ends_with(" …") && long.chars().count() <= 50, "{long}");
    }
}
