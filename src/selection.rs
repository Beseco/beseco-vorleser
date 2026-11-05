//! Markierten Text der aktiven Anwendung ermitteln: zuerst über UI Automation,
//! sonst über Strg+C und die Zwischenablage (die danach wiederhergestellt wird).

use std::mem::size_of;
use std::thread::sleep;
use std::time::{Duration, Instant};

use windows::core::{w, Result};
use windows::Win32::Foundation::{HANDLE, HGLOBAL};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData,
    GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation, IUIAutomation, IUIAutomationTextPattern, UIA_TextPatternId,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
    KEYEVENTF_KEYUP, VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};

const CF_UNICODETEXT: u32 = 13;

pub fn selected_text() -> Option<String> {
    if let Some(text) = via_ui_automation() {
        unsafe { OutputDebugStringW(w!("Vorleser: Markierung über UI Automation")) };
        return Some(text);
    }
    let text = via_copy();
    if text.is_some() {
        unsafe { OutputDebugStringW(w!("Vorleser: Markierung über Zwischenablage")) };
    }
    text
}

/// Ab so vielen Wörtern wird die Zwischenablage ersatzweise vorgelesen. Kürzeres (Passwörter,
/// Codes, Kundennummern) bleibt still.
const CLIPBOARD_MIN_WORDS: usize = 6;

/// Wie [`selected_text`]; ist nichts markiert, der Text aus der Zwischenablage, sofern er nach
/// Fließtext aussieht und kein Passwortmanager ihn als vertraulich gekennzeichnet hat.
pub fn selected_or_clipboard_text() -> Option<String> {
    if let Some(text) = selected_text() {
        return Some(text);
    }
    let text = clipboard_text()?;
    if !enough_words(&text) || clipboard_is_private() {
        return None;
    }
    unsafe { OutputDebugStringW(w!("Vorleser: nichts markiert, lese die Zwischenablage")) };
    Some(text)
}

fn enough_words(text: &str) -> bool {
    text.split_whitespace().count() >= CLIPBOARD_MIN_WORDS
}

/// Kennzeichen, mit denen Passwortmanager (z. B. KeePass) Kopiertes aus dem Zwischenablageverlauf
/// heraushalten. "ExcludeClipboardContentFromMonitorProcessing" setzt auch [`restore`]; dort kommt
/// dann zusätzlich [`RESTORED_MARKER`] dazu, damit wiederhergestellter Text vorlesbar bleibt.
fn clipboard_is_private() -> bool {
    unsafe {
        let format = |name: &str| RegisterClipboardFormatW(&windows::core::HSTRING::from(name));
        let available = |name: &str| IsClipboardFormatAvailable(format(name)).is_ok();
        if available("Clipboard Viewer Ignore") {
            return true;
        }
        if available(EXCLUDE_FORMAT) && !available(RESTORED_MARKER) {
            return true;
        }
        // Ein DWORD 0 heißt "nicht in den Verlauf" bzw. "nicht in die Cloud".
        ["CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"]
            .iter()
            .any(|name| available(name) && clipboard_dword(format(name)) == Some(0))
    }
}

/// Eigenes Format, das [`restore`] setzt: Der Inhalt war vorher schon da und ist nicht vertraulich.
const RESTORED_MARKER: &str = "Beseco.Vorleser.Restored";
const EXCLUDE_FORMAT: &str = "ExcludeClipboardContentFromMonitorProcessing";

fn clipboard_dword(format: u32) -> Option<u32> {
    let _clipboard = Clipboard::open()?;
    unsafe {
        let handle = GetClipboardData(format).ok()?;
        let global = HGLOBAL(handle.0);
        if GlobalSize(global) < 4 {
            return None;
        }
        let ptr = GlobalLock(global) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let value = u32::from_le_bytes(std::slice::from_raw_parts(ptr, 4).try_into().ok()?);
        let _ = GlobalUnlock(global);
        Some(value)
    }
}

pub fn clipboard_text() -> Option<String> {
    let _clipboard = Clipboard::open()?;
    unsafe {
        let handle = GetClipboardData(CF_UNICODETEXT).ok()?;
        let global = HGLOBAL(handle.0);
        let ptr = GlobalLock(global) as *const u16;
        if ptr.is_null() {
            return None;
        }
        let mut len = 0;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len));
        let _ = GlobalUnlock(global);
        non_blank(text)
    }
}

/// Inhalt der Zwischenablage: jedes Format mit seinen Rohdaten.
struct Snapshot(Vec<(u32, Vec<u8>)>);

/// Mehr wird nicht gesichert (z. B. riesige Bilder); solche Formate gehen dann verloren.
const SNAPSHOT_LIMIT: usize = 64 << 20;

/// Bitmaps, Metadateien, Paletten, private und GDI-Formate sind Handles statt Speicherblöcke.
/// Das Bitmap erzeugt Windows aus dem mitgesicherten CF_DIB neu.
fn is_memory_format(format: u32) -> bool {
    !matches!(format, 2 | 3 | 9 | 14 | 0x80..=0x8E | 0x200..=0x3FF)
}

