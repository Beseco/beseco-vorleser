//! Einstellungen: links die Seiten (Vorlesen, Diktieren, KI-Assistent, Tastenkürzel, Allgemein),
//! rechts die gewählte Seite, unten „Speichern“ und „Abbrechen“. Alles gilt erst mit „Speichern“;
//! nur Stimme und Geschwindigkeit wirken sofort, damit die Hörprobe passt („Abbrechen“ stellt sie
//! wieder her).

use std::cell::{Cell, RefCell};
use std::sync::Mutex;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontIndirectW, CreateRoundRectRgn, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect,
    FillRgn, GetObjectW, SelectObject, SetBkColor, SetBkMode, SetTextColor, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE,
    DT_VCENTER, HBRUSH, HDC, HFONT, LOGFONTW, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{DRAWITEMSTRUCT, MEASUREITEMSTRUCT, ODS_SELECTED};
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetClientRect, GetWindowTextLengthW, GetWindowTextW, LoadCursorW,
    MessageBoxW, PostMessageW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    BS_AUTOCHECKBOX, BS_DEFPUSHBUTTON, CBN_SELCHANGE, CBS_DROPDOWN, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL,
    CB_GETITEMDATA, CB_RESETCONTENT, CB_SETCURSEL, CB_SETITEMDATA, CW_USEDEFAULT, ES_AUTOHSCROLL, ES_PASSWORD, HMENU,
    IDC_ARROW, MB_ICONWARNING, SW_HIDE, SW_SHOW, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORBTN, WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_MEASUREITEM, WM_PAINT,
    WM_SETFONT, WM_USER, WNDCLASSW, WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_OVERLAPPED,
    WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::ai::{Correction, Kind, Provider};
use crate::config::{format_hotkey, parse_hotkey, parse_mouse_trigger, MOD_ALT, MOD_CONTROL, MOD_SHIFT, MOD_WIN};
use crate::i18n::{self, t};
use crate::theme;
use crate::window::message_font;

// IDOK und IDCANCEL: so lösen Eingabe und Esc die beiden Schaltflächen aus.
const ID_SAVE: u32 = 1;
const ID_CANCEL: u32 = 2;
const ID_NAV: u32 = 9;
const ID_VOICE: u32 = 10;
const ID_RATE: u32 = 11;
const ID_SAMPLE: u32 = 12;
const ID_AI_VOICES: u32 = 13;
const ID_DICT_SETUP: u32 = 14;
const ID_HISTORY_CLEAR: u32 = 15;
const ID_AI_SETUP: u32 = 16;
const ID_KIND: u32 = 17;
const ID_MODELS: u32 = 18;
const ID_TEST: u32 = 19;

// Nachrichten von Tastenkombinations-Feld, Schieberegler, Liste und Feldern.
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
const EM_SETCUEBANNER: u32 = 0x1501;
const CB_SETEDITSEL: u32 = 0x0142;
const CB_SETDROPPEDWIDTH: u32 = 0x0160;
const LB_ADDSTRING: u32 = 0x0180;
const LB_SETCURSEL: u32 = 0x0186;
const LB_GETCURSEL: u32 = 0x0188;
const LBN_SELCHANGE: u32 = 1;
const LBS_NOTIFY: u32 = 0x0001;
const LBS_OWNERDRAWFIXED: u32 = 0x0010;
const LBS_HASSTRINGS: u32 = 0x0040;
const LBS_NOINTEGRALHEIGHT: u32 = 0x0100;

/// Ergebnis eines Hintergrundauftrags der KI-Seite: `wparam` 0 = Modelle, 1 = Verbindungstest.
const WM_RESULT: u32 = WM_APP + 1;

// Maße in geräteunabhängigen Pixeln.
const WIDTH: i32 = 800;
const HEIGHT: i32 = 660;
/// Seitenleiste links.
const NAV: i32 = 212;
/// Inhalt: linker und rechter Rand, Spalte der Felder.
const CX: i32 = NAV + 28;
const CR: i32 = WIDTH - 28;
const CW: i32 = CR - CX;
const FX: i32 = CX + 172;
const FW: i32 = CR - FX;
/// Schaltfläche rechts neben einem Feld.
const BW: i32 = 150;
const BX: i32 = CR - BW;
const SIDE_W: i32 = BX - 10 - FX;
/// Leiste unten mit „Speichern“ und „Abbrechen“.
const FOOTER: i32 = 64;
/// Beginn des Inhalts unter Überschrift und Untertitel.
const TOP: i32 = 104;
/// Höhe von Eingabe- und Tastenfeldern (so hoch wie eine Auswahlliste).
const FIELD_H: i32 = 25;

// Farben (Windows 11 hell).
const WHITE: COLORREF = rgb(0xFF, 0xFF, 0xFF);
const SIDEBAR: COLORREF = rgb(0xF3, 0xF3, 0xF3);
const SELECTED: COLORREF = rgb(0xE5, 0xE5, 0xE5);
const LINE: COLORREF = rgb(0xE3, 0xE3, 0xE3);
const ACCENT: COLORREF = rgb(0x00, 0x67, 0xC0);
const TEXT: COLORREF = rgb(0x1B, 0x1B, 0x1B);
const MUTED: COLORREF = rgb(0x5F, 0x5F, 0x5F);

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Die Seiten, in der Reihenfolge der Seitenleiste.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Page {
    Reading,
    Dictation,
    Ai,
    Shortcuts,
    General,
}

impl Page {
    const ALL: [Page; 5] = [Page::Reading, Page::Dictation, Page::Ai, Page::Shortcuts, Page::General];

    fn label(self) -> &'static str {
        match self {
            Page::Reading => t("prefs.nav_reading"),
            Page::Dictation => t("prefs.nav_dictation"),
            Page::Ai => t("prefs.nav_ai"),
            Page::Shortcuts => t("prefs.nav_shortcuts"),
            Page::General => t("prefs.nav_general"),
        }
    }

    fn subtitle(self) -> &'static str {
        match self {
            Page::Reading => t("prefs.sub_reading"),
            Page::Dictation => t("prefs.sub_dictation"),
            Page::Ai => t("prefs.sub_ai"),
            Page::Shortcuts => t("prefs.sub_shortcuts"),
            Page::General => t("prefs.sub_general"),
        }
    }

    /// Symbol aus „Segoe MDL2 Assets“ (Windows 10 und 11).
    fn icon(self) -> char {
        match self {
            Page::Reading => '\u{E767}',   // Lautsprecher
            Page::Dictation => '\u{E720}', // Mikrofon
            Page::Ai => '\u{E945}',        // Blitz
            Page::Shortcuts => '\u{E765}', // Tastatur
            Page::General => '\u{E713}',   // Zahnrad
        }
    }

    fn index(self) -> usize {
        Page::ALL.iter().position(|p| *p == self).unwrap_or(0)
    }
}

