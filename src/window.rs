//! Das Hauptfenster: Menüleiste, das Textblatt, unten die Wiedergabe-Leiste (Stimme, Satz
//! zurück, Vorlesen/Pause, Satz vor, Geschwindigkeit, Stopp) und, mit dem Officeassistenten,
//! rechts eine Seitenleiste mit Zusammenfassen, Diktieren und Korrigieren.
//! Schließen blendet es nur aus; das Programm läuft im Infobereich weiter.
//!
//! Die Schaltflächen sind echte Windows-Schaltflächen (Tastatur, Bildschirmleser), zeichnen sich
//! aber selbst (`BS_OWNERDRAW`), hell oder dunkel wie in Windows eingestellt.
//!
//! Die Menüleiste baut `main.rs`, weil sie Stimmen und Zustand kennt; ihre Befehle kommen hier
//! als `WM_COMMAND` mit [`MENU_BASE`] + Befehlsnummer an und gehen an denselben Rückruf wie die
//! Schaltflächen.

use std::cell::{Cell, RefCell};
use std::mem::size_of;

use windows::core::{s, w, HSTRING, PCSTR, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_USE_IMMERSIVE_DARK_MODE};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreateFontW, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    GetWindowDC, InvalidateRect, MapWindowPoints, ReleaseDC, SetBkColor, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, DEFAULT_CHARSET, DT_CENTER, DT_HIDEPREFIX, DT_SINGLELINE, DT_VCENTER, FF_DONTCARE, FW_NORMAL,
    HBRUSH, HDC, HFONT, OUT_DEFAULT_PRECIS, PAINTSTRUCT, TRANSPARENT, CLIP_DEFAULT_PRECIS,
};
use windows::Win32::System::ApplicationInstallationAndServicing::{ActivateActCtx, CreateActCtxW, ACTCTXW};
use windows::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress, LoadLibraryW};
use windows::Win32::System::SystemInformation::GetSystemDirectoryW;
use windows::Win32::UI::Controls::{SetWindowTheme, DRAWITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_HOTLIGHT, ODS_NOACCEL, ODS_SELECTED};
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetFocus, SetFocus};
use windows::Win32::UI::Shell::{DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateAcceleratorTableW, CreateWindowExW, DefWindowProcW, DestroyMenu, DrawMenuBar, GetClientRect, GetMenu,
    GetMenuBarInfo, GetMenuItemInfoW, GetWindowRect, GetWindowTextLengthW, GetWindowTextW, IsChild, IsIconic,
    IsWindowVisible, LoadCursorW, MoveWindow, RegisterClassW, SendMessageW, SetForegroundWindow, SetMenu,
    SetWindowPos, SetWindowTextW, ShowWindow, TranslateAcceleratorW, ACCEL, ACCEL_VIRT_FLAGS, BS_OWNERDRAW,
    CBN_SELCHANGE, CW_USEDEFAULT, ES_AUTOVSCROLL, ES_MULTILINE, ES_NOHIDESEL, ES_WANTRETURN, HACCEL, HICON, HMENU,
    HWND_NOTOPMOST, HWND_TOPMOST, ICON_BIG, ICON_SMALL, IDC_ARROW, MENUBARINFO, MENUITEMINFOW, MIIM_STRING,
    MINMAXINFO, MSG, NONCLIENTMETRICSW, OBJID_MENU, SPI_GETNONCLIENTMETRICS, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLEAR, WM_CLOSE, WM_COMMAND,
    WM_COPY, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_CUT, WM_DPICHANGED, WM_DRAWITEM, WM_DROPFILES, WM_ERASEBKGND,
    WM_GETMINMAXINFO, WM_NCACTIVATE, WM_NCPAINT, WM_PAINT, WM_PASTE, WM_SETFOCUS, WM_SETFONT, WM_SETICON,
    WM_SETTINGCHANGE, WM_SHOWWINDOW, WM_SIZE, WM_UNDO, PostMessageW, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_ACCEPTFILES,
    WS_EX_CONTROLPARENT, WS_OVERLAPPEDWINDOW, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::i18n::t;
use crate::paint::{self, Align, Canvas, Color, Icon};
use crate::theme::{self, Palette};

// Im windows-Crate nur über weitere Features erreichbar, daher hier von Hand.
const ACTCTX_FLAG_ASSEMBLY_DIRECTORY_VALID: u32 = 0x04;
const ACTCTX_FLAG_RESOURCE_NAME_VALID: u32 = 0x08;
const EM_GETSEL: u32 = 0x00B0;
const EM_SETSEL: u32 = 0x00B1;
const EM_REPLACESEL: u32 = 0x00C2;
const EM_SCROLLCARET: u32 = 0x00B7;
const EM_SETLIMITTEXT: u32 = 0x00C5;
const EM_SETMARGINS: u32 = 0x00D3;
/// Menüleiste selbst zeichnen (undokumentiert, aber seit Windows Vista stabil): nötig, damit sie
/// im dunklen Modus nicht hell bleibt.
const WM_UAHDRAWMENU: u32 = 0x0091;
const WM_UAHDRAWMENUITEM: u32 = 0x0092;

// 1 und 2 sind für Eingabe/Esc im Dialog reserviert (IDOK/IDCANCEL).
const ID_CANCEL: u32 = 2;
pub const ID_READ: u32 = 101;
pub const ID_PAUSE: u32 = 102;
pub const ID_STOP: u32 = 103;
pub const ID_PREV: u32 = 112;
pub const ID_NEXT: u32 = 113;
pub const ID_SUMMARIZE: u32 = 114;
pub const ID_SUMMARY_MODE: u32 = 115;
pub const ID_READ_RESULT: u32 = 116;
const ID_RESULT: u32 = 117;
pub const ID_DICTATE: u32 = 118;
pub const ID_CORRECT: u32 = 119;
pub const ID_CORRECTION_MODE: u32 = 120;
pub const ID_SETUP_AI: u32 = 121;
/// Stimme unten links: öffnet die Stimmenauswahl.
pub const ID_VOICE: u32 = 122;
/// Geschwindigkeit unten rechts: öffnet die Auswahl.
pub const ID_SPEED: u32 = 123;
const ID_TABS: [u32; 3] = [124, 125, 126];
const ID_CHIPS: [u32; 4] = [130, 131, 132, 133];
const ID_COPY: u32 = 134;
const ID_APPLY: u32 = 135;
/// Das Fenster wurde gezeigt oder ausgeblendet (Benachrichtigung an `main.rs`).
pub const ID_VISIBILITY: u32 = 140;
/// Meldet die Sichtbarkeit verzögert, damit `main.rs` nicht mitten in eigenem Code gerufen wird.
const WM_VISIBILITY: u32 = windows::Win32::UI::WindowsAndMessaging::WM_APP + 50;
/// Befehle der Menüleiste und der Tastenkürzel: `MENU_BASE` + Befehlsnummer aus `main.rs`.
pub const MENU_BASE: u32 = 20_000;

/// Benachrichtigungscode eines Klicks auf eine Schaltfläche.
pub const CLICKED: u32 = 0;
/// Benachrichtigungscode einer geänderten Auswahl (Art der Zusammenfassung, Korrekturstufe).
pub const SELECTED: u32 = CBN_SELCHANGE;

/// Die beiden Textfelder des Arbeitsbereichs: links der Text, rechts das Ergebnis der KI.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Pane {
    Text,
    Result,
}

/// Bearbeiten-Befehle für das Textfeld.
#[derive(Clone, Copy)]
pub enum Edit {
    Undo,
    Cut,
    Copy,
    Paste,
    SelectAll,
    Clear,
}

/// Register der Seitenleiste.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Tab {
    Summary,
    Dictate,
    Correct,
}

const TABS: [Tab; 3] = [Tab::Summary, Tab::Dictate, Tab::Correct];

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    edit: HWND,
    read: HWND,
    /// Unsichtbar; „Pause“ ist der große Knopf, solange vorgelesen wird.
    pause: HWND,
    stop: HWND,
    prev: HWND,
    next: HWND,
    voice: HWND,
    speed: HWND,
    progress: HWND,
    // Officeassistent
    text_label: HWND,
    text_hint: HWND,
    tabs: [HWND; 3],
    chips_label: HWND,
    chips: [HWND; 4],
    summarize: HWND,
    correct: HWND,
    dictate: HWND,
    dict_status: HWND,
    dict_hint: HWND,
    result_label: HWND,
    result: HWND,
    read_result: HWND,
    copy: HWND,
    apply: HWND,
    ai_status: HWND,
    setup_ai: HWND,
}

