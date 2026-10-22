//! HTTPS-Abrufe über WinHTTP (Teil von Windows): Update-Suche und Download der KI-Stimmen.
//! Weiterleitungen, z. B. von Hugging Face zum Speicherdienst, folgt WinHTTP selbst.
//!
//! Proxy: Standard sind die Einstellungen von Windows, einschließlich automatischer Erkennung
//! und Konfigurationsskript (`wpad.dat`/PAC). In den Einstellungen lässt sich stattdessen ein
//! Skript (`http://…/wpad.dat`) oder ein fester Proxy (`proxy:8080`) eintragen. Verlangt der
//! Proxy eine Anmeldung, meldet sich Vorleser mit dem Windows-Konto an (Negotiate/NTLM).

use std::fs::File;
use std::io::Write;
use std::path::Path;
use std::sync::Mutex;

use windows::core::{w, HSTRING, PCWSTR};

use crate::i18n::t;
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpGetProxyForUrl, WinHttpOpen, WinHttpOpenRequest,
    WinHttpQueryAuthSchemes, WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
    WinHttpSendRequest, WinHttpSetCredentials, WinHttpSetOption, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_NAMED_PROXY,
    WINHTTP_ACCESS_TYPE_NO_PROXY, WINHTTP_AUTH_SCHEME_NEGOTIATE, WINHTTP_AUTH_SCHEME_NTLM,
    WINHTTP_AUTH_TARGET_PROXY, WINHTTP_AUTOLOGON_SECURITY_LEVEL_LOW, WINHTTP_AUTOPROXY_CONFIG_URL,
    WINHTTP_AUTOPROXY_OPTIONS, WINHTTP_FLAG_SECURE, WINHTTP_OPTION_AUTOLOGON_POLICY,
    WINHTTP_OPTION_PROXY, WINHTTP_PROXY_INFO, WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER,
    WINHTTP_QUERY_STATUS_CODE,
};
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};

/// Proxy-Einstellung: leer = wie Windows; sonst Skript-Adresse oder `host:port`.
static PROXY: Mutex<String> = Mutex::new(String::new());

pub fn set_proxy(setting: &str) {
    *PROXY.lock().unwrap() = setting.trim().to_string();
}

enum Proxy {
    Windows,
    Script(String),
    Fixed(String),
}

fn proxy() -> Proxy {
    let setting = PROXY.lock().unwrap().clone();
    let lower = setting.to_lowercase();
    if setting.is_empty() {
        Proxy::Windows
    } else if lower.starts_with("http://") || lower.starts_with("https://") || lower.ends_with(".dat") || lower.ends_with(".pac") {
        Proxy::Script(setting)
    } else {
        Proxy::Fixed(setting.trim_start_matches("http://").trim_end_matches('/').to_string())
    }
}

/// Wendet das Ergebnis eines Proxy-Skripts auf die Anfrage an.
unsafe fn apply_script(session: &Handle, request: &Handle, script: &str, url: &str) -> Result<(), String> {
    let script = HSTRING::from(script);
    let mut options = WINHTTP_AUTOPROXY_OPTIONS {
        dwFlags: WINHTTP_AUTOPROXY_CONFIG_URL,
        lpszAutoConfigUrl: PCWSTR(script.as_ptr()),
        fAutoLogonIfChallenged: true.into(),
        ..Default::default()
    };
    let mut info = WINHTTP_PROXY_INFO::default();
    WinHttpGetProxyForUrl(session.0, &HSTRING::from(url), &mut options, &mut info)
        .map_err(|e| format!("{} ({})", t("err.proxy_script"), e.message()))?;
    let bytes = std::slice::from_raw_parts(&info as *const _ as *const u8, std::mem::size_of::<WINHTTP_PROXY_INFO>());
    let result = WinHttpSetOption(Some(request.0), WINHTTP_OPTION_PROXY, Some(bytes)).map_err(|e| e.message());
    for text in [info.lpszProxy, info.lpszProxyBypass] {
        if !text.is_null() {
            let _ = GlobalFree(Some(HGLOBAL(text.0 as *mut _)));
        }
    }
    result
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

/// Eine offene Antwort mit Status 200. Die drei Handles müssen gemeinsam leben.
struct Response {
    request: Handle,
    _connection: Handle,
    _session: Handle,
}

fn query_number(request: &Handle, info: u32) -> Option<u64> {
    let mut value = 0u32;
    let mut size = 4u32;
    unsafe {
        WinHttpQueryHeaders(
            request.0,
            info | WINHTTP_QUERY_FLAG_NUMBER,
            PCWSTR::null(),
            Some(&mut value as *mut u32 as *mut _),
            &mut size,
            std::ptr::null_mut(),
        )
        .ok()?;
    }
    Some(value as u64)
}

/// Eine HTTP-Anfrage. `http://` ist für Server im eigenen Netz gedacht (z. B. Ollama).
pub struct Request<'a> {
    pub method: &'a str,
    pub url: &'a str,
    /// Zusätzliche Kopfzeilen, z. B. `Authorization: Bearer …`.
    pub headers: Vec<String>,
    pub body: Option<&'a [u8]>,
    /// Höchste Wartezeit auf Daten in Millisekunden (KI-Antworten können dauern).
    pub timeout: i32,
}