/// Schlüssel der Anzeige und zugehöriger Wert für `mouse_menu`.
const MOUSE_OPTIONS: [(&str, &str); 5] = [
    ("mouse.off", ""),
    ("mouse.ctrl", "Strg+Rechtsklick"),
    ("mouse.shift", "Shift+Rechtsklick"),
    ("mouse.ctrl_shift", "Strg+Shift+Rechtsklick"),
    ("mouse.ctrl_alt", "Strg+Alt+Rechtsklick"),
];

/// Anzeige der Werte von `theme::CHOICES`.
const THEME_LABELS: [&str; 3] = ["settings.theme_auto", "settings.theme_light", "settings.theme_dark"];

/// Anzeige und Wert für `language`: „automatisch“ und alle verfügbaren Sprachen.
fn language_options() -> Vec<(String, String)> {
    let mut options = vec![(t("menu.language_auto").replace('&', ""), i18n::AUTO.to_string())];
    options.extend(i18n::languages().into_iter().map(|(code, name)| (name.to_string(), code.to_string())));
    options
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::None => t("aiset.kind_none"),
        Kind::Ollama => t("aiset.kind_ollama"),
        Kind::OpenAi => t("aiset.kind_openai"),
    }
}

fn correction_label(mode: Correction) -> &'static str {
    match mode {
        Correction::Spelling => t("correction.spelling"),
        Correction::Style => t("correction.style"),
        Correction::Rewrite => t("correction.rewrite"),
        Correction::Custom => t("correction.custom"),
    }
}

#[derive(Clone)]
pub struct Values {
    // Vorlesen
    pub volume: u32,
    pub auto_language: bool,
    pub clean_text: bool,
    pub skip_page_edges: bool,
    pub page_edges_ai: bool,
    // Diktieren
    pub microphone: String,
    pub dict_clean_fillers: bool,
    pub dict_auto_correct: bool,
    pub correction_mode: String,
    pub correction_custom: String,
    // KI-Assistent
    pub assistant: bool,
    pub provider: Provider,
    // Tastenkürzel
    pub hotkey_read: String,
    pub hotkey_pause: String,
    pub hotkey_menu: String,
    pub hotkey_ocr: String,
    pub hotkey_dictate: String,
    pub mouse_menu: String,
    // Allgemein
    pub language: String,
    pub theme: String,
    pub autostart: bool,
    pub close_to_pill: bool,
    pub remember_text: bool,
    pub update_check: bool,
    pub proxy: String,
}

/// Stimmenauswahl auf der Seite „Vorlesen“.
pub struct Voices {
    /// Programmweiter Index der Stimme und Anzeige, schon nach Sprache sortiert.
    pub items: Vec<(usize, String)>,
    pub selected: Option<usize>,
    pub rates: Vec<String>,
    pub rate: usize,
}

/// Was das Fenster beim Programm auslöst.
#[derive(Clone, Copy)]
pub struct Callbacks {
    /// Neue Stimme bzw. Geschwindigkeit (Index in `rates`); wirkt sofort.
    pub on_voice: fn(Option<usize>, usize),
    pub on_sample: fn(),
    pub on_ai_voices: fn(),
    pub on_dict_setup: fn(),
    pub on_ai_setup: fn(),
    pub on_save: fn(Values),
}

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    nav: HWND,
    // Vorlesen
    voice: HWND,
    rate: HWND,
    volume: HWND,
    auto_language: HWND,
    clean: HWND,
    edges: HWND,
    edges_ai: HWND,
    // Diktieren
    microphone: HWND,
    dict_model: HWND,
    fillers: HWND,
    auto_correct: HWND,
    correction: HWND,
    custom: HWND,
    history_note: HWND,
    // KI
    assistant: HWND,
    kind: HWND,
    url: HWND,
    key: HWND,
    model: HWND,
    models: HWND,
    test: HWND,
    status: HWND,
    // Tastenkürzel
    hotkeys: [HWND; 5],
    mouse: HWND,
    // Allgemein
    language: HWND,
    theme: HWND,
    autostart: HWND,
    pill: HWND,
    remember: HWND,
    update: HWND,
    proxy: HWND,
}

/// Schriften und Pinsel des Fensters; werden beim Schließen freigegeben.
struct Look {
    font: HFONT,
    title: HFONT,
    heading: HFONT,
    icons: HFONT,
    white: HBRUSH,
    sidebar: HBRUSH,
    selected: HBRUSH,
    line: HBRUSH,
    accent: HBRUSH,
    /// Steuerelemente je Seite; nur die der gewählten Seite sind sichtbar.
    pages: Vec<(HWND, usize)>,
    /// Graue Hinweistexte.
    muted: Vec<HWND>,
    /// Überschriften und Untertitel je Seite.
    titles: Vec<(HWND, HWND)>,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static LOOK: RefCell<Option<Look>> = const { RefCell::new(None) };
    static ORIGINAL: RefCell<Option<Values>> = const { RefCell::new(None) };
    static CALLBACKS: Cell<Option<Callbacks>> = const { Cell::new(None) };
    /// Stimme und Geschwindigkeit beim Öffnen, für „Abbrechen“.
    static VOICE_BEFORE: Cell<(Option<usize>, usize)> = const { Cell::new((None, 0)) };
    /// Zuletzt gewählter Anbieter, damit beim Wechsel die Standardadresse nachrückt.
    static LAST_KIND: Cell<usize> = const { Cell::new(0) };
}

static MODELS: Mutex<Option<Result<Vec<String>, String>>> = Mutex::new(None);
static TEST: Mutex<Option<Result<(u128, String), String>>> = Mutex::new(None);

