//! Ersatz für einen Kontextmenü-Eintrag: Rechtsklick mit gedrückter Zusatztaste (z. B. Strg)
//! wird abgefangen und dem Hauptfenster gemeldet, das dann das Vorleser-Menü zeigt.
//!
//! Der Maus-Hook läuft im eigenen Prozess auf einem eigenen Thread. Windows wartet bei jedem
//! Mausereignis auf den Hook; auf dem Hauptthread würde die Maus stocken, sobald dieser gerade
//! die Markierung ermittelt.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering::Relaxed};
use std::sync::Once;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetMessageW, PostMessageW, SetWindowsHookExW, HC_ACTION, MSG, WH_MOUSE_LL,
    WM_RBUTTONDOWN, WM_RBUTTONUP,
};

use crate::config::{MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};

static TARGET: AtomicIsize = AtomicIsize::new(0);
static MESSAGE: AtomicU32 = AtomicU32::new(0);
static MODIFIERS: AtomicU32 = AtomicU32::new(0);
/// Der zugehörige Tastendruck wurde abgefangen, also auch das Loslassen abfangen.
static SWALLOW_UP: AtomicBool = AtomicBool::new(false);

/// Sendet `message` an `hwnd`, sobald mit genau den Zusatztasten `modifiers` rechts geklickt wird.
/// `modifiers == 0` schaltet den Auslöser ab. Kann wiederholt aufgerufen werden.
pub fn set(hwnd: HWND, message: u32, modifiers: u32) {
    static INSTALL: Once = Once::new();
    TARGET.store(hwnd.0 as isize, Relaxed);
    MESSAGE.store(message, Relaxed);
    MODIFIERS.store(modifiers, Relaxed);
    if modifiers == 0 {
        return;
    }
    INSTALL.call_once(|| {
        std::thread::spawn(|| unsafe {
            let Ok(module) = GetModuleHandleW(None) else { return };
            if SetWindowsHookExW(WH_MOUSE_LL, Some(hook), Some(module.into()), 0).is_err() {
                return;
            }
            // Der Hook wird über die Nachrichtenschleife dieses Threads aufgerufen.
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
        });
    });
}

fn pressed_modifiers() -> u32 {
    let down = |vk: u16| unsafe { GetAsyncKeyState(vk as i32) } < 0;
    let mut mods = 0;
    if down(VK_CONTROL.0) {
        mods |= MOD_CONTROL;
    }
    if down(VK_MENU.0) {
        mods |= MOD_ALT;
    }
    if down(VK_SHIFT.0) {
        mods |= MOD_SHIFT;
    }
    if down(VK_LWIN.0) || down(VK_RWIN.0) {
        mods |= MOD_WIN;
    }
    mods
}

fn matches_trigger() -> bool {
    let wanted = MODIFIERS.load(Relaxed);
    wanted != 0 && pressed_modifiers() == wanted
}

unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        match wparam.0 as u32 {
            WM_RBUTTONDOWN if matches_trigger() => {
                SWALLOW_UP.store(true, Relaxed);
                return LRESULT(1);
            }
            WM_RBUTTONUP if SWALLOW_UP.swap(false, Relaxed) => {
                let hwnd = HWND(TARGET.load(Relaxed) as *mut _);
                let _ = PostMessageW(Some(hwnd), MESSAGE.load(Relaxed), WPARAM(0), LPARAM(0));
                return LRESULT(1);
            }
            _ => {}
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}