fn snapshot() -> Option<Snapshot> {
    let _clipboard = Clipboard::open()?;
    let mut items = Vec::new();
    let mut total = 0;
    let mut format = 0;
    unsafe {
        loop {
            format = EnumClipboardFormats(format);
            if format == 0 {
                break;
            }
            if !is_memory_format(format) {
                continue;
            }
            let Ok(handle) = GetClipboardData(format) else { continue };
            let global = HGLOBAL(handle.0);
            let size = GlobalSize(global);
            if size == 0 || total + size > SNAPSHOT_LIMIT {
                continue;
            }
            let ptr = GlobalLock(global) as *const u8;
            if ptr.is_null() {
                continue;
            }
            items.push((format, std::slice::from_raw_parts(ptr, size).to_vec()));
            total += size;
            let _ = GlobalUnlock(global);
        }
    }
    Some(Snapshot(items))
}

fn restore(snapshot: &Snapshot) {
    let Some(_clipboard) = Clipboard::open() else { return };
    unsafe {
        let _ = EmptyClipboard();
        for (format, bytes) in &snapshot.0 {
            put(*format, bytes);
        }
        // Sonst stünde der alte Inhalt ein zweites Mal im Zwischenablageverlauf (Win+V). Hatte
        // ihn ein Passwortmanager schon so gekennzeichnet, bleibt er vertraulich; sonst zeigt
        // RESTORED_MARKER, dass das Kennzeichen nur von hier stammt.
        let exclude = RegisterClipboardFormatW(&windows::core::HSTRING::from(EXCLUDE_FORMAT));
        if !snapshot.0.iter().any(|(format, _)| *format == exclude) {
            put(exclude, &[0; 4]);
            put(RegisterClipboardFormatW(&windows::core::HSTRING::from(RESTORED_MARKER)), &[0; 4]);
        }
    }
}

/// Legt `bytes` im Format `format` in die bereits geöffnete Zwischenablage.
unsafe fn put(format: u32, bytes: &[u8]) {
    let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, bytes.len()) else { return };
    let ptr = GlobalLock(global) as *mut u8;
    if ptr.is_null() {
        return;
    }
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
    let _ = GlobalUnlock(global);
    // Bei Erfolg gehört der Speicher der Zwischenablage.
    let _ = SetClipboardData(format, Some(HANDLE(global.0)));
}

/// Offene Zwischenablage; wird beim Verlassen des Gültigkeitsbereichs geschlossen.
struct Clipboard;

impl Clipboard {
    fn open() -> Option<Clipboard> {
        // Andere Programme halten die Zwischenablage manchmal kurz offen.
        for _ in 0..10 {
            if unsafe { OpenClipboard(None).is_ok() } {
                return Some(Clipboard);
            }
            sleep(Duration::from_millis(10));
        }
        None
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn via_ui_automation() -> Option<String> {
    unsafe {
        let automation: IUIAutomation =
            CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).ok()?;
        let walker = automation.ControlViewWalker().ok()?;
        let mut element = automation.GetFocusedElement().ok()?;
        // Nicht immer bietet das fokussierte Element selbst das Textmuster an, sondern ein Elternelement.
        for _ in 0..6 {
            let pattern: Result<IUIAutomationTextPattern> =
                element.GetCurrentPatternAs(UIA_TextPatternId);
            if let Ok(pattern) = pattern {
                let ranges = pattern.GetSelection().ok()?;
                let mut text = String::new();
                for i in 0..ranges.Length().ok()? {
                    if let Ok(part) = ranges.GetElement(i).and_then(|r| r.GetText(-1)) {
                        text.push_str(&part.to_string());
                    }
                }
                return non_blank(text);
            }
            element = walker.GetParentElement(&element).ok()?;
        }
        None
    }
}

fn via_copy() -> Option<String> {
    // Die Tasten des auslösenden Hotkeys sind evtl. noch gedrückt; sonst käme z. B. Strg+Alt+C an.
    let deadline = Instant::now() + Duration::from_millis(1000);
    while modifier_down() {
        if Instant::now() > deadline {
            return None;
        }
        sleep(Duration::from_millis(10));
    }

    let saved = snapshot();
    let before = unsafe { GetClipboardSequenceNumber() };
    send_ctrl_c();

    let deadline = Instant::now() + Duration::from_millis(400);
    while unsafe { GetClipboardSequenceNumber() } == before {
        if Instant::now() > deadline {
            return None; // nichts markiert oder Kopieren nicht möglich
        }
        sleep(Duration::from_millis(10));
    }
    // Manche Programme legen ihre Formate nacheinander ab.
    sleep(Duration::from_millis(40));
    let text = clipboard_text();

    // Den kopierten Text selbst trägt die Anwendung in den Verlauf ein; das lässt sich von
    // außen nicht verhindern. Der vorherige Inhalt steht danach wieder in der Zwischenablage.
    if let Some(saved) = saved {
        restore(&saved);
    }
    text
}

fn modifier_down() -> bool {
    [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN]
        .iter()
        .any(|vk| unsafe { GetAsyncKeyState(vk.0 as i32) } < 0)
}

fn send_ctrl_c() {
    let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    };
    let c = VIRTUAL_KEY(b'C' as u16);
    let inputs = [key(VK_CONTROL, false), key(c, false), key(c, true), key(VK_CONTROL, true)];
    unsafe {
        SendInput(&inputs, size_of::<INPUT>() as i32);
    }
}

fn non_blank(text: String) -> Option<String> {
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_clipboard_only_from_six_words() {
        assert!(!enough_words(""));
        assert!(!enough_words("Geheim123!"));
        assert!(!enough_words("ein zwei drei vier fünf"));
        assert!(!enough_words("  ein\tzwei\ndrei  vier fünf  "));
        assert!(enough_words("Das ist ein ganz normaler Satz."));
        assert!(enough_words("eins\nzwei\ndrei\nvier\nfünf\nsechs"));
    }
}