impl<'a> Request<'a> {
    pub fn get(url: &'a str) -> Request<'a> {
        Request { method: "GET", url, headers: Vec::new(), body: None, timeout: 60_000 }
    }
}

/// Zerlegt eine Adresse in (https?, Rechner, Port, Pfad).
fn split_url(url: &str) -> Option<(bool, String, u16, String)> {
    let (secure, rest) = if let Some(rest) = url.strip_prefix("https://") {
        (true, rest)
    } else {
        (false, url.strip_prefix("http://")?)
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => rest.split_at(i),
        None => (rest, "/"),
    };
    let default = if secure { INTERNET_DEFAULT_HTTPS_PORT } else { 80 };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() && port.chars().all(|c| c.is_ascii_digit()) => (host, port.parse().ok()?),
        _ => (authority, default),
    };
    if host.is_empty() {
        return None;
    }
    Some((secure, host.to_string(), port, path.to_string()))
}

/// Schickt eine Anfrage und liefert die Antwort samt Status, ohne ihn zu bewerten.
fn send(request: &Request) -> Result<(Response, u16), String> {
    let server = || t("err.server").to_string();
    let (secure, host, port, path) = split_url(request.url).ok_or_else(server)?;
    let agent = HSTRING::from(format!("Vorleser/{}", env!("CARGO_PKG_VERSION")));
    let proxy = proxy();
    unsafe {
        let session = match &proxy {
            Proxy::Windows => WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            Proxy::Script(_) => WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_NO_PROXY, PCWSTR::null(), PCWSTR::null(), 0),
            Proxy::Fixed(address) => {
                WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_NAMED_PROXY, &HSTRING::from(address.as_str()), w!("<local>"), 0)
            }
        };
        let session = Handle(session);
        if session.0.is_null() {
            return Err(server());
        }
        // Auflösen, Verbinden, Senden: 15 s; Warten auf Daten: je nach Anfrage.
        let _ = WinHttpSetTimeouts(session.0, 15_000, 15_000, 30_000, request.timeout);
        let connection = Handle(WinHttpConnect(session.0, &HSTRING::from(host.as_str()), port, 0));
        if connection.0.is_null() {
            return Err(server());
        }
        let handle = Handle(WinHttpOpenRequest(
            connection.0,
            &HSTRING::from(request.method),
            &HSTRING::from(path.as_str()),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            if secure { WINHTTP_FLAG_SECURE } else { Default::default() },
        ));
        if handle.0.is_null() {
            return Err(server());
        }
        if let Proxy::Script(script) = &proxy {
            apply_script(&session, &handle, script, request.url)?;
        }
        // Erlaubt die Anmeldung mit dem Windows-Konto auch an Proxys außerhalb des Intranets.
        let policy = WINHTTP_AUTOLOGON_SECURITY_LEVEL_LOW.to_ne_bytes();
        let _ = WinHttpSetOption(Some(handle.0), WINHTTP_OPTION_AUTOLOGON_POLICY, Some(&policy));

        let headers: Vec<u16> = request.headers.join("\r\n").encode_utf16().collect();
        let headers = (!headers.is_empty()).then_some(headers);
        let (body, length) = match request.body {
            Some(body) => (Some(body.as_ptr() as *const core::ffi::c_void), body.len() as u32),
            None => (None, 0),
        };
        let mut authenticated = false;
        loop {
            WinHttpSendRequest(handle.0, headers.as_deref(), body, length, length, 0).map_err(|_| server())?;
            WinHttpReceiveResponse(handle.0, std::ptr::null_mut()).map_err(|e| {
                // Zeitüberschreitung beim Warten auf die Antwort deutlich benennen.
                if e.code().0 as u32 & 0xFFFF == 12002 { t("err.timeout").to_string() } else { server() }
            })?;
            match query_number(&handle, WINHTTP_QUERY_STATUS_CODE).map(|s| s as u16) {
                // Der Proxy verlangt eine Anmeldung: einmal mit dem Windows-Konto versuchen.
                Some(407) if !authenticated => {
                    authenticated = true;
                    let (mut supported, mut first, mut target) = (0, 0, 0);
                    WinHttpQueryAuthSchemes(handle.0, &mut supported, &mut first, &mut target)
                        .map_err(|_| t("err.proxy_auth").to_string())?;
                    let scheme = [WINHTTP_AUTH_SCHEME_NEGOTIATE.0, WINHTTP_AUTH_SCHEME_NTLM.0]
                        .into_iter()
                        .find(|scheme| supported & scheme != 0)
                        .ok_or_else(|| t("err.proxy_auth").to_string())?;
                    WinHttpSetCredentials(handle.0, WINHTTP_AUTH_TARGET_PROXY, scheme, PCWSTR::null(), PCWSTR::null(), std::ptr::null_mut())
                        .map_err(|_| t("err.proxy_auth").to_string())?;
                }
                Some(407) => return Err(t("err.proxy_auth").to_string()),
                Some(status) => return Ok((Response { request: handle, _connection: connection, _session: session }, status)),
                None => return Err(server()),
            }
        }
    }
}

