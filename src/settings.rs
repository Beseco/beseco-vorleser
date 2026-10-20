//! Einstellungsfenster: Tastenkombinationen, Maus-Auslöser, Lautstärke, Textbereinigung.

use std::cell::{Cell, RefCell};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBRUSH, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, MessageBoxW, RegisterClassW,
    SendMessageW, SetForegroundWindow, ShowWindow, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
    CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_SETCURSEL, CW_USEDEFAULT, HMENU, IDC_ARROW,
    MB_ICONWARNING, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_SETFONT, WM_USER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::config::{
    format_hotkey, parse_hotkey, parse_mouse_trigger, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};
use crate::window::message_font;

// IDOK und IDCANCEL: so lösen Eingabe und Esc die beiden Schaltflächen aus.
const ID_SAVE: u32 = 1;
const ID_CANCEL: u32 = 2;

// Nachrichten des Tastenkombinations-Felds und des Schiebereglers.
const HKM_SETHOTKEY: u32 = WM_USER + 1;
const HKM_GETHOTKEY: u32 = WM_USER + 2;
const HKM_SETRULES: u32 = WM_USER + 3;
const HOTKEYF_SHIFT: u32 = 1;
const HOTKEYF_CONTROL: u32 = 2;
const HOTKEYF_ALT: u32 = 4;
const TBM_GETPOS: u32 = WM_USER;
const TBM_SETPOS: u32 = WM_USER + 5;
const TBM_SETRANGE: u32 = WM_USER + 6;
const TBM_SETTICFREQ: u32 = WM_USER + 20;
const TBS_AUTOTICKS: u32 = 1;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const COLOR_BTNFACE: usize = 15;

/// Anzeige und zugehöriger Wert für `mouse_menu`.
const MOUSE_OPTIONS: [(&str, &str); 5] = [
    ("Aus", ""),
    ("Strg + Rechtsklick", "Strg+Rechtsklick"),
    ("Umschalt + Rechtsklick", "Shift+Rechtsklick"),
    ("Strg + Umschalt + Rechtsklick", "Strg+Shift+Rechtsklick"),
    ("Strg + Alt + Rechtsklick", "Strg+Alt+Rechtsklick"),
];

#[derive(Clone)]
pub struct Values {
    pub hotkey_read: String,
    pub hotkey_pause: String,
    pub hotkey_menu: String,
    pub mouse_menu: String,
    pub volume: u32,
    pub clean_text: bool,
    pub update_check: bool,
}

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    hotkeys: [HWND; 3],
    mouse: HWND,
    volume: HWND,
    clean: HWND,
    update: HWND,
    font: HFONT,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static ORIGINAL: RefCell<Option<Values>> = const { RefCell::new(None) };
    static ON_SAVE: Cell<Option<fn(Values)>> = const { Cell::new(None) };
}

/// Das Fenster, solange es offen ist.
pub fn handle() -> Option<HWND> {
    CONTROLS.with(|c| c.get()).map(|c| c.window)
}