impl Controls {
    fn edit_of(&self, pane: Pane) -> HWND {
        match pane {
            Pane::Text => self.edit,
            Pane::Result => self.result,
        }
    }

    fn assistant_controls(&self) -> Vec<HWND> {
        let mut all = vec![
            self.text_label,
            self.text_hint,
            self.chips_label,
            self.summarize,
            self.correct,
            self.dictate,
            self.dict_status,
            self.dict_hint,
            self.result_label,
            self.result,
            self.read_result,
            self.copy,
            self.apply,
            self.ai_status,
            self.setup_ai,
        ];
        all.extend(self.tabs);
        all.extend(self.chips);
        all
    }

    fn children(&self) -> Vec<HWND> {
        let mut all = vec![self.edit, self.read, self.pause, self.stop, self.prev, self.next, self.voice, self.speed, self.progress];
        all.extend(self.assistant_controls());
        all
    }

    /// Liegt `hwnd` in der Seitenleiste oder der Wiedergabe-Leiste (beide in Leistenfarbe)?
    fn on_bar(&self, hwnd: HWND) -> bool {
        let bottom = [self.read, self.stop, self.prev, self.next, self.voice, self.speed, self.progress];
        bottom.contains(&hwnd) || (self.assistant_controls().contains(&hwnd) && ![self.text_label, self.text_hint].contains(&hwnd))
    }
}

/// Pinsel und Schriften des Fensters (neu, wenn sich Farbschema oder Skalierung ändern).
struct Look {
    palette: Palette,
    ground: HBRUSH,
    bar: HBRUSH,
    surface: HBRUSH,
    ui_font: HFONT,
    text_font: HFONT,
    /// Etwas kleiner für das Ergebnis in der schmalen Seitenleiste.
    result_font: HFONT,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static LOOK: RefCell<Option<Look>> = const { RefCell::new(None) };
    static ON_COMMAND: Cell<Option<fn(u32, u32)>> = const { Cell::new(None) };
    static ON_DROP: Cell<Option<fn(String)>> = const { Cell::new(None) };
    static ACCELERATORS: Cell<HACCEL> = const { Cell::new(HACCEL(std::ptr::null_mut())) };
    static ACTIVE: Cell<bool> = const { Cell::new(false) };
    static PAUSED: Cell<bool> = const { Cell::new(false) };
    static ASSISTANT: Cell<bool> = const { Cell::new(false) };
    static DICTATING: Cell<bool> = const { Cell::new(false) };
    static RESULT_KIND: Cell<&'static str> = const { Cell::new("assist.result") };
    static AI_READY: Cell<bool> = const { Cell::new(false) };
    static AI_BUSY: Cell<bool> = const { Cell::new(false) };
    static TAB: Cell<Tab> = const { Cell::new(Tab::Summary) };
    static SUMMARY_MODES: RefCell<(Vec<String>, usize)> = const { RefCell::new((Vec::new(), 0)) };
    static CORRECTION_MODES: RefCell<(Vec<String>, usize)> = const { RefCell::new((Vec::new(), 0)) };
    /// Fortschritt beim Vorlesen: (Satz, Sätze).
    static PROGRESS: Cell<Option<(usize, usize)>> = const { Cell::new(None) };
    static VOICE: RefCell<String> = const { RefCell::new(String::new()) };
    static SPEED: RefCell<String> = const { RefCell::new(String::new()) };
}

fn controls() -> Option<Controls> {
    CONTROLS.with(|c| c.get())
}

fn palette() -> Palette {
    LOOK.with(|l| l.borrow().as_ref().map(|look| look.palette)).unwrap_or_else(theme::palette)
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
        set_app_dark_mode(theme::dark());
    }
}

/// Meldet die Klassen für Tastenkombinations-Feld und Schieberegler an. Die Bibliothek wird
/// erst hier geladen: Ein fester Import würde vor dem Aktivierungskontext die alte Version binden.
unsafe fn init_common_controls() {
    const ICC_BAR_CLASSES: u32 = 0x04;
    const ICC_HOTKEY_CLASS: u32 = 0x40;
    const ICC_PROGRESS_CLASS: u32 = 0x20;
    let Ok(library) = LoadLibraryW(w!("comctl32.dll")) else { return };
    if let Some(init) = GetProcAddress(library, s!("InitCommonControlsEx")) {
        // INITCOMMONCONTROLSEX: Größe der Struktur, gewünschte Klassen.
        let init: unsafe extern "system" fn(*const [u32; 2]) -> i32 = std::mem::transmute(init);
        init(&[8, ICC_BAR_CLASSES | ICC_HOTKEY_CLASS | ICC_PROGRESS_CLASS]);
    }
}

/// Kontextmenüs und Menüs im dunklen Modus dunkel (uxtheme, Ordnungszahlen 135 und 136; seit
/// Windows 10 1903 vorhanden, sonst ohne Wirkung).
fn set_app_dark_mode(dark: bool) {
    unsafe {
        let Ok(library) = LoadLibraryW(w!("uxtheme.dll")) else { return };
        if let Some(set_mode) = GetProcAddress(library, PCSTR(135 as *const u8)) {
            let set_mode: unsafe extern "system" fn(i32) -> i32 = std::mem::transmute(set_mode);
            // 2 = immer dunkel, 3 = immer hell.
            set_mode(if dark { 2 } else { 3 });
        }
        if let Some(flush) = GetProcAddress(library, PCSTR(136 as *const u8)) {
            let flush: unsafe extern "system" fn() = std::mem::transmute(flush);
            flush();
        }
    }
}

/// Legt das (noch unsichtbare) Fenster an. `on_command` erhält die ID des Steuerelements oder
/// Menübefehls und den Benachrichtigungscode, `on_drop` den Pfad einer hineingezogenen Datei.
pub fn create(icon: HICON, on_command: fn(u32, u32), on_drop: fn(String)) -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserDialog"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WS_EX_CONTROLPARENT | WS_EX_ACCEPTFILES,
            class.lpszClassName,
            w!("Hedgie"),
            WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
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

        let child = |class: PCWSTR, style: u32, id: u32| {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                class,
                PCWSTR::null(),
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
        let tab = WS_TABSTOP.0;
        let button = |id: u32| child(w!("BUTTON"), tab | BS_OWNERDRAW as u32, id);
        let label = |style: u32| child(w!("STATIC"), style, 0);
        let edit_style = tab | WS_VSCROLL.0 | (ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | ES_NOHIDESEL) as u32;
        // Die Reihenfolge der Erzeugung ist die Tab-Reihenfolge.
        let c = Controls {
            window,
            edit: child(w!("EDIT"), edit_style, 0),
            tabs: ID_TABS.map(button),
            chips_label: label(0),
            chips: ID_CHIPS.map(button),
            summarize: button(ID_SUMMARIZE),
            correct: button(ID_CORRECT),
            dictate: button(ID_DICTATE),
            dict_status: label(0),
            dict_hint: label(0),
            result_label: label(0),
            result: child(w!("EDIT"), edit_style, ID_RESULT),
            read_result: button(ID_READ_RESULT),
            copy: button(ID_COPY),
            apply: button(ID_APPLY),
            ai_status: label(0x4000 /* SS_ENDELLIPSIS */),
            setup_ai: button(ID_SETUP_AI),
            voice: button(ID_VOICE),
            prev: button(ID_PREV),
            read: button(ID_READ),
            next: button(ID_NEXT),
            progress: label(0x2 /* SS_RIGHT */),
            speed: button(ID_SPEED),
            stop: button(ID_STOP),
            pause: child(w!("BUTTON"), 0, ID_PAUSE),
            text_label: label(0),
            text_hint: label(0x2 /* SS_RIGHT */ | 0x4000),
        };
        let _ = ShowWindow(c.pause, SW_HIDE);
        for control in c.assistant_controls() {
            let _ = ShowWindow(control, SW_HIDE);
        }
        // Ohne Grenze fasst ein Textfeld nur rund 30 000 Zeichen.
        SendMessageW(c.edit, EM_SETLIMITTEXT, Some(WPARAM(0)), Some(LPARAM(0)));
        SendMessageW(c.result, EM_SETLIMITTEXT, Some(WPARAM(0)), Some(LPARAM(0)));
        CONTROLS.with(|cell| cell.set(Some(c)));
        ON_COMMAND.with(|cell| cell.set(Some(on_command)));
        ON_DROP.with(|cell| cell.set(Some(on_drop)));
        relabel();
        for size in [ICON_SMALL, ICON_BIG] {
            SendMessageW(window, WM_SETICON, Some(WPARAM(size as usize)), Some(LPARAM(icon.0 as isize)));
        }
        apply_look(&c);
        let dpi = GetDpiForWindow(window) as i32;
        let (width, height) = (860 * dpi / 96, 580 * dpi / 96);
        let _ = SetWindowPos(window, None, 0, 0, width, height, SWP_NOZORDER | SWP_NOACTIVATE | SWP_NOMOVE);
        Some(window)
    }
}

