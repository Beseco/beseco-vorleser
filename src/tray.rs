//! Symbol im Infobereich und Popup-Menü.

use std::mem::size_of;

use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_INFO, NIM_ADD, NIM_DELETE,
    NIM_MODIFY, NOTIFYICONDATAW, NOTIFY_ICON_DATA_FLAGS, NOTIFY_ICON_MESSAGE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, GetCursorPos, GetSystemMetrics, LoadIconW, PostMessageW,
    SetForegroundWindow, TrackPopupMenu, HICON, HMENU, IDI_APPLICATION, LR_DEFAULTCOLOR,
    SM_CXSMICON, SM_CYSMICON, TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WM_NULL,
};

static ICON_PNG: &[u8] = include_bytes!("../res/icon.png");

pub fn load_icon() -> HICON {
    unsafe {
        let (cx, cy) = (GetSystemMetrics(SM_CXSMICON), GetSystemMetrics(SM_CYSMICON));
        CreateIconFromResourceEx(ICON_PNG, true, 0x0003_0000, cx, cy, LR_DEFAULTCOLOR)
            .or_else(|_| LoadIconW(None, IDI_APPLICATION))
            .unwrap_or_default()
    }
}

fn data(hwnd: HWND, flags: NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: hwnd,
        uID: 1,
        uFlags: flags,
        ..Default::default()
    }
}

fn notify(message: NOTIFY_ICON_MESSAGE, nid: &NOTIFYICONDATAW) {
    unsafe {
        let _ = Shell_NotifyIconW(message, nid);
    }
}

/// Kopiert `text` nullterminiert in ein Feld fester Länge (gekürzt, falls nötig).
fn copy_to(buf: &mut [u16], text: &str) {
    let max = buf.len() - 1;
    let mut i = 0;
    for unit in text.encode_utf16().take(max) {
        buf[i] = unit;
        i += 1;
    }
    buf[i] = 0;
}

pub fn add(hwnd: HWND, icon: HICON, callback_msg: u32, tip: &str) {
    let mut nid = data(hwnd, NIF_ICON | NIF_MESSAGE | NIF_TIP);
    nid.hIcon = icon;
    nid.uCallbackMessage = callback_msg;
    copy_to(&mut nid.szTip, tip);
    notify(NIM_ADD, &nid);
}

pub fn set_tip(hwnd: HWND, tip: &str) {
    let mut nid = data(hwnd, NIF_TIP);
    copy_to(&mut nid.szTip, tip);
    notify(NIM_MODIFY, &nid);
}

/// Kennung der zuletzt gezeigten Meldung, damit ein Klick darauf zugeordnet werden kann.
static BALLOON_TAG: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Wie [`balloon`], merkt sich aber `tag` für [`take_balloon_tag`].
pub fn balloon_tagged(hwnd: HWND, title: &str, text: &str, tag: u32) {
    balloon(hwnd, title, text);
    BALLOON_TAG.store(tag, std::sync::atomic::Ordering::Relaxed);
}

/// Kennung der zuletzt gezeigten Meldung (0 = ohne) und setzt sie zurück.
pub fn take_balloon_tag() -> u32 {
    BALLOON_TAG.swap(0, std::sync::atomic::Ordering::Relaxed)
}

pub fn balloon(hwnd: HWND, title: &str, text: &str) {
    BALLOON_TAG.store(0, std::sync::atomic::Ordering::Relaxed);
    let mut nid = data(hwnd, NIF_INFO);
    copy_to(&mut nid.szInfoTitle, title);
    copy_to(&mut nid.szInfo, text);
    nid.dwInfoFlags = NIIF_INFO;
    notify(NIM_MODIFY, &nid);
}

pub fn remove(hwnd: HWND) {
    notify(NIM_DELETE, &data(hwnd, NOTIFY_ICON_DATA_FLAGS(0)));
}

/// Zeigt `menu` an der Mausposition und gibt die gewählte Befehls-ID zurück (0 = abgebrochen).
pub fn popup(hwnd: HWND, menu: HMENU) -> u32 {
    unsafe {
        let mut pt = Default::default();
        let _ = GetCursorPos(&mut pt);
        // Ohne Vordergrundfenster schließt sich das Menü nicht beim Klick daneben.
        let _ = SetForegroundWindow(hwnd);
        let flags = TPM_RIGHTBUTTON | TPM_RETURNCMD | TPM_NONOTIFY;
        let cmd = TrackPopupMenu(menu, flags, pt.x, pt.y, None, hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        cmd.0 as u32
    }
}
