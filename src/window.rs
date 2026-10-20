//! Das kleine Hauptfenster: Textfeld zum Einfügen, Stimme und Geschwindigkeit, Hörprobe,
//! Vorlesen/Pause/Stopp. Schließen blendet es nur aus; das Programm läuft im Infobereich weiter.

use std::cell::Cell;
use std::mem::size_of;

use windows::core::{s, w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, HBRUSH, HFONT};
use windows::Win32::System::ApplicationInstallationAndServicing::{
    ActivateActCtx, CreateActCtxW, ACTCTXW,
};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetWindowTextLengthW, GetWindowTextW, IsIconic, IsWindowVisible,
    LoadCursorW, MoveWindow, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowPos,
    SetWindowTextW, ShowWindow, BS_AUTOCHECKBOX, CBN_SELCHANGE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOSIZE, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL,
    CB_GETITEMDATA, CB_RESETCONTENT, CB_SETCURSEL, CB_SETITEMDATA, CW_USEDEFAULT, ES_AUTOVSCROLL, ES_MULTILINE, ES_NOHIDESEL, ES_WANTRETURN, HICON,
    HMENU, ICON_BIG, ICON_SMALL, IDC_ARROW, MINMAXINFO, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS,
    SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_CLOSE, WM_COMMAND, WM_DPICHANGED, WM_GETMINMAXINFO, WM_SETFOCUS, WM_SETFONT, WM_SETICON,
    WM_SIZE, WNDCLASSW, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW,
    WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

// Im windows-Crate nur über weitere Features erreichbar, daher hier von Hand.
const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x04;
const ACTCTX_FLAG_RESOURCE_NAME_VALID: u32 = 0x08;
const COLOR_BTNFACE: usize = 15;
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SCROLLCARET: u32 = 0x00B7;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const EM_SETLIMITTEXT: u32 = 0x00C5;

// 1 und 2 sind für Eingabe/Esc im Dialog reserviert (IDOK/IDCANCEL).
const ID_CANCEL: u32 = 2;
pub const ID_READ: u32 = 101;
pub const ID_PAUSE: u32 = 102;
pub const ID_STOP: u32 = 103;
pub const ID_SAMPLE: u32 = 104;
pub const ID_VOICE: u32 = 105;
pub const ID_RATE: u32 = 106;
pub const ID_ABOUT: u32 = 107;
pub const ID_SAVE_AUDIO: u32 = 108;
pub const ID_SETTINGS: u32 = 109;
pub const ID_TOPMOST: u32 = 110;

/// Benachrichtigungscode eines Klicks auf eine Schaltfläche.
pub const CLICKED: u32 = 0;
/// Benachrichtigungscode einer geänderten Auswahl in einer Liste.
pub const SELECTED: u32 = CBN_SELCHANGE;

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    voice_label: HWND,
    voice: HWND,
    sample: HWND,
    rate_label: HWND,
    rate: HWND,
    topmost: HWND,
    edit: HWND,
    read: HWND,
    pause: HWND,
    stop: HWND,
    save: HWND,
    settings: HWND,
    about: HWND,
}

impl Controls {
    fn children(&self) -> [HWND; 13] {
        [
            self.voice_label,
            self.voice,
            self.sample,
            self.rate_label,
            self.rate,
            self.topmost,
            self.edit,
            self.read,
            self.pause,
            self.stop,
            self.save,
            self.settings,
            self.about,
        ]
    }
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static FONT: Cell<HFONT> = const { Cell::new(HFONT(std::ptr::null_mut())) };
    static ON_COMMAND: Cell<Option<fn(u32, u32)>> = const { Cell::new(None) };
}

fn controls() -> Option<Controls> {
    CONTROLS.with(|c| c.get())
}