/// Farben, Schriften und dunkle Titelleiste nach dem Windows-Farbschema.
unsafe fn apply_look(c: &Controls) {
    let p = theme::palette();
    let dpi = GetDpiForWindow(c.window).max(96) as i32;
    let ui_font = message_font(c.window);
    // Das Textblatt in angenehmer Lesegröße (16 px bei 100 %).
    let font = |size: i32| CreateFontW(
        -(size * dpi / 96),
        0,
        0,
        0,
        FW_NORMAL.0 as i32,
        0,
        0,
        0,
        DEFAULT_CHARSET,
        OUT_DEFAULT_PRECIS,
        CLIP_DEFAULT_PRECIS,
        CLEARTYPE_QUALITY,
        FF_DONTCARE.0 as u32,
        w!("Segoe UI"),
    );
    let (text_font, result_font) = (font(16), font(14));
    let look = Look {
        palette: p,
        ground: CreateSolidBrush(p.ground.colorref()),
        bar: CreateSolidBrush(p.bar.colorref()),
        surface: CreateSolidBrush(p.surface.colorref()),
        ui_font,
        text_font,
        result_font,
    };
    for child in c.children() {
        let font = if child == c.edit {
            text_font
        } else if child == c.result {
            result_font
        } else {
            ui_font
        };
        SendMessageW(child, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
    }
    // Innenabstand im Textblatt.
    let margin = (24 * dpi / 96) as isize;
    SendMessageW(c.edit, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM(margin | margin << 16)));
    let small = (12 * dpi / 96) as isize;
    SendMessageW(c.result, EM_SETMARGINS, Some(WPARAM(3)), Some(LPARAM(small | small << 16)));
    let scroll_theme = if p.dark { w!("DarkMode_Explorer") } else { w!("Explorer") };
    for edit in [c.edit, c.result] {
        let _ = SetWindowTheme(edit, scroll_theme, PCWSTR::null());
    }
    let dark = windows::core::BOOL::from(p.dark);
    let _ = DwmSetWindowAttribute(c.window, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const _, size_of::<windows::core::BOOL>() as u32);
    set_app_dark_mode(p.dark);
    if let Some(old) = LOOK.with(|l| l.borrow_mut().replace(look)) {
        for brush in [old.ground, old.bar, old.surface] {
            let _ = DeleteObject(brush.into());
        }
        for font in [old.ui_font, old.text_font, old.result_font] {
            let _ = DeleteObject(font.into());
        }
    }
    let _ = DrawMenuBar(c.window);
    let _ = InvalidateRect(Some(c.window), None, true);
}

/// Färbt das Fenster neu, wenn sich hell/dunkel geändert hat (in Windows oder in den Einstellungen).
pub fn refresh_look() {
    let Some(c) = controls() else { return };
    if palette().dark != theme::dark() {
        unsafe {
            apply_look(&c);
            for child in c.children() {
                let _ = InvalidateRect(Some(child), None, true);
            }
        }
    }
}

/// Setzt alle Beschriftungen in der eingestellten Sprache (beim Anlegen und nach einem Wechsel).
pub fn relabel() {
    let Some(c) = controls() else { return };
    let texts = [
        (c.stop, t("app.stop")),
        (c.prev, t("window.prev_sentence")),
        (c.next, t("window.next_sentence")),
        (c.summarize, t("assist.summarize")),
        (c.read_result, t("assist.read_short")),
        (c.copy, t("assist.copy")),
        (c.apply, t("assist.apply")),
        (c.text_label, t("assist.text_label")),
        (c.text_hint, t("assist.text_hint")),
        (c.correct, t("assist.correct")),
        (c.setup_ai, t("assist.setup_ai")),
        (c.tabs[0], t("assist.tab_summary")),
        (c.tabs[1], t("assist.tab_dictate")),
        (c.tabs[2], t("assist.tab_correct")),
        (c.speed, t("window.speed_button")),
    ];
    for (control, text) in texts {
        set_label(control, text);
    }
    show_playback(&c);
    let dictating = DICTATING.with(|d| d.get());
    set_label(c.dictate, if dictating { t("assist.dictate_stop") } else { t("assist.dictate") });
    set_label(c.result_label, t(RESULT_KIND.with(|k| k.get())));
    update_tab(&c);
}

fn set_label(control: HWND, text: &str) {
    unsafe {
        let _ = SetWindowTextW(control, &HSTRING::from(text));
    }
}

fn window_text(control: HWND) -> String {
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(control) as usize + 1];
        let len = GetWindowTextW(control, &mut buffer) as usize;
        String::from_utf16_lossy(&buffer[..len])
    }
}

/// Blendet den Arbeitsbereich des Officeassistenten ein oder aus.
pub fn set_assistant(on: bool) {
    let Some(c) = controls() else { return };
    ASSISTANT.with(|a| a.set(on));
    if !on {
        for control in c.assistant_controls() {
            unsafe {
                let _ = ShowWindow(control, SW_HIDE);
            }
        }
    }
    update_tab(&c);
    unsafe {
        layout(&c);
        let _ = InvalidateRect(Some(c.window), None, true);
    }
}

/// Zeigt die Bedienelemente des gewählten Registers (und nur diese).
fn update_tab(c: &Controls) {
    if !ASSISTANT.with(|a| a.get()) {
        return;
    }
    let tab = TAB.with(|t| t.get());
    let ready = AI_READY.with(|r| r.get());
    let (labels, _) = mode_labels(tab);
    let ai_tab = tab != Tab::Dictate;
    let show = |control: HWND, visible: bool| unsafe {
        let _ = ShowWindow(control, if visible { SW_SHOW } else { SW_HIDE });
    };
    for control in [c.text_label, c.text_hint] {
        show(control, true);
    }
    c.tabs.iter().for_each(|t| show(*t, true));
    show(c.chips_label, ai_tab);
    for (i, chip) in c.chips.iter().enumerate() {
        show(*chip, ai_tab && i < labels.len());
        if let Some(label) = labels.get(i) {
            set_label(*chip, label);
        }
    }
    set_label(c.chips_label, if tab == Tab::Summary { t("assist.kind") } else { t("assist.level") });
    let result = match tab {
        Tab::Correct => "assist.corrected",
        _ if RESULT_KIND.with(|k| k.get()) == "assist.minutes" => "assist.minutes",
        _ => "assist.result",
    };
    set_label(c.result_label, t(result));
    show(c.summarize, tab == Tab::Summary);
    show(c.correct, tab == Tab::Correct);
    for control in [c.result_label, c.result, c.read_result, c.copy, c.apply] {
        show(control, ai_tab);
    }
    for control in [c.dictate, c.dict_status, c.dict_hint] {
        show(control, tab == Tab::Dictate);
    }
    show(c.ai_status, ready);
    show(c.setup_ai, !ready);
    show_ai_state(c);
    unsafe {
        layout(c);
        for control in c.tabs.iter().chain(&c.chips) {
            let _ = InvalidateRect(Some(*control), None, false);
        }
        let _ = InvalidateRect(Some(c.window), None, true);
    }
}

fn select_tab(c: &Controls, tab: Tab) {
    TAB.with(|t| t.set(tab));
    update_tab(c);
}

/// Beschriftungen und Auswahl der Stufen des Registers.
fn mode_labels(tab: Tab) -> (Vec<String>, usize) {
    match tab {
        Tab::Summary => SUMMARY_MODES.with(|m| m.borrow().clone()),
        Tab::Correct => CORRECTION_MODES.with(|m| m.borrow().clone()),
        Tab::Dictate => (Vec::new(), 0),
    }
}

/// Ohne eingerichtete KI sind deren Schaltflächen gesperrt; statt des Status steht dann
/// „KI einrichten …“ da. Diktieren geht immer.
pub fn set_ai_ready(ready: bool) {
    AI_READY.with(|r| r.set(ready));
    if let Some(c) = controls() {
        update_tab(&c);
    }
}

fn show_ai_state(c: &Controls) {
    let ready = AI_READY.with(|r| r.get());
    let busy = AI_BUSY.with(|b| b.get());
    // Während die KI schreibt, bricht derselbe Knopf ab.
    set_label(c.summarize, if busy { t("assist.cancel") } else { t("assist.summarize") });
    set_label(c.correct, if busy { t("assist.cancel") } else { t("assist.correct") });
    unsafe {
        for control in [c.summarize, c.correct] {
            let _ = EnableWindow(control, ready);
            let _ = InvalidateRect(Some(control), None, false);
        }
        for control in c.chips.iter().chain([&c.read_result, &c.copy, &c.apply]) {
            let _ = EnableWindow(*control, ready);
        }
    }
}