/// Das Fenster, solange es offen ist.
pub fn handle() -> Option<HWND> {
    CONTROLS.with(|c| c.get()).map(|c| c.window)
}

fn px(value: i32) -> i32 {
    value * unsafe { GetDpiForSystem() } as i32 / 96
}

/// Legt Steuerelemente an und merkt sich, zu welcher Seite sie gehören.
struct Builder {
    window: HWND,
    font: HFONT,
    page: usize,
    pages: Vec<(HWND, usize)>,
    muted: Vec<HWND>,
    headings: Vec<HWND>,
    titles: Vec<(HWND, HWND)>,
    y: i32,
}

impl Builder {
    unsafe fn create(&mut self, class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, id: u32, rect: (i32, i32, i32, i32)) -> HWND {
        let (x, y, width, height) = rect;
        let visible = if self.page == 0 { WS_VISIBLE } else { WINDOW_STYLE(0) };
        let control = CreateWindowExW(
            ex,
            class,
            &HSTRING::from(text),
            WS_CHILD | visible | WINDOW_STYLE(style),
            px(x),
            px(y),
            px(width),
            px(height),
            Some(self.window),
            Some(HMENU(id as usize as *mut _)),
            GetModuleHandleW(None).ok().map(|i| i.into()),
            None,
        )
        .unwrap_or_default();
        SendMessageW(control, WM_SETFONT, Some(WPARAM(self.font.0 as usize)), Some(LPARAM(1)));
        self.pages.push((control, self.page));
        control
    }

    /// Neue Seite mit Überschrift und Untertitel.
    unsafe fn page(&mut self, page: Page) {
        self.page = page.index();
        let title = self.create(w!("STATIC"), page.label(), 0, WINDOW_EX_STYLE(0), 0, (CX, 24, CW, 36));
        let subtitle = self.create(w!("STATIC"), page.subtitle(), 0, WINDOW_EX_STYLE(0), 0, (CX, 62, CW, 22));
        self.muted.push(subtitle);
        self.titles.push((title, subtitle));
        self.y = TOP;
    }

    /// Zwischenüberschrift; davor etwas Abstand, außer ganz oben.
    unsafe fn section(&mut self, text: &str) {
        if self.y > TOP {
            self.y += 14;
        }
        let heading = self.create(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), 0, (CX, self.y, CW, 22));
        self.headings.push(heading);
        self.y += 32;
    }

    /// Beschriftung links in der aktuellen Zeile.
    unsafe fn label(&mut self, text: &str) {
        self.label_until(text, FX);
    }

    /// Beschriftung links, die bis kurz vor `right` reicht.
    unsafe fn label_until(&mut self, text: &str, right: i32) {
        self.create(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), 0, (CX, self.y + 5, right - CX - 12, 22));
    }

    unsafe fn combo(&mut self, id: u32, x: i32, width: i32, editable: bool) -> HWND {
        let kind = if editable { CBS_DROPDOWN } else { CBS_DROPDOWNLIST } as u32;
        self.create(w!("COMBOBOX"), "", WS_TABSTOP.0 | WS_VSCROLL.0 | kind, WINDOW_EX_STYLE(0), id, (x, self.y, width, 320))
    }

    /// Eingabefeld; so hoch wie die Auswahllisten, damit die Zeilen gleich aussehen.
    unsafe fn edit(&mut self, text: &str, cue: &str, password: bool) -> HWND {
        let style = WS_TABSTOP.0 | ES_AUTOHSCROLL as u32 | if password { ES_PASSWORD as u32 } else { 0 };
        let edit = self.create(w!("EDIT"), text, style, WS_EX_CLIENTEDGE, 0, (FX, self.y, FW, FIELD_H));
        if !cue.is_empty() {
            let cue = HSTRING::from(cue);
            SendMessageW(edit, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));
        }
        edit
    }

    unsafe fn button(&mut self, text: &str, id: u32, x: i32, width: i32) -> HWND {
        self.create(w!("BUTTON"), text, WS_TABSTOP.0, WINDOW_EX_STYLE(0), id, (x, self.y - 1, width, 30))
    }

    unsafe fn checkbox(&mut self, text: &str, indent: i32) -> HWND {
        let check = self.create(w!("BUTTON"), text, WS_TABSTOP.0 | BS_AUTOCHECKBOX as u32, WINDOW_EX_STYLE(0), 0, (CX + indent, self.y, CW - indent, 24));
        self.y += 32;
        check
    }

    /// Grauer Hinweis über `lines` Zeilen.
    unsafe fn note(&mut self, text: &str, x: i32, width: i32, lines: i32) -> HWND {
        let note = self.create(w!("STATIC"), text, 0, WINDOW_EX_STYLE(0), 0, (x, self.y + 5, width, 20 * lines));
        self.muted.push(note);
        note
    }

    fn next(&mut self, height: i32) {
        self.y += height;
    }
}