/// Schaltet die modernen Steuerelemente ein (sonst sähen Schaltflächen aus wie unter Windows 95).
/// Üblich wäre ein eingebettetes Manifest; so kommt das Programm ohne Ressourcen-Compiler aus.
pub fn enable_visual_styles() {
    unsafe {
        let mut system_dir = [0u16; 260];
        GetSystemDirectoryW(Some(&mut system_dir));
        let context = ACTCTXW {
            cbSize: size_of::<ACTCTXW>() as u32,
            dwFlags: ACTCTX_FLAG_RESOURCE_NAME_VALID | ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID,
            lpSource: w!("shell32.dll"),
            lpAssemblyDirectory: PCWSTR(system_dir.as_ptr()),
            // Ressource 124 von shell32.dll ist ein Manifest für die Steuerelemente in Version 6.
            lpResourceName: PCWSTR(124 as *const u16),
            ..Default::default()
        };
        // Der Kontext bleibt für die ganze Laufzeit auf diesem Thread aktiv.
        let mut cookie = 0;
        if let Ok(handle) = CreateActCtxW(&context) {
            if ActivateActCtx(Some(handle), &mut cookie).is_ok() {
                init_common_controls();
            }
        }
    }
}

/// Meldet die Klassen für Tastenkombinations-Feld und Schieberegler an. Die Bibliothek wird
/// erst hier geladen: Ein fester Import würde vor dem Aktivierungskontext die alte Version binden.
unsafe fn init_common_controls() {
    const ICC_BAR_CLASSES: u32 = 0x04;
    const ICC_HOTKEY_CLASS: u32 = 0x40;
    let Ok(library) = LoadLibraryW(w!("comctl32.dll")) else { return };
    if let Some(init) = GetProcAddress(library, s!("InitCommonControlsEx")) {
        // INITCOMMONCONTROLSEX: Größe der Struktur, gewünschte Klassen.
        let init: unsafe extern "system" fn(*const [u32; 2]) -> i32 = std::mem::transmute(init);
        init(&[8, ICC_BAR_CLASSES | ICC_HOTKEY_CLASS]);
    }
}

/// Legt das (noch unsichtbare) Fenster an. `on_command` erhält die ID des Steuerelements und
/// den Benachrichtigungscode.
pub fn create(icon: HICON, on_command: fn(u32, u32)) -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserDialog"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            w!("Vorleser"),
            WS_OVERLAPPEDWINDOW,
            CW_USEDEFAULT,
            CW_USEDEFAULT,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;

        let child = |class: PCWSTR, text: PCWSTR, style: u32, ex_style: WINDOW_EX_STYLE, id: u32| {
            CreateWindowExW(
                ex_style,
                class,
                text,
                WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style),
                0,
                0,
                0,
                0,
                Some(window),
                Some(HMENU(id as usize as *mut _)),
                Some(instance.into()),
                None,
            )
            .unwrap_or_default()
        };
        let none = WINDOW_EX_STYLE(0);
        let tab = WS_TABSTOP.0;
        let list = tab | WS_VSCROLL.0 | CBS_DROPDOWNLIST as u32;
        let edit_style =
            tab | WS_VSCROLL.0 | (ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | ES_NOHIDESEL) as u32;
        // Die Reihenfolge der Erzeugung ist die Tab-Reihenfolge.
        let c = Controls {
            window,
            voice_label: child(w!("STATIC"), w!("Stimme:"), 0, none, 0),
            voice: child(w!("COMBOBOX"), w!(""), list, none, ID_VOICE),
            sample: child(w!("BUTTON"), w!("Hörprobe"), tab, none, ID_SAMPLE),
            rate_label: child(w!("STATIC"), w!("Geschwindigkeit:"), 0, none, 0),
            rate: child(w!("COMBOBOX"), w!(""), list, none, ID_RATE),
            topmost: child(w!("BUTTON"), w!("Immer im Vordergrund"), tab | BS_AUTOCHECKBOX as u32, none, ID_TOPMOST),
            edit: child(w!("EDIT"), w!(""), edit_style, WS_EX_CLIENTEDGE, 0),
            read: child(w!("BUTTON"), w!("Vorlesen"), tab, none, ID_READ),
            pause: child(w!("BUTTON"), w!("Pause"), tab, none, ID_PAUSE),
            stop: child(w!("BUTTON"), w!("Stopp"), tab, none, ID_STOP),
            save: child(w!("BUTTON"), w!("Als WAV speichern …"), tab, none, ID_SAVE_AUDIO),
            settings: child(w!("BUTTON"), w!("Einstellungen"), tab, none, ID_SETTINGS),
            about: child(w!("BUTTON"), w!("Info"), tab, none, ID_ABOUT),
        };
        CONTROLS.with(|cell| cell.set(Some(c)));
        ON_COMMAND.with(|cell| cell.set(Some(on_command)));

        // Ohne Grenze fasst ein Textfeld nur rund 30 000 Zeichen.
        SendMessageW(c.edit, EM_SETLIMITTEXT, Some(WPARAM(0)), Some(LPARAM(0)));
        for size in [ICON_SMALL, ICON_BIG] {
            SendMessageW(window, WM_SETICON, Some(WPARAM(size as usize)), Some(LPARAM(icon.0 as isize)));
        }
        apply_font(&c);
        let dpi = GetDpiForWindow(window) as i32;
        let (width, height) = (740 * dpi / 96, 480 * dpi / 96);
        let _ = SetWindowPos(window, None, 0, 0, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE);
        Some(window)
    }
}

