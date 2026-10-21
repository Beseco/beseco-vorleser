//! Diktat in fremde Programme: tippt Text an der Schreibmarke des Programms im Vordergrund, als
//! käme er von der Tastatur (Unicode-Tastenanschläge, ohne die Zwischenablage zu benutzen).

use std::path::Path;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
    KEYEVENTF_UNICODE, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};

fn key(vk: VIRTUAL_KEY, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: scan, dwFlags: flags, time: 0, dwExtraInfo: 0 } },
    }
}

/// Wartet (höchstens eine Sekunde), bis Strg, Alt, Umschalt und Windows-Taste losgelassen sind;
/// sonst würden aus Buchstaben Tastenkürzel.
fn wait_for_modifiers() {
    let start = Instant::now();
    let held = || [VK_CONTROL, VK_MENU, VK_SHIFT, VK_LWIN, VK_RWIN].iter().any(|k| unsafe { GetAsyncKeyState(k.0 as i32) } < 0);
    while held() && start.elapsed() < Duration::from_secs(1) {
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// Tippt `text` in das Programm im Vordergrund.
pub fn type_text(text: &str) {
    wait_for_modifiers();
    let mut inputs = Vec::new();
    for unit in text.encode_utf16() {
        // Zeilenumbrüche als Eingabetaste, damit sie auch in Programmen ankommen, die U+000A
        // ignorieren.
        if unit == b'\n' as u16 {
            inputs.push(key(VIRTUAL_KEY(0x0D), 0, KEYBD_EVENT_FLAGS(0)));
            inputs.push(key(VIRTUAL_KEY(0x0D), 0, KEYEVENTF_KEYUP));
            continue;
        }
        if unit == b'\r' as u16 {
            continue;
        }
        inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE));
        inputs.push(key(VIRTUAL_KEY(0), unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP));
    }
    send(&inputs);
}

/// Löscht `count` Zeichen links der Schreibmarke.
pub fn backspace(count: usize) {
    wait_for_modifiers();
    let mut inputs = Vec::with_capacity(count * 2);
    for _ in 0..count {
        inputs.push(key(VK_BACK, 0, KEYBD_EVENT_FLAGS(0)));
        inputs.push(key(VK_BACK, 0, KEYEVENTF_KEYUP));
    }
    send(&inputs);
}

fn send(inputs: &[INPUT]) {
    // In Paketen, damit langsame Programme nichts verschlucken.
    // In kleinen Paketen (20 Zeichen), damit empfindliche Programme nichts verschlucken.
    for chunk in inputs.chunks(40) {
        unsafe {
            SendInput(chunk, std::mem::size_of::<INPUT>() as i32);
        }
        std::thread::sleep(Duration::from_millis(8));
    }
}

pub fn foreground() -> HWND {
    unsafe { GetForegroundWindow() }
}

/// Anzeigename des Programms hinter `hwnd`, z. B. „Outlook“ für OUTLOOK.EXE.
pub fn program_name(hwnd: HWND) -> String {
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    let path = unsafe {
        let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return String::new() };
        let mut buffer = [0u16; 512];
        let mut len = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, windows::core::PWSTR(buffer.as_mut_ptr()), &mut len).is_ok();
        let _ = CloseHandle(process);
        if !ok {
            return String::new();
        }
        String::from_utf16_lossy(&buffer[..len as usize])
    };
    friendly_name(&path)
}

fn friendly_name(path: &str) -> String {
    let stem = Path::new(path).file_stem().and_then(|s| s.to_str()).unwrap_or_default();
    let known = [
        ("winword", "Word"),
        ("outlook", "Outlook"),
        ("olk", "Outlook"),
        ("excel", "Excel"),
        ("powerpnt", "PowerPoint"),
        ("onenote", "OneNote"),
        ("msedge", "Edge"),
        ("chrome", "Chrome"),
        ("firefox", "Firefox"),
        ("notepad", "Editor"),
        ("ms-teams", "Teams"),
        ("teams", "Teams"),
        ("thunderbird", "Thunderbird"),
        ("soffice", "LibreOffice"),
        ("code", "VS Code"),
    ];
    let lower = stem.to_lowercase();
    if let Some((_, name)) = known.iter().find(|(exe, _)| *exe == lower) {
        return name.to_string();
    }
    let mut chars = stem.chars();
    chars.next().map(|c| c.to_uppercase().collect::<String>() + chars.as_str()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_programs() {
        assert_eq!(friendly_name(r"C:\Program Files\Microsoft Office\root\Office16\OUTLOOK.EXE"), "Outlook");
        assert_eq!(friendly_name(r"C:\Windows\notepad.exe"), "Editor");
        assert_eq!(friendly_name(r"C:\Tools\myapp.exe"), "Myapp");
    }
}