/// Wie [`send`], verlangt aber Status 200.
fn open(url: &str, accept: Option<&str>) -> Result<Response, String> {
    let mut request = Request::get(url);
    request.headers.extend(accept.map(|a| format!("Accept: {a}")));
    match send(&request)? {
        (response, 200) => Ok(response),
        (_, status) => Err(format!("{} (HTTP {status})", t("err.server"))),
    }
}

fn read_all(response: &Response, limit: usize) -> Option<Vec<u8>> {
    let mut body = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        let n = read(response, &mut chunk)?;
        if n == 0 {
            return Some(body);
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > limit {
            return None;
        }
    }
}

/// Schickt eine Anfrage und liefert Status und Text der Antwort (höchstens 8 MB).
pub fn fetch(request: &Request) -> Result<(u16, String), String> {
    let (response, status) = send(request)?;
    let body = read_all(&response, 8 << 20).ok_or_else(|| t("err.connection_lost").to_string())?;
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

/// Schickt eine Anfrage und gibt die Antwort zeilenweise an `on_line`, sobald sie eintrifft (z. B.
/// den Fortschritt von Ollama). `on_line` bricht mit `false` ab. Liefert den Status.
pub fn stream(request: &Request, mut on_line: impl FnMut(&str) -> bool) -> Result<u16, String> {
    let (response, status) = send(request)?;
    let mut pending = Vec::new();
    let mut chunk = [0u8; 16384];
    loop {
        // Nur lesen, was schon da ist: sonst wartet Windows, bis der Puffer voll ist, und die
        // Stücke kämen erst am Ende an.
        let mut available = 0u32;
        unsafe { WinHttpQueryDataAvailable(response.request.0, &mut available) }.map_err(|_| t("err.connection_lost").to_string())?;
        if available == 0 {
            break;
        }
        let want = (available as usize).min(chunk.len());
        let n = read(&response, &mut chunk[..want]).ok_or_else(|| t("err.connection_lost").to_string())?;
        if n == 0 {
            break;
        }
        pending.extend_from_slice(&chunk[..n]);
        while let Some(end) = pending.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = pending.drain(..=end).collect();
            if !on_line(String::from_utf8_lossy(&line).trim()) {
                return Ok(status);
            }
        }
    }
    if !pending.is_empty() {
        on_line(String::from_utf8_lossy(&pending).trim());
    }
    Ok(status)
}