/// Das Fenster, solange es sichtbar ist (als Besitzer für Meldungen).
pub fn visible() -> Option<HWND> {
    let c = controls()?;
    unsafe { IsWindowVisible(c.window).as_bool().then_some(c.window) }
}

/// Zeigt das Fenster und holt es nach vorn.
pub fn show() {
    let Some(c) = controls() else { return };
    unsafe {
        let _ = ShowWindow(c.window, if IsIconic(c.window).as_bool() { SW_RESTORE } else { SW_SHOW });
        let _ = SetForegroundWindow(c.window);
        let _ = SetFocus(Some(c.edit));
    }
}

/// Füllt die Stimmenliste; jeder Eintrag trägt seinen Index in der Stimmenliste des Programms.
pub fn set_voices(items: &[(usize, String)]) {
    let Some(c) = controls() else { return };
    fill(c.voice, items.iter().map(|(index, label)| (*index, label.as_str())));
}

pub fn set_rates(labels: &[&str]) {
    let Some(c) = controls() else { return };
    fill(c.rate, labels.iter().copied().enumerate());
}

pub fn select_voice(index: Option<usize>) {
    if let Some(c) = controls() {
        select(c.voice, index);
    }
}

pub fn select_rate(index: Option<usize>) {
    if let Some(c) = controls() {
        select(c.rate, index);
    }
}

pub fn selected_voice() -> Option<usize> {
    selected(controls()?.voice)
}

pub fn selected_rate() -> Option<usize> {
    selected(controls()?.rate)
}

/// Inhalt des Textfelds und die Markierung darin (Anfang und Ende in UTF-16-Einheiten).
pub struct Source {
    pub content: Vec<u16>,
    pub selection: (u32, u32),
}

pub fn source() -> Option<Source> {
    let c = controls()?;
    unsafe {
        let mut content = vec![0u16; GetWindowTextLengthW(c.edit) as usize + 1];
        let len = GetWindowTextW(c.edit, &mut content) as usize;
        content.truncate(len);
        let (mut start, mut end) = (0u32, 0u32);
        SendMessageW(
            c.edit,
            EM_GETSEL,
            Some(WPARAM(&mut start as *mut u32 as usize)),
            Some(LPARAM(&mut end as *mut u32 as isize)),
        );
        Some(Source { content, selection: (start.min(len as u32), end.min(len as u32)) })
    }
}

/// Ersetzt den Inhalt des Textfelds. Anders als `SetWindowTextW` lässt sich das mit Strg+Z
/// rückgängig machen, sodass ein vorher eingefügter Text nicht verloren ist.
pub fn set_text(text: &str) {
    if let Some(c) = controls() {
        let text = HSTRING::from(text);
        unsafe {
            SendMessageW(c.edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            SendMessageW(c.edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(text.as_ptr() as isize)));
        }
    }
}