/// Zeigt unten die eingestellte Stimme.
pub fn set_voice(name: &str) {
    VOICE.with(|v| *v.borrow_mut() = name.to_string());
    if let Some(c) = controls() {
        set_label(c.voice, &format!("{} {name}", t("window.voice")));
        unsafe {
            let _ = InvalidateRect(Some(c.voice), None, false);
        }
    }
}

/// Zeigt unten die Geschwindigkeit, z. B. „1,2×“.
pub fn set_speed(label: &str) {
    SPEED.with(|s| *s.borrow_mut() = label.to_string());
    if let Some(c) = controls() {
        unsafe {
            let _ = InvalidateRect(Some(c.speed), None, false);
        }
    }
}

/// Fortschritt beim Vorlesen („Satz 3 von 9“ und der Balken über der Leiste); `None` im Leerlauf.
pub fn set_progress(progress: Option<(usize, usize)>) {
    if PROGRESS.with(|p| p.replace(progress)) == progress {
        return;
    }
    let Some(c) = controls() else { return };
    let text = progress.map_or(String::new(), |(current, total)| {
        t("player.sentence").replacen("{}", &current.to_string(), 1).replacen("{}", &total.to_string(), 1)
    });
    set_label(c.progress, &text);
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(c.window, &mut client);
        let dpi = GetDpiForWindow(c.window).max(96) as i32;
        let top = client.bottom - BAR * dpi / 96;
        let line = RECT { left: 0, top, right: client.right, bottom: top + 4 * dpi / 96 };
        let _ = InvalidateRect(Some(c.window), Some(&line), false);
    }
}

/// Bildschirmrechteck einer Schaltfläche, z. B. um darüber ein Menü zu öffnen.
pub fn anchor(id: u32) -> Option<RECT> {
    let c = controls()?;
    let control = match id {
        ID_VOICE => c.voice,
        ID_SPEED => c.speed,
        _ => return None,
    };
    let mut rect = RECT::default();
    unsafe { GetWindowRect(control, &mut rect).ok()? };
    Some(rect)
}

/// Überschrift des Ergebnisses, z. B. „Zusammenfassung“ oder „Korrigierte Fassung“; wechselt
/// dabei ins passende Register.
pub fn set_result_label(key: &'static str) {
    RESULT_KIND.with(|k| k.set(key));
    if let Some(c) = controls() {
        set_label(c.result_label, t(key));
        let tab = if key == "assist.corrected" { Tab::Correct } else { Tab::Summary };
        if ASSISTANT.with(|a| a.get()) && TAB.with(|t| t.get()) != tab {
            select_tab(&c, tab);
        }
    }
}

/// Schaltfläche und Hinweis während eines Diktats.
pub fn set_dictating(on: bool, status: &str) {
    DICTATING.with(|d| d.set(on));
    if let Some(c) = controls() {
        set_label(c.dictate, if on { t("assist.dictate_stop") } else { t("assist.dictate") });
        set_label(c.dict_status, status);
        unsafe {
            let _ = InvalidateRect(Some(c.dictate), None, false);
        }
        if on && ASSISTANT.with(|a| a.get()) && TAB.with(|t| t.get()) != Tab::Dictate {
            select_tab(&c, Tab::Dictate);
        }
    }
}

/// Hinweis im Register „Diktieren“, z. B. mit der Tastenkombination für andere Programme.
pub fn set_dict_hint(text: &str) {
    if let Some(c) = controls() {
        set_label(c.dict_hint, text);
    }
}

pub fn set_correction_modes(labels: &[&str], selected: usize) {
    CORRECTION_MODES.with(|m| *m.borrow_mut() = (labels.iter().map(|l| l.to_string()).collect(), selected));
    if let Some(c) = controls() {
        update_tab(&c);
    }
}

pub fn selected_correction_mode() -> Option<usize> {
    Some(CORRECTION_MODES.with(|m| m.borrow().1))
}

pub fn set_summary_modes(labels: &[&str], selected: usize) {
    SUMMARY_MODES.with(|m| *m.borrow_mut() = (labels.iter().map(|l| l.to_string()).collect(), selected));
    if let Some(c) = controls() {
        update_tab(&c);
    }
}

pub fn selected_summary_mode() -> Option<usize> {
    Some(SUMMARY_MODES.with(|m| m.borrow().1))
}

/// Fügt diktierten Text an der Schreibmarke des linken Feldes ein (mit Leerzeichen davor, wenn
/// dort schon Text steht) und lässt sich mit Strg+Z zurücknehmen.
pub fn insert_text(text: &str) {
    let Some(source) = source() else { return };
    let Some(c) = controls() else { return };
    let before = source.content.get(..source.selection.0 as usize).unwrap_or_default();
    let text = HSTRING::from(dictated_text(&String::from_utf16_lossy(before), text));
    unsafe {
        SendMessageW(c.edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(text.as_ptr() as isize)));
        SendMessageW(c.edit, EM_SCROLLCARET, None, None);
    }
}

/// Fügt einen erkannten Abschnitt an `before` an: mit Leerzeichen dazwischen und großem Anfang
/// nach einem Satzende (die Erkennung arbeitet abschnittsweise und weiß das nicht).
pub fn dictated_text(before: &str, text: &str) -> String {
    let last = before.chars().last();
    let space = if last.is_some_and(|ch| !ch.is_whitespace()) { " " } else { "" };
    let sentence_start = before.trim_end().chars().last().is_none_or(|ch| matches!(ch, '.' | '!' | '?' | ':'));
    let mut chars = text.chars();
    let first = chars.next().map(|ch| if sentence_start { ch.to_uppercase().collect() } else { ch.to_string() });
    format!("{space}{}{}", first.unwrap_or_default(), chars.as_str())
}

/// Zeile unten in der Seitenleiste, z. B. Anbieter und Modell.
pub fn set_ai_status(text: &str) {
    if let Some(c) = controls() {
        set_label(c.ai_status, text);
    }
}

/// Während die KI arbeitet, sind „Zusammenfassen“ und „Korrigieren“ gesperrt.
pub fn set_ai_busy(busy: bool) {
    AI_BUSY.with(|b| b.set(busy));
    if let Some(c) = controls() {
        show_ai_state(&c);
    }
}

/// Zeigt die entstehende Antwort der KI rechts und rollt ans Ende.
pub fn set_result_live(text: &str) {
    if let Some(c) = controls() {
        let text = HSTRING::from(text);
        unsafe {
            let _ = SetWindowTextW(c.result, &text);
            let len = GetWindowTextLengthW(c.result) as usize;
            SendMessageW(c.result, EM_SETSEL, Some(WPARAM(len)), Some(LPARAM(len as isize)));
            SendMessageW(c.result, EM_SCROLLCARET, None, None);
        }
    }
}

/// Das Textfeld mit der Tastatureingabe.
pub fn focused_pane() -> Pane {
    let Some(c) = controls() else { return Pane::Text };
    let focus = unsafe { GetFocus() };
    if ASSISTANT.with(|a| a.get()) && focus == c.result { Pane::Result } else { Pane::Text }
}

/// Ersetzt die Menüleiste; die alte wird freigegeben.
pub fn set_menu_bar(menu: HMENU) {
    let Some(c) = controls() else { return };
    unsafe {
        let old = GetMenu(c.window);
        let _ = SetMenu(c.window, Some(menu));
        if !old.is_invalid() {
            let _ = DestroyMenu(old);
        }
        let _ = DrawMenuBar(c.window);
    }
}

/// Tastenkürzel des Fensters: (Zusatztasten wie `FCONTROL | FVIRTKEY`, Taste, Befehlsnummer).
pub fn set_accelerators(entries: &[(u8, u16, u32)]) {
    let table: Vec<ACCEL> = entries
        .iter()
        .map(|&(flags, key, cmd)| ACCEL { fVirt: ACCEL_VIRT_FLAGS(flags), key, cmd: (MENU_BASE + cmd) as u16 })
        .collect();
    if let Ok(handle) = unsafe { CreateAcceleratorTableW(&table) } {
        ACCELERATORS.with(|cell| cell.set(handle));
    }
}