/// Öffnet das Fenster mit `values`; beim Speichern erhält `on_save` die neuen Werte.
pub fn open(owner: Option<HWND>, values: Values, on_save: fn(Values)) {
    if let Some(window) = handle() {
        unsafe {
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserEinstellungen"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: crate::tray::load_icon(),
            hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&class);

        let dpi = GetDpiForSystem();
        let px = |value: i32| value * dpi as i32 / 96;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT { left: 0, top: 0, right: px(470), bottom: px(352) };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_CONTROLPARENT, dpi);
        let Ok(window) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            w!("Vorleser – Einstellungen"),
            style,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            frame.right - frame.left,
            frame.bottom - frame.top,
            owner,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };

        let font = message_font(window);
        let child = |class: PCWSTR, text: &str, style: u32, id: u32, x: i32, y: i32, width: i32, height: i32| {
            let control = CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                &HSTRING::from(text),
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                px(x),
                px(y),
                px(width),
                px(height),
                Some(window),
                Some(HMENU(id as usize as *mut _)),
                Some(instance.into()),
                None,
            )
            .unwrap_or_default();
            SendMessageW(control, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            control
        };
        let tab = WS_TABSTOP.0;
        let (label_x, field_x, label_width, field_width, row) = (16, 220, 200, 234, 36);
        let label = |text: &str, line: i32| child(w!("STATIC"), text, 0, 0, label_x, 48 + line * row, label_width, 22);
        let hotkey = |line: i32| child(w!("msctls_hotkey32"), "", tab, 0, field_x, 44 + line * row, field_width, 26);

        child(
            w!("STATIC"),
            "Feld anklicken und die gewünschte Kombination drücken. Rücktaste = keine.",
            0, 0, label_x, 14, 438, 22,
        );
        label("Markierten Text vorlesen / Stopp:", 0);
        label("Pause / Fortsetzen:", 1);
        label("Menü an der Mausposition:", 2);
        label("Menü per Maus:", 3);
        label("Lautstärke:", 4);
        // Die Reihenfolge der Erzeugung ist die Tab-Reihenfolge.
        let hotkeys = [hotkey(0), hotkey(1), hotkey(2)];
        let list = tab | WS_VSCROLL.0 | CBS_DROPDOWNLIST as u32;
        let mouse = child(w!("COMBOBOX"), "", list, 0, field_x, 44 + 3 * row, field_width, 300);
        let volume = child(w!("msctls_trackbar32"), "", tab | TBS_AUTOTICKS, 0, field_x - 8, 42 + 4 * row, field_width + 16, 30);
        let clean = child(
            w!("BUTTON"),
            "Text vor dem Vorlesen bereinigen (Zeilenumbrüche, Silbentrennung, Links)",
            tab | BS_AUTOCHECKBOX as u32,
            0, label_x, 46 + 5 * row, 438, 24,
        );
        let update = child(
            w!("BUTTON"),
            "Einmal täglich auf GitHub nach neuen Versionen suchen",
            tab | BS_AUTOCHECKBOX as u32,
            0, label_x, 46 + 6 * row, 438, 24,
        );
        child(w!("BUTTON"), "Speichern", tab | BS_DEFPUSHBUTTON as u32, ID_SAVE, 248, 306, 100, 30);
        child(w!("BUTTON"), "Abbrechen", tab, ID_CANCEL, 356, 306, 98, 30);

        let c = Controls { window, hotkeys, mouse, volume, clean, update, font };
        fill(&c, &values);
        CONTROLS.with(|cell| cell.set(Some(c)));
        ORIGINAL.with(|cell| *cell.borrow_mut() = Some(values));
        ON_SAVE.with(|cell| cell.set(Some(on_save)));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(hotkeys[0]));
    }
}