/// Zeigt, dass gerade eine Audiodatei geschrieben wird.
pub fn set_saving(saving: bool) {
    let Some(c) = controls() else { return };
    unsafe {
        let label = if saving { w!("Speichere …") } else { w!("Als WAV speichern …") };
        let _ = SetWindowTextW(c.save, label);
        let _ = EnableWindow(c.save, !saving);
    }
}

/// Markiert einen Bereich des Textfelds und rollt ihn ins Bild (Mitlesen).
pub fn mark(start: u32, end: u32) {
    if let Some(c) = controls() {
        unsafe {
            SendMessageW(c.edit, EM_SETSEL, Some(WPARAM(start as usize)), Some(LPARAM(end as isize)));
            SendMessageW(c.edit, EM_SCROLLCARET, None, None);
        }
    }
}

/// Hält das Fenster über anderen Fenstern (oder nicht) und setzt das Häkchen passend.
pub fn set_topmost(on: bool) {
    if let Some(c) = controls() {
        unsafe {
            let order = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
            let _ = SetWindowPos(c.window, Some(order), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            SendMessageW(c.topmost, BM_SETCHECK, Some(WPARAM(on as usize)), None);
        }
    }
}

pub fn topmost_checked() -> bool {
    controls().is_some_and(|c| unsafe { SendMessageW(c.topmost, BM_GETCHECK, None, None).0 == 1 })
}

/// Der markierte Teil des Textfelds, sonst der ganze Text; `None`, wenn nichts drinsteht.
pub fn text() -> Option<String> {
    let c = controls()?;
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(c.edit) as usize + 1];
        let len = GetWindowTextW(c.edit, &mut buffer) as usize;
        let (mut start, mut end) = (0u32, 0u32);
        SendMessageW(
            c.edit,
            EM_GETSEL,
            Some(WPARAM(&mut start as *mut u32 as usize)),
            Some(LPARAM(&mut end as *mut u32 as isize)),
        );
        let (start, end) = (start as usize, (end as usize).min(len));
        let part = if start < end { &buffer[start..end] } else { &buffer[..len] };
        let text = String::from_utf16_lossy(part);
        (!text.trim().is_empty()).then_some(text)
    }
}

/// Passt die Schaltflächen an den Zustand der Sprachausgabe an.
pub fn set_playback(active: bool, paused: bool) {
    let Some(c) = controls() else { return };
    unsafe {
        let _ = SetWindowTextW(c.pause, if paused { w!("Fortsetzen") } else { w!("Pause") });
        let _ = EnableWindow(c.pause, active);
        let _ = EnableWindow(c.stop, active);
    }
}