/// Öffnet das Fenster auf `page` (oder holt es nach vorn und wechselt dorthin).
pub fn open(owner: Option<HWND>, page: Page, values: Values, voices: Voices, callbacks: Callbacks) {
    if let Some(window) = handle() {
        show_page(page);
        unsafe {
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let white = CreateSolidBrush(WHITE);
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserEinstellungen"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: crate::tray::load_icon(),
            ..Default::default()
        };
        RegisterClassW(&class);

        let dpi = GetDpiForSystem();
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT { left: 0, top: 0, right: px(WIDTH), bottom: px(HEIGHT) };
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
            let _ = DeleteObject(white.into());
            return;
        };

        let font = message_font(window);
        let look = Look {
            font,
            title: derived_font(font, 1.6, 600, None),
            heading: derived_font(font, 1.0, 600, None),
            icons: derived_font(font, 1.2, 400, Some("Segoe MDL2 Assets")),
            white,
            sidebar: CreateSolidBrush(SIDEBAR),
            selected: CreateSolidBrush(SELECTED),
            line: CreateSolidBrush(LINE),
            accent: CreateSolidBrush(ACCENT),
            pages: Vec::new(),
            muted: Vec::new(),
            titles: Vec::new(),
        };
        // Vor dem Anlegen der Seitenleiste bereitstellen: Sie fragt beim Erzeugen nach der Zeilenhöhe.
        LOOK.with(|l| *l.borrow_mut() = Some(look));

        let mut b = Builder { window, font, page: 0, pages: Vec::new(), muted: Vec::new(), headings: Vec::new(), titles: Vec::new(), y: TOP };

        // Seitenleiste.
        let nav = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("LISTBOX"),
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(LBS_NOTIFY | LBS_OWNERDRAWFIXED | LBS_HASSTRINGS | LBS_NOINTEGRALHEIGHT),
            px(10),
            px(16),
            px(NAV - 20),
            px(HEIGHT - 32),
            Some(window),
            Some(HMENU(ID_NAV as usize as *mut _)),
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();
        for page in Page::ALL {
            let label = HSTRING::from(page.label());
            SendMessageW(nav, LB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        }

        let tab = WS_TABSTOP.0;
        let none = WINDOW_EX_STYLE(0);

        // ── Vorlesen ──
        b.page(Page::Reading);
        b.label(t("prefs.voice"));
        let voice = b.combo(ID_VOICE, FX, FW, false);
        b.next(40);
        b.label(t("prefs.speed"));
        let rate = b.combo(ID_RATE, FX, SIDE_W, false);
        b.button(t("prefs.sample"), ID_SAMPLE, BX, BW);
        b.next(40);
        b.label(t("prefs.volume"));
        let volume = b.create(w!("msctls_trackbar32"), "", tab | TBS_AUTOTICKS, none, 0, (FX - 8, b.y - 2, SIDE_W + 16, 32));
        b.next(44);
        let auto_language = b.checkbox(t("settings.auto_language"), 0);
        b.section(t("prefs.ai_voices_head"));
        b.note(t("prefs.ai_voices_note"), CX, BX - CX - 16, 2);
        b.button(t("prefs.ai_voices_button"), ID_AI_VOICES, BX, BW);
        b.next(46);
        b.section(t("prefs.texts_head"));
        let clean = b.checkbox(t("settings.clean_up_text_before"), 0);
        let edges = b.checkbox(t("settings.skip_page_edges"), 0);
        let edges_ai = b.checkbox(t("settings.page_edges_ai"), 24);

        // ── Diktieren ──
        b.page(Page::Dictation);
        b.label(t("prefs.microphone"));
        let microphone = b.combo(0, FX, FW, false);
        b.next(40);
        b.label(t("prefs.recognition"));
        let dict_model = b.note("", FX, SIDE_W, 1);
        b.button(t("prefs.models_button"), ID_DICT_SETUP, BX, BW);
        b.next(42);
        let fillers = b.checkbox(t("prefs.fillers"), 0);
        let auto_correct = b.checkbox(t("prefs.auto_correct"), 0);
        b.section(t("prefs.correction_head"));
        b.label(t("prefs.correction_mode"));
        let correction = b.combo(0, FX, SIDE_W, false);
        b.next(40);
        b.label(t("prefs.correction_custom"));
        let custom = b.edit(&values.correction_custom, t("aiset.custom_hint"), false);
        b.next(40);
        b.section(t("prefs.history_head"));
        let history_note = b.note(t("prefs.history_note"), CX, BX - CX - 16, 2);
        b.button(t("prefs.history_clear"), ID_HISTORY_CLEAR, BX, BW);
        b.next(46);

        // ── KI-Assistent ──
        b.page(Page::Ai);
        let assistant = b.create(w!("BUTTON"), t("prefs.assistant"), tab | BS_AUTOCHECKBOX as u32, none, 0, (CX, b.y + 3, BX - CX - 16, 24));
        b.button(t("prefs.setup_button"), ID_AI_SETUP, BX, BW);
        b.next(32);
        b.note(t("prefs.setup_note"), CX, CW, 1);
        b.next(26);
        b.section(t("prefs.connection_head"));
        b.label(t("prefs.kind"));
        let kind = b.combo(ID_KIND, FX, FW, false);
        b.next(38);
        b.label(t("prefs.url"));
        let url = b.edit(&values.provider.url, "", false);
        b.next(38);
        b.label(t("prefs.key"));
        let key = b.edit(&values.provider.key, t("aiset.key_hint"), true);
        b.next(38);
        b.label(t("prefs.model"));
        let model = b.combo(0, FX, SIDE_W, true);
        // Ein Kombinationsfeld übernimmt den Fenstertext beim Anlegen nicht.
        set_text(model, &values.provider.model);
        let models = b.button(t("aiset.fetch_models"), ID_MODELS, BX, BW);
        b.next(40);
        let test = b.button(t("aiset.test"), ID_TEST, FX, BW);
        b.next(36);
        let status = b.note("", CX, CW, 3);
        b.next(52);
        b.section(t("prefs.this_pc_head"));
        let report = crate::aicheck::detect().report();
        b.y -= 6;
        b.note(&report, CX, CW, 5);

        // ── Tastenkürzel ──
        b.page(Page::Shortcuts);
        b.note(t("settings.click_field_press_shortcut"), CX, CW, 1);
        b.next(36);
        let mut hotkeys = [HWND::default(); 5];
        let labels = ["prefs.hk_read", "prefs.hk_pause", "prefs.hk_menu", "prefs.hk_ocr", "prefs.hk_dictate"];
        for (slot, key) in hotkeys.iter_mut().zip(labels) {
            b.label_until(t(key), BX - 80);
            *slot = b.create(w!("msctls_hotkey32"), "", tab, none, 0, (BX - 80, b.y, BW + 80, FIELD_H));
            b.next(40);
        }
        b.label_until(t("prefs.mouse"), BX - 80);
        let mouse = b.combo(0, BX - 80, BW + 80, false);
        b.next(40);

        // ── Allgemein ──
        b.page(Page::General);
        b.label("Sprache / Language");
        let language = b.combo(0, FX, SIDE_W, false);
        b.next(40);
        b.label(t("prefs.theme"));
        let theme = b.combo(0, FX, SIDE_W, false);
        b.next(40);
        b.section(t("prefs.start_head"));
        let autostart = b.checkbox(t("prefs.autostart"), 0);
        let pill = b.checkbox(t("settings.close_to_pill"), 0);
        let remember = b.checkbox(t("settings.remember_text"), 0);
        b.section(t("prefs.updates_head"));
        let update = b.checkbox(t("settings.check_github_for_new"), 0);
        b.next(4);
        b.label(t("prefs.proxy"));
        let proxy = b.edit(&values.proxy, t("settings.proxy_example"), false);

        // Leiste unten: auf allen Seiten.
        let footer_y = HEIGHT - FOOTER + 16;
        let save = CreateWindowExW(
            none,
            w!("BUTTON"),
            &HSTRING::from(t("settings.save")),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_DEFPUSHBUTTON as u32),
            px(CR - 2 * 112 - 8),
            px(footer_y),
            px(112),
            px(32),
            Some(window),
            Some(HMENU(ID_SAVE as usize as *mut _)),
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();
        let cancel = CreateWindowExW(
            none,
            w!("BUTTON"),
            &HSTRING::from(t("settings.cancel")),
            WS_CHILD | WS_VISIBLE | WS_TABSTOP,
            px(CR - 112),
            px(footer_y),
            px(112),
            px(32),
            Some(window),
            Some(HMENU(ID_CANCEL as usize as *mut _)),
            Some(instance.into()),
            None,
        )
        .unwrap_or_default();
        for button in [save, cancel] {
            SendMessageW(button, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
        }

        LOOK.with(|l| {
            if let Some(look) = l.borrow_mut().as_mut() {
                for (title, _) in &b.titles {
                    SendMessageW(*title, WM_SETFONT, Some(WPARAM(look.title.0 as usize)), Some(LPARAM(1)));
                }
                for heading in &b.headings {
                    SendMessageW(*heading, WM_SETFONT, Some(WPARAM(look.heading.0 as usize)), Some(LPARAM(1)));
                }
                look.pages = std::mem::take(&mut b.pages);
                look.muted = std::mem::take(&mut b.muted);
                look.titles = std::mem::take(&mut b.titles);
            }
        });

        let c = Controls {
            window, nav, voice, rate, volume, auto_language, clean, edges, edges_ai, microphone, dict_model, fillers, auto_correct,
            correction, custom, history_note, assistant, kind, url, key, model, models, test, status, hotkeys, mouse, language,
            theme, autostart, pill, remember, update, proxy,
        };
        fill(&c, &values, &voices);
        CONTROLS.with(|cell| cell.set(Some(c)));
        ORIGINAL.with(|cell| *cell.borrow_mut() = Some(values));
        CALLBACKS.with(|cell| cell.set(Some(callbacks)));
        VOICE_BEFORE.with(|cell| cell.set((voices.selected, voices.rate)));
        show_page(page);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(nav));
    }
}

