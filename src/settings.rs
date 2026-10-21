//! Einstellungsfenster: Stimme und Geschwindigkeit, Tastenkombinationen, Maus-Auslöser,
//! Lautstärke, Textbereinigung.

use std::cell::{Cell, RefCell};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBRUSH, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::Input::KeyboardAndMouse::SetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindowTextW, ES_AUTOHSCROLL, WS_EX_CLIENTEDGE,
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, MessageBoxW, RegisterClassW,
    SendMessageW, SetForegroundWindow, ShowWindow, BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON,
    CBN_SELCHANGE, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL, CB_GETITEMDATA, CB_RESETCONTENT, CB_SETCURSEL,
    CB_SETITEMDATA, CW_USEDEFAULT, HMENU, IDC_ARROW,
    MB_ICONWARNING, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_DESTROY,
    WM_SETFONT, WM_USER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::config::{
    format_hotkey, parse_hotkey, parse_mouse_trigger, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN,
};
use crate::i18n::{self, t};
use crate::window::message_font;

// IDOK und IDCANCEL: so lösen Eingabe und Esc die beiden Schaltflächen aus.
const ID_SAVE: u32 = 1;
const ID_CANCEL: u32 = 2;
const ID_VOICE: u32 = 10;
const ID_RATE: u32 = 11;
const ID_SAMPLE: u32 = 12;
const ID_AI_VOICES: u32 = 13;

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
const EM_SETCUEBANNER: u32 = 0x1501;

/// Schlüssel der Anzeige und zugehöriger Wert für `mouse_menu`.
const MOUSE_OPTIONS: [(&str, &str); 5] = [
    ("mouse.off", ""),
    ("mouse.ctrl", "Strg+Rechtsklick"),
    ("mouse.shift", "Shift+Rechtsklick"),
    ("mouse.ctrl_shift", "Strg+Shift+Rechtsklick"),
    ("mouse.ctrl_alt", "Strg+Alt+Rechtsklick"),
];

/// Anzeige und Wert für `language`: „automatisch“ und alle verfügbaren Sprachen.
fn language_options() -> Vec<(String, String)> {
    let mut options = vec![(t("menu.language_auto").replace('&', ""), i18n::AUTO.to_string())];
    options.extend(i18n::languages().into_iter().map(|(code, name)| (name.to_string(), code.to_string())));
    options
}

#[derive(Clone)]
pub struct Values {
    pub hotkey_read: String,
    pub hotkey_pause: String,
    pub hotkey_menu: String,
    pub hotkey_ocr: String,
    pub hotkey_dictate: String,
    pub mouse_menu: String,
    pub volume: u32,
    pub clean_text: bool,
    pub update_check: bool,
    pub proxy: String,
    pub remember_text: bool,
    pub language: String,
}

/// Stimmenauswahl oben im Fenster. Sie wirkt sofort (damit die Hörprobe passt); „Abbrechen“
/// stellt die vorherige Stimme wieder her.
pub struct Voices {
    /// Programmweiter Index der Stimme und Anzeige, schon nach Sprache sortiert.
    pub items: Vec<(usize, String)>,
    pub selected: Option<usize>,
    pub rates: Vec<String>,
    pub rate: usize,
    /// Neue Stimme bzw. Geschwindigkeit (Index in `rates`).
    pub on_change: fn(Option<usize>, usize),
    pub on_sample: fn(),
    pub on_ai_voices: fn(),
}

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    voice: HWND,
    rate: HWND,
    hotkeys: [HWND; 5],
    mouse: HWND,
    volume: HWND,
    clean: HWND,
    update: HWND,
    proxy: HWND,
    remember: HWND,
    language: HWND,
    font: HFONT,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static ORIGINAL: RefCell<Option<Values>> = const { RefCell::new(None) };
    static ON_SAVE: Cell<Option<fn(Values)>> = const { Cell::new(None) };
    static VOICE_CALLBACKS: Cell<Option<(fn(Option<usize>, usize), fn(), fn())>> = const { Cell::new(None) };
    /// Stimme und Geschwindigkeit beim Öffnen, für „Abbrechen“.
    static VOICE_BEFORE: Cell<(Option<usize>, usize)> = const { Cell::new((None, 0)) };
}

/// Das Fenster, solange es offen ist.
pub fn handle() -> Option<HWND> {
    CONTROLS.with(|c| c.get()).map(|c| c.window)
}

