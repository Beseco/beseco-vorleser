//! Sucht höchstens einmal am Tag auf GitHub nach einer neueren Version. Abschaltbar; gesendet
//! wird nur eine gewöhnliche Anfrage ohne Text, Einstellungen oder Kennung des Rechners.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const RELEASES_URL: &str = "https://github.com/Beseco/beseco-vorleser/releases/latest";
const API_URL: &str = "https://api.github.com/repos/Beseco/beseco-vorleser/releases/latest";
const INTERVAL: u64 = 24 * 60 * 60;
/// Nach dem Start wird kurz gewartet, damit die Prüfung den Systemstart nicht ausbremst.
const STARTUP_DELAY: Duration = Duration::from_secs(60);

static ENABLED: AtomicBool = AtomicBool::new(false);
/// Zeitpunkt der letzten Prüfung in Sekunden seit 1970; 0 = noch nie.
static LAST_CHECK: AtomicU64 = AtomicU64::new(0);
static TARGET: AtomicIsize = AtomicIsize::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);
/// Gefundene neuere Version, z. B. "0.2.0".
static NEWER: Mutex<Option<String>> = Mutex::new(None);

/// Startet die Prüfung im Hintergrund. Findet sie eine neuere Version, erhält `hwnd` die
/// Nachricht `message`; den Zeitpunkt jeder Prüfung liefert danach [`last_check`].
pub fn start(hwnd: HWND, message: u32, enabled: bool, last_check: u64) {
    TARGET.store(hwnd.0 as isize, Relaxed);
    MESSAGE.store(message, Relaxed);
    ENABLED.store(enabled, Relaxed);
    LAST_CHECK.store(last_check, Relaxed);
    std::thread::spawn(|| {
        std::thread::sleep(STARTUP_DELAY);
        loop {
            let now = now();
            if ENABLED.load(Relaxed) && now >= LAST_CHECK.load(Relaxed) + INTERVAL {
                // Fehlschläge (kein Netz) zählen nicht als Prüfung; es geht in einer Stunde weiter.
                if let Some(tag) = latest_tag() {
                    LAST_CHECK.store(now, Relaxed);
                    let newer = is_newer(&tag, env!("CARGO_PKG_VERSION")).then(|| clean(&tag));
                    let found = newer.is_some();
                    *NEWER.lock().unwrap() = newer;
                    let hwnd = HWND(TARGET.load(Relaxed) as *mut _);
                    unsafe {
                        let _ = PostMessageW(Some(hwnd), MESSAGE.load(Relaxed), WPARAM(found as usize), LPARAM(0));
                    }
                }
            }
            std::thread::sleep(Duration::from_secs(60 * 60));
        }
    });
}

pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Relaxed);
}

pub fn last_check() -> u64 {
    LAST_CHECK.load(Relaxed)
}

/// Die neuere Version, falls eine gefunden wurde.
pub fn newer_version() -> Option<String> {
    NEWER.lock().unwrap().clone()
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn clean(tag: &str) -> String {
    tag.trim().trim_start_matches(['v', 'V']).to_string()
}

fn parse(version: &str) -> Option<(u32, u32, u32)> {
    let version = clean(version);
    // Vorabversionen wie "1.0.0-beta" werden nicht angeboten.
    if version.contains('-') {
        return None;
    }
    let mut parts = version.split('.').map(|part| part.parse::<u32>().ok());
    let major = parts.next()??;
    let minor = parts.next().unwrap_or(Some(0))?;
    let patch = parts.next().unwrap_or(Some(0))?;
    Some((major, minor, patch))
}

fn is_newer(candidate: &str, current: &str) -> bool {
    matches!((parse(candidate), parse(current)), (Some(a), Some(b)) if a > b)
}

/// Liest `"tag_name": "v0.2.0"` aus der Antwort, ohne einen JSON-Parser mitzuschleppen.
fn tag_from_json(json: &str) -> Option<String> {
    let rest = &json[json.find("\"tag_name\"")? + 10..];
    let rest = rest.trim_start().strip_prefix(':')?.trim_start().strip_prefix('"')?;
    Some(rest[..rest.find('"')?].to_string())
}

fn latest_tag() -> Option<String> {
    tag_from_json(&crate::http::get_text(API_URL, Some("application/vnd.github+json"))?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(is_newer("v1", "0.9.9"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("v0.0.9", "0.1.0"));
        assert!(!is_newer("v0.3.0-beta", "0.1.0"));
        assert!(!is_newer("Mist", "0.1.0"));
    }

    #[test]
    fn reads_tag_from_json() {
        let json = r#"{"url":"x","tag_name" : "v0.2.0","name":"Vorleser 0.2.0"}"#;
        assert_eq!(tag_from_json(json).as_deref(), Some("v0.2.0"));
        assert_eq!(tag_from_json(r#"{"message":"Not Found"}"#), None);
    }

    /// Probe von Hand gegen GitHub: `cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn fetches_latest_release() {
        let tag = latest_tag();
        println!("neueste Version auf GitHub: {tag:?}");
        assert!(tag.is_some());
    }
}