/// Eine Schrift wie `base`, um `scale` größer, mit Strichstärke `weight` und ggf. anderer Familie.
unsafe fn derived_font(base: HFONT, scale: f32, weight: i32, face: Option<&str>) -> HFONT {
    let mut logfont = LOGFONTW::default();
    GetObjectW(base.into(), std::mem::size_of::<LOGFONTW>() as i32, Some(&mut logfont as *mut _ as *mut _));
    logfont.lfHeight = (logfont.lfHeight as f32 * scale).round() as i32;
    logfont.lfWeight = weight;
    if let Some(face) = face {
        logfont.lfFaceName = [0; 32];
        for (slot, unit) in logfont.lfFaceName.iter_mut().zip(face.encode_utf16().take(31)) {
            *slot = unit;
        }
    }
    CreateFontIndirectW(&logfont)
}

/// Wechselt die sichtbare Seite.
pub fn show_page(page: Page) {
    let Some(c) = CONTROLS.with(|cell| cell.get()) else { return };
    let index = page.index();
    unsafe {
        SendMessageW(c.nav, LB_SETCURSEL, Some(WPARAM(index)), None);
    }
    LOOK.with(|l| {
        if let Some(look) = l.borrow().as_ref() {
            for (control, on) in &look.pages {
                unsafe {
                    let _ = ShowWindow(*control, if *on == index { SW_SHOW } else { SW_HIDE });
                }
            }
        }
    });
}

fn current_page(c: &Controls) -> usize {
    unsafe { SendMessageW(c.nav, LB_GETCURSEL, None, None).0.max(0) as usize }
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

/// Nach dem Einrichtungsassistenten: dessen Anbieter in die Felder übernehmen.
pub fn set_provider(provider: &Provider) {
    let Some(c) = CONTROLS.with(|cell| cell.get()) else { return };
    let index = Kind::ALL.iter().position(|k| *k == provider.kind).unwrap_or(0);
    unsafe {
        SendMessageW(c.kind, CB_SETCURSEL, Some(WPARAM(index)), None);
    }
    LAST_KIND.with(|l| l.set(index));
    set_text(c.url, &provider.url);
    set_text(c.key, &provider.key);
    set_text(c.model, &provider.model);
    if provider.kind != Kind::None {
        unsafe {
            SendMessageW(c.assistant, BM_SETCHECK, Some(WPARAM(1)), None);
        }
    }
    update_ai_fields(&c);
}

/// Nach „Modelle verwalten“: das installierte Modell neu anzeigen.
pub fn refresh_dictation() {
    if let Some(c) = CONTROLS.with(|cell| cell.get()) {
        set_text(c.dict_model, &dict_model_text());
    }
}

fn dict_model_text() -> String {
    let Some(model) = crate::dictation::best_installed() else { return t("prefs.model_none").to_string() };
    // Dieselben Namen wie in „Modelle verwalten“, z. B. „Sehr genau – braucht …“ → „Sehr genau“.
    let description = i18n::lookup(&format!("dict.model.{}", model.key)).unwrap_or(model.key);
    let name = description.split(" – ").next().unwrap_or(description);
    t("prefs.model_installed").replace("{}", &format!("{name} ({})", model.key))
}

unsafe fn add_items(combo: HWND, labels: impl IntoIterator<Item = impl AsRef<str>>, selected: usize) {
    for label in labels {
        let label = HSTRING::from(label.as_ref());
        SendMessageW(combo, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
    }
    SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(selected)), None);
}

unsafe fn check(control: HWND, on: bool) {
    SendMessageW(control, BM_SETCHECK, Some(WPARAM(on as usize)), None);
}

unsafe fn checked(control: HWND) -> bool {
    SendMessageW(control, BM_GETCHECK, None, None).0 == 1
}

