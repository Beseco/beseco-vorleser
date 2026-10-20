//! Sucht höchstens einmal am Tag auf GitHub nach einer neueren Version. Abschaltbar; gesendet
//! wird nur eine gewöhnliche Anfrage ohne Text, Einstellungen oder Kennung des Rechners.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

pub const RELEASES_URL: &str = "https://github.com/Beseco/beseco-vorleser/releases/latest";
const HOST: PCWSTR = w!("api.github.com");
const PATH: PCWSTR = w!("/repos/Beseco/beseco-vorleser/releases/latest");
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
    let agent = HSTRING::from(format!("Vorleser/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = Handle(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return None;
        }
        let _ = WinHttpSetTimeouts(session.0, 10_000, 10_000, 10_000, 10_000);
        let connection = Handle(WinHttpConnect(session.0, HOST, INTERNET_DEFAULT_HTTPS_PORT, 0));
        if connection.0.is_null() {
            return None;
        }
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            PATH,
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return None;
        }
        let headers: Vec<u16> = "Accept: application/vnd.github+json".encode_utf16().collect();
        WinHttpSendRequest(request.0, Some(&headers), None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;

        let mut status = 0u32;
        let mut size = 4u32;
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut status as *mut u32 as *mut _),
            &mut size,
            std::ptr::null_mut(),
        )
        .ok()?;
        if status != 200 {
            return None;
        }

        let mut body = Vec::new();
        let mut chunk = [0u8; 8192];
        loop {
            let mut read = 0;
            WinHttpReadData(request.0, chunk.as_mut_ptr() as *mut _, chunk.len() as u32, &mut read).ok()?;
            if read == 0 || body.len() > 1 << 20 {
                break;
            }
            body.extend_from_slice(&chunk[..read as usize]);
        }
        tag_from_json(&String::from_utf8_lossy(&body))
    }
}

/// Schließt ein WinHTTP-Handle beim Verlassen des Gültigkeitsbereichs.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                let _ = WinHttpCloseHandle(self.0);
            }
        }
    }
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