/// In der Nachrichtenschleife vor allem anderen aufrufen; `true`, wenn die Nachricht ein
/// Tastenkürzel des Fensters war.
pub fn translate_accelerator(msg: &MSG) -> bool {
    let (Some(c), accelerators) = (controls(), ACCELERATORS.with(|cell| cell.get())) else { return false };
    if accelerators.is_invalid() {
        return false;
    }
    unsafe {
        let ours = msg.hwnd == c.window || IsChild(c.window, msg.hwnd).as_bool();
        ours && TranslateAcceleratorW(c.window, accelerators, msg) != 0
    }
}

/// Das Fenster, solange es sichtbar ist (als Besitzer für Meldungen).
pub fn visible() -> Option<HWND> {
    let c = controls()?;
    unsafe { IsWindowVisible(c.window).as_bool().then_some(c.window) }
}

/// Ist das Fenster offen und nicht minimiert? (Sonst übernehmen Mini-Pille und Mini-Player.)
pub fn is_open() -> bool {
    controls().is_some_and(|c| unsafe { IsWindowVisible(c.window).as_bool() && !IsIconic(c.window).as_bool() })
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

pub fn hide() {
    if let Some(c) = controls() {
        unsafe {
            let _ = ShowWindow(c.window, SW_HIDE);
        }
    }
}

/// Fenstertitel, z. B. während eine Audiodatei geschrieben wird; `None` = „Vorleser“.
pub fn set_title(title: Option<&str>) {
    if let Some(c) = controls() {
        set_label(c.window, title.unwrap_or("Hedgie"));
    }
}

/// Inhalt des Textfelds und die Markierung darin (Anfang und Ende in UTF-16-Einheiten).
pub struct Source {
    pub content: Vec<u16>,
    pub selection: (u32, u32),
}

pub fn source() -> Option<Source> {
    source_of(Pane::Text)
}

pub fn source_of(pane: Pane) -> Option<Source> {
    let c = controls()?;
    let edit = c.edit_of(pane);
    unsafe {
        let mut content = vec![0u16; GetWindowTextLengthW(edit) as usize + 1];
        let len = GetWindowTextW(edit, &mut content) as usize;
        content.truncate(len);
        let (mut start, mut end) = (0u32, 0u32);
        SendMessageW(edit, EM_GETSEL, Some(WPARAM(&mut start as *mut u32 as usize)), Some(LPARAM(&mut end as *mut u32 as isize)));
        Some(Source { content, selection: (start.min(len as u32), end.min(len as u32)) })
    }
}

/// Ersetzt den Inhalt des Textfelds. Anders als `SetWindowTextW` lässt sich das mit Strg+Z
/// rückgängig machen, sodass ein vorher eingefügter Text nicht verloren ist.
pub fn set_text(text: &str) {
    set_text_in(Pane::Text, text);
}

pub fn set_text_in(pane: Pane, text: &str) {
    if let Some(c) = controls() {
        let edit = c.edit_of(pane);
        let text = HSTRING::from(text);
        unsafe {
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
            SendMessageW(edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(text.as_ptr() as isize)));
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(0)));
            SendMessageW(edit, EM_SCROLLCARET, None, None);
        }
    }
}

pub fn edit(action: Edit) {
    let Some(c) = controls() else { return };
    // Gilt für das Textfeld, in dem gerade geschrieben wird.
    let target = c.edit_of(focused_pane());
    unsafe {
        match action {
            Edit::Undo => SendMessageW(target, WM_UNDO, None, None),
            Edit::Cut => SendMessageW(target, WM_CUT, None, None),
            Edit::Copy => SendMessageW(target, WM_COPY, None, None),
            Edit::Paste => SendMessageW(target, WM_PASTE, None, None),
            Edit::SelectAll => SendMessageW(target, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1))),
            Edit::Clear => {
                SendMessageW(target, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
                SendMessageW(target, WM_CLEAR, None, None)
            }
        };
        let _ = SetFocus(Some(target));
    }
}

/// Markiert einen Bereich des Textfelds und rollt ihn ins Bild (Mitlesen).
pub fn mark(start: u32, end: u32) {
    mark_in(Pane::Text, start, end);
}

pub fn mark_in(pane: Pane, start: u32, end: u32) {
    if let Some(c) = controls() {
        let edit = c.edit_of(pane);
        unsafe {
            SendMessageW(edit, EM_SETSEL, Some(WPARAM(start as usize)), Some(LPARAM(end as isize)));
            SendMessageW(edit, EM_SCROLLCARET, None, None);
        }
    }
}