unsafe fn fill(c: &Controls, values: &Values, voices: &Voices) {
    // Vorlesen
    fill_voices(c, &voices.items, voices.selected);
    SendMessageW(c.voice, CB_SETDROPPEDWIDTH, Some(WPARAM(px(320) as usize)), None);
    add_items(c.rate, &voices.rates, voices.rate);
    SendMessageW(c.volume, TBM_SETRANGE, Some(WPARAM(1)), Some(LPARAM(100 << 16)));
    SendMessageW(c.volume, TBM_SETTICFREQ, Some(WPARAM(10)), None);
    SendMessageW(c.volume, TBM_SETPOS, Some(WPARAM(1)), Some(LPARAM(values.volume as isize)));
    check(c.auto_language, values.auto_language);
    check(c.clean, values.clean_text);
    check(c.edges, values.skip_page_edges);
    check(c.edges_ai, values.page_edges_ai);

    // Diktieren
    let microphones = crate::dictation::microphones();
    let current = microphones.iter().position(|m| *m == values.microphone).map_or(0, |i| i + 1);
    let mut labels = vec![t("menu.microphone_default").to_string()];
    labels.extend(microphones);
    add_items(c.microphone, &labels, current);
    set_text(c.dict_model, &dict_model_text());
    check(c.fillers, values.dict_clean_fillers);
    check(c.auto_correct, values.dict_auto_correct);
    let mode = Correction::from_code(&values.correction_mode);
    let current = Correction::ALL.iter().position(|m| *m == mode).unwrap_or(0);
    add_items(c.correction, Correction::ALL.iter().map(|m| correction_label(*m)), current);

    // KI
    check(c.assistant, values.assistant);
    let index = Kind::ALL.iter().position(|k| *k == values.provider.kind).unwrap_or(0);
    add_items(c.kind, Kind::ALL.iter().map(|k| kind_label(*k)), index);
    LAST_KIND.with(|l| l.set(index));
    update_ai_fields(c);

    // Tastenkürzel
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
    let mut labels: Vec<String> = MOUSE_OPTIONS.iter().map(|(key, _)| t(key).to_string()).collect();
    // Eine von Hand in die Datei geschriebene Kombination bleibt als eigener Eintrag wählbar.
    let selected = MOUSE_OPTIONS.iter().position(|(_, value)| parse_mouse_trigger(value) == current).unwrap_or_else(|| {
        labels.push(values.mouse_menu.clone());
        MOUSE_OPTIONS.len()
    });
    add_items(c.mouse, &labels, selected);

    // Allgemein
    let options = language_options();
    let current = options.iter().position(|(_, value)| *value == values.language).unwrap_or(0);
    add_items(c.language, options.iter().map(|(label, _)| label.as_str()), current);
    let current = theme::CHOICES.iter().position(|c| *c == values.theme).unwrap_or(0);
    add_items(c.theme, THEME_LABELS.iter().map(|key| t(key)), current);
    check(c.autostart, values.autostart);
    check(c.pill, values.close_to_pill);
    check(c.remember, values.remember_text);
    check(c.update, values.update_check);
}

fn text(control: HWND) -> String {
    unsafe {
        let mut buffer = vec![0u16; GetWindowTextLengthW(control) as usize + 1];
        let len = GetWindowTextW(control, &mut buffer) as usize;
        String::from_utf16_lossy(&buffer[..len]).trim().to_string()
    }
}

fn set_text(control: HWND, value: &str) {
    unsafe {
        let _ = SetWindowTextW(control, &HSTRING::from(value));
    }
}

fn selected_kind(c: &Controls) -> Kind {
    let row = unsafe { SendMessageW(c.kind, CB_GETCURSEL, None, None).0 };
    Kind::ALL.get(row.max(0) as usize).copied().unwrap_or(Kind::None)
}

fn current_provider(c: &Controls) -> Provider {
    Provider { kind: selected_kind(c), url: text(c.url), key: text(c.key), model: text(c.model) }
}

/// Felder je nach Anbieter freigeben und einen passenden Hinweis zeigen.
fn update_ai_fields(c: &Controls) {
    let kind = selected_kind(c);
    let active = kind != Kind::None;
    unsafe {
        for control in [c.url, c.key, c.model, c.models, c.test] {
            let _ = EnableWindow(control, active);
        }
    }
    let hint = match kind {
        Kind::None => t("aiset.hint_none"),
        Kind::Ollama => t("aiset.hint_ollama"),
        Kind::OpenAi => t("aiset.hint_openai"),
    };
    set_text(c.status, hint);
}

/// Wechsel des Anbieters: leere oder zum alten Anbieter gehörende Adresse ersetzen.
fn on_kind_changed(c: &Controls) {
    let index = unsafe { SendMessageW(c.kind, CB_GETCURSEL, None, None).0.max(0) as usize };
    let old = Kind::ALL[LAST_KIND.with(|l| l.replace(index))];
    let new = Kind::ALL[index];
    let url = text(c.url);
    if url.is_empty() || url == old.default_url() {
        set_text(c.url, new.default_url());
    }
    update_ai_fields(c);
}

fn run_in_background(c: &Controls, kind: usize) {
    let provider = current_provider(c);
    if provider.url.is_empty() {
        set_text(c.status, t("aiset.need_url"));
        return;
    }
    set_text(c.status, if kind == 0 { t("aiset.fetching") } else { t("aiset.testing") });
    unsafe {
        let _ = EnableWindow(c.models, false);
        let _ = EnableWindow(c.test, false);
    }
    let target = c.window.0 as isize;
    std::thread::spawn(move || {
        if kind == 0 {
            *MODELS.lock().unwrap() = Some(provider.models());
        } else {
            *TEST.lock().unwrap() = Some(if provider.model.is_empty() { Err(t("aiset.need_model").to_string()) } else { provider.test() });
        }
        unsafe {
            let _ = PostMessageW(Some(HWND(target as *mut _)), WM_RESULT, WPARAM(kind), LPARAM(0));
        }
    });
}