/// Öffnet das Fenster mit `values`; beim Speichern erhält `on_save` die neuen Werte.
pub fn open(owner: Option<HWND>, values: Values, voices: Voices, on_save: fn(Values)) {
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
        let mut frame = RECT { left: 0, top: 0, right: px(470), bottom: px(612) };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_CONTROLPARENT, dpi);
        let Ok(window) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            &HSTRING::from(t("settings.vorleser_settings")),
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
        // Oben die Stimme, darunter (um OFFSET verschoben) die übrigen Einstellungen.
        const OFFSET: i32 = 84;
        let list = tab | WS_VSCROLL.0 | CBS_DROPDOWNLIST as u32;
        child(w!("STATIC"), t("window.voice"), 0, 0, label_x, 18, 110, 22);
        let voice = child(w!("COMBOBOX"), "", list, ID_VOICE, 130, 14, 214, 400);
        child(w!("BUTTON"), t("window.sample"), tab, ID_SAMPLE, 352, 13, 102, 28);
        child(w!("STATIC"), t("window.speed"), 0, 0, label_x, 54, 110, 22);
        let rate = child(w!("COMBOBOX"), "", list, ID_RATE, 130, 50, 214, 400);
        child(w!("BUTTON"), t("app.ai_voices"), tab, ID_AI_VOICES, 352, 49, 102, 28);
        child(w!("STATIC"), "", 0x10 /* SS_ETCHEDHORZ */, 0, label_x, 90, 438, 2);
        let label = |text: &str, line: i32| child(w!("STATIC"), text, 0, 0, label_x, OFFSET + 48 + line * row, label_width, 22);
        let hotkey = |line: i32| child(w!("msctls_hotkey32"), "", tab, 0, field_x, OFFSET + 44 + line * row, field_width, 26);

        child(
            w!("STATIC"),
            t("settings.click_field_press_shortcut"),
            0, 0, label_x, OFFSET + 14, 438, 22,
        );
        label(t("settings.read_selected_text_stop"), 0);
        label(t("settings.pause_resume"), 1);
        label(t("settings.menu_at_mouse_position"), 2);
        label(t("settings.screen_ocr"), 3);
        label(t("settings.dictate_anywhere"), 4);
        label(t("settings.menu_by_mouse"), 5);
        label(t("settings.volume"), 6);
        label("Sprache / Language:", 7);
        label(t("settings.proxy"), 8);
        // Die Reihenfolge der Erzeugung ist die Tab-Reihenfolge.
        let hotkeys = [hotkey(0), hotkey(1), hotkey(2), hotkey(3), hotkey(4)];
        let mouse = child(w!("COMBOBOX"), "", list, 0, field_x, OFFSET + 44 + 5 * row, field_width, 300);
        let volume = child(w!("msctls_trackbar32"), "", tab | TBS_AUTOTICKS, 0, field_x - 8, OFFSET + 42 + 6 * row, field_width + 16, 30);
        let language = child(w!("COMBOBOX"), "", list, 0, field_x, OFFSET + 44 + 7 * row, field_width, 300);
        let proxy = CreateWindowExW(
            WS_EX_CLIENTEDGE,
            w!("EDIT"),
            &HSTRING::from(values.proxy.as_str()),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
            px(field_x),
            px(OFFSET + 44 + 8 * row),
            px(field_width),
            px(26),
            Some(window),
            None,
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();
        SendMessageW(proxy, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        // Grauer Hinweistext im leeren Feld.
        let example = HSTRING::from(t("settings.proxy_example"));
        SendMessageW(proxy, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(example.as_ptr() as isize)));
        let clean = child(
            w!("BUTTON"),
            t("settings.clean_up_text_before"),
            tab | BS_AUTOCHECKBOX as u32,
            0, label_x, OFFSET + 46 + 9 * row, 438, 24,
        );
        let update = child(
            w!("BUTTON"),
            t("settings.check_github_for_new"),
            tab | BS_AUTOCHECKBOX as u32,
            0, label_x, OFFSET + 46 + 10 * row, 438, 24,
        );
        let remember = child(
            w!("BUTTON"),
            t("settings.remember_text"),
            tab | BS_AUTOCHECKBOX as u32,
            0, label_x, OFFSET + 42 + 11 * row, 438, 24,
        );
        child(w!("BUTTON"), t("settings.save"), tab | BS_DEFPUSHBUTTON as u32, ID_SAVE, 248, OFFSET + 482, 100, 30);
        child(w!("BUTTON"), t("settings.cancel"), tab, ID_CANCEL, 356, OFFSET + 482, 98, 30);

        let c = Controls { window, voice, rate, hotkeys, mouse, volume, clean, update, language, proxy, remember, font };
        fill(&c, &values);
        fill_voices(&c, &voices.items, voices.selected);
        for label in &voices.rates {
            let label = HSTRING::from(label.as_str());
            SendMessageW(rate, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        }
        SendMessageW(rate, CB_SETCURSEL, Some(WPARAM(voices.rate)), None);
        VOICE_CALLBACKS.with(|cell| cell.set(Some((voices.on_change, voices.on_sample, voices.on_ai_voices))));
        VOICE_BEFORE.with(|cell| cell.set((voices.selected, voices.rate)));
        CONTROLS.with(|cell| cell.set(Some(c)));
        ORIGINAL.with(|cell| *cell.borrow_mut() = Some(values));
        ON_SAVE.with(|cell| cell.set(Some(on_save)));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(voice));
    }
}