/// Hält das Fenster über anderen Fenstern (oder nicht).
pub fn set_topmost(on: bool) {
    if let Some(c) = controls() {
        unsafe {
            let order = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
            let _ = SetWindowPos(c.window, Some(order), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// Der markierte Teil des Textfelds, sonst der ganze Text; `None`, wenn nichts drinsteht.
pub fn text() -> Option<String> {
    let source = source()?;
    let (start, end) = (source.selection.0 as usize, source.selection.1 as usize);
    let part = if start < end { &source.content[start..end] } else { &source.content[..] };
    let text = String::from_utf16_lossy(part);
    (!text.trim().is_empty()).then_some(text)
}

/// Passt die Wiedergabe-Leiste an den Zustand der Sprachausgabe an.
pub fn set_playback(active: bool, paused: bool) {
    ACTIVE.with(|a| a.set(active));
    PAUSED.with(|p| p.set(paused));
    if let Some(c) = controls() {
        show_playback(&c);
    }
}

fn show_playback(c: &Controls) {
    let (active, paused) = (ACTIVE.with(|a| a.get()), PAUSED.with(|p| p.get()));
    // Der große Knopf heißt, was er tut (für Bildschirmleser).
    let label = match (active, paused) {
        (false, _) => t("window.read_aloud"),
        (true, false) => t("app.pause"),
        (true, true) => t("app.resume"),
    };
    set_label(c.read, label);
    unsafe {
        let _ = EnableWindow(c.stop, active);
        for control in [c.read, c.stop] {
            let _ = InvalidateRect(Some(control), None, false);
        }
    }
}

/// Die Schrift der Windows-Oberfläche, passend zur Bildschirmskalierung von `window`.
/// Der Aufrufer gibt sie mit `DeleteObject` wieder frei.
pub fn message_font(window: HWND) -> HFONT {
    unsafe {
        let dpi = GetDpiForWindow(window);
        let mut metrics = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let ptr = &mut metrics as *mut NONCLIENTMETRICSW as *mut _;
        let _ = SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS.0, metrics.cbSize, Some(ptr), 0, dpi);
        CreateFontIndirectW(&metrics.lfMessageFont)
    }
}

/// Höhe der Wiedergabe-Leiste und Breite der Seitenleiste (bei 100 %).
const BAR: i32 = 80;
const PANEL: i32 = 360;

/// Flächen, die das Fenster selbst malt (Pixel).
struct Areas {
    card: RECT,
    panel: Option<RECT>,
    tabs: Option<RECT>,
    result_card: Option<RECT>,
    status_dot: Option<(i32, i32)>,
    bar: RECT,
}

/// Ordnet die Steuerelemente an und liefert die selbst gemalten Flächen.
unsafe fn layout(c: &Controls) -> Areas {
    let mut client = RECT::default();
    let _ = GetClientRect(c.window, &mut client);
    let dpi = GetDpiForWindow(c.window).max(96) as i32;
    let px = |value: i32| value * dpi / 96;
    let (width, height) = (client.right, client.bottom);
    let place = |hwnd: HWND, x: i32, y: i32, w: i32, h: i32| {
        let _ = MoveWindow(hwnd, x, y, w.max(0), h.max(0), true);
    };
    let bar_top = height - px(BAR);
    let bar = RECT { left: 0, top: bar_top, right: width, bottom: height };

    // Wiedergabe-Leiste: Stimme links, Satz/Vorlesen/Satz in der Mitte, rechts Geschwindigkeit und Stopp.
    let cy = bar_top + px(BAR) / 2 + px(2);
    place(c.voice, px(16), cy - px(22), px(210), px(44));
    let center = width / 2;
    place(c.read, center - px(29), cy - px(29), px(58), px(58));
    place(c.prev, center - px(29) - px(16) - px(44), cy - px(22), px(44), px(44));
    place(c.next, center + px(29) + px(16), cy - px(22), px(44), px(44));
    place(c.stop, width - px(16) - px(40), cy - px(20), px(40), px(40));
    place(c.speed, width - px(16) - px(40) - px(10) - px(70), cy - px(19), px(70), px(38));
    let progress_right = width - px(16) - px(40) - px(10) - px(70) - px(12);
    let progress_left = (center + px(29) + px(16) + px(44) + px(12)).min(progress_right);
    place(c.progress, progress_left, cy - px(9), progress_right - progress_left, px(20));

    let assistant = ASSISTANT.with(|a| a.get());
    let content_right = if assistant { width - px(PANEL) } else { width };
    let mut top = px(20);
    if assistant {
        place(c.text_label, px(20), top, px(200), px(20));
        place(c.text_hint, px(220), top, content_right - px(240), px(20));
        top += px(28);
    }
    let available = content_right - px(40);
    let card_w = if assistant { available } else { available.min(px(860)) };
    let card_x = (content_right - card_w) / 2;
    let card = RECT { left: card_x, top, right: card_x + card_w, bottom: bar_top - px(20) };
    // Das Textfeld liegt innen im Blatt, mit Luft oben und unten.
    place(c.edit, card.left + px(1), card.top + px(16), card.right - card.left - px(2), card.bottom - card.top - px(28));

    let mut areas = Areas { card, panel: None, tabs: None, result_card: None, status_dot: None, bar };
    if !assistant {
        return areas;
    }
    let x0 = content_right;
    let panel = RECT { left: x0, top: 0, right: width, bottom: bar_top };
    areas.panel = Some(panel);
    let (pad, inner) = (px(18), px(PANEL) - 2 * px(18));
    let x = x0 + pad;
    let mut y = px(18);
    let tabs = RECT { left: x, top: y, right: x + inner, bottom: y + px(40) };
    areas.tabs = Some(tabs);
    let tab_w = (inner - px(8)) / 3;
    for (i, tab) in c.tabs.iter().enumerate() {
        place(*tab, x + px(4) + i as i32 * tab_w, y + px(4), tab_w, px(32));
    }
    y += px(52);
    // Unten: KI-Status oder „KI einrichten …“.
    let status_y = bar_top - px(18) - px(36);
    place(c.setup_ai, x, status_y, inner, px(36));
    place(c.ai_status, x + px(16), status_y + px(9), inner - px(16), px(20));
    areas.status_dot = Some((x + px(4), status_y + px(18)));
    match TAB.with(|t| t.get()) {
        Tab::Dictate => {
            place(c.dictate, x, y, inner, px(52));
            y += px(64);
            place(c.dict_status, x, y, inner, px(40));
            y += px(48);
            place(c.dict_hint, x, y, inner, px(80));
        }
        tab => {
            place(c.chips_label, x, y, inner, px(18));
            y += px(24);
            // Stufen als Knöpfe, die umbrechen, wenn die Zeile voll ist.
            let (labels, _) = mode_labels(tab);
            let mut cx = x;
            for (i, chip) in c.chips.iter().enumerate().take(labels.len()) {
                let w = px((paint::measure(&labels[i], 13.0, false) + 28.0) as i32);
                if cx > x && cx + w > x + inner {
                    cx = x;
                    y += px(38);
                }
                place(*chip, cx, y, w.min(inner), px(32));
                cx += w + px(6);
            }
            y += px(46);
            let action = if tab == Tab::Summary { c.summarize } else { c.correct };
            place(action, x, y, inner, px(42));
            y += px(56);
            place(c.result_label, x, y, inner, px(18));
            y += px(24);
            let result_bottom = status_y - px(12) - px(32) - px(12);
            let result_card = RECT { left: x, top: y, right: x + inner, bottom: result_bottom.max(y + px(40)) };
            areas.result_card = Some(result_card);
            place(c.result, x + px(1), y + px(8), inner - px(2), result_card.bottom - y - px(16));
            let mut bx = x;
            let actions_y = result_card.bottom + px(12);
            for (control, key) in [(c.apply, "assist.apply"), (c.copy, "assist.copy"), (c.read_result, "assist.read_short")] {
                let w = px((paint::measure(t(key), 12.0, false) + 26.0) as i32);
                place(control, bx, actions_y, w, px(32));
                bx += w + px(8);
            }
        }
    }
    areas
}

/// Malt den Hintergrund: Fläche, Textblatt, Seitenleiste, Wiedergabe-Leiste mit Fortschritt.
unsafe fn paint_background(c: &Controls, hdc: HDC) {
    let areas = layout_only(c);
    let mut client = RECT::default();
    let _ = GetClientRect(c.window, &mut client);
    let dpi = GetDpiForWindow(c.window).max(96);
    let p = palette();
    let scale = 96.0 / dpi as f32;
    let f = |v: i32| v as f32 * scale;
    let r = |rect: &RECT| (f(rect.left), f(rect.top), f(rect.right - rect.left), f(rect.bottom - rect.top));
    paint::draw_dc(hdc, client, dpi, p.ground, |canvas, w, _| {
        let (x, y, cw, ch) = r(&areas.card);
        canvas.fill_round(x, y, cw, ch, 12.0, p.surface);
        canvas.stroke_round(x, y, cw, ch, 12.0, p.border.alpha(0.8), 1.0);
        if let Some(panel) = &areas.panel {
            let (x, y, pw, ph) = r(panel);
            canvas.fill_round(x, y, pw, ph, 0.0, p.bar);
            canvas.line(x + 0.5, y, x + 0.5, y + ph, p.border.alpha(0.8), 1.0);
        }
        if let Some(tabs) = &areas.tabs {
            let (x, y, tw, th) = r(tabs);
            canvas.fill_round(x, y, tw, th, 10.0, if p.dark { paint::rgb(0x16181C) } else { p.raised });
        }
        if let Some(card) = &areas.result_card {
            let (x, y, rw, rh) = r(card);
            canvas.fill_round(x, y, rw, rh, 10.0, p.surface);
            canvas.stroke_round(x, y, rw, rh, 10.0, p.border.alpha(0.8), 1.0);
        }
        if let Some((dx, dy)) = areas.status_dot {
            if AI_READY.with(|r| r.get()) {
                canvas.circle(f(dx), f(dy), 4.0, p.ok);
            }
        }
        let (x, y, bw, bh) = r(&areas.bar);
        canvas.fill_round(x, y, bw, bh, 0.0, p.bar);
        canvas.line(x, y + 0.5, x + bw, y + 0.5, p.border.alpha(0.8), 1.0);
        // Fortschritt als dünne Linie über der Leiste.
        if let Some((current, total)) = PROGRESS.with(|pr| pr.get()) {
            let share = (current as f32 / total.max(1) as f32).clamp(0.0, 1.0);
            canvas.fill_round(x, y, w, 3.0, 0.0, p.border.alpha(0.6));
            canvas.fill_round(x, y, w * share, 3.0, 0.0, p.accent);
        }
    });
}

/// Wie [`layout`], ohne etwas zu verschieben (zum Malen).
unsafe fn layout_only(c: &Controls) -> Areas {
    // Verschieben ohne Änderung ist billig; MoveWindow zeichnet nur bei echter Änderung neu.
    layout(c)
}

/// Welche Art Knopf ein Steuerelement ist.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Play,
    Round(Icon),
    Stop,
    Voice,
    Speed,
    Tab(usize),
    Chip(usize),
    Primary,
    Ghost,
}

fn kind_of(c: &Controls, hwnd: HWND) -> Kind {
    if hwnd == c.read {
        return Kind::Play;
    }
    if hwnd == c.prev {
        return Kind::Round(Icon::Prev);
    }
    if hwnd == c.next {
        return Kind::Round(Icon::Next);
    }
    if hwnd == c.stop {
        return Kind::Stop;
    }
    if hwnd == c.voice {
        return Kind::Voice;
    }
    if hwnd == c.speed {
        return Kind::Speed;
    }
    if let Some(i) = c.tabs.iter().position(|t| *t == hwnd) {
        return Kind::Tab(i);
    }
    if let Some(i) = c.chips.iter().position(|t| *t == hwnd) {
        return Kind::Chip(i);
    }
    if [c.summarize, c.correct, c.dictate, c.setup_ai].contains(&hwnd) {
        return Kind::Primary;
    }
    Kind::Ghost
}

/// Malt eine selbst gezeichnete Schaltfläche.
unsafe fn draw_button(c: &Controls, item: &DRAWITEMSTRUCT) {
    let p = palette();
    let hwnd = item.hwndItem;
    let state = item.itemState;
    let pressed = state.0 & ODS_SELECTED.0 != 0;
    let disabled = state.0 & ODS_DISABLED.0 != 0;
    let focused = state.0 & ODS_FOCUS.0 != 0;
    let kind = kind_of(c, hwnd);
    let background = if c.on_bar(hwnd) { p.bar } else { p.ground };
    let text = window_text(hwnd);
    let dpi = GetDpiForWindow(hwnd).max(96);
    let press = |color: Color| if pressed { color.mix(p.text, 0.12) } else { color };
    let (active, paused) = (ACTIVE.with(|a| a.get()), PAUSED.with(|pa| pa.get()));
    let tab = TAB.with(|t| t.get());
    paint::draw_dc(item.hDC, item.rcItem, dpi, background, |canvas: &Canvas, w, h| {
        let (cx, cy) = (w / 2.0, h / 2.0);
        match kind {
            Kind::Play => {
                canvas.circle(cx, cy, w / 2.0 - 1.0, press(p.accent));
                let icon = if active && !paused { Icon::Pause } else { Icon::Play };
                canvas.icon(icon, cx + if icon == Icon::Play { 1.5 } else { 0.0 }, cy, 24.0, p.on_accent);
            }
            Kind::Round(icon) => {
                if pressed {
                    canvas.circle(cx, cy, w / 2.0 - 1.0, p.raised);
                }
                canvas.icon(icon, cx, cy, 22.0, if disabled { p.muted } else { p.text });
            }
            Kind::Stop => {
                canvas.circle(cx, cy, w / 2.0 - 1.0, press(p.ground));
                canvas.stroke_round(1.0, 1.0, w - 2.0, h - 2.0, h / 2.0 - 1.0, p.border, 1.0);
                canvas.icon(Icon::Stop, cx, cy, 14.0, if disabled { p.muted.alpha(0.6) } else { p.text });
            }
            Kind::Voice => {
                canvas.fill_round(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0, press(p.ground));
                canvas.stroke_round(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0, p.border, 1.0);
                let name = VOICE.with(|v| v.borrow().clone());
                let r = h / 2.0 - 5.0;
                canvas.hedgehog(5.0 + r, cy, 2.0 * r + 2.0);
                let tx = 10.0 + 2.0 * r;
                canvas.text(&name, tx, cy - 16.0, w - tx - 28.0, 18.0, 13.0, true, p.text, Align::Left);
                canvas.text(t("window.voice_sub"), tx, cy + 1.0, w - tx - 28.0, 16.0, 11.0, false, p.muted, Align::Left);
                canvas.icon(Icon::Chevron, w - 18.0, cy, 14.0, p.muted);
            }
            Kind::Speed => {
                canvas.fill_round(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0, press(p.ground));
                canvas.stroke_round(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0, p.border, 1.0);
                let speed = SPEED.with(|s| s.borrow().clone());
                canvas.text(&speed, 0.0, 0.0, w, h, 14.0, true, p.text, Align::Center);
            }
            Kind::Tab(i) => {
                let selected = TABS[i] == tab;
                if selected {
                    canvas.fill_round(0.0, 0.0, w, h, 8.0, if p.dark { p.raised } else { p.surface });
                }
                canvas.text(&text, 0.0, 0.0, w, h, 12.5, selected, if selected { p.text } else { p.muted }, Align::Center);
            }
            Kind::Chip(i) => {
                let selected = mode_labels(tab).1 == i;
                let (fill, border, color) = if selected {
                    (p.accent_soft, p.accent, p.text)
                } else {
                    (background, p.border, if disabled { p.muted } else { p.text })
                };
                canvas.fill_round(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0, press(fill));
                canvas.stroke_round(0.5, 0.5, w - 1.0, h - 1.0, h / 2.0, border, 1.0);
                canvas.text(&text, 0.0, 0.0, w, h, 13.0, false, color, Align::Center);
            }
            Kind::Primary => {
                let recording = hwnd == c.dictate && DICTATING.with(|d| d.get());
                let cancelling = (hwnd == c.summarize || hwnd == c.correct) && AI_BUSY.with(|b| b.get());
                let fill = if disabled { p.raised } else if recording || cancelling { p.record } else { p.accent };
                canvas.fill_round(0.0, 0.0, w, h, 10.0, press(fill));
                let color = if disabled { p.muted } else if recording || cancelling { paint::rgb(0xFFFFFF) } else { p.on_accent };
                if hwnd == c.dictate {
                    let label_w = paint::measure(&text, 14.0, true);
                    let start = (w - label_w - 28.0) / 2.0;
                    canvas.icon(if recording { Icon::Stop } else { Icon::Mic }, start + 10.0, cy, 18.0, color);
                    canvas.text(&text, start + 28.0, 0.0, label_w + 4.0, h, 14.0, true, color, Align::Left);
                } else {
                    canvas.text(&text, 0.0, 0.0, w, h, 14.0, true, color, Align::Center);
                }
            }
            Kind::Ghost => {
                canvas.fill_round(0.5, 0.5, w - 1.0, h - 1.0, 8.0, press(background));
                canvas.stroke_round(0.5, 0.5, w - 1.0, h - 1.0, 8.0, p.border, 1.0);
                canvas.text(&text, 0.0, 0.0, w, h, 12.0, false, if disabled { p.muted } else { p.text }, Align::Center);
            }
        }
        // Tastaturfokus sichtbar machen.
        if focused && !matches!(kind, Kind::Tab(_)) {
            let radius = match kind {
                Kind::Play | Kind::Round(_) | Kind::Stop | Kind::Voice | Kind::Speed | Kind::Chip(_) => h / 2.0,
                _ => 10.0,
            };
            canvas.stroke_round(1.0, 1.0, w - 2.0, h - 2.0, radius, p.accent.alpha(0.9), 2.0);
        }
    });
}

/// „Übernehmen“: Das Ergebnis ersetzt die Markierung links bzw. den ganzen Text (mit Strg+Z
/// rückgängig zu machen).
unsafe fn apply_result(c: &Controls) {
    let result = window_text(c.result);
    if result.trim().is_empty() {
        return;
    }
    let (mut start, mut end) = (0u32, 0u32);
    SendMessageW(c.edit, EM_GETSEL, Some(WPARAM(&mut start as *mut u32 as usize)), Some(LPARAM(&mut end as *mut u32 as isize)));
    if start == end {
        SendMessageW(c.edit, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
    }
    let text = HSTRING::from(result);
    SendMessageW(c.edit, EM_REPLACESEL, Some(WPARAM(1)), Some(LPARAM(text.as_ptr() as isize)));
    let _ = SetFocus(Some(c.edit));
}

/// „Kopieren“: das ganze Ergebnis in die Zwischenablage.
unsafe fn copy_result(c: &Controls) {
    let (mut start, mut end) = (0u32, 0u32);
    SendMessageW(c.result, EM_GETSEL, Some(WPARAM(&mut start as *mut u32 as usize)), Some(LPARAM(&mut end as *mut u32 as isize)));
    SendMessageW(c.result, EM_SETSEL, Some(WPARAM(0)), Some(LPARAM(-1)));
    SendMessageW(c.result, WM_COPY, None, None);
    SendMessageW(c.result, EM_SETSEL, Some(WPARAM(start as usize)), Some(LPARAM(end as isize)));
}

/// Klicks, die das Fenster selbst erledigt; `true`, wenn erledigt.
unsafe fn own_command(c: &Controls, id: u32) -> bool {
    if let Some(i) = ID_TABS.iter().position(|t| *t == id) {
        select_tab(c, TABS[i]);
        return true;
    }
    if let Some(i) = ID_CHIPS.iter().position(|t| *t == id) {
        let tab = TAB.with(|t| t.get());
        let notify = match tab {
            Tab::Summary => {
                SUMMARY_MODES.with(|m| m.borrow_mut().1 = i);
                ID_SUMMARY_MODE
            }
            Tab::Correct => {
                CORRECTION_MODES.with(|m| m.borrow_mut().1 = i);
                ID_CORRECTION_MODE
            }
            Tab::Dictate => return true,
        };
        for chip in c.chips {
            let _ = InvalidateRect(Some(chip), None, false);
        }
        if let Some(on_command) = ON_COMMAND.with(|cell| cell.get()) {
            on_command(notify, SELECTED);
        }
        return true;
    }
    match id {
        ID_COPY => copy_result(c),
        ID_APPLY => apply_result(c),
        _ => return false,
    }
    true
}

/// Menüleiste im dunklen Modus selbst malen.
#[repr(C)]
struct UahMenu {
    hmenu: HMENU,
    hdc: HDC,
    flags: u32,
}

#[repr(C)]
struct UahMenuItem {
    position: i32,
    metrics: [i32; 8],
    popup_metrics: [u32; 5],
}

#[repr(C)]
struct UahDrawMenuItem {
    dis: DRAWITEMSTRUCT,
    um: UahMenu,
    umi: UahMenuItem,
}

unsafe fn draw_menu_bar(hwnd: HWND, menu: &UahMenu) {
    let mut info = MENUBARINFO { cbSize: size_of::<MENUBARINFO>() as u32, ..Default::default() };
    if GetMenuBarInfo(hwnd, OBJID_MENU, 0, &mut info).is_err() {
        return;
    }
    let mut window = RECT::default();
    let _ = GetWindowRect(hwnd, &mut window);
    let mut bar = info.rcBar;
    for (a, b) in [(&mut bar.left, window.left), (&mut bar.right, window.left), (&mut bar.top, window.top), (&mut bar.bottom, window.top)] {
        *a -= b;
    }
    LOOK.with(|l| {
        if let Some(look) = l.borrow().as_ref() {
            FillRect(menu.hdc, &bar, look.bar);
        }
    });
}

unsafe fn draw_menu_item(item: &UahDrawMenuItem) {
    let p = palette();
    let mut buffer = [0u16; 256];
    let mut info = MENUITEMINFOW {
        cbSize: size_of::<MENUITEMINFOW>() as u32,
        fMask: MIIM_STRING,
        dwTypeData: windows::core::PWSTR(buffer.as_mut_ptr()),
        cch: buffer.len() as u32 - 1,
        ..Default::default()
    };
    if GetMenuItemInfoW(item.um.hmenu, item.umi.position as u32, true, &mut info).is_err() {
        return;
    }
    let state = item.dis.itemState.0;
    let hot = state & (ODS_HOTLIGHT.0 | ODS_SELECTED.0) != 0;
    let fill = if hot { p.raised } else { p.bar };
    let brush = CreateSolidBrush(fill.colorref());
    let mut rect = item.dis.rcItem;
    FillRect(item.um.hdc, &rect, brush);
    let _ = DeleteObject(brush.into());
    SetBkMode(item.um.hdc, TRANSPARENT);
    SetTextColor(item.um.hdc, if state & ODS_DISABLED.0 != 0 { p.muted.colorref() } else { p.text.colorref() });
    let mut flags = DT_CENTER | DT_SINGLELINE | DT_VCENTER;
    if state & ODS_NOACCEL.0 != 0 {
        flags |= DT_HIDEPREFIX;
    }
    let len = info.cch as usize;
    DrawTextW(item.um.hdc, &mut buffer[..len], &mut rect, flags);
}

/// Unter der Menüleiste zieht Windows im dunklen Modus eine helle Linie; die wird übermalt.
unsafe fn cover_menu_line(hwnd: HWND) {
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    let mut points = [POINT { x: client.left, y: client.top }, POINT { x: client.right, y: client.bottom }];
    MapWindowPoints(Some(hwnd), None, &mut points);
    let mut window = RECT::default();
    let _ = GetWindowRect(hwnd, &mut window);
    let line = RECT { left: points[0].x - window.left, top: points[0].y - window.top - 1, right: points[1].x - window.left, bottom: points[0].y - window.top };
    let dc = GetWindowDC(Some(hwnd));
    LOOK.with(|l| {
        if let Some(look) = l.borrow().as_ref() {
            FillRect(dc, &line, look.bar);
        }
    });
    ReleaseDC(Some(hwnd), dc);
}

unsafe fn dropped_file(drop: HDROP) -> Option<String> {
    let mut path = [0u16; 1024];
    let len = DragQueryFileW(drop, 0, Some(&mut path)) as usize;
    DragFinish(drop);
    (len > 0).then(|| String::from_utf16_lossy(&path[..len]))
}

/// Farbe und Pinsel für ein Textfeld oder eine Beschriftung.
unsafe fn control_colors(c: &Controls, hdc: HDC, control: HWND, edit: bool) -> LRESULT {
    let p = palette();
    let muted = [c.text_label, c.text_hint, c.chips_label, c.result_label, c.progress, c.dict_hint, c.ai_status].contains(&control);
    SetTextColor(hdc, if muted { p.muted.colorref() } else { p.text.colorref() });
    let (color, brush) = LOOK.with(|l| {
        let l = l.borrow();
        let look = l.as_ref();
        if edit {
            (p.surface, look.map(|l| l.surface))
        } else if c.on_bar(control) {
            (p.bar, look.map(|l| l.bar))
        } else {
            (p.ground, look.map(|l| l.ground))
        }
    });
    SetBkColor(hdc, color.colorref());
    LRESULT(brush.map_or(0, |b| b.0 as isize))
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let Some(c) = controls() else {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    };
    unsafe {
        match msg {
            WM_SIZE => {
                // Minimieren und Wiederherstellen zählt wie Schließen und Öffnen (Mini-Pille).
                let _ = PostMessageW(Some(hwnd), WM_VISIBILITY, WPARAM(0), LPARAM(0));
                layout(&c);
                let _ = InvalidateRect(Some(hwnd), None, true);
            }
            WM_ERASEBKGND => return LRESULT(1),
            WM_SHOWWINDOW => {
                let _ = PostMessageW(Some(hwnd), WM_VISIBILITY, WPARAM(0), LPARAM(0));
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            WM_VISIBILITY => {
                if let Some(on_command) = ON_COMMAND.with(|cell| cell.get()) {
                    on_command(ID_VISIBILITY, CLICKED);
                }
            }
            WM_PAINT => {
                let mut ps = PAINTSTRUCT::default();
                let hdc = BeginPaint(hwnd, &mut ps);
                paint_background(&c, hdc);
                let _ = EndPaint(hwnd, &ps);
            }
            WM_DRAWITEM => {
                let item = &*(lparam.0 as *const DRAWITEMSTRUCT);
                draw_button(&c, item);
                return LRESULT(1);
            }
            WM_CTLCOLOREDIT => return control_colors(&c, HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _), true),
            WM_CTLCOLORSTATIC => return control_colors(&c, HDC(wparam.0 as *mut _), HWND(lparam.0 as *mut _), false),
            WM_UAHDRAWMENU if palette().dark => {
                draw_menu_bar(hwnd, &*(lparam.0 as *const UahMenu));
                return LRESULT(1);
            }
            WM_UAHDRAWMENUITEM if palette().dark => {
                draw_menu_item(&*(lparam.0 as *const UahDrawMenuItem));
                return LRESULT(1);
            }
            WM_NCPAINT | WM_NCACTIVATE => {
                let result = DefWindowProcW(hwnd, msg, wparam, lparam);
                if palette().dark {
                    cover_menu_line(hwnd);
                }
                return result;
            }
            WM_SETTINGCHANGE => {
                // Wechsel zwischen hellem und dunklem Modus in Windows.
                refresh_look();
                return DefWindowProcW(hwnd, msg, wparam, lparam);
            }
            WM_SETFOCUS => {
                let _ = SetFocus(Some(c.edit));
            }
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u32, ((wparam.0 >> 16) & 0xffff) as u32);
                if id == ID_CANCEL {
                    let _ = ShowWindow(hwnd, SW_HIDE);
                } else if !own_command(&c, id) {
                    // Der große Knopf pausiert bzw. setzt fort, solange vorgelesen wird.
                    let id = if id == ID_READ && ACTIVE.with(|a| a.get()) { ID_PAUSE } else { id };
                    if let Some(on_command) = ON_COMMAND.with(|cell| cell.get()) {
                        on_command(id, code);
                    }
                }
            }
            WM_DROPFILES => {
                let path = dropped_file(HDROP(wparam.0 as *mut _));
                if let (Some(path), Some(on_drop)) = (path, ON_DROP.with(|cell| cell.get())) {
                    on_drop(path);
                }
            }
            // Schließen blendet nur aus; beendet wird über Datei → Beenden oder das Infobereich-Menü.
            WM_CLOSE => {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
            WM_GETMINMAXINFO => {
                let dpi = GetDpiForWindow(hwnd) as i32;
                let info = &mut *(lparam.0 as *mut MINMAXINFO);
                let assistant = ASSISTANT.with(|a| a.get());
                info.ptMinTrackSize.x = if assistant { 820 } else { 640 } * dpi / 96;
                info.ptMinTrackSize.y = if assistant { 560 } else { 320 } * dpi / 96;
            }
            WM_DPICHANGED => {
                let r = &*(lparam.0 as *const RECT);
                let flags = SWP_NOZORDER | SWP_NOACTIVATE;
                let _ = SetWindowPos(hwnd, None, r.left, r.top, r.right - r.left, r.bottom - r.top, flags);
                apply_look(&c);
                layout(&c);
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_dictated_sections() {
        assert_eq!(dictated_text("", "hallo zusammen."), "Hallo zusammen.");
        assert_eq!(dictated_text("Ein Angebot.", "mit freundlichen Grüßen"), " Mit freundlichen Grüßen");
        assert_eq!(dictated_text("Wir melden uns", "bis Freitag"), " bis Freitag");
        assert_eq!(dictated_text("Zeile\r\n", "neu"), "neu");
        assert_eq!(dictated_text("Frage? ", "ja"), "Ja");
    }
}