fn on_result(c: &Controls, kind: usize) {
    unsafe {
        let _ = EnableWindow(c.models, true);
        let _ = EnableWindow(c.test, true);
    }
    if kind == 0 {
        match MODELS.lock().unwrap().take() {
            Some(Ok(models)) if !models.is_empty() => {
                let chosen = text(c.model);
                unsafe {
                    SendMessageW(c.model, CB_RESETCONTENT, None, None);
                    // Das bisherige Modell behalten, sonst das erste vorschlagen.
                    let keep = models.iter().position(|m| *m == chosen);
                    add_items(c.model, &models, keep.unwrap_or(0));
                    SendMessageW(c.model, CB_SETEDITSEL, None, Some(LPARAM(0)));
                }
                set_text(c.status, &t("aiset.models_found").replace("{}", &models.len().to_string()));
            }
            Some(Ok(_)) => set_text(c.status, t("aiset.no_models")),
            Some(Err(error)) => set_text(c.status, &error),
            None => {}
        }
    } else {
        match TEST.lock().unwrap().take() {
            Some(Ok((ms, _))) => {
                let seconds = format!("{:.1}", ms as f64 / 1000.0);
                set_text(c.status, &t("aiset.test_ok").replace("{}", &seconds));
            }
            Some(Err(error)) => set_text(c.status, &format!("{} {error}", t("aiset.test_failed"))),
            None => {}
        }
    }
}

/// Schließt ohne Speichern und stellt die Stimme von vorher wieder her.
unsafe fn cancel(hwnd: HWND) {
    let changed = CONTROLS.with(|cell| cell.get()).map(|c| chosen_voice(&c)) != Some(VOICE_BEFORE.with(|b| b.get()));
    let callbacks = CALLBACKS.with(|cell| cell.get());
    let _ = DestroyWindow(hwnd);
    if let (true, Some(callbacks)) = (changed, callbacks) {
        let (voice, rate) = VOICE_BEFORE.with(|b| b.get());
        (callbacks.on_voice)(voice, rate);
    }
}

unsafe fn chosen_voice(c: &Controls) -> (Option<usize>, usize) {
    let row = SendMessageW(c.voice, CB_GETCURSEL, None, None).0;
    let voice = (row >= 0).then(|| SendMessageW(c.voice, CB_GETITEMDATA, Some(WPARAM(row as usize)), None).0 as usize);
    let rate = SendMessageW(c.rate, CB_GETCURSEL, None, None).0.max(0) as usize;
    (voice, rate)
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

unsafe fn selection(combo: HWND) -> Option<usize> {
    let row = SendMessageW(combo, CB_GETCURSEL, None, None).0;
    (row >= 0).then_some(row as usize)
}

unsafe fn save(c: &Controls) {
    let Some(original) = ORIGINAL.with(|cell| cell.borrow().clone()) else { return };
    let originals = [&original.hotkey_read, &original.hotkey_pause, &original.hotkey_menu, &original.hotkey_ocr, &original.hotkey_dictate];
    let mut hotkeys = Vec::new();
    for (control, original) in c.hotkeys.iter().zip(originals) {
        match read_hotkey(*control, original) {
            Ok(text) => hotkeys.push(text),
            Err(()) => {
                show_page(Page::Shortcuts);
                let text = t("settings.key_not_supported_possible");
                MessageBoxW(Some(c.window), &HSTRING::from(text), w!("Hedgie"), MB_ICONWARNING);
                let _ = SetFocus(Some(*control));
                return;
            }
        }
    }
    let provider = current_provider(c);
    if provider.kind != Kind::None && (provider.url.is_empty() || provider.model.is_empty()) {
        show_page(Page::Ai);
        set_text(c.status, if provider.url.is_empty() { t("aiset.need_url") } else { t("aiset.need_model") });
        let _ = SetFocus(Some(if provider.url.is_empty() { c.url } else { c.model }));
        return;
    }
    let microphone = match selection(c.microphone) {
        Some(0) | None => String::new(),
        Some(row) => crate::dictation::microphones().get(row - 1).cloned().unwrap_or_default(),
    };
    let values = Values {
        volume: (SendMessageW(c.volume, TBM_GETPOS, None, None).0 as u32).min(100),
        auto_language: checked(c.auto_language),
        clean_text: checked(c.clean),
        skip_page_edges: checked(c.edges),
        page_edges_ai: checked(c.edges_ai),
        microphone,
        dict_clean_fillers: checked(c.fillers),
        dict_auto_correct: checked(c.auto_correct),
        correction_mode: selection(c.correction)
            .and_then(|row| Correction::ALL.get(row))
            .map_or(original.correction_mode.clone(), |m| m.code().to_string()),
        correction_custom: text(c.custom),
        assistant: checked(c.assistant),
        provider,
        hotkey_dictate: hotkeys.pop().unwrap_or_default(),
        hotkey_ocr: hotkeys.pop().unwrap_or_default(),
        hotkey_menu: hotkeys.pop().unwrap_or_default(),
        hotkey_pause: hotkeys.pop().unwrap_or_default(),
        hotkey_read: hotkeys.pop().unwrap_or_default(),
        mouse_menu: match selection(c.mouse).and_then(|row| MOUSE_OPTIONS.get(row)) {
            Some((_, value)) => value.to_string(),
            None => original.mouse_menu.clone(),
        },
        language: selection(c.language)
            .and_then(|row| language_options().get(row).map(|(_, value)| value.clone()))
            .unwrap_or(original.language.clone()),
        theme: selection(c.theme).and_then(|row| theme::CHOICES.get(row)).map_or(original.theme.clone(), |v| v.to_string()),
        autostart: checked(c.autostart),
        close_to_pill: checked(c.pill),
        remember_text: checked(c.remember),
        update_check: checked(c.update),
        proxy: text(c.proxy),
    };
    let callbacks = CALLBACKS.with(|cell| cell.get());
    let _ = DestroyWindow(c.window);
    if let Some(callbacks) = callbacks {
        (callbacks.on_save)(values);
    }
}

/// Malt die Seitenleiste und die Linie über der Leiste unten.
unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    let mut client = RECT::default();
    let _ = GetClientRect(hwnd, &mut client);
    LOOK.with(|l| {
        if let Some(look) = l.borrow().as_ref() {
            let _ = FillRect(hdc, &client, look.white);
            let side = RECT { left: 0, top: 0, right: px(NAV), bottom: client.bottom };
            let _ = FillRect(hdc, &side, look.sidebar);
            let edge = RECT { left: px(NAV) - 1, top: 0, right: px(NAV), bottom: client.bottom };
            let _ = FillRect(hdc, &edge, look.line);
            let rule = RECT { left: px(NAV), top: client.bottom - px(FOOTER), right: client.right, bottom: client.bottom - px(FOOTER) + 1 };
            let _ = FillRect(hdc, &rule, look.line);
        }
    });
    let _ = EndPaint(hwnd, &ps);
}