fn fill<'a>(list: HWND, items: impl Iterator<Item = (usize, &'a str)>) {
    unsafe {
        SendMessageW(list, CB_RESETCONTENT, None, None);
        for (data, label) in items {
            let label = HSTRING::from(label);
            let row = SendMessageW(list, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
            SendMessageW(list, CB_SETITEMDATA, Some(WPARAM(row.0 as usize)), Some(LPARAM(data as isize)));
        }
    }
}

/// Wählt die Zeile mit den Daten `data` aus; `None` oder unbekannt hebt die Auswahl auf.
fn select(list: HWND, data: Option<usize>) {
    unsafe {
        let mut row = 0;
        let found = loop {
            let value = SendMessageW(list, CB_GETITEMDATA, Some(WPARAM(row)), None).0;
            if value == -1 {
                break usize::MAX; // CB_ERR: Ende der Liste; hebt als Zeilennummer die Auswahl auf
            }
            if Some(value as usize) == data {
                break row;
            }
            row += 1;
        };
        SendMessageW(list, CB_SETCURSEL, Some(WPARAM(found)), None);
    }
}

fn selected(list: HWND) -> Option<usize> {
    unsafe {
        let row = SendMessageW(list, CB_GETCURSEL, None, None).0;
        if row < 0 {
            return None;
        }
        Some(SendMessageW(list, CB_GETITEMDATA, Some(WPARAM(row as usize)), None).0 as usize)
    }
}

/// Die Schrift der Windows-Oberfläche, passend zur Bildschirmskalierung von `window`.
/// Der Aufrufer gibt sie mit `DeleteObject` wieder frei.
pub fn message_font(window: HWND) -> HFONT {
    unsafe {
        let dpi = GetDpiForWindow(window);
        let mut metrics = NONCLIENTMETRICSW {
            cbSize: size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        let ptr = &mut metrics as *mut NONCLIENTMETRICSW as *mut _;
        let _ = SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS.0, metrics.cbSize, Some(ptr), 0, dpi);
        CreateFontIndirectW(&metrics.lfMessageFont)
    }
}

unsafe fn apply_font(c: &Controls) {
    let font = message_font(c.window);
    for child in c.children() {
        SendMessageW(child, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    }
    let old = FONT.with(|cell| cell.replace(font));
    if !old.is_invalid() {
        let _ = DeleteObject(old.into());
    }
}

unsafe fn layout(c: &Controls) {
    let mut client = RECT::default();
    let _ = GetClientRect(c.window, &mut client);
    let dpi = GetDpiForWindow(c.window) as i32;
    let px = |value: i32| value * dpi / 96;
    let (width, height) = (client.right, client.bottom);
    let (margin, gap, row, label, button) = (px(12), px(8), px(26), px(112), px(30));
    let place = |hwnd: HWND, x: i32, y: i32, w: i32, h: i32| {
        let _ = MoveWindow(hwnd, x, y, w.max(0), h.max(0), true);
    };

    let fields = margin + label;
    let sample_width = px(100);
    let mut y = margin;
    place(c.voice_label, margin, y + px(4), label, row);
    // Bei Auswahllisten bestimmt die Höhe, wie weit die Liste aufklappt.
    place(c.voice, fields, y, width - fields - margin - gap - sample_width, px(400));
    place(c.sample, width - margin - sample_width, y - px(1), sample_width, row + px(2));
    y += row + gap;
    place(c.rate_label, margin, y + px(4), label, row);
    place(c.rate, fields, y, px(200), px(400));
    place(c.topmost, fields + px(200) + 2 * gap, y, px(200), row);
    y += row + gap;

    let buttons = height - margin - button;
    place(c.edit, margin, y, width - 2 * margin, buttons - gap - y);
    place(c.read, margin, buttons, px(100), button);
    place(c.pause, margin + px(108), buttons, px(100), button);
    place(c.stop, margin + px(216), buttons, px(80), button);
    // Die übrigen Schaltflächen stehen rechtsbündig.
    let mut right = width - margin;
    for (control, control_width) in [(c.about, 60), (c.settings, 110), (c.save, 150)] {
        right -= px(control_width);
        place(control, right, buttons, px(control_width), button);
        right -= gap;
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(c) = controls() else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    unsafe {
        match msg {
            WM_SIZE => layout(&c),
            WM_SETFOCUS => {
                let _ = SetFocus(Some(c.edit));
            }
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u32, ((wparam.0 >> 16) & 0xffff) as u32);
                if id == ID_CANCEL {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else if let Some(on_command) = ON_COMMAND.with(|cell| cell.get()) {
                    on_command(id, code);
                }
            }
            // Schließen blendet nur aus; beendet wird über das Menü im Infobereich.
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            WM_GETMINMAXINFO => {
                let dpi = GetDpiForWindow(hwnd) as i32;
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                info.ptMinTrackSize.x = 700 * dpi / 96;
                info.ptMinTrackSize.y = 280 * dpi / 96;
            }
            WM_DPICHANGED => {
                let r = &*(lparam.0 as *const RECT);
                let flags = SWP_NOZORDER | SWP_NOACTIVATE;
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, flags);
                apply_font(&c);
                layout(&c);
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}