/// Liest den nächsten Block; `Some(0)` am Ende, `None` bei einem Fehler.
fn read(response: &Response, buffer: &mut [u8]) -> Option<usize> {
    let mut read = 0;
    unsafe {
        WinHttpReadData(response.request.0, buffer.as_mut_ptr() as *mut _, buffer.len() as u32, &mut read).ok()?;
    }
    Some(read as usize)
}

/// Lädt eine kleine Antwort (höchstens 1 MB) als Text.
pub fn get_text(url: &str, accept: Option<&str>) -> Option<String> {
    let response = open(url, accept).ok()?;
    let body = read_all(&response, 1 << 20)?;
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// Lädt `url` nach `target`. `progress` erhält gelesene und erwartete Bytes (0, wenn unbekannt)
/// und bricht mit `false` ab. Bei einem Fehler bleibt keine halbe Datei liegen.
pub fn download(url: &str, target: &Path, mut progress: impl FnMut(u64, u64) -> bool) -> Result<(), String> {
    let response = open(url, None)?;
    let total = query_number(&response.request, WINHTTP_QUERY_CONTENT_LENGTH).unwrap_or(0);
    let result = (|| {
        let mut file = File::create(target).map_err(|e| e.to_string())?;
        let mut chunk = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let n = read(&response, &mut chunk).ok_or_else(|| t("err.connection_lost").to_string())?;
            if n == 0 {
                break;
            }
            file.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if !progress(done, total) {
                return Err(t("err.cancelled").to_string());
            }
        }
        if total != 0 && done != total {
            return Err(t("err.incomplete").to_string());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_urls() {
        assert_eq!(split_url("https://api.example.com/v1/models"), Some((true, "api.example.com".into(), 443, "/v1/models".into())));
        assert_eq!(split_url("http://ki-server:11434/v1"), Some((false, "ki-server".into(), 11434, "/v1".into())));
        assert_eq!(split_url("http://10.0.0.5"), Some((false, "10.0.0.5".into(), 80, "/".into())));
        assert_eq!(split_url("ftp://x"), None);
        assert_eq!(split_url("https://"), None);
    }

    #[test]
    fn recognizes_proxy_settings() {
        for (setting, expected) in [
            ("", "windows"),
            ("http://wpad.firma.local/wpad.dat", "script"),
            ("\\\\server\\proxy.pac", "script"),
            ("proxy.firma.local:8080", "fixed"),
            ("http://proxy:3128/", "script"),
        ] {
            set_proxy(setting);
            let kind = match proxy() {
                Proxy::Windows => "windows",
                Proxy::Script(_) => "script",
                Proxy::Fixed(_) => "fixed",
            };
            assert_eq!(kind, expected, "{setting}");
        }
        set_proxy("");
    }

    /// Probe von Hand über einen Proxy: `VORLESER_TEST_PROXY=http://127.0.0.1:8765/wpad.dat`
    /// (oder `127.0.0.1:8888`) setzen, dann `cargo test via_proxy -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn via_proxy() {
        set_proxy(&std::env::var("VORLESER_TEST_PROXY").unwrap_or_default());
        let body = get_text("https://api.github.com/repos/Beseco/beseco-vorleser/releases/latest", None);
        println!("Antwort: {:?}", body.as_deref().map(|b| &b[..b.len().min(80)]));
        assert!(body.is_some_and(|b| b.contains("tag_name")));
    }
}