unsafe fn fill_voices(c: &Controls, items: &[(usize, String)], selected: Option<usize>) {
    SendMessageW(c.voice, CB_RESETCONTENT, None, None);
    for (row, (index, label)) in items.iter().enumerate() {
        let label = HSTRING::from(label.as_str());
        SendMessageW(c.voice, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        SendMessageW(c.voice, CB_SETITEMDATA, Some(WPARAM(row)), Some(LPARAM(*index as isize)));
        if Some(*index) == selected {
            SendMessageW(c.voice, CB_SETCURSEL, Some(WPARAM(row)), None);
        }
    }
}

/// Neue Stimmenliste, z. B. nach dem Download einer KI-Stimme, solange das Fenster offen ist.
pub fn set_voices(items: &[(usize, String)], selected: Option<usize>) {
    if let Some(c) = CONTROLS.with(|cell| cell.get()) {
        unsafe { fill_voices(&c, items, selected) };
        VOICE_BEFORE.with(|cell| cell.set((selected, cell.get().1)));
    }
}

unsafe fn chosen_voice(c: &Controls) -> (Option<usize>, usize) {
    let row = SendMessageW(c.voice, CB_GETCURSEL, None, None).0;
    let voice = (row >= 0).then(|| SendMessageW(c.voice, CB_GETITEMDATA, Some(WPARAM(row as usize)), None).0 as usize);
    let rate = SendMessageW(c.rate, CB_GETCURSEL, None, None).0.max(0) as usize;
    (voice, rate)
}

/// Schließt ohne Speichern und stellt die Stimme von vorher wieder her.
unsafe fn cancel(hwnd: HWND) {
    let changed = CONTROLS.with(|cell| cell.get()).map(|c| chosen_voice(&c)) != Some(VOICE_BEFORE.with(|b| b.get()));
    let _ = DestroyWindow(hwnd);
    if changed {
        if let Some((on_change, _, _)) = VOICE_CALLBACKS.with(|cell| cell.get()) {
            let (voice, rate) = VOICE_BEFORE.with(|b| b.get());
            on_change(voice, rate);
        }
    }
}

unsafe fn fill(c: &Controls, values: &Values) {
    let texts = [&values.hotkey_read, &values.hotkey_pause, &values.hotkey_menu, &values.hotkey_ocr, &values.hotkey_dictate];
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
    for (i, (key, value)) in MOUSE_OPTIONS.iter().enumerate() {
        let label = HSTRING::from(t(key));
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
    SendMessageW(c.remember, BM_SETCHECK, Some(WPARAM(values.remember_text as usize)), None);
    let mut current = 0;
    for (i, (label, value)) in language_options().iter().enumerate() {
        let label = HSTRING::from(label.as_str());
        SendMessageW(c.language, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        if *value == values.language {
            current = i;
        }
    }
    SendMessageW(c.language, CB_SETCURSEL, Some(WPARAM(current)), None);
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
    let originals = [&original.hotkey_read, &original.hotkey_pause, &original.hotkey_menu, &original.hotkey_ocr, &original.hotkey_dictate];
    let mut hotkeys = Vec::new();
    for (control, original) in c.hotkeys.iter().zip(originals) {
        match read_hotkey(*control, original) {
            Ok(text) => hotkeys.push(text),
            Err(()) => {
                let text = t("settings.key_not_supported_possible");
                MessageBoxW(Some(c.window), &HSTRING::from(text), w!("Vorleser"), MB_ICONWARNING);
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
        hotkey_dictate: hotkeys.pop().unwrap_or_default(),
        hotkey_ocr: hotkeys.pop().unwrap_or_default(),
        hotkey_menu: hotkeys.pop().unwrap_or_default(),
        hotkey_pause: hotkeys.pop().unwrap_or_default(),
        hotkey_read: hotkeys.pop().unwrap_or_default(),
        mouse_menu,
        volume: (SendMessageW(c.volume, TBM_GETPOS, None, None).0 as u32).min(100),
        clean_text: SendMessageW(c.clean, BM_GETCHECK, None, None).0 == 1,
        update_check: SendMessageW(c.update, BM_GETCHECK, None, None).0 == 1,
        remember_text: SendMessageW(c.remember, BM_GETCHECK, None, None).0 == 1,
        proxy: {
            let mut buffer = [0u16; 512];
            let len = GetWindowTextW(c.proxy, &mut buffer) as usize;
            String::from_utf16_lossy(&buffer[..len]).trim().to_string()
        },
        language: {
            let row = SendMessageW(c.language, CB_GETCURSEL, None, None).0;
            language_options().get(row as usize).map_or(original.language.clone(), |(_, value)| value.clone())
        },
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
                let callbacks = VOICE_CALLBACKS.with(|cell| cell.get());
                match (id, code as u32, controls, callbacks) {
                    (ID_SAVE, 0, Some(c), _) => save(&c),
                    (ID_CANCEL, 0, _, _) => cancel(hwnd),
                    (ID_VOICE | ID_RATE, CBN_SELCHANGE, Some(c), Some((on_change, _, _))) => {
                        let (voice, rate) = chosen_voice(&c);
                        on_change(voice, rate);
                    }
                    (ID_SAMPLE, 0, _, Some((_, on_sample, _))) => on_sample(),
                    (ID_AI_VOICES, 0, _, Some((_, _, on_ai_voices))) => on_ai_voices(),
                    _ => {}
                }
            }
            WM_CLOSE => cancel(hwnd),
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