unsafe fn fill(c: &Controls, values: &Values) {
    let texts = [&values.hotkey_read, &values.hotkey_pause, &values.hotkey_menu];
    for (control, text) in c.hotkeys.iter().zip(texts) {
        // Ohne Zusatztaste wäre die Kombination ungültig; das Feld ergänzt dann Strg+Alt.
        let rules = LPARAM((HOTKEYF_CONTROL | HOTKEYF_ALT) as isize);
        SendMessageW(*control, HKM_SETRULES, Some(WPARAM(0x1 | 0x2)), Some(rules));
        if let Some((mods, vk)) = parse_hotkey(text).filter(|(mods, _)| mods & MOD_WIN == 0) {
            let mut flags = 0;
            for (modifier, flag) in [(MOD_SHIFT, HOTKEYF_SHIFT), (MOD_CONTROL, HOTKEYF_CONTROL), (MOD_ALT, HOTKEYF_ALT)] {
                if mods & modifier != 0 {
                    flags |= flag;
                }
            }
            SendMessageW(*control, HKM_SETHOTKEY, Some(WPARAM((vk | flags << 8) as usize)), None);
        }
    }

    let current = parse_mouse_trigger(&values.mouse_menu);
    let mut selected = None;
    for (i, (label, value)) in MOUSE_OPTIONS.iter().enumerate() {
        let label = HSTRING::from(*label);
        SendMessageW(c.mouse, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        if parse_mouse_trigger(value) == current {
            selected = Some(i);
        }
    }
    // Eine von Hand in die Datei geschriebene Kombination bleibt als eigener Eintrag wählbar.
    let selected = selected.unwrap_or_else(|| {
        let label = HSTRING::from(values.mouse_menu.as_str());
        SendMessageW(c.mouse, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        MOUSE_OPTIONS.len()
    });
    SendMessageW(c.mouse, CB_SETCURSEL, Some(WPARAM(selected)), None);

    SendMessageW(c.volume, TBM_SETRANGE, Some(WPARAM(1)), Some(LPARAM(100 << 16)));
    SendMessageW(c.volume, TBM_SETTICFREQ, Some(WPARAM(10)), None);
    SendMessageW(c.volume, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(values.volume as isize)));
    SendMessageW(c.clean, BM_SETCHECK, Some(WPARAM(values.clean_text as usize)), None);
    SendMessageW(c.update, BM_SETCHECK, Some(WPARAM(values.update_check as usize)), None);
}

/// Liest ein Tastenkombinations-Feld; `Err`, wenn die gedrückte Taste nicht unterstützt wird.
unsafe fn read_hotkey(control: HWND, original: &str) -> Result<String, ()> {
    let value = SendMessageW(control, HKM_GETHOTKEY, None, None).0 as u32;
    let (vk, flags) = (value & 0xff, (value >> 8) & 0xff);
    if vk == 0 {
        // Kombinationen mit der Windows-Taste kann das Feld nicht anzeigen; die bleiben erhalten.
        let hidden = parse_hotkey(original).is_some_and(|(mods, _)| mods & MOD_WIN != 0);
        return Ok(if hidden { original.to_string() } else { String::new() });
    }
    let mut mods = 0;
    for (modifier, flag) in [(MOD_SHIFT, HOTKEYF_SHIFT), (MOD_CONTROL, HOTKEYF_CONTROL), (MOD_ALT, HOTKEYF_ALT)] {
        if flags & flag != 0 {
            mods |= modifier;
        }
    }
    format_hotkey(mods, vk).ok_or(())
}

unsafe fn save(c: &Controls) {
    let Some(original) = ORIGINAL.with(|cell| cell.borrow().clone()) else { return };
    let originals = [&original.hotkey_read, &original.hotkey_pause, &original.hotkey_menu];
    let mut hotkeys = Vec::new();
    for (control, original) in c.hotkeys.iter().zip(originals) {
        match read_hotkey(*control, original) {
            Ok(text) => hotkeys.push(text),
            Err(()) => {
                let text = w!("Diese Taste wird nicht unterstützt.\n\nMöglich sind Buchstaben, Ziffern, F1 bis F24 und die Leertaste, jeweils zusammen mit Strg, Alt oder Umschalt.");
                MessageBoxW(Some(c.window), text, w!("Vorleser"), MB_ICONWARNING);
                let _ = SetFocus(Some(*control));
                return;
            }
        }
    }
    let row = SendMessageW(c.mouse, CB_GETCURSEL, None, None).0;
    let mouse_menu = match MOUSE_OPTIONS.get(row as usize) {
        Some((_, value)) => value.to_string(),
        None => original.mouse_menu.clone(),
    };
    let values = Values {
        hotkey_menu: hotkeys.pop().unwrap_or_default(),
        hotkey_pause: hotkeys.pop().unwrap_or_default(),
        hotkey_read: hotkeys.pop().unwrap_or_default(),
        mouse_menu,
        volume: (SendMessageW(c.volume, TBM_GETPOS, None, None).0 as u32).min(100),
        clean_text: SendMessageW(c.clean, BM_GETCHECK, None, None).0 == 1,
        update_check: SendMessageW(c.update, BM_GETCHECK, None, None).0 == 1,
    };
    let on_save = ON_SAVE.with(|cell| cell.get());
    let _ = DestroyWindow(c.window);
    if let Some(on_save) = on_save {
        on_save(values);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u32, (wparam.0 >> 16) & 0xffff);
                let controls = CONTROLS.with(|c| c.get());
                match (id, code, controls) {
                    (ID_SAVE, 0, Some(c)) => save(&c),
                    (ID_CANCEL, 0, _) => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
            }
            WM_DESTROY => {
                if let Some(c) = CONTROLS.with(|cell| cell.take()) {
                    let _ = DeleteObject(c.font.into());
                }
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}
