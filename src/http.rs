//! HTTPS-Abrufe über WinHTTP (Teil von Windows): Update-Suche und Download der KI-Stimmen.
//! Weiterleitungen, z. B. von Hugging Face zum Speicherdienst, folgt WinHTTP selbst.

use std::fs::File;
use std::io::Write;
use std::path::Path;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Networking::WinHttp::{
    WinHttpCloseHandle, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders,
    WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest, WinHttpSetTimeouts,
    INTERNET_DEFAULT_HTTPS_PORT, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE,
    WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
};

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

fn open(url: &str, accept: Option<&str>) -> Option<Response> {
    let rest = url.strip_prefix("https://")?;
    let (host, path) = rest.split_at(rest.find('/')?);
    let agent = HSTRING::from(format!("Vorleser/{}", env!("CARGO_PKG_VERSION")));
    unsafe {
        let session = Handle(WinHttpOpen(&agent, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, PCWSTR::null(), PCWSTR::null(), 0));
        if session.0.is_null() {
            return None;
        }
        // Auflösen, Verbinden, Senden: 15 s; Empfangen eines Datenblocks: 60 s.
        let _ = WinHttpSetTimeouts(session.0, 15_000, 15_000, 15_000, 60_000);
        let connection = Handle(WinHttpConnect(session.0, &HSTRING::from(host), INTERNET_DEFAULT_HTTPS_PORT, 0));
        if connection.0.is_null() {
            return None;
        }
        let request = Handle(WinHttpOpenRequest(
            connection.0,
            w!("GET"),
            &HSTRING::from(path),
            PCWSTR::null(),
            PCWSTR::null(),
            std::ptr::null(),
            WINHTTP_FLAG_SECURE,
        ));
        if request.0.is_null() {
            return None;
        }
        let headers: Option<Vec<u16>> = accept.map(|a| format!("Accept: {a}").encode_utf16().collect());
        WinHttpSendRequest(request.0, headers.as_deref(), None, 0, 0, 0).ok()?;
        WinHttpReceiveResponse(request.0, std::ptr::null_mut()).ok()?;
        if query_number(&request, WINHTTP_QUERY_STATUS_CODE)? != 200 {
            return None;
        }
        Some(Response { request, _connection: connection, _session: session })
    }
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
    let response = open(url, accept)?;
    let mut body = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = read(&response, &mut chunk)?;
        if n == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..n]);
        if body.len() > 1 << 20 {
            return None;
        }
    }
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// Lädt `url` nach `target`. `progress` erhält gelesene und erwartete Bytes (0, wenn unbekannt)
/// und bricht mit `false` ab. Bei einem Fehler bleibt keine halbe Datei liegen.
pub fn download(url: &str, target: &Path, mut progress: impl FnMut(u64, u64) -> bool) -> Result<(), String> {
    let response = open(url, None).ok_or("Server nicht erreichbar oder Datei nicht gefunden")?;
    let total = query_number(&response.request, WINHTTP_QUERY_CONTENT_LENGTH).unwrap_or(0);
    let result = (|| {
        let mut file = File::create(target).map_err(|e| e.to_string())?;
        let mut chunk = vec![0u8; 256 * 1024];
        let mut done = 0u64;
        loop {
            let n = read(&response, &mut chunk).ok_or("Verbindung abgebrochen")?;
            if n == 0 {
                break;
            }
            file.write_all(&chunk[..n]).map_err(|e| e.to_string())?;
            done += n as u64;
            if !progress(done, total) {
                return Err("abgebrochen".to_string());
            }
        }
        if total != 0 && done != total {
            return Err("Download unvollständig".to_string());
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(target);
    }
    result
}