/// Ein Eintrag der Seitenleiste: Symbol und Name, der gewählte hinterlegt und mit Akzentstrich.
unsafe fn draw_nav_item(item: &DRAWITEMSTRUCT) {
    let Some(page) = Page::ALL.get(item.itemID as usize).copied() else { return };
    let hdc: HDC = item.hDC;
    let rect = item.rcItem;
    let selected = item.itemState.0 & ODS_SELECTED.0 != 0;
    LOOK.with(|l| {
        let Some(look) = l.borrow().as_ref().map(|look| (look.sidebar, look.selected, look.accent, look.font, look.heading, look.icons)) else { return };
        let (sidebar, highlight, accent, font, bold, icons) = look;
        let _ = FillRect(hdc, &rect, sidebar);
        if selected {
            let inset = px(2);
            let region = CreateRoundRectRgn(rect.left + inset, rect.top + inset, rect.right - inset + 1, rect.bottom - inset + 1, px(8), px(8));
            let _ = FillRgn(hdc, region, highlight);
            let _ = DeleteObject(region.into());
            let mid = (rect.top + rect.bottom) / 2;
            let bar = CreateRoundRectRgn(rect.left + inset, mid - px(8), rect.left + inset + px(3) + 1, mid + px(8) + 1, px(3), px(3));
            let _ = FillRgn(hdc, bar, accent);
            let _ = DeleteObject(bar.into());
        }
        SetBkMode(hdc, TRANSPARENT);
        let _ = SetTextColor(hdc, TEXT);
        let old = SelectObject(hdc, icons.into());
        let mut icon: Vec<u16> = page.icon().encode_utf16(&mut [0; 2]).to_vec();
        let mut area = RECT { left: rect.left + px(14), top: rect.top, right: rect.left + px(40), bottom: rect.bottom };
        DrawTextW(hdc, &mut icon, &mut area, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX);
        SelectObject(hdc, if selected { bold } else { font }.into());
        let mut label: Vec<u16> = page.label().encode_utf16().collect();
        let mut area = RECT { left: rect.left + px(46), top: rect.top, right: rect.right - px(8), bottom: rect.bottom };
        DrawTextW(hdc, &mut label, &mut area, DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX);
        SelectObject(hdc, old);
    });
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let controls = CONTROLS.with(|c| c.get());
    unsafe {
        match msg {
            WM_COMMAND => {
                let (id, code) = ((wparam.0 & 0xffff) as u32, ((wparam.0 >> 16) & 0xffff) as u32);
                let callbacks = CALLBACKS.with(|cell| cell.get());
                let Some(c) = controls else { return LRESULT(0) };
                match (id, code) {
                    (ID_SAVE, 0) => save(&c),
                    (ID_CANCEL, 0) => cancel(hwnd),
                    (ID_NAV, LBN_SELCHANGE) => {
                        if let Some(page) = Page::ALL.get(current_page(&c)) {
                            show_page(*page);
                        }
                    }
                    (ID_VOICE | ID_RATE, CBN_SELCHANGE) => {
                        if let Some(callbacks) = callbacks {
                            let (voice, rate) = chosen_voice(&c);
                            (callbacks.on_voice)(voice, rate);
                        }
                    }
                    (ID_SAMPLE | ID_AI_VOICES | ID_DICT_SETUP | ID_AI_SETUP, 0) => {
                        if let Some(callbacks) = callbacks {
                            let action = match id {
                                ID_SAMPLE => callbacks.on_sample,
                                ID_AI_VOICES => callbacks.on_ai_voices,
                                ID_DICT_SETUP => callbacks.on_dict_setup,
                                _ => callbacks.on_ai_setup,
                            };
                            action();
                        }
                    }
                    (ID_HISTORY_CLEAR, 0) => {
                        crate::history::clear();
                        set_text(c.history_note, t("prefs.history_cleared"));
                    }
                    (ID_KIND, CBN_SELCHANGE) => on_kind_changed(&c),
                    (ID_MODELS, 0) => run_in_background(&c, 0),
                    (ID_TEST, 0) => run_in_background(&c, 1),
                    _ => {}
                }
            }
            WM_RESULT => {
                if let Some(c) = controls {
                    on_result(&c, wparam.0);
                }
            }
            WM_PAINT => paint(hwnd),
            WM_MEASUREITEM => {
                let item = &mut *(lparam.0 as *mut MEASUREITEMSTRUCT);
                item.itemHeight = px(42) as u32;
                return LRESULT(1);
            }
            WM_DRAWITEM => {
                draw_nav_item(&*(lparam.0 as *const DRAWITEMSTRUCT));
                return LRESULT(1);
            }
            // Beschriftungen, Kästchen und Regler auf weißem Grund; Hinweise grau; die Seitenleiste grau.
            WM_CTLCOLORSTATIC | WM_CTLCOLORBTN | WM_CTLCOLORLISTBOX => {
                let hdc = HDC(wparam.0 as *mut _);
                let control = HWND(lparam.0 as *mut _);
                let brush = LOOK.with(|l| {
                    l.borrow().as_ref().map(|look| {
                        if controls.is_some_and(|c| c.nav == control) {
                            SetBkColor(hdc, SIDEBAR);
                            return look.sidebar;
                        }
                        let _ = SetTextColor(hdc, if look.muted.contains(&control) { MUTED } else { TEXT });
                        SetBkColor(hdc, WHITE);
                        look.white
                    })
                });
                return match brush {
                    Some(brush) => LRESULT(brush.0 as isize),
                    None => DefWindowProcW(hwnd, msg, wparam, lparam),
                };
            }
            WM_CLOSE => cancel(hwnd),
            WM_DESTROY => {
                CONTROLS.with(|cell| cell.take());
                if let Some(look) = LOOK.with(|l| l.borrow_mut().take()) {
                    for font in [look.font, look.title, look.heading, look.icons] {
                        let _ = DeleteObject(font.into());
                    }
                    for brush in [look.white, look.sidebar, look.selected, look.line, look.accent] {
                        let _ = DeleteObject(brush.into());
                    }
                }
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}
