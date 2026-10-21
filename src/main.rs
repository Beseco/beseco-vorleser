//! Vorleser: liest markierten Text mit den Windows-Stimmen vor.
//! Ein Thread, ein verstecktes Fenster, ein Symbol im Infobereich.

#![windows_subsystem = "windows"]

mod ai;
mod audio;
mod aicheck;
mod aisettings;
mod aisetup;
mod aivoices;
mod config;
mod dictation;
mod dictbar;
mod dictsetup;
mod documents;
mod http;
mod i18n;
mod json;
mod minipill;
mod mouse;
mod mp3;
mod ocr;
mod ollama;
mod paint;
mod piper;
mod player;
mod selection;
mod settings;
mod snip;
mod textprep;
mod theme;
mod tray;
mod tts;
mod typing;
mod update;
mod window;
mod zip;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Globalization::GetUserDefaultUILanguage;
use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Registry::{
    RegDeleteKeyValueW, RegGetValueW, RegSetKeyValueW, HKEY_CURRENT_USER, REG_SZ, RRF_RT_REG_SZ,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::Controls::Dialogs::{
    GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST,
    OPENFILENAMEW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
};
use windows::Win32::System::DataExchange::COPYDATASTRUCT;
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageW, WM_COPYDATA,
    AllowSetForegroundWindow, AppendMenuW, CheckMenuRadioItem, CreateMenu, IDYES, MB_ICONQUESTION,
    MB_ICONWARNING, MB_YESNO, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW, IsDialogMessageW,
    MessageBoxW, SW_SHOWNORMAL,
    PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, TranslateMessage, HMENU, MB_ICONERROR, MB_ICONINFORMATION,
    MB_SETFOREGROUND, MENU_ITEM_FLAGS, MF_BYCOMMAND,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, WINDOW_EX_STYLE, WM_APP,
    WM_DESTROY, WM_ENDSESSION, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use config::Config;
use i18n::t;
use tts::Tts;

const APP_NAME: PCWSTR = w!("Vorleser");
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

const WM_TRAY: u32 = WM_APP + 1;
const WM_TTS: u32 = WM_APP + 2;
const WM_MOUSE_MENU: u32 = WM_APP + 3;
/// Von einer zweiten Instanz gesendet: Fenster zeigen.
const WM_SHOW_WINDOW: u32 = WM_APP + 4;
/// Die Suche nach Updates ist fertig; `wparam` = 1, wenn es eine neuere Version gibt.
const WM_UPDATE: u32 = WM_APP + 5;
/// Die Audiodatei ist geschrieben; `wparam` = 1 bei Erfolg.
const WM_WAV_DONE: u32 = WM_APP + 6;
/// Ergebnis einer von Hand gestarteten Update-Suche: `wparam` 0 = Fehler, 1 = aktuell, 2 = neuer.
const WM_UPDATE_MANUAL: u32 = WM_APP + 7;
/// Eine Datei ist gelesen; das Ergebnis liegt in `LOADED_FILE`.
const WM_FILE_LOADED: u32 = WM_APP + 8;
/// Fortschritt beim Lesen: `wparam` = Seite, `lparam` = Seitenzahl.
const WM_FILE_PROGRESS: u32 = WM_APP + 9;
/// Die Texterkennung eines Bildschirmausschnitts ist fertig; das Ergebnis liegt in `RECOGNIZED`.
const WM_OCR_DONE: u32 = WM_APP + 10;
/// Die KI ist fertig; das Ergebnis liegt in `AI_RESULT`.
const WM_AI_DONE: u32 = WM_APP + 11;
/// Fortschritt der KI: `wparam` = Schritt, `lparam` = Zahl der Schritte.
const WM_AI_PROGRESS: u32 = WM_APP + 12;
/// Diktat: Spracherkennung bereit (`wparam` 1), gescheitert (0, Text in `DICT_ERROR`) oder das
/// Mikrofon bleibt stumm (2).
const WM_DICT_READY: u32 = WM_APP + 13;
/// Diktat: neuer Text in `DICT_TEXTS`.
const WM_DICT_TEXT: u32 = WM_APP + 14;
/// Diktat: alle Abschnitte erkannt, die Erkennung ist beendet.
const WM_DICT_DONE: u32 = WM_APP + 15;
/// Einrichtungsassistent der KI öffnen (nach dem Einschalten im Installer).
const WM_OPEN_AI_SETUP: u32 = WM_APP + 16;
/// Abschrift einer Aufnahme: Fortschritt in Prozent (`wparam`).
const WM_TRANSCRIBE_PROGRESS: u32 = WM_APP + 17;
/// Abschrift fertig, Ergebnis in `TRANSCRIPT`.
const WM_TRANSCRIBE_DONE: u32 = WM_APP + 18;
/// Kennung der Update-Meldung im Infobereich.
const BALLOON_UPDATE: u32 = 1;
/// Klick auf eine Meldung im Infobereich (NIN_BALLOONUSERCLICK).
const BALLOON_CLICKED: u32 = 0x0405;

const MAIN_CLASS: PCWSTR = w!("VorleserFenster");
/// Mit diesem Argument (Autostart) bleibt das Fenster beim Start zu.
const ARG_TRAY: &str = "--tray";
/// Wie Windows' ASFW_ANY: jeder Prozess darf das Vordergrundfenster setzen.
const ANY_PROCESS: u32 = u32::MAX;

const HOTKEY_READ: i32 = 1;
const HOTKEY_PAUSE: i32 = 2;
const HOTKEY_MENU: i32 = 3;
const HOTKEY_OCR: i32 = 4;
const HOTKEY_DICTATE: i32 = 5;

const CMD_READ_SELECTION: u32 = 1;
const CMD_READ_CLIPBOARD: u32 = 2;
const CMD_PAUSE: u32 = 3;
const CMD_STOP: u32 = 4;
const CMD_AUTOSTART: u32 = 5;
const CMD_QUIT: u32 = 6;
const CMD_OPEN: u32 = 7;
const CMD_ABOUT: u32 = 8;
const CMD_SETTINGS: u32 = 9;
const CMD_UPDATE: u32 = 10;
const CMD_AI_VOICES: u32 = 11;
const CMD_OPEN_FILE: u32 = 12;
const CMD_SAVE_WAV: u32 = 13;
const CMD_CLOSE_WINDOW: u32 = 14;
const CMD_UNDO: u32 = 15;
const CMD_CUT: u32 = 16;
const CMD_COPY: u32 = 17;
const CMD_PASTE: u32 = 18;
const CMD_SELECT_ALL: u32 = 19;
const CMD_CLEAN: u32 = 20;
const CMD_CLEAR: u32 = 21;
const CMD_READ_WINDOW: u32 = 22;
const CMD_SAMPLE: u32 = 23;
const CMD_TOPMOST: u32 = 24;
const CMD_MANUAL: u32 = 25;
const CMD_CHECK_UPDATE: u32 = 26;
const CMD_REPORT: u32 = 27;
const CMD_PREV: u32 = 28;
const CMD_NEXT: u32 = 29;
const CMD_SCREEN_OCR: u32 = 30;
const CMD_GUIDE_SHOW: u32 = 31;
const CMD_GUIDE_READ: u32 = 32;
const CMD_ASSISTANT: u32 = 33;
const CMD_SUMMARIZE: u32 = 34;
const CMD_READ_RESULT: u32 = 35;
const CMD_AI_SETTINGS: u32 = 36;
const CMD_DICTATE: u32 = 37;
const CMD_CORRECT: u32 = 38;
const CMD_DICT_SETUP: u32 = 39;
const CMD_AI_SETUP: u32 = 40;
const CMD_PILL: u32 = 41;
const CMD_TRANSCRIBE: u32 = 42;
/// Korrekturstufe: `CMD_CORRECTION_MODE` + Nummer.
const CMD_CORRECTION_MODE: u32 = 3600;
/// Mikrofon: `CMD_MICROPHONE` (Windows-Standard) bzw. + 1 + Geräte-Nummer.
const CMD_MICROPHONE: u32 = 3700;
/// Art der Zusammenfassung: `CMD_SUMMARY_MODE` + Nummer.
const CMD_SUMMARY_MODE: u32 = 3500;

/// Registrierungswert, mit dem der Installer den Officeassistenten einschaltet.
const INSTALLER_KEY: PCWSTR = w!(r"Software\Beseco\Vorleser");
const CMD_RATE: u32 = 100;
const CMD_VOICE: u32 = 1000;
/// Sprache der Oberfläche: `CMD_LANGUAGE` = automatisch, danach eine Nummer je Sprache.
const CMD_LANGUAGE: u32 = 3000;

const MANUAL_URL: &str = "https://github.com/Beseco/beseco-vorleser#readme";
const MANUAL_URL_EN: &str = "https://github.com/Beseco/beseco-vorleser/blob/main/README.en.md";
const REPORT_URL: &str = "https://github.com/Beseco/beseco-vorleser/issues/new/choose";

/// SAPI-Geschwindigkeit und Schlüssel des Namens.
const RATES: [(i32, &str); 9] = [
    (-6, "rate.very_slow"),
    (-4, "rate.slow"),
    (-2, "rate.slightly_slower"),
    (0, "rate.normal"),
    (2, "rate.slightly_faster"),
    (4, "rate.fast"),
    (6, "rate.faster"),
    (8, "rate.very_fast"),
    (10, "rate.maximum"),
];

#[derive(Clone, Copy, PartialEq)]
enum State {
    Idle,
    Speaking,
    Paused,
}

struct App {
    hwnd: HWND,
    tts: Tts,
    cfg: Config,
    state: State,
    /// Aktueller Text für genau einen neuen Versuch, falls die Stimme mit einem Fehler abbricht.
    retry: Option<String>,
    /// Mitlesen: Der gesprochene Text steht im Fenster; das aktuelle Wort wird dort markiert.
    follow: Option<Follow>,
    /// Laufendes Diktat (Mikrofon); beim Verwerfen endet die Aufnahme.
    dictation: Option<dictation::Recorder>,
    dict_target: DictTarget,
    /// Was in diesem Diktat in ein anderes Programm getippt wurde (für „Korrigieren“).
    dict_typed: String,
    /// „Korrigieren“ wurde während des Diktats gedrückt: nach dem letzten Abschnitt korrigieren.
    dict_correct_pending: bool,
    /// Erkannter Text, der noch nicht getippt ist, weil das Zielprogramm nicht vorn war.
    dict_waiting: Vec<String>,
    /// Der Mini-Player begleitet das laufende Vorlesen (Text von außen, Fenster zu).
    player_on: bool,
    /// Satzanfänge des Textes, den der Player begleitet (für „Satz 2 von 5“).
    player_sentences: Vec<u32>,
}

#[derive(Clone, Copy)]
struct Follow {
    /// Position des gesprochenen Textes im Textfeld.
    base: u32,
    /// Markierung, die nach dem Vorlesen wiederhergestellt wird.
    restore: (u32, u32),
    /// Zuletzt gemeldetes Wort bzw. Satz, im Textfeld.
    pos: u32,
    /// Ende des vorzulesenden Bereichs im Textfeld.
    end: u32,
    /// Das Textfeld, in dem mitgelesen wird.
    pane: window::Pane,
}

impl Follow {
    fn new(base: u32, end: u32, restore: (u32, u32)) -> Follow {
        Follow { base, restore, pos: base, end, pane: window::Pane::Text }
    }

    fn in_pane(self, pane: window::Pane) -> Follow {
        Follow { pane, ..self }
    }
}

/// Zeilenenden so, wie das Textfeld sie speichert. Der gesprochene Text muss zeichengenau dem
/// angezeigten entsprechen, sonst trifft die Markierung beim Mitlesen das falsche Wort.
fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n").replace('\n', "\r\n")
}

thread_local! {
    static APP: RefCell<Option<App>> = const { RefCell::new(None) };
}

/// Nachricht, mit der der Explorer nach einem Neustart zum erneuten Anlegen des Symbols auffordert.
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);

/// Testlauf neben einer laufenden Instanz (Umgebungsvariable `VORLESER_TEST`): eigene
/// Instanzsperre, keine globalen Tastenkombinationen, kein Maus-Auslöser, kein Ton, keine
/// gespeicherten Einstellungen.
static TEST_MODE: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Ziel der gerade geschriebenen Audiodatei.
    static PENDING_WAV: RefCell<Option<String>> = const { RefCell::new(None) };
}

/// Ergebnis des Hintergrund-Threads, der eine Datei liest bzw. einen Ausschnitt erkennt.
static LOADED_FILE: std::sync::Mutex<Option<Result<String, String>>> = std::sync::Mutex::new(None);
static RECOGNIZED: std::sync::Mutex<Option<Result<String, String>>> = std::sync::Mutex::new(None);
static AI_RESULT: std::sync::Mutex<Option<Result<String, String>>> = std::sync::Mutex::new(None);
/// Die KI arbeitet gerade.
static AI_BUSY: AtomicBool = AtomicBool::new(false);
/// Woran die KI gerade arbeitet: Zusammenfassung (false) oder Korrektur (true).
static AI_CORRECTING: AtomicBool = AtomicBool::new(false);
/// Erkannter Text des Diktats, der noch ins Fenster muss.
static DICT_TEXTS: std::sync::Mutex<std::collections::VecDeque<String>> = std::sync::Mutex::new(std::collections::VecDeque::new());
static DICT_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
/// Die laufende Zusammenfassung ist ein Protokoll (wird nicht gleich vorgelesen).
static AI_MINUTES: AtomicBool = AtomicBool::new(false);
/// Eine Aufnahme wird gerade abgeschrieben; `TRANSCRIBE_CANCEL` bricht ab.
static TRANSCRIBING: AtomicBool = AtomicBool::new(false);
static TRANSCRIBE_CANCEL: AtomicBool = AtomicBool::new(false);
/// Ergebnis der Abschrift: Dateiname, Dauer in Sekunden und Abschnitte (Anfang, Text).
static TRANSCRIPT: std::sync::Mutex<Option<Result<(String, f64, Vec<(f64, String)>), String>>> = std::sync::Mutex::new(None);
/// Die laufende Korrektur gilt einem Diktat in einem anderen Programm.
static AI_FOR_PROGRAM: AtomicBool = AtomicBool::new(false);
/// Name in der Diktier-Pille, wenn ins eigene Fenster diktiert wird.
const APP_NAME_TEXT: &str = "Vorleser";

/// Wohin ein Diktat schreibt.
#[derive(Clone, Copy, PartialEq, Debug)]
enum DictTarget {
    /// Ins Textfeld des Vorleser-Fensters.
    Window,
    /// In ein anderes Programm (Fenster als Zahl), per Tastatureingabe.
    Program(isize),
}

/// Im laufenden Diktat wurde schon Text erkannt (sonst gibt es nichts zu korrigieren).
static DICT_GOT_TEXT: AtomicBool = AtomicBool::new(false);
/// Es wird gerade eine Datei gelesen.
static LOADING: AtomicBool = AtomicBool::new(false);

/// Solange ein Menü offen ist, wird kein zweites geöffnet.
static MENU_OPEN: AtomicBool = AtomicBool::new(false);

/// Führt `f` mit dem App-Zustand aus. `None` vor der Initialisierung oder bei verschachteltem Aufruf.
fn with_app<R>(f: impl FnOnce(&mut App) -> R) -> Option<R> {
    APP.with(|cell| {
        let mut guard = cell.try_borrow_mut().ok()?;
        guard.as_mut().map(f)
    })
}

impl App {
    fn new(hwnd: HWND) -> windows::core::Result<App> {
        let cfg = Config::load();
        http::set_proxy(&cfg.proxy);
        let mut tts = Tts::new()?;
        tts.notify_window(hwnd, WM_TTS)?;
        if !cfg.voice.is_empty() {
            tts.set_voice(&cfg.voice);
        }
        tts.set_rate(cfg.rate);
        // Der Testlauf bleibt stumm.
        tts.set_volume(if TEST_MODE.load(Ordering::Relaxed) { 0 } else { cfg.volume });
        let mut app = App { hwnd, tts, cfg, state: State::Idle, retry: None, follow: None, dictation: None, dict_target: DictTarget::Window, dict_typed: String::new(), dict_correct_pending: false, dict_waiting: Vec::new(), player_on: false, player_sentences: Vec::new() };
        app.add_icon();
        app.register_hotkeys();
        app.fill_window();
        if app.apply_installer_choice() && !app.provider().is_ready() {
            unsafe {
                let _ = PostMessageW(Some(hwnd), WM_OPEN_AI_SETUP, WPARAM(0), LPARAM(0));
            }
        }
        app.show_assistant();
        if !TEST_MODE.load(Ordering::Relaxed) {
            update::start(hwnd, WM_UPDATE, app.cfg.update_check, app.cfg.last_update_check);
        }
        Ok(app)
    }

    fn add_icon(&self) {
        tray::add(self.hwnd, tray::load_icon(), WM_TRAY, &self.tip());
    }

    fn tip(&self) -> String {
        match self.state {
            State::Idle => t("app.vorleser_reads_selected_text")
                .replace("{}", &i18n::hotkey(&self.cfg.hotkey_read)),
            State::Speaking => t("app.vorleser_reading").to_string(),
            State::Paused => t("app.vorleser_paused").to_string(),
        }
    }

    fn register_hotkeys(&self) {
        if TEST_MODE.load(Ordering::Relaxed) {
            return;
        }
        let hotkeys = [
            (HOTKEY_READ, &self.cfg.hotkey_read),
            (HOTKEY_PAUSE, &self.cfg.hotkey_pause),
            (HOTKEY_MENU, &self.cfg.hotkey_menu),
            (HOTKEY_OCR, &self.cfg.hotkey_ocr),
            (HOTKEY_DICTATE, &self.cfg.hotkey_dictate),
        ];
        let mut failed = Vec::new();
        for (id, text) in hotkeys {
            if text.trim().is_empty() {
                continue;
            }
            let ok = config::parse_hotkey(text).is_some_and(|(mods, vk)| unsafe {
                let mods = HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT;
                RegisterHotKey(Some(self.hwnd), id, mods, vk).is_ok()
            });
            if !ok {
                failed.push(text.as_str());
            }
        }
        let trigger = config::parse_mouse_trigger(&self.cfg.mouse_menu);
        mouse::set(self.hwnd, WM_MOUSE_MENU, trigger.unwrap_or(0));
        if trigger.is_none() && !self.cfg.mouse_menu.trim().is_empty() {
            failed.push(self.cfg.mouse_menu.as_str());
        }
        if !failed.is_empty() {
            let failed: Vec<String> = failed.iter().map(|text| i18n::hotkey(text)).collect();
            let text = format!("{} {}", t("app.unavailable_or_invalid"), failed.join(", "));
            tray::balloon(self.hwnd, t("app.shortcut"), &text);
        }
    }

    fn settings(&self) -> settings::Values {
        settings::Values {
            hotkey_read: self.cfg.hotkey_read.clone(),
            hotkey_pause: self.cfg.hotkey_pause.clone(),
            hotkey_menu: self.cfg.hotkey_menu.clone(),
            hotkey_ocr: self.cfg.hotkey_ocr.clone(),
            hotkey_dictate: self.cfg.hotkey_dictate.clone(),
            mouse_menu: self.cfg.mouse_menu.clone(),
            volume: self.cfg.volume,
            clean_text: self.cfg.clean_text,
            remember_text: self.cfg.remember_text,
            update_check: self.cfg.update_check,
            language: self.cfg.language.clone(),
            proxy: self.cfg.proxy.clone(),
            close_to_pill: self.cfg.close_to_pill,
        }
    }

    fn apply_settings(&mut self, values: settings::Values) {
        self.cfg.hotkey_read = values.hotkey_read;
        self.cfg.hotkey_pause = values.hotkey_pause;
        self.cfg.hotkey_menu = values.hotkey_menu;
        self.cfg.hotkey_ocr = values.hotkey_ocr;
        self.cfg.hotkey_dictate = values.hotkey_dictate;
        self.cfg.mouse_menu = values.mouse_menu;
        self.cfg.volume = values.volume;
        self.cfg.clean_text = values.clean_text;
        self.cfg.update_check = values.update_check;
        self.cfg.remember_text = values.remember_text;
        self.cfg.close_to_pill = values.close_to_pill;
        self.cfg.proxy = values.proxy;
        http::set_proxy(&self.cfg.proxy);
        if values.language != self.cfg.language {
            self.set_language(&values.language);
        }
        update::set_enabled(values.update_check);
        self.cfg.save();
        for id in [HOTKEY_READ, HOTKEY_PAUSE, HOTKEY_MENU, HOTKEY_OCR, HOTKEY_DICTATE] {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), id);
            }
        }
        self.register_hotkeys();
        self.tts.set_volume(self.cfg.volume);
        tray::set_tip(self.hwnd, &self.tip());
        self.update_pill();
        self.refresh_menu_bar();
    }

    /// Mini-Pille am Bildschirmrand: sichtbar, solange das Fenster zu ist und weder Diktat noch
    /// Mini-Player laufen (wenn so eingestellt).
    fn update_pill(&self) {
        let show = self.cfg.close_to_pill && window::visible().is_none() && !dictbar::visible() && !player::visible();
        if !show {
            minipill::hide();
            return;
        }
        let key = |value: &str| if value.trim().is_empty() { t("help.off").to_string() } else { i18n::hotkey(value) };
        let hint = t("pill.hint").replacen("{}", &key(&self.cfg.hotkey_dictate), 1).replacen("{}", &key(&self.cfg.hotkey_read), 1);
        minipill::show(&hint, on_pill_action);
    }

    /// Der Installer kann den Officeassistenten vorab einschalten (einmalig). `true`, wenn er
    /// ihn gerade eingeschaltet hat.
    fn apply_installer_choice(&mut self) -> bool {
        let mut value = [0u16; 8];
        let mut size = (value.len() * 2) as u32;
        let status = unsafe {
            RegGetValueW(HKEY_CURRENT_USER, INSTALLER_KEY, w!("Assistant"), RRF_RT_REG_SZ, None, Some(value.as_mut_ptr() as *mut _), Some(&mut size))
        };
        if status == ERROR_SUCCESS && !config::is_read_only() {
            self.cfg.assistant = value[0] == b'1' as u16;
            self.cfg.save();
            unsafe {
                let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, INSTALLER_KEY, w!("Assistant"));
            }
            return self.cfg.assistant;
        }
        false
    }

    fn provider(&self) -> ai::Provider {
        ai::Provider {
            kind: ai::Kind::from_code(&self.cfg.ai_kind),
            url: self.cfg.ai_url.clone(),
            key: ai::unprotect(&self.cfg.ai_key),
            model: self.cfg.ai_model.clone(),
        }
    }

    /// Arbeitsbereich, Auswahl der Zusammenfassungsart und Anzeige des Anbieters.
    fn show_assistant(&self) {
        window::set_assistant(self.cfg.assistant);
        let labels: Vec<&str> = ai::Summary::ALL.iter().map(|s| summary_label(*s)).collect();
        let mode = ai::Summary::from_code(&self.cfg.summary_mode);
        window::set_summary_modes(&labels, ai::Summary::ALL.iter().position(|s| *s == mode).unwrap_or(0));
        let provider = self.provider();
        let status = if provider.is_ready() {
            t("assist.status").replace("{}", &format!("{} · {}", provider_name(provider.kind), provider.model))
        } else {
            t("assist.not_configured").to_string()
        };
        window::set_ai_status(&status);
        window::set_ai_ready(provider.is_ready());
        let labels: Vec<&str> = ai::Correction::ALL.iter().map(|c| correction_label(*c)).collect();
        let correction = ai::Correction::from_code(&self.cfg.correction_mode);
        window::set_correction_modes(&labels, ai::Correction::ALL.iter().position(|c| *c == correction).unwrap_or(0));
        if self.dictation.is_none() {
            let hint = if dictation::best_installed().is_some() { t("assist.dict_hint") } else { t("assist.dict_not_set_up") };
            window::set_dictating(false, hint);
        }
        let anywhere = if self.cfg.hotkey_dictate.trim().is_empty() { t("help.off").to_string() } else { i18n::hotkey(&self.cfg.hotkey_dictate) };
        window::set_dict_hint(&t("assist.dict_anywhere").replace("{}", &anywhere));
    }

    fn set_correction_mode(&mut self, mode: ai::Correction) {
        self.cfg.correction_mode = mode.code().to_string();
        self.cfg.save();
        self.show_assistant();
        self.refresh_menu_bar();
    }

    /// Diktat ins Vorleser-Fenster (F8). `false`, wenn die Spracherkennung erst eingerichtet werden muss.
    fn toggle_dictation(&mut self) -> bool {
        self.toggle_dictation_to(DictTarget::Window)
    }

    /// Startet oder beendet das Diktat. `false`, wenn die Spracherkennung erst eingerichtet werden muss.
    fn toggle_dictation_to(&mut self, target: DictTarget) -> bool {
        if self.dictation.is_some() {
            // Das Verwerfen beendet die Aufnahme; der letzte Abschnitt wird noch erkannt.
            self.dictation = None;
            window::set_dictating(false, t("assist.dict_finishing"));
            dictbar::set_state(dictbar::State::Writing);
            self.refresh_menu_bar();
            return true;
        }
        let Some(model) = dictation::best_installed() else { return false };
        // Sonst nähme das Mikrofon die eigene Sprachausgabe auf.
        self.stop();
        DICT_GOT_TEXT.store(false, Ordering::Relaxed);
        let (sender, segments) = std::sync::mpsc::channel::<Vec<i16>>();
        let recorder = match dictation::Recorder::start(sender, &self.cfg.microphone) {
            Ok(recorder) => recorder,
            Err(error) => {
                unsafe {
                    MessageBoxW(window::visible(), &HSTRING::from(error), APP_NAME, MB_ICONWARNING);
                }
                return true;
            }
        };
        let hwnd = self.hwnd.0 as isize;
        let language = i18n::current().to_string();
        std::thread::spawn(move || {
            let post = |msg: u32, wparam: usize| unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(0));
            };
            match dictation::Server::start(model, &language) {
                Ok(server) => {
                    post(WM_DICT_READY, 1);
                    // Läuft, bis die Aufnahme endet und alle Abschnitte erkannt sind.
                    for segment in segments {
                        if segment.is_empty() {
                            post(WM_DICT_READY, 2);
                            continue;
                        }
                        post(WM_DICT_READY, 3);
                        match server.transcribe(&segment) {
                            Ok(text) if !text.is_empty() => {
                                DICT_TEXTS.lock().unwrap().push_back(text);
                                post(WM_DICT_TEXT, 0);
                            }
                            Ok(_) => {}
                            Err(error) => *DICT_ERROR.lock().unwrap() = Some(error),
                        }
                        post(WM_DICT_READY, 1);
                    }
                }
                Err(error) => {
                    *DICT_ERROR.lock().unwrap() = Some(error);
                    post(WM_DICT_READY, 0);
                }
            }
            post(WM_DICT_DONE, 0);
        });
        let program = match target {
            DictTarget::Window => APP_NAME_TEXT.to_string(),
            DictTarget::Program(hwnd) => typing::program_name(HWND(hwnd as *mut _)),
        };
        dictbar::show(&program, self.provider().is_ready(), Some(recorder.level_meter()), on_dictbar_action);
        minipill::hide();
        self.dictation = Some(recorder);
        self.dict_target = target;
        self.dict_typed.clear();
        self.dict_waiting.clear();
        self.dict_correct_pending = false;
        if target == DictTarget::Window {
            window::show();
        }
        window::set_dictating(true, t("assist.dict_preparing"));
        self.refresh_menu_bar();
        true
    }

    fn on_dict_ready(&mut self, state: usize) {
        if state != 0 {
            let running = self.dictation.is_some();
            // Nach dem Beenden wird nur noch der letzte Abschnitt geschrieben.
            let bar = match state {
                2 if running => dictbar::State::Silent,
                1 if running => dictbar::State::Listening,
                _ => dictbar::State::Writing,
            };
            dictbar::set_state(bar);
            if running {
                window::set_dictating(true, if state == 2 { t("assist.dict_silent") } else { t("assist.dict_listening") });
            }
            return;
        }
        self.dictation = None;
        let error = DICT_ERROR.lock().unwrap().take().unwrap_or_default();
        window::set_dictating(false, "");
        dictbar::hide();
        unsafe {
            MessageBoxW(window::visible(), &HSTRING::from(error), APP_NAME, MB_ICONWARNING);
        }
    }

    fn on_dict_text(&mut self) {
        let texts: Vec<String> = DICT_TEXTS.lock().unwrap().drain(..).collect();
        for text in texts {
            DICT_GOT_TEXT.store(true, Ordering::Relaxed);
            match self.dict_target {
                DictTarget::Window => window::insert_text(&text),
                DictTarget::Program(_) => self.dict_waiting.push(text),
            }
        }
        self.flush_dictation();
    }

    /// Tippt wartenden Text, aber nur ins Zielprogramm: Ist gerade ein anderes Fenster vorn,
    /// wartet er, bis das Zielprogramm wieder vorn ist (nie in ein fremdes Fenster).
    fn flush_dictation(&mut self) -> bool {
        let DictTarget::Program(target) = self.dict_target else { return true };
        if self.dict_waiting.is_empty() {
            return true;
        }
        if typing::foreground().0 as isize != target {
            dictbar::set_state(dictbar::State::Waiting);
            return false;
        }
        for text in std::mem::take(&mut self.dict_waiting) {
            let piece = window::dictated_text(&self.dict_typed, &text);
            typing::type_text(&piece);
            self.dict_typed.push_str(&piece);
        }
        true
    }

    fn on_dict_done(&mut self) {
        if self.dictation.is_some() {
            return; // Ein neues Diktat läuft schon.
        }
        self.show_assistant();
        dictbar::set_state(if self.flush_dictation() { dictbar::State::Finished } else { dictbar::State::Waiting });
        if let Some(error) = DICT_ERROR.lock().unwrap().take() {
            tray::balloon(self.hwnd, "Vorleser", &error);
        }
        let got_text = DICT_GOT_TEXT.swap(false, Ordering::Relaxed);
        if std::mem::take(&mut self.dict_correct_pending) {
            self.correct_dictation();
        } else if got_text && self.dict_target == DictTarget::Window && self.cfg.dict_auto_correct && self.provider().is_ready() && window::text().is_some() {
            self.correct();
        }
    }

    /// „Korrigieren“ in der Diktier-Pille. Läuft das Diktat noch, wird es erst beendet, damit der
    /// letzte Abschnitt dabei ist. Im Fenster erscheint die Fassung rechts; in einem anderen
    /// Programm ersetzt sie den eben diktierten Text.
    fn correct_dictation(&mut self) {
        if self.dictation.is_some() {
            self.dict_correct_pending = true;
            self.toggle_dictation_to(self.dict_target);
            return;
        }
        if self.dict_target == DictTarget::Window {
            if self.correct() {
                dictbar::set_state(dictbar::State::Correcting);
            }
            return;
        }
        let provider = self.provider();
        if !provider.is_ready() || self.dict_typed.trim().is_empty() || AI_BUSY.swap(true, Ordering::Relaxed) {
            return;
        }
        AI_CORRECTING.store(true, Ordering::Relaxed);
        AI_FOR_PROGRAM.store(true, Ordering::Relaxed);
        dictbar::set_state(dictbar::State::Correcting);
        let text = self.dict_typed.clone();
        let mode = ai::Correction::from_code(&self.cfg.correction_mode);
        let custom = self.cfg.correction_custom.clone();
        let hwnd = self.hwnd.0 as isize;
        std::thread::spawn(move || {
            *AI_RESULT.lock().unwrap() = Some(provider.correct(&text, mode, &custom));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_AI_DONE, WPARAM(0), LPARAM(0));
            }
        });
    }

    /// Ersetzt im anderen Programm den diktierten Text durch die korrigierte Fassung, aber nur,
    /// wenn das Programm noch vorn ist; sonst bleibt der Text unangetastet.
    fn replace_in_program(&mut self, corrected: &str) {
        let DictTarget::Program(target) = self.dict_target else { return };
        if typing::foreground().0 as isize != target || !self.dict_waiting.is_empty() {
            tray::balloon(self.hwnd, "Vorleser", t("dictbar.not_replaced"));
            return;
        }
        let corrected = corrected.trim();
        let leading = self.dict_typed.len() - self.dict_typed.trim_start().len();
        let corrected = format!("{}{corrected}", &self.dict_typed[..leading]);
        typing::backspace(self.dict_typed.chars().count());
        typing::type_text(&corrected);
        self.dict_typed = corrected;
    }

    /// Überarbeitet den Text links (oder die Markierung) in der eingestellten Stufe; das Ergebnis
    /// erscheint rechts.
    fn correct(&mut self) -> bool {
        let provider = self.provider();
        if !provider.is_ready() {
            tray::balloon(self.hwnd, "Vorleser", t("app.ai_not_ready"));
            return false;
        }
        let Some(text) = window::text() else {
            tray::balloon(self.hwnd, "Vorleser", t("app.text_field_empty"));
            return true;
        };
        if AI_BUSY.swap(true, Ordering::Relaxed) {
            return true;
        }
        AI_CORRECTING.store(true, Ordering::Relaxed);
        let mode = ai::Correction::from_code(&self.cfg.correction_mode);
        let custom = self.cfg.correction_custom.clone();
        let hwnd = self.hwnd.0 as isize;
        window::set_ai_busy(true);
        window::set_title(Some(t("app.ai_correcting")));
        std::thread::spawn(move || {
            *AI_RESULT.lock().unwrap() = Some(provider.correct(&text, mode, &custom));
            unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_AI_DONE, WPARAM(0), LPARAM(0));
            }
        });
        true
    }

    fn set_assistant(&mut self, on: bool) -> bool {
        self.cfg.assistant = on;
        self.cfg.save();
        self.show_assistant();
        self.refresh_menu_bar();
        if on {
            window::show();
        }
        // Beim ersten Einschalten gleich die Einrichtung anbieten.
        on && !self.provider().is_ready()
    }

    fn set_summary_mode(&mut self, mode: ai::Summary) {
        self.cfg.summary_mode = mode.code().to_string();
        self.cfg.save();
        self.show_assistant();
        self.refresh_menu_bar();
    }

    /// Fasst den markierten Text bzw. den ganzen Text links zusammen; das Ergebnis erscheint
    /// rechts und wird vorgelesen.
    fn summarize(&mut self) -> bool {
        let provider = self.provider();
        if !provider.is_ready() {
            tray::balloon(self.hwnd, "Vorleser", t("app.ai_not_ready"));
            return false;
        }
        let Some(text) = window::text() else {
            tray::balloon(self.hwnd, "Vorleser", t("app.text_field_empty"));
            return true;
        };
        if AI_BUSY.swap(true, Ordering::Relaxed) {
            return true;
        }
        AI_CORRECTING.store(false, Ordering::Relaxed);
        let text = self.prepare(text);
        let mode = ai::Summary::from_code(&self.cfg.summary_mode);
        AI_MINUTES.store(mode == ai::Summary::Minutes, Ordering::Relaxed);
        let hwnd = self.hwnd.0 as isize;
        window::set_ai_busy(true);
        window::set_title(Some(t("app.ai_working")));
        std::thread::spawn(move || {
            let post = |msg: u32, wparam: usize, lparam: isize| unsafe {
                let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(lparam));
            };
            let result = provider.summarize(&text, mode, |step, steps| post(WM_AI_PROGRESS, step, steps as isize));
            *AI_RESULT.lock().unwrap() = Some(result);
            post(WM_AI_DONE, 0, 0);
        });
        true
    }

    fn on_ai_done(&mut self) {
        AI_BUSY.store(false, Ordering::Relaxed);
        window::set_ai_busy(false);
        window::set_title(None);
        match AI_RESULT.lock().unwrap().take() {
            Some(Ok(result)) if AI_FOR_PROGRAM.swap(false, Ordering::Relaxed) => {
                self.replace_in_program(&result);
                dictbar::set_state(dictbar::State::Finished);
            }
            Some(Ok(result)) if AI_CORRECTING.load(Ordering::Relaxed) => {
                window::set_result_label("assist.corrected");
                window::set_text_in(window::Pane::Result, &to_crlf(&result));
                dictbar::set_state(dictbar::State::Finished);
            }
            Some(Ok(minutes)) if AI_MINUTES.swap(false, Ordering::Relaxed) => {
                // Ein Protokoll ist zum Lesen und Weitergeben da, nicht zum Vorlesen.
                window::set_result_label("assist.minutes");
                window::set_text_in(window::Pane::Result, &to_crlf(&minutes));
            }
            Some(Ok(summary)) => {
                self.stop();
                window::set_result_label("assist.result");
                window::set_text_in(window::Pane::Result, &to_crlf(&summary));
                self.speak_pane(window::Pane::Result);
            }
            Some(Err(error)) => {
                AI_FOR_PROGRAM.store(false, Ordering::Relaxed);
                dictbar::set_state(dictbar::State::Finished);
                let text = HSTRING::from(format!("{}\n\n{error}", t("app.ai_failed")));
                unsafe {
                    MessageBoxW(window::visible(), &text, APP_NAME, MB_ICONWARNING);
                }
            }
            None => {}
        }
    }

    fn apply_ai_settings(&mut self, provider: ai::Provider, custom: String) {
        self.cfg.correction_custom = custom;
        self.cfg.ai_kind = provider.kind.code().to_string();
        self.cfg.ai_url = provider.url;
        self.cfg.ai_key = ai::protect(&provider.key);
        self.cfg.ai_model = provider.model;
        self.cfg.save();
        self.show_assistant();
        self.refresh_menu_bar();
    }

    /// Stellt die Sprache der Oberfläche um; Fenster und Menüs wechseln sofort.
    fn set_language(&mut self, code: &str) {
        self.cfg.language = code.to_string();
        self.cfg.save();
        i18n::init(code);
        window::relabel();
        self.show_assistant();
        // Die Namen der KI-Stimmen tragen einen übersetzten Zusatz.
        self.tts.reload_voices();
        self.fill_window();
        tray::set_tip(self.hwnd, &self.tip());
    }

    /// Text so, wie er gesprochen wird (je nach Einstellung bereinigt).
    fn prepare(&self, text: String) -> String {
        if self.cfg.clean_text {
            textprep::clean(&text)
        } else {
            text
        }
    }

    fn set_state(&mut self, state: State) {
        if self.state != state {
            self.state = state;
            tray::set_tip(self.hwnd, &self.tip());
            window::set_playback(state != State::Idle, state == State::Paused);
            self.refresh_menu_bar();
            self.update_player();
            self.update_progress();
        }
    }

    /// Gerade gesprochener Satz und Zahl der Sätze im vorgelesenen Bereich.
    fn sentence_progress(&self) -> Option<(usize, usize)> {
        let follow = self.follow?;
        let total = self.player_sentences.iter().filter(|s| **s < follow.end).count().max(1);
        let current = self.player_sentences.iter().filter(|s| **s <= follow.pos).count().clamp(1, total);
        Some((current, total))
    }

    /// „Satz 3 von 9“ und der Balken im Fenster.
    fn update_progress(&self) {
        window::set_progress(if self.state == State::Idle { None } else { self.sentence_progress() });
    }

    /// Zeigt den Mini-Player mit dem aktuellen Stand oder blendet ihn aus.
    fn update_player(&mut self) {
        if !self.player_on {
            return;
        }
        if self.state == State::Idle || window::visible().is_some() {
            self.player_on = false;
            player::hide();
            self.update_pill();
            return;
        }
        let voice = self.current_voice().map_or(String::new(), |i| short_voice_name(&self.tts.voices()[i].name));
        let progress = self.sentence_progress().map_or(String::new(), |(current, total)| {
            t("player.sentence").replacen("{}", &current.to_string(), 1).replacen("{}", &total.to_string(), 1)
        });
        let rates = RATES.iter().map(|(rate, key)| format!("{}  {}", speed_label(*rate), t(key))).collect();
        let info = player::Info {
            voice,
            progress,
            paused: self.state == State::Paused,
            speed: speed_label(self.cfg.rate),
            rates,
            rate: self.rate_index(),
        };
        player::show(info, on_player_action);
        minipill::hide();
    }

    /// Index der gerade eingestellten Stimme in `tts.voices()`.
    fn current_voice(&self) -> Option<usize> {
        let current = self.tts.current_voice_id()?;
        self.tts.voices().iter().position(|v| v.id == current)
    }

    /// Stimmen nach Sprache gruppiert (Indizes in `tts.voices()`); die Sprache der
    /// Windows-Oberfläche steht vorn.
    fn voice_groups(&self) -> Vec<(&str, Vec<usize>)> {
        let primary = |lang_id: u16| lang_id & 0x3ff;
        let ui_language = primary(unsafe { GetUserDefaultUILanguage() });
        let mut groups: BTreeMap<(bool, &str), Vec<usize>> = BTreeMap::new();
        for (i, voice) in self.tts.voices().iter().enumerate() {
            let foreign = primary(voice.lang_id) != ui_language;
            groups.entry((foreign, voice.language.as_str())).or_default().push(i);
        }
        groups.into_iter().map(|((_, language), indices)| (language, indices)).collect()
    }

    /// Alle Stimmen, nach Sprache sortiert: Index und Name.
    fn voice_items(&self) -> Vec<(usize, String)> {
        let voices = self.tts.voices();
        self.voice_groups().into_iter().flat_map(|(_, indices)| indices).map(|i| (i, voices[i].name.clone())).collect()
    }

    fn rate_index(&self) -> usize {
        RATES.iter().position(|(rate, _)| *rate == self.cfg.rate).unwrap_or(0)
    }

    fn fill_window(&self) {
        settings::set_voices(&self.voice_items(), self.current_voice());
        window::set_topmost(self.cfg.always_on_top);
        self.sync_window();
    }

    /// Gleicht das Fenster mit den Einstellungen ab (z. B. nach Änderung über das Menü).
    fn sync_window(&self) {
        let voice = self.current_voice().map_or(String::new(), |i| short_voice_name(&self.tts.voices()[i].name));
        window::set_voice(&voice);
        window::set_speed(&speed_label(self.cfg.rate));
        window::set_playback(self.state != State::Idle, self.state == State::Paused);
        self.refresh_menu_bar();
    }

    fn refresh_menu_bar(&self) {
        window::set_menu_bar(self.build_menu_bar());
    }

    fn select_voice(&mut self, index: usize) {
        if let Some(voice) = self.tts.voices().get(index) {
            let id = voice.id.clone();
            if self.tts.set_voice(&id) {
                self.cfg.voice = id;
                self.cfg.save();
            } else {
                tray::balloon(self.hwnd, "Vorleser", t("app.voice_cannot_used"));
            }
        }
        self.sync_window();
    }

    fn select_rate(&mut self, index: usize) {
        if let Some((rate, _)) = RATES.get(index) {
            self.tts.set_rate(*rate);
            self.cfg.rate = *rate;
            self.cfg.save();
        }
        self.sync_window();
        self.update_player();
    }

    /// Kurzer Beispielsatz in der Sprache der eingestellten Stimme.
    fn speak_sample(&mut self) {
        let german = self
            .current_voice()
            .is_none_or(|i| self.tts.voices()[i].lang_id & 0x3ff == 0x07);
        let sample = if german {
            "Guten Tag! So klingt diese Stimme beim Vorlesen eines kurzen Textes."
        } else {
            "Hello! This is how this voice sounds when reading a short text."
        };
        self.end_follow();
        self.start(sample.to_string(), None);
    }

    /// Liest Text von außerhalb (Markierung, Zwischenablage) vor oder zeigt `hint`, wenn es
    /// nichts vorzulesen gibt. Ist das Fenster offen, erscheint der Text dort zum Mitlesen.
    fn speak(&mut self, text: Option<String>, hint: &str) {
        let Some(text) = text else {
            tray::balloon(self.hwnd, "Vorleser", hint);
            return;
        };
        self.end_follow();
        let text = to_crlf(&self.prepare(text));
        // Der Text kommt immer ins Fenster, auch wenn es zu ist: So gehen Satzsprünge, und bei
        // geschlossenem Fenster begleitet ihn der Mini-Player.
        window::set_text(&text);
        let follow = Follow::new(0, text.encode_utf16().count() as u32, (0, 0));
        self.player_on = window::visible().is_none();
        self.start(text, Some(follow));
        self.update_player();
    }

    /// Liest den markierten Teil des Textfelds vor; ohne Markierung ab dem Satz, in dem die
    /// Schreibmarke steht, und steht sie am Anfang oder Ende, den ganzen Text.
    fn speak_window(&mut self) {
        self.speak_pane(window::focused_pane());
    }

    /// Wie [`App::speak_window`], für ein bestimmtes Textfeld des Arbeitsbereichs.
    fn speak_pane(&mut self, pane: window::Pane) {
        // Zuerst die eigene Wortmarkierung zurücknehmen, sonst gälte sie als Auswahl.
        self.end_follow();
        let Some(source) = window::source_of(pane) else { return };
        let (start, end) = source.selection;
        let len = source.content.len() as u32;
        if start == end && start > 0 && start < len {
            let starts = textprep::sentence_starts(&source.content);
            let from = textprep::sentence_at(&starts, start).unwrap_or(0);
            self.speak_range(pane, &source.content, from, len, (from, from));
            return;
        }
        let (text, follow) = if start < end {
            let part = String::from_utf16_lossy(&source.content[start as usize..end as usize]);
            (part, Follow::new(start, end, (start, end)))
        } else {
            // Die Bereinigung wird sichtbar ins Textfeld übernommen: gelesen wird, was dort steht.
            let shown = String::from_utf16_lossy(&source.content);
            let text = to_crlf(&self.prepare(shown.clone()));
            if text != shown {
                window::set_text_in(pane, &text);
            }
            let len = text.encode_utf16().count() as u32;
            (text, Follow::new(0, len, (0, 0)))
        };
        if text.trim().is_empty() {
            tray::balloon(self.hwnd, "Vorleser", t("app.text_field_empty"));
            return;
        }
        self.start(text, Some(follow.in_pane(pane)));
    }

    /// Liest einen Bereich des Textfelds vor (Weiterlesen, Navigation).
    fn speak_range(&mut self, pane: window::Pane, content: &[u16], start: u32, end: u32, restore: (u32, u32)) {
        let end = end.min(content.len() as u32);
        if start >= end {
            return;
        }
        let text = String::from_utf16_lossy(&content[start as usize..end as usize]);
        self.start(text, Some(Follow::new(start, end, restore).in_pane(pane)));
    }

    /// Springt einen Satz vor oder zurück. Beim Vorlesen geht es dort weiter, sonst wandert
    /// nur die Schreibmarke (F5 liest dann ab dort).
    fn navigate(&mut self, forward: bool) {
        let pane = self.follow.map_or_else(window::focused_pane, |f| f.pane);
        let Some(source) = window::source_of(pane) else { return };
        let starts = textprep::sentence_starts(&source.content);
        match self.follow {
            Some(follow) => match textprep::neighbor(&starts, follow.pos, forward) {
                Some(target) if target < follow.end => {
                    self.tts.stop();
                    self.speak_range(pane, &source.content, target, follow.end, follow.restore);
                }
                // Hinter dem letzten Satz: aufhören, wie am Ende des Textes.
                _ if forward => self.stop(),
                _ => {}
            },
            None => {
                if let Some(target) = textprep::neighbor(&starts, source.selection.0, forward) {
                    window::mark_in(pane, target, target);
                }
            }
        }
    }

    /// Merkt sich Text und Leseposition des Fensters, damit es später dort weitergeht.
    fn save_session(&mut self) {
        let Some(path) = config::session_path() else { return };
        if config::is_read_only() {
            return;
        }
        if !self.cfg.remember_text {
            let _ = std::fs::remove_file(path);
            return;
        }
        let Some(source) = window::source() else { return };
        let pos = self.follow.filter(|f| f.pane == window::Pane::Text).map_or(source.selection.0, |f| f.pos);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, String::from_utf16_lossy(&source.content)).is_ok() {
            self.cfg.resume_pos = pos;
            self.cfg.save();
        }
    }

    /// Die Anleitung mit den eingestellten Tastenkombinationen.
    fn guide(&self) -> String {
        let mut text = t("help.guide").to_string();
        for (name, value) in [
            ("{read}", &self.cfg.hotkey_read),
            ("{pause}", &self.cfg.hotkey_pause),
            ("{menu}", &self.cfg.hotkey_menu),
            ("{ocr}", &self.cfg.hotkey_ocr),
            ("{mouse}", &self.cfg.mouse_menu),
        ] {
            let shown = if value.trim().is_empty() { t("help.off").to_string() } else { i18n::hotkey(value) };
            text = text.replace(name, &shown);
        }
        to_crlf(&text)
    }

    /// Zeigt die Anleitung im Fenster; mit `read` wird sie gleich vorgelesen.
    fn show_guide(&mut self, read: bool) {
        self.stop();
        window::set_text(&self.guide());
        window::show();
        window::mark(0, 0);
        if read {
            self.speak_window();
        }
    }

    /// Holt Text und Leseposition vom letzten Mal zurück; beim allerersten Start erscheint
    /// stattdessen die Anleitung.
    fn restore_session(&mut self) {
        if !self.cfg.guide_shown && !config::is_read_only() {
            self.cfg.guide_shown = true;
            self.cfg.save();
            window::set_text(&self.guide());
            window::mark(0, 0);
            return;
        }
        if !self.cfg.remember_text || config::is_read_only() {
            return;
        }
        let Some(text) = config::session_path().and_then(|p| std::fs::read_to_string(p).ok()) else { return };
        if text.is_empty() {
            return;
        }
        window::set_text(&text);
        let pos = self.cfg.resume_pos.min(text.encode_utf16().count() as u32);
        window::mark(pos, pos);
    }

    fn start(&mut self, text: String, follow: Option<Follow>) {
        match self.tts.speak(&text) {
            Ok(()) => {
                self.retry = Some(text);
                self.player_sentences = follow
                    .and_then(|f| window::source_of(f.pane))
                    .map(|s| textprep::sentence_starts(&s.content))
                    .unwrap_or_default();
                self.follow = follow;
                self.update_progress();
                self.set_state(State::Speaking);
                // Nach einem Satzsprung zeigt der Player gleich den neuen Satz.
                self.update_player();
            }
            Err(_) => tray::balloon(self.hwnd, "Vorleser", t("app.speech_output_failed")),
        }
    }

    /// Markiert das gerade gesprochene Wort im Fenster.
    fn on_word(&mut self, start: u32, len: u32) {
        let mut sentence_changed = false;
        if let Some(follow) = &mut self.follow {
            let before = self.player_sentences.iter().filter(|s| **s <= follow.pos).count();
            follow.pos = follow.base + start;
            window::mark_in(follow.pane, follow.pos, follow.pos + len);
            sentence_changed = before != self.player_sentences.iter().filter(|s| **s <= follow.pos).count();
        }
        if sentence_changed {
            self.update_progress();
            if self.player_on {
                self.update_player();
            }
        }
    }

    /// Beendet das Mitlesen und stellt die ursprüngliche Markierung wieder her.
    fn end_follow(&mut self) {
        if let Some(follow) = self.follow.take() {
            window::mark_in(follow.pane, follow.restore.0, follow.restore.1);
        }
    }

    fn set_topmost(&mut self, on: bool) {
        self.cfg.always_on_top = on;
        self.cfg.save();
        window::set_topmost(on);
        self.refresh_menu_bar();
    }

    /// Die Ausgabe ist zu Ende; `result` ist ein Fehler, wenn die Stimme abgebrochen hat.
    fn on_finished(&mut self, result: windows::core::Result<()>) {
        let retry = self.retry.take();
        if result.is_err() {
            // Online-Stimmen scheitern gelegentlich an ihrem Dienst; ein zweiter Versuch hilft oft.
            if retry.is_some_and(|text| self.tts.speak(&text).is_ok()) {
                return;
            }
            let hint = t("app.voice_could_not_speak");
            tray::balloon(self.hwnd, "Vorleser", hint);
        }
        self.end_follow();
        self.set_state(State::Idle);
        self.save_session();
    }

    fn toggle_pause(&mut self) {
        match self.state {
            State::Speaking => {
                self.tts.pause();
                self.set_state(State::Paused);
            }
            State::Paused => {
                self.tts.resume();
                self.set_state(State::Speaking);
            }
            State::Idle => {}
        }
    }

    /// Hält an. Die Schreibmarke bleibt am Anfang des gerade gesprochenen Satzes, damit F5 dort
    /// weiterliest.
    fn stop(&mut self) {
        self.retry = None;
        self.tts.stop();
        if let Some(follow) = self.follow.take() {
            let starts = window::source_of(follow.pane).map(|s| textprep::sentence_starts(&s.content)).unwrap_or_default();
            let at = textprep::sentence_at(&starts, follow.pos).unwrap_or(follow.pos);
            window::mark_in(follow.pane, at, at);
        }
        self.set_state(State::Idle);
        self.save_session();
    }

    /// `selection`: `None` lässt den Eintrag für markierten Text weg (Menü am Symbol),
    /// sonst gibt der Wert an, ob eine Markierung gefunden wurde.
    fn build_menu(&self, selection: Option<bool>) -> HMENU {
        unsafe {
            let menu = CreatePopupMenu().unwrap_or_default();
            let active = self.state != State::Idle;
            if let Some(version) = update::newer_version() {
                let label = t("app.download_new_version").replace("{}", &version);
                item(menu, CMD_UPDATE, &label, true, false);
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            item(menu, CMD_OPEN, t("app.open_vorleser"), true, false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            if let Some(found) = selection {
                item(menu, CMD_READ_SELECTION, t("app.read_selected_text"), found, false);
            }
            item(menu, CMD_READ_CLIPBOARD, t("app.read_clipboard"), true, false);
            let ocr = format!("{}\t{}", t("app.read_screen"), i18n::hotkey(&self.cfg.hotkey_ocr));
            item(menu, CMD_SCREEN_OCR, &ocr, true, false);
            let pause = if self.state == State::Paused { t("app.resume") } else { t("app.pause") };
            item(menu, CMD_PAUSE, &format!("{pause}\t{}", i18n::hotkey(&self.cfg.hotkey_pause)), active, false);
            item(menu, CMD_STOP, t("app.stop"), active, false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            let voices = self.build_voice_menu(0);
            let _ = AppendMenuW(menu, MF_POPUP, voices.0 as usize, &HSTRING::from(t("app.voice")));
            let rates = self.build_rate_menu(0);
            let _ = AppendMenuW(menu, MF_POPUP, rates.0 as usize, &HSTRING::from(t("app.speed")));

            item(menu, CMD_AUTOSTART, t("app.start_windows"), true, autostart_enabled());
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            item(menu, CMD_AI_VOICES, t("app.ai_voices"), true, false);
            item(menu, CMD_SETTINGS, t("app.settings"), true, false);
            item(menu, CMD_ABOUT, t("app.about_vorleser"), true, false);
            item(menu, CMD_QUIT, t("app.exit"), true, false);
            menu
        }
    }

    /// Geschwindigkeiten als Auswahlliste; `base` wird zu jeder Befehlsnummer addiert.
    fn build_rate_menu(&self, base: u32) -> HMENU {
        unsafe {
            let rates = CreatePopupMenu().unwrap_or_default();
            for (i, (rate, key)) in RATES.iter().enumerate() {
                let id = base + CMD_RATE + i as u32;
                item(rates, id, t(key), true, false);
                if *rate == self.cfg.rate {
                    let last = base + CMD_RATE + RATES.len() as u32;
                    let _ = CheckMenuRadioItem(rates, base + CMD_RATE, last, id, MF_BYCOMMAND.0);
                }
            }
            rates
        }
    }

    /// Die Menüleiste des Fensters. Ihre Befehle tragen [`window::MENU_BASE`] als Aufschlag.
    fn build_menu_bar(&self) -> HMENU {
        let base = window::MENU_BASE;
        let active = self.state != State::Idle;
        unsafe {
            let bar = CreateMenu().unwrap_or_default();
            let popup = |title: &str, menu: HMENU| {
                let _ = AppendMenuW(bar, MF_POPUP, menu.0 as usize, &HSTRING::from(title));
            };
            let separator = |menu: HMENU| {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            };

            let file = CreatePopupMenu().unwrap_or_default();
            item(file, base + CMD_OPEN_FILE, t("menu.open"), true, false);
            let saving = PENDING_WAV.with(|cell| cell.borrow().is_some());
            item(file, base + CMD_SAVE_WAV, t("menu.save_wav"), !saving, false);
            separator(file);
            item(file, base + CMD_CLOSE_WINDOW, t("menu.close_window"), true, false);
            item(file, base + CMD_QUIT, t("menu.exit"), true, false);
            popup(t("menu.file"), file);

            let edit = CreatePopupMenu().unwrap_or_default();
            item(edit, base + CMD_UNDO, t("menu.undo"), true, false);
            separator(edit);
            item(edit, base + CMD_CUT, t("menu.cut"), true, false);
            item(edit, base + CMD_COPY, t("menu.copy"), true, false);
            item(edit, base + CMD_PASTE, t("menu.paste"), true, false);
            item(edit, base + CMD_SELECT_ALL, t("menu.select_all"), true, false);
            separator(edit);
            item(edit, base + CMD_CLEAN, t("menu.clean"), true, false);
            item(edit, base + CMD_CLEAR, t("menu.clear"), true, false);
            popup(t("menu.edit"), edit);

            let playback = CreatePopupMenu().unwrap_or_default();
            item(playback, base + CMD_READ_WINDOW, t("menu.read"), true, false);
            let pause = if self.state == State::Paused { t("menu.resume") } else { t("menu.pause") };
            item(playback, base + CMD_PAUSE, pause, active, false);
            item(playback, base + CMD_STOP, t("menu.stop"), active, false);
            separator(playback);
            item(playback, base + CMD_PREV, t("menu.prev_sentence"), true, false);
            item(playback, base + CMD_NEXT, t("menu.next_sentence"), true, false);
            separator(playback);
            item(playback, base + CMD_READ_CLIPBOARD, t("menu.read_clipboard"), true, false);
            let ocr = format!("{}\t{}", t("menu.read_screen"), i18n::hotkey(&self.cfg.hotkey_ocr));
            item(playback, base + CMD_SCREEN_OCR, &ocr, true, false);
            separator(playback);
            let rates = self.build_rate_menu(base);
            let _ = AppendMenuW(playback, MF_POPUP, rates.0 as usize, &HSTRING::from(t("menu.speed")));
            popup(t("menu.playback"), playback);

            let voice = self.build_voice_menu(base);
            separator(voice);
            item(voice, base + CMD_SAMPLE, t("menu.sample"), true, false);
            item(voice, base + CMD_AI_VOICES, t("menu.ai_voices"), true, false);
            popup(t("menu.voice"), voice);

            if self.cfg.assistant {
                let assistant = CreatePopupMenu().unwrap_or_default();
                let busy = AI_BUSY.load(Ordering::Relaxed);
                // Ohne eingerichtete KI bleiben deren Befehle grau; Diktieren geht immer.
                let ready = self.provider().is_ready();
                item(assistant, base + CMD_SUMMARIZE, t("menu.summarize"), ready && !busy, false);
                let modes = CreatePopupMenu().unwrap_or_default();
                let current = ai::Summary::from_code(&self.cfg.summary_mode);
                for (i, mode) in ai::Summary::ALL.iter().enumerate() {
                    let id = base + CMD_SUMMARY_MODE + i as u32;
                    item(modes, id, summary_label(*mode), ready, false);
                    if *mode == current {
                        let _ = CheckMenuRadioItem(modes, id, id, id, MF_BYCOMMAND.0);
                    }
                }
                let _ = AppendMenuW(assistant, MF_POPUP, modes.0 as usize, &HSTRING::from(t("menu.summary_mode")));
                item(assistant, base + CMD_READ_RESULT, t("menu.read_result"), ready, false);
                let transcribing = TRANSCRIBING.load(Ordering::Relaxed);
                item(assistant, base + CMD_TRANSCRIBE, if transcribing { t("menu.transcribe_cancel") } else { t("menu.transcribe") }, true, false);
                separator(assistant);
                let dictating = self.dictation.is_some();
                item(assistant, base + CMD_DICTATE, if dictating { t("menu.dictate_stop") } else { t("menu.dictate") }, true, dictating);
                item(assistant, base + CMD_CORRECT, t("menu.correct"), ready && !busy, false);
                let corrections = CreatePopupMenu().unwrap_or_default();
                let current = ai::Correction::from_code(&self.cfg.correction_mode);
                for (i, mode) in ai::Correction::ALL.iter().enumerate() {
                    let id = base + CMD_CORRECTION_MODE + i as u32;
                    item(corrections, id, correction_label(*mode), ready, false);
                    if *mode == current {
                        let _ = CheckMenuRadioItem(corrections, id, id, id, MF_BYCOMMAND.0);
                    }
                }
                let _ = AppendMenuW(assistant, MF_POPUP, corrections.0 as usize, &HSTRING::from(t("menu.correction_mode")));
                let microphones = CreatePopupMenu().unwrap_or_default();
                item(microphones, base + CMD_MICROPHONE, t("menu.microphone_default"), true, false);
                let names = dictation::microphones();
                let current = names.iter().position(|n| *n == self.cfg.microphone).map_or(0, |i| i as u32 + 1);
                for (i, name) in names.iter().enumerate() {
                    item(microphones, base + CMD_MICROPHONE + 1 + i as u32, name, true, false);
                }
                let id = base + CMD_MICROPHONE + current;
                let _ = CheckMenuRadioItem(microphones, id, id, id, MF_BYCOMMAND.0);
                let _ = AppendMenuW(assistant, MF_POPUP, microphones.0 as usize, &HSTRING::from(t("menu.microphone")));
                item(assistant, base + CMD_DICT_SETUP, t("menu.dict_setup"), true, false);
                separator(assistant);
                item(assistant, base + CMD_AI_SETUP, t("menu.ai_setup"), true, false);
                item(assistant, base + CMD_AI_SETTINGS, t("menu.ai_settings"), true, false);
                popup(t("menu.assistant"), assistant);
            }

            let tools = CreatePopupMenu().unwrap_or_default();
            item(tools, base + CMD_TOPMOST, t("menu.topmost"), true, self.cfg.always_on_top);
            item(tools, base + CMD_PILL, t("menu.pill"), true, self.cfg.close_to_pill);
            item(tools, base + CMD_AUTOSTART, t("menu.autostart"), true, autostart_enabled());
            item(tools, base + CMD_ASSISTANT, t("menu.assistant_toggle"), true, self.cfg.assistant);
            separator(tools);
            let languages = CreatePopupMenu().unwrap_or_default();
            item(languages, base + CMD_LANGUAGE, t("menu.language_auto"), true, false);
            separator(languages);
            let available = i18n::languages();
            for (i, (_, name)) in available.iter().enumerate() {
                item(languages, base + CMD_LANGUAGE + 1 + i as u32, name, true, false);
            }
            let chosen = if self.cfg.language == i18n::AUTO {
                Some(0)
            } else {
                available.iter().position(|(code, _)| *code == self.cfg.language).map(|i| i + 1)
            };
            if let Some(index) = chosen {
                let id = base + CMD_LANGUAGE + index as u32;
                let _ = CheckMenuRadioItem(languages, id, id, id, MF_BYCOMMAND.0);
            }
            // "Sprache / Language" bleibt zweisprachig, damit man auch aus einer fremden Sprache zurückfindet.
            let own = t("menu.language");
            let title = if own.replace('&', "") == "Language" { own.to_string() } else { format!("{own} / Language") };
            let _ = AppendMenuW(tools, MF_POPUP, languages.0 as usize, &HSTRING::from(title));
            item(tools, base + CMD_SETTINGS, t("menu.settings"), true, false);
            popup(t("menu.tools"), tools);

            let help = CreatePopupMenu().unwrap_or_default();
            item(help, base + CMD_GUIDE_SHOW, t("menu.guide_show"), true, false);
            item(help, base + CMD_GUIDE_READ, t("menu.guide_read"), true, false);
            separator(help);
            item(help, base + CMD_MANUAL, t("menu.manual"), true, false);
            item(help, base + CMD_CHECK_UPDATE, t("menu.check_update"), true, false);
            item(help, base + CMD_REPORT, t("menu.report"), true, false);
            separator(help);
            item(help, base + CMD_ABOUT, t("menu.about"), true, false);
            popup(t("menu.help"), help);
            bar
        }
    }

    /// Ein Untermenü je Sprache; bei nur einer Sprache eine einfache Liste.
    /// `base` wird zu jeder Befehlsnummer addiert.
    fn build_voice_menu(&self, base: u32) -> HMENU {
        let voices = self.tts.voices();
        let current = self.current_voice();
        let groups = self.voice_groups();
        unsafe {
            let menu = CreatePopupMenu().unwrap_or_default();
            let flat = groups.len() == 1;
            for (language, indices) in &groups {
                let target = if flat { menu } else { CreatePopupMenu().unwrap_or_default() };
                let mut holds_current = false;
                for &i in indices {
                    let voice = &voices[i];
                    let id = base + CMD_VOICE + i as u32;
                    // Der Zusatz " - German (Germany)" wiederholt nur den Namen der Gruppe.
                    let name = voice.name.split(" - ").next().unwrap_or(&voice.name);
                    item(target, id, if flat { &voice.name } else { name }, true, false);
                    if current == Some(i) {
                        let _ = CheckMenuRadioItem(target, id, id, id, MF_BYCOMMAND.0);
                        holds_current = true;
                    }
                }
                if !flat {
                    let flags = if holds_current { MF_POPUP | MF_CHECKED } else { MF_POPUP };
                    let _ = AppendMenuW(menu, flags, target.0 as usize, &HSTRING::from(*language));
                }
            }
            menu
        }
    }

    fn command(&mut self, cmd: u32, selection: Option<String>) {
        match cmd {
            CMD_READ_SELECTION => self.speak(selection, t("app.no_text_selected")),
            CMD_READ_CLIPBOARD => {
                self.speak(selection::clipboard_text(), t("app.clipboard_contains_no_text"))
            }
            CMD_PAUSE => self.toggle_pause(),
            CMD_STOP => self.stop(),
            CMD_AUTOSTART => {
                set_autostart(!autostart_enabled());
                self.refresh_menu_bar();
            }
            CMD_OPEN => window::show(),
            CMD_UPDATE => open_url(update::RELEASES_URL),
            CMD_READ_WINDOW => self.speak_window(),
            CMD_READ_RESULT => self.speak_pane(window::Pane::Result),
            _ if (CMD_MICROPHONE..CMD_MICROPHONE + 64).contains(&cmd) => {
                let index = (cmd - CMD_MICROPHONE) as usize;
                self.cfg.microphone = if index == 0 { String::new() } else { dictation::microphones().get(index - 1).cloned().unwrap_or_default() };
                self.cfg.save();
                self.refresh_menu_bar();
            }
            _ if (CMD_CORRECTION_MODE..CMD_CORRECTION_MODE + 4).contains(&cmd) => {
                if let Some(mode) = ai::Correction::ALL.get((cmd - CMD_CORRECTION_MODE) as usize) {
                    self.set_correction_mode(*mode);
                }
            }
            _ if (CMD_SUMMARY_MODE..CMD_SUMMARY_MODE + 3).contains(&cmd) => {
                if let Some(mode) = ai::Summary::ALL.get((cmd - CMD_SUMMARY_MODE) as usize) {
                    self.set_summary_mode(*mode);
                }
            }
            CMD_GUIDE_SHOW => self.show_guide(false),
            CMD_GUIDE_READ => self.show_guide(true),
            CMD_PREV => self.navigate(false),
            CMD_NEXT => self.navigate(true),
            CMD_SAMPLE => self.speak_sample(),
            CMD_TOPMOST => self.set_topmost(!self.cfg.always_on_top),
            CMD_CLEAN => {
                if let Some(text) = window::source().map(|s| String::from_utf16_lossy(&s.content)) {
                    self.stop();
                    window::set_text(&to_crlf(&textprep::clean(&text)));
                }
            }
            _ if cmd >= CMD_LANGUAGE => {
                let index = (cmd - CMD_LANGUAGE) as usize;
                let code = if index == 0 { Some(i18n::AUTO) } else { i18n::languages().get(index - 1).map(|(code, _)| *code) };
                if let Some(code) = code {
                    self.set_language(code);
                }
            }
            _ if cmd >= CMD_VOICE => self.select_voice((cmd - CMD_VOICE) as usize),
            _ if cmd >= CMD_RATE => self.select_rate((cmd - CMD_RATE) as usize),
            _ => {}
        }
    }
}

unsafe fn item(menu: HMENU, id: u32, text: &str, enabled: bool, checked: bool) {
    let mut flags = MF_STRING;
    if !enabled {
        flags |= MF_GRAYED;
    }
    if checked {
        flags |= MF_CHECKED;
    }
    let _ = AppendMenuW(menu, flags as MENU_ITEM_FLAGS, id as usize, &HSTRING::from(text));
}

fn autostart_enabled() -> bool {
    unsafe {
        RegGetValueW(HKEY_CURRENT_USER, RUN_KEY, APP_NAME, RRF_RT_REG_SZ, None, None, None)
            == ERROR_SUCCESS
    }
}

/// Der eingetragene Befehl des Autostarts, falls vorhanden.
fn autostart_command() -> Option<String> {
    let mut buffer = [0u16; 1024];
    let mut size = (buffer.len() * 2) as u32;
    unsafe {
        let status = RegGetValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            APP_NAME,
            RRF_RT_REG_SZ,
            None,
            Some(buffer.as_mut_ptr() as *mut _),
            Some(&mut size),
        );
        if status != ERROR_SUCCESS {
            return None;
        }
    }
    let len = buffer.iter().position(|&unit| unit == 0).unwrap_or(buffer.len());
    Some(String::from_utf16_lossy(&buffer[..len]))
}

fn set_autostart(enable: bool) {
    unsafe {
        if !enable {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, RUN_KEY, APP_NAME);
            return;
        }
        let Ok(exe) = std::env::current_exe() else { return };
        let value = HSTRING::from(format!("\"{}\" {ARG_TRAY}", exe.display()));
        let bytes = (value.len() as u32 + 1) * 2;
        let _ = RegSetKeyValueW(
            HKEY_CURRENT_USER,
            RUN_KEY,
            APP_NAME,
            REG_SZ.0,
            Some(value.as_ptr() as *const _),
            bytes,
        );
    }
}

/// Zeigt das Menü an der Mausposition. `selection` wie bei [`App::build_menu`], aber mit dem Text.
fn show_menu(hwnd: HWND, selection: Option<Option<String>>) {
    if MENU_OPEN.swap(true, Ordering::Relaxed) {
        return;
    }
    let found = selection.as_ref().map(|s| s.is_some());
    let Some(menu) = with_app(|app| app.build_menu(found)) else {
        MENU_OPEN.store(false, Ordering::Relaxed);
        return;
    };
    // Das Menü läuft in einer eigenen Nachrichtenschleife; der App-Zustand ist dabei nicht ausgeliehen.
    let cmd = tray::popup(hwnd, menu);
    MENU_OPEN.store(false, Ordering::Relaxed);
    unsafe {
        let _ = DestroyMenu(menu);
    }
    if cmd != 0 {
        run(cmd, selection.flatten());
    }
}

/// Führt einen Befehl aus Tray-Menü, Menüleiste oder Tastenkürzel aus. Befehle, die eigene
/// Fenster oder Dialoge öffnen, laufen außerhalb von `with_app`.
fn run(cmd: u32, selection: Option<String>) {
    match cmd {
        CMD_QUIT => {
            if let Some(hwnd) = with_app(|app| app.hwnd) {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
            }
        }
        CMD_ABOUT => show_about(),
        CMD_AI_VOICES => open_ai_voices(),
        CMD_SETTINGS => open_settings(),
        CMD_OPEN_FILE => open_file(),
        CMD_SAVE_WAV => {
            if PENDING_WAV.with(|cell| cell.borrow().is_none()) {
                save_audio();
            }
        }
        CMD_CLOSE_WINDOW => window::hide(),
        CMD_UNDO => window::edit(window::Edit::Undo),
        CMD_CUT => window::edit(window::Edit::Cut),
        CMD_COPY => window::edit(window::Edit::Copy),
        CMD_PASTE => window::edit(window::Edit::Paste),
        CMD_SELECT_ALL => window::edit(window::Edit::SelectAll),
        CMD_CLEAR => {
            with_app(App::stop);
            window::edit(window::Edit::Clear);
        }
        CMD_MANUAL => open_url(if i18n::current() == "de" { MANUAL_URL } else { MANUAL_URL_EN }),
        CMD_REPORT => open_url(REPORT_URL),
        CMD_CHECK_UPDATE => check_update_now(),
        CMD_SCREEN_OCR => screen_ocr(),
        CMD_AI_SETTINGS => open_ai_settings(),
        CMD_AI_SETUP => open_ai_setup(),
        CMD_TRANSCRIBE => {
            if TRANSCRIBING.load(Ordering::Relaxed) {
                TRANSCRIBE_CANCEL.store(true, Ordering::Relaxed);
            } else if let Some(path) = choose_recording() {
                transcribe(path);
            }
        }
        CMD_PILL => {
            with_app(|app| {
                app.cfg.close_to_pill = !app.cfg.close_to_pill;
                app.cfg.save();
                app.update_pill();
                app.refresh_menu_bar();
            });
        }
        CMD_ASSISTANT => {
            if with_app(|app| app.set_assistant(!app.cfg.assistant)) == Some(true) {
                open_ai_setup();
            }
        }
        CMD_SUMMARIZE => {
            if with_app(App::summarize) == Some(false) {
                open_ai_setup();
            }
        }
        CMD_CORRECT => {
            if with_app(App::correct) == Some(false) {
                open_ai_setup();
            }
        }
        CMD_DICTATE => {
            if with_app(App::toggle_dictation) == Some(false) {
                open_dict_setup();
            }
        }
        CMD_DICT_SETUP => open_dict_setup(),
        _ => {
            with_app(|app| app.command(cmd, selection));
        }
    }
}

/// Fragt GitHub sofort nach einer neueren Version; das Ergebnis kommt als `WM_UPDATE_MANUAL`.
fn check_update_now() {
    let Some(hwnd) = with_app(|app| app.hwnd.0 as isize) else { return };
    std::thread::spawn(move || {
        let code = match update::check_now() {
            None => 0,
            Some(None) => 1,
            Some(Some(_)) => 2,
        };
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_UPDATE_MANUAL, WPARAM(code), LPARAM(0));
        }
    });
}

fn on_manual_update(code: usize) {
    let owner = window::visible();
    unsafe {
        match (code, update::newer_version()) {
            (2, Some(version)) => {
                let text = HSTRING::from(t("app.update_available").replace("{}", &version));
                if MessageBoxW(owner, &text, APP_NAME, MB_YESNO | MB_ICONQUESTION) == IDYES {
                    open_url(update::RELEASES_URL);
                }
                with_app(|app| app.refresh_menu_bar());
            }
            (1, _) => {
                let text = HSTRING::from(t("app.up_to_date").replace("{}", env!("CARGO_PKG_VERSION")));
                MessageBoxW(owner, &text, APP_NAME, MB_ICONINFORMATION);
            }
            _ => {
                MessageBoxW(owner, &HSTRING::from(t("app.update_check_failed")), APP_NAME, MB_ICONWARNING);
            }
        }
    }
}

/// Startet die Auswahl eines Bildschirmausschnitts; der Text darin wird erkannt und vorgelesen.
fn screen_ocr() {
    if !snip::is_open() {
        snip::start(on_snip);
    }
}

fn on_snip(selection: Option<snip::Selection>) {
    let (Some((bgra, width, height)), Some(hwnd)) = (selection, with_app(|app| app.hwnd.0 as isize)) else { return };
    std::thread::spawn(move || {
        ocr::init_thread();
        *RECOGNIZED.lock().unwrap() = Some(ocr::pixels(&bgra, width, height));
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_OCR_DONE, WPARAM(0), LPARAM(0));
        }
    });
}

fn on_ocr_done() {
    let result = RECOGNIZED.lock().unwrap().take();
    with_app(|app| match result {
        Some(Ok(text)) if !text.trim().is_empty() => app.speak(Some(text.trim().to_string()), ""),
        Some(Err(error)) => tray::balloon(app.hwnd, "Vorleser", &error),
        _ => tray::balloon(app.hwnd, "Vorleser", t("app.no_text_recognized")),
    });
}

/// Lädt eine Datei ins Textfeld (über Datei → Öffnen oder durch Hineinziehen). Gelesen wird
/// im Hintergrund; PDFs und Bilder laufen über die Texterkennung und können dauern.
fn load_file(path: String) {
    // Aufnahmen werden abgeschrieben (Officeassistent).
    if audio::is_audio(&path) {
        transcribe(path);
        return;
    }
    let owner = window::visible();
    let message = if !documents::is_supported(&path) && !path.to_lowercase().ends_with(".doc") {
        Some(t("app.only_text_files"))
    } else if LOADING.swap(true, Ordering::Relaxed) {
        Some(t("app.still_loading"))
    } else {
        None
    };
    if let Some(message) = message {
        unsafe {
            MessageBoxW(owner, &HSTRING::from(message), APP_NAME, MB_ICONINFORMATION);
        }
        return;
    }
    let Some(hwnd) = with_app(|app| app.hwnd.0 as isize) else { return };
    window::set_title(Some(t("app.loading_title")));
    std::thread::spawn(move || {
        ocr::init_thread();
        let post = |msg: u32, wparam: usize, lparam: isize| unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(lparam));
        };
        let result = documents::extract(&path, |page, pages| {
            post(WM_FILE_PROGRESS, page as usize, pages as isize);
            true
        });
        *LOADED_FILE.lock().unwrap() = Some(result);
        post(WM_FILE_LOADED, 0, 0);
    });
}

fn on_file_loaded() {
    LOADING.store(false, Ordering::Relaxed);
    window::set_title(None);
    match LOADED_FILE.lock().unwrap().take() {
        Some(Ok(text)) => {
            with_app(App::stop);
            window::set_text(&to_crlf(&text));
            window::show();
            window::mark(0, 0);
        }
        Some(Err(error)) => unsafe {
            MessageBoxW(window::visible(), &HSTRING::from(error), APP_NAME, MB_ICONINFORMATION);
        },
        None => {}
    }
}

fn open_file() {
    let mut file = [0u16; 1024];
    // Dateitypen für den Dialog, je Eintrag Name und Muster, getrennt durch Nullzeichen.
    let all: Vec<String> = documents::EXTENSIONS.iter().map(|e| format!("*.{e}")).collect();
    let all = all.join(";");
    let types = [
        (t("app.all_supported"), all.as_str()),
        ("Word (*.docx)", "*.docx"),
        ("OpenDocument (*.odt)", "*.odt"),
        ("PDF (*.pdf)", "*.pdf"),
        ("RTF (*.rtf)", "*.rtf"),
        ("E-Book (*.epub)", "*.epub"),
        ("HTML (*.html, *.htm)", "*.html;*.htm;*.xhtml"),
        (t("app.images"), "*.png;*.jpg;*.jpeg;*.bmp;*.tif;*.tiff;*.gif"),
        (t("app.text_files"), "*.txt;*.md;*.csv"),
        (t("app.recordings"), "*.mp3;*.m4a;*.aac;*.wav;*.wma;*.flac;*.ogg;*.opus;*.mp4;*.m4v;*.webm"),
    ];
    let mut filter = String::new();
    for (name, pattern) in types {
        filter.push_str(&format!("{name}\0{pattern}\0"));
    }
    filter.push('\0');
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: window::visible().unwrap_or_default(),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if unsafe { GetOpenFileNameW(&mut dialog) }.as_bool() {
        let len = file.iter().position(|&unit| unit == 0).unwrap_or(file.len());
        load_file(String::from_utf16_lossy(&file[..len]));
    }
}

/// Dateiauswahl für eine Aufnahme.
fn choose_recording() -> Option<String> {
    let mut file = [0u16; 1024];
    let all: Vec<String> = audio::EXTENSIONS.iter().map(|e| format!("*.{e}")).collect();
    let filter = format!("{}\0{}\0\0", t("app.recordings"), all.join(";"));
    let filter: Vec<u16> = filter.encode_utf16().collect();
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: window::visible().unwrap_or_default(),
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        Flags: OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    unsafe { GetOpenFileNameW(&mut dialog) }.as_bool().then(|| {
        let len = file.iter().position(|&unit| unit == 0).unwrap_or(file.len());
        String::from_utf16_lossy(&file[..len])
    })
}

/// Schreibt eine Aufnahme im Hintergrund ab; danach entsteht mit KI das Protokoll.
fn transcribe(path: String) {
    let owner = window::visible();
    let message = |text: &str| unsafe {
        MessageBoxW(owner, &HSTRING::from(text), APP_NAME, MB_ICONINFORMATION);
    };
    if with_app(|app| app.cfg.assistant) != Some(true) {
        message(t("audio.needs_assistant"));
        return;
    }
    let Some(model) = dictation::best_installed() else {
        open_dict_setup();
        return;
    };
    if TRANSCRIBING.swap(true, Ordering::Relaxed) {
        message(t("audio.busy"));
        return;
    }
    TRANSCRIBE_CANCEL.store(false, Ordering::Relaxed);
    let Some(hwnd) = with_app(|app| app.hwnd.0 as isize) else { return };
    with_app(|app| app.refresh_menu_bar());
    window::set_title(Some(&t("audio.reading").replace("{}", "0")));
    let language = i18n::current().to_string();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let post = |msg: u32, wparam: usize| unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), msg, WPARAM(wparam), LPARAM(0));
        };
        let cancelled = || TRANSCRIBE_CANCEL.load(Ordering::Relaxed);
        let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        // Entpacken zählt bis 5 %, das Erkennen den Rest.
        let result = audio::decode(&path, |done, total| {
            if total > 0.0 {
                post(WM_TRANSCRIBE_PROGRESS, (done / total * 5.0) as usize);
            }
            !cancelled()
        })
        .and_then(|samples| {
            let duration = samples.len() as f64 / audio::RATE as f64;
            let lines = dictation::transcribe_recording(&samples, model, &language, |done, total| {
                post(WM_TRANSCRIBE_PROGRESS, 5 + (done / total.max(1.0) * 95.0) as usize);
                !cancelled()
            })?;
            Ok((name, duration, lines))
        });
        *TRANSCRIPT.lock().unwrap() = Some(result);
        post(WM_TRANSCRIBE_DONE, 0);
    });
}

fn on_transcribe_done() {
    TRANSCRIBING.store(false, Ordering::Relaxed);
    window::set_title(None);
    with_app(|app| app.refresh_menu_bar());
    match TRANSCRIPT.lock().unwrap().take() {
        Some(Ok((name, duration, lines))) => {
            if lines.is_empty() {
                unsafe {
                    MessageBoxW(window::visible(), &HSTRING::from(t("audio.no_speech")), APP_NAME, MB_ICONINFORMATION);
                }
                return;
            }
            let header = t("audio.header").replacen("{}", &name, 1).replacen("{}", &audio::timestamp(duration), 1);
            let mut text = format!("{header}\r\n\r\n");
            for (start, line) in &lines {
                text.push_str(&format!("[{}] {line}\r\n", audio::timestamp(*start)));
            }
            with_app(App::stop);
            window::set_text(&text);
            window::show();
            window::mark(0, 0);
            // Mit KI gleich das Protokoll dazu.
            with_app(|app| {
                if app.provider().is_ready() {
                    app.set_summary_mode(ai::Summary::Minutes);
                    app.summarize();
                }
            });
        }
        Some(Err(error)) if error != t("err.cancelled") => unsafe {
            MessageBoxW(window::visible(), &HSTRING::from(error), APP_NAME, MB_ICONWARNING);
        },
        _ => {}
    }
}

fn open_ai_voices() {
    aivoices::open(window::visible(), on_ai_voices_changed);
}

/// Nach Download (`Some` mit der neuen Stimme) oder Entfernen einer KI-Stimme.
fn on_ai_voices_changed(new_voice: Option<String>) {
    with_app(|app| {
        let before = app.tts.current_voice_id();
        app.tts.reload_voices();
        app.fill_window();
        if let Some(index) = new_voice.and_then(|id| app.tts.voices().iter().position(|v| v.id == id)) {
            app.select_voice(index);
        } else if app.tts.current_voice_id() != before {
            // Die eingestellte Stimme wurde entfernt; es gilt wieder die Windows-Stimme.
            app.stop();
            if let Some(id) = app.tts.current_voice_id() {
                app.cfg.voice = id;
                app.cfg.save();
            }
            app.sync_window();
        }
    });
}

fn summary_label(mode: ai::Summary) -> &'static str {
    match mode {
        ai::Summary::Short => t("summary.short"),
        ai::Summary::Bullets => t("summary.bullets"),
        ai::Summary::Detailed => t("summary.detailed"),
        ai::Summary::Minutes => t("summary.minutes"),
    }
}

fn provider_name(kind: ai::Kind) -> &'static str {
    match kind {
        ai::Kind::None => t("aiset.kind_none"),
        ai::Kind::Ollama => "Ollama",
        ai::Kind::OpenAi => t("assist.cloud"),
    }
}

fn open_ai_settings() {
    let Some((provider, custom)) = with_app(|app| (app.provider(), app.cfg.correction_custom.clone())) else { return };
    let report = aicheck::detect().report();
    aisettings::open(window::visible(), provider, custom, report, |provider, custom| {
        with_app(|app| app.apply_ai_settings(provider, custom));
    });
}

/// Der Einrichtungsassistent: eine Frage, ein Knopf. Die ausführlichen KI-Einstellungen sind
/// von dort aus erreichbar.
fn open_ai_setup() {
    let Some(provider) = with_app(|app| app.provider()) else { return };
    aisetup::open(window::visible(), &provider, |provider| {
        with_app(|app| {
            let custom = app.cfg.correction_custom.clone();
            app.apply_ai_settings(provider, custom);
        });
    }, open_ai_settings);
}

/// Klick in der Diktier-Pille.
fn on_dictbar_action(action: dictbar::Action) {
    let needs_setup = with_app(|app| match action {
        dictbar::Action::Toggle => !app.toggle_dictation_to(app.dict_target),
        dictbar::Action::Correct => {
            app.correct_dictation();
            false
        }
        dictbar::Action::Close => {
            if app.dictation.is_some() {
                app.toggle_dictation_to(app.dict_target);
            }
            app.dict_waiting.clear();
            dictbar::hide();
            app.update_pill();
            false
        }
        dictbar::Action::Poll => {
            if app.flush_dictation() {
                let state = if app.dictation.is_some() { dictbar::State::Listening } else { dictbar::State::Finished };
                dictbar::set_state(state);
            }
            false
        }
    });
    if needs_setup == Some(true) {
        open_dict_setup();
    }
}

/// Strg+Alt+D: diktiert in das Programm im Vordergrund (oder ins Vorleser-Fenster).
fn dictate_anywhere() {
    let foreground = typing::foreground();
    let target = if window::visible() == Some(foreground) { DictTarget::Window } else { DictTarget::Program(foreground.0 as isize) };
    let needs_setup = with_app(|app| {
        let target = if app.dictation.is_some() { app.dict_target } else { target };
        !app.toggle_dictation_to(target)
    });
    if needs_setup == Some(true) {
        open_dict_setup();
    }
}

fn open_dict_setup() {
    dictsetup::open(window::visible(), |_| {
        with_app(|app| app.show_assistant());
    });
}

/// Kurzer Name für die Anzeige unten im Fenster: „Microsoft Hedda Desktop - German“ → „Hedda“,
/// „Thorsten HD (KI-Stimme)“ → „Thorsten HD“.
fn short_voice_name(name: &str) -> String {
    let name = name.split(" - ").next().unwrap_or(name);
    let name = name.split(" (").next().unwrap_or(name);
    let name = name.strip_prefix("Microsoft ").unwrap_or(name);
    name.strip_suffix(" Desktop").unwrap_or(name).trim().to_string()
}

/// Geschwindigkeit als Faktor, z. B. „1,2×“. Bei SAPI verdreifacht +10 das Tempo ungefähr.
fn speed_label(rate: i32) -> String {
    format!("{:.1}×", 3f32.powf(rate as f32 / 10.0)).replace('.', t("number.decimal"))
}

/// Stimme oder Geschwindigkeit unten im Fenster: Menü über der Schaltfläche.
fn choose_from_bar(id: u32) {
    use windows::Win32::UI::WindowsAndMessaging::{TrackPopupMenu, TPM_BOTTOMALIGN, TPM_LEFTALIGN, TPM_NONOTIFY, TPM_RETURNCMD};
    let Some(anchor) = window::anchor(id) else { return };
    let Some(menu) = with_app(|app| unsafe {
        if id == window::ID_VOICE {
            app.build_voice_menu(0)
        } else {
            let menu = CreatePopupMenu().unwrap_or_default();
            for (i, (rate, key)) in RATES.iter().enumerate() {
                let label = format!("{}\t{}", speed_label(*rate), t(key));
                let _ = AppendMenuW(menu, MF_STRING, (CMD_RATE + i as u32) as usize, &HSTRING::from(label));
            }
            let id = CMD_RATE + app.rate_index() as u32;
            let _ = CheckMenuRadioItem(menu, id, id, id, MF_BYCOMMAND.0);
            menu
        }
    }) else {
        return;
    };
    let Some(owner) = window::visible() else { return };
    let cmd = unsafe {
        let flags = TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN | TPM_LEFTALIGN;
        let chosen = TrackPopupMenu(menu, flags, anchor.left, anchor.top - 4, None, owner, None);
        let _ = DestroyMenu(menu);
        chosen.0 as u32
    };
    if cmd != 0 {
        run(cmd, None);
    }
}

/// Markierten Text vorlesen (Strg+Alt+V, sprechender Kopf der Mini-Pille); während des
/// Vorlesens hört es auf.
fn read_selection() {
    if with_app(|app| app.state != State::Idle) == Some(true) {
        with_app(App::stop);
    } else {
        // Die Markierung wird außerhalb von `with_app` ermittelt, weil das bis zu
        // einer Sekunde dauern kann und dabei Nachrichten eintreffen können.
        let text = selection::selected_text();
        with_app(|app| app.speak(text, t("app.no_text_selected")));
    }
}

/// Klick in der Mini-Pille.
fn on_pill_action(action: minipill::Action) {
    match action {
        minipill::Action::Dictate => dictate_anywhere(),
        minipill::Action::Read => read_selection(),
        minipill::Action::OpenWindow => window::show(),
        minipill::Action::Hide => {
            with_app(|app| {
                app.cfg.close_to_pill = false;
                app.cfg.save();
                app.update_pill();
                app.refresh_menu_bar();
            });
        }
    }
}

/// Klick im Mini-Player.
fn on_player_action(action: player::Action) {
    with_app(|app| match action {
        player::Action::Prev => app.navigate(false),
        player::Action::Next => app.navigate(true),
        player::Action::PlayPause => app.toggle_pause(),
        player::Action::Rate(index) => app.select_rate(index),
        player::Action::Stop => app.stop(),
        player::Action::OpenWindow => {
            window::show();
            app.update_player();
        }
    });
}

fn correction_label(mode: ai::Correction) -> &'static str {
    match mode {
        ai::Correction::Spelling => t("correction.spelling"),
        ai::Correction::Style => t("correction.style"),
        ai::Correction::Rewrite => t("correction.rewrite"),
        ai::Correction::Custom => t("correction.custom"),
    }
}

fn open_settings() {
    let Some((values, items, selected, rate)) = with_app(|app| (app.settings(), app.voice_items(), app.current_voice(), app.rate_index())) else {
        return;
    };
    let voices = settings::Voices {
        items,
        selected,
        rates: RATES.iter().map(|(_, key)| t(key).to_string()).collect(),
        rate,
        on_change: |voice, rate| {
            with_app(|app| {
                if let Some(voice) = voice.filter(|v| Some(*v) != app.current_voice()) {
                    app.select_voice(voice);
                }
                if rate != app.rate_index() {
                    app.select_rate(rate);
                }
            });
        },
        on_sample: || {
            with_app(App::speak_sample);
        },
        on_ai_voices: open_ai_voices,
    };
    settings::open(window::visible(), values, voices, |values| {
        with_app(|app| app.apply_settings(values));
    });
}

/// Fragt nach einem Dateinamen und schreibt den Text des Fensters als MP3- oder WAV-Datei.
fn save_audio() {
    let owner = window::visible();
    let Some(text) = window::text() else {
        unsafe {
            MessageBoxW(owner, &HSTRING::from(t("app.text_field_empty")), APP_NAME, MB_ICONINFORMATION);
        }
        return;
    };
    let mut file = [0u16; 520];
    for (slot, unit) in file.iter_mut().zip("Vorleser.mp3".encode_utf16()) {
        *slot = unit;
    }
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        lpstrFilter: w!("MP3 (*.mp3)\0*.mp3\0WAV (*.wav)\0*.wav\0"),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        // Ohne angegebene Endung hängt der Dialog die des gewählten Dateityps an.
        lpstrDefExt: w!("mp3"),
        Flags: OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST,
        ..Default::default()
    };
    if !unsafe { GetSaveFileNameW(&mut dialog) }.as_bool() {
        return;
    }
    let len = file.iter().position(|&unit| unit == 0).unwrap_or(file.len());
    let path = String::from_utf16_lossy(&file[..len]);
    let Some((hwnd, voice, rate, text)) = with_app(|app| {
        (app.hwnd.0 as isize, app.tts.current_voice_id(), app.tts.current_rate(), app.prepare(text))
    }) else {
        return;
    };
    // Die Synthese läuft auf einem eigenen Thread, damit das Fenster bedienbar bleibt.
    PENDING_WAV.with(|cell| *cell.borrow_mut() = Some(path.clone()));
    window::set_title(Some(t("app.saving_title")));
    with_app(|app| app.refresh_menu_bar());
    std::thread::spawn(move || {
        let saved = unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            let mp3 = path.to_lowercase().ends_with(".mp3");
            if mp3 {
                // Erst als WAV in den Temp-Ordner, dann umwandeln.
                let wav = std::env::temp_dir().join(format!("vorleser-export-{}.wav", std::process::id()));
                let saved = tts::save_wav(voice.as_deref(), rate, &text, &wav.to_string_lossy()).is_ok()
                    && mp3::wav_to_mp3(&wav, &path).is_ok();
                let _ = std::fs::remove_file(&wav);
                saved
            } else {
                tts::save_wav(voice.as_deref(), rate, &text, &path).is_ok()
            }
        };
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_WAV_DONE, WPARAM(saved as usize), LPARAM(0));
        }
    });
}

fn on_wav_done(saved: bool) {
    window::set_title(None);
    let path = PENDING_WAV.with(|cell| cell.borrow_mut().take()).unwrap_or_default();
    with_app(|app| app.refresh_menu_bar());
    let (message, icon) = if saved {
        (format!("{}\n{path}", t("app.saved")), MB_ICONINFORMATION)
    } else {
        (t("app.audio_file_could_not").to_string(), MB_ICONERROR)
    };
    unsafe {
        MessageBoxW(window::visible(), &HSTRING::from(message), APP_NAME, icon);
    }
}

fn open_url(url: &str) {
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
    }
}

fn on_update_found() {
    with_app(|app| {
        app.cfg.last_update_check = update::last_check();
        app.cfg.save();
    });
    if let (Some(version), Some(hwnd)) = (update::newer_version(), with_app(|app| app.hwnd)) {
        let text = t("app.version_available_click_here")
            .replace("{}", &version);
        tray::balloon_tagged(hwnd, t("app.new_version"), &text, BALLOON_UPDATE);
    }
}

/// Version und Hersteller. Läuft als eigenes Meldungsfenster, deshalb außerhalb von `with_app`.
fn show_about() {
    let body = t("app.reads_selected_text_aloud");
    let text = HSTRING::from(format!("Vorleser {}\n{body}", env!("CARGO_PKG_VERSION")));
    let title = HSTRING::from(t("app.about_vorleser_2"));
    unsafe {
        MessageBoxW(window::visible(), &text, &title, MB_ICONINFORMATION | MB_SETFOREGROUND);
    }
}

/// Menü mit "Markierten Text vorlesen" an der Mausposition (Tastenkombination oder Mausauslöser).
fn show_selection_menu(hwnd: HWND) {
    if MENU_OPEN.load(Ordering::Relaxed) {
        return;
    }
    // Vor dem Öffnen lesen: Das Menü nimmt der Anwendung den Fokus.
    let text = selection::selected_text();
    show_menu(hwnd, Some(text));
}

/// Befehle aus dem Fenster (Schaltflächen, Auswahllisten, Menüleiste und Tastenkürzel).
fn on_window_command(id: u32, code: u32) {
    if id >= window::MENU_BASE {
        run(id - window::MENU_BASE, None);
        return;
    }
    match (id, code) {
        (window::ID_READ, window::CLICKED) => {
            with_app(App::speak_window);
        }
        (window::ID_PAUSE, window::CLICKED) => {
            with_app(App::toggle_pause);
        }
        (window::ID_STOP, window::CLICKED) => {
            with_app(App::stop);
        }
        (window::ID_SETUP_AI, window::CLICKED) => open_ai_setup(),
        (window::ID_VISIBILITY, _) => {
            with_app(|app| {
                app.update_pill();
                app.update_player();
            });
        }
        (window::ID_VOICE, window::CLICKED) => choose_from_bar(window::ID_VOICE),
        (window::ID_SPEED, window::CLICKED) => choose_from_bar(window::ID_SPEED),
        (window::ID_SUMMARIZE, window::CLICKED) => run(CMD_SUMMARIZE, None),
        (window::ID_DICTATE, window::CLICKED) => run(CMD_DICTATE, None),
        (window::ID_CORRECT, window::CLICKED) => run(CMD_CORRECT, None),
        (window::ID_CORRECTION_MODE, window::SELECTED) => {
            if let Some(mode) = window::selected_correction_mode().and_then(|i| ai::Correction::ALL.get(i)) {
                with_app(|app| app.set_correction_mode(*mode));
            }
        }
        (window::ID_READ_RESULT, window::CLICKED) => run(CMD_READ_RESULT, None),
        (window::ID_SUMMARY_MODE, window::SELECTED) => {
            if let Some(mode) = window::selected_summary_mode().and_then(|i| ai::Summary::ALL.get(i)) {
                with_app(|app| app.set_summary_mode(*mode));
            }
        }
        (window::ID_PREV, window::CLICKED) => {
            with_app(|app| app.navigate(false));
        }
        (window::ID_NEXT, window::CLICKED) => {
            with_app(|app| app.navigate(true));
        }
        _ => {}
    }
}

fn on_hotkey(hwnd: HWND, id: i32) {
    match id {
        HOTKEY_READ => read_selection(),
        HOTKEY_PAUSE => {
            with_app(App::toggle_pause);
        }
        HOTKEY_MENU => show_selection_menu(hwnd),
        HOTKEY_OCR => screen_ocr(),
        HOTKEY_DICTATE => dictate_anywhere(),
        _ => {}
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_HOTKEY => on_hotkey(hwnd, wparam.0 as i32),
        WM_TRAY => match lparam.0 as u32 {
            WM_RBUTTONUP => show_menu(hwnd, None),
            BALLOON_CLICKED => {
                if tray::take_balloon_tag() == BALLOON_UPDATE {
                    open_url(update::RELEASES_URL);
                }
            }
            WM_LBUTTONUP => {
                if with_app(|app| app.state != State::Idle) == Some(true) {
                    with_app(App::toggle_pause);
                } else {
                    window::show();
                }
            }
            _ => {}
        },
        WM_MOUSE_MENU => show_selection_menu(hwnd),
        WM_SHOW_WINDOW => window::show(),
        WM_COPYDATA => {
            let data = unsafe { &*(lparam.0 as *const COPYDATASTRUCT) };
            if data.dwData == COPYDATA_OPEN_FILE && !data.lpData.is_null() {
                let units = unsafe { std::slice::from_raw_parts(data.lpData as *const u16, data.cbData as usize / 2) };
                load_file(String::from_utf16_lossy(units));
            }
            return LRESULT(1);
        }
        WM_UPDATE => {
            if wparam.0 == 1 {
                on_update_found();
            } else {
                with_app(|app| {
                    app.cfg.last_update_check = update::last_check();
                    app.cfg.save();
                });
            }
        }
        WM_WAV_DONE => on_wav_done(wparam.0 == 1),
        WM_UPDATE_MANUAL => on_manual_update(wparam.0),
        WM_FILE_LOADED => on_file_loaded(),
        WM_FILE_PROGRESS => {
            let title = t("app.loading_page").replacen("{}", &wparam.0.to_string(), 1).replacen("{}", &lparam.0.to_string(), 1);
            window::set_title(Some(&title));
        }
        WM_OCR_DONE => on_ocr_done(),
        WM_AI_DONE => {
            with_app(App::on_ai_done);
        }
        WM_DICT_READY => {
            with_app(|app| app.on_dict_ready(wparam.0));
        }
        WM_DICT_TEXT => {
            with_app(App::on_dict_text);
        }
        WM_DICT_DONE => {
            with_app(App::on_dict_done);
        }
        WM_TRANSCRIBE_PROGRESS => {
            window::set_title(Some(&t("audio.progress").replace("{}", &wparam.0.min(100).to_string())));
        }
        WM_TRANSCRIBE_DONE => on_transcribe_done(),
        WM_OPEN_AI_SETUP => {
            window::show();
            open_ai_setup();
        }
        WM_AI_PROGRESS => {
            let title = t("app.ai_working_part").replacen("{}", &wparam.0.to_string(), 1).replacen("{}", &lparam.0.to_string(), 1);
            window::set_title(Some(&title));
        }
        WM_TTS => {
            with_app(|app| {
                let progress = app.tts.poll();
                if let Some((start, len)) = progress.word {
                    app.on_word(start, len);
                }
                if let Some(result) = progress.finished {
                    app.on_finished(result);
                }
            });
        }
        // Auch beim Abmelden oder Herunterfahren den Text für später merken.
        WM_ENDSESSION if wparam.0 != 0 => {
            with_app(App::save_session);
        }
        WM_DESTROY => {
            with_app(App::save_session);
            tray::remove(hwnd);
            unsafe { PostQuitMessage(0) };
        }
        _ if msg != 0 && msg == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            with_app(|app| app.add_icon());
        }
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
    LRESULT(0)
}

/// Kennung von `WM_COPYDATA`, mit der eine zweite Instanz eine Datei zum Öffnen übergibt.
const COPYDATA_OPEN_FILE: usize = 0x564F_524C;

/// Die beim Start übergebene Datei (erstes Argument, das keine Option ist).
fn file_argument() -> Option<String> {
    std::env::args().skip(1).find(|arg| !arg.starts_with("--"))
}

fn main() {
    unsafe {
        // Nur eine Instanz; das Handle bleibt bis zum Prozessende offen.
        let test = std::env::var_os("VORLESER_TEST").is_some();
        TEST_MODE.store(test, Ordering::Relaxed);
        // Mit VORLESER_TEST_DATA darf ein Testlauf schreiben; APPDATA zeigt dann auf einen Testordner.
        if test && std::env::var_os("VORLESER_TEST_DATA").is_none() {
            config::set_read_only();
        }
        let name = if test { w!("Local\\Vorleser.Test") } else { w!("Local\\Vorleser.Instanz") };
        let _mutex = CreateMutexW(None, true, name);
        if GetLastError() == ERROR_ALREADY_EXISTS {
            // Das Programm läuft schon: dessen Fenster zeigen bzw. ihm die Datei übergeben,
            // statt ein zweites Mal zu starten.
            if let Ok(running) = FindWindowW(MAIN_CLASS, APP_NAME) {
                let _ = AllowSetForegroundWindow(ANY_PROCESS);
                match file_argument() {
                    Some(path) => {
                        let wide: Vec<u16> = path.encode_utf16().collect();
                        let data = COPYDATASTRUCT {
                            dwData: COPYDATA_OPEN_FILE,
                            cbData: (wide.len() * 2) as u32,
                            lpData: wide.as_ptr() as *mut _,
                        };
                        SendMessageW(running, WM_COPYDATA, Some(WPARAM(0)), Some(LPARAM(&data as *const _ as isize)));
                    }
                    None => {
                        let _ = PostMessageW(Some(running), WM_SHOW_WINDOW, WPARAM(0), LPARAM(0));
                    }
                }
            }
            return;
        }
        // Sonst zeichnet Windows das Menü auf hochauflösenden Bildschirmen unscharf.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: MAIN_CLASS,
            ..Default::default()
        };
        RegisterClassW(&class);
        // Verstecktes Top-Level-Fenster statt Message-only-Fenster, damit "TaskbarCreated" ankommt.
        let Ok(hwnd) = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            class.lpszClassName,
            APP_NAME,
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            return;
        };
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);

        // Die Sprache muss feststehen, bevor das Fenster seine Beschriftungen bekommt.
        // Im Testlauf lässt sich die Sprache über VORLESER_LANG vorgeben (für Screenshots).
        let language = std::env::var("VORLESER_LANG").ok().filter(|_| test).unwrap_or(Config::load().language);
        i18n::init(&language);
        window::enable_visual_styles();
        let dialog = window::create(tray::load_icon(), on_window_command, load_file).unwrap_or_default();
        // Tastenkürzel des Fensters; Strg+Z, X, C und V erledigt das Textfeld selbst.
        const CONTROL: u8 = 0x08 | 0x01; // FCONTROL | FVIRTKEY
        const KEY: u8 = 0x01; // FVIRTKEY
        const ALT: u8 = 0x10 | 0x01; // FALT | FVIRTKEY
        const SHIFT: u8 = 0x04 | 0x01; // FSHIFT | FVIRTKEY
        window::set_accelerators(&[
            (CONTROL, b'O' as u16, CMD_OPEN_FILE),
            (CONTROL, b'S' as u16, CMD_SAVE_WAV),
            (CONTROL, b'A' as u16, CMD_SELECT_ALL),
            (KEY, 0x74, CMD_READ_WINDOW), // F5
            (KEY, 0x75, CMD_PAUSE),       // F6
            (KEY, 0x76, CMD_STOP),        // F7
            (ALT, 0x25, CMD_PREV),        // Alt+Links
            (ALT, 0x27, CMD_NEXT),        // Alt+Rechts
            (KEY, 0x78, CMD_SUMMARIZE),   // F9
            (SHIFT, 0x78, CMD_READ_RESULT), // Umschalt+F9
            (KEY, 0x77, CMD_DICTATE),     // F8
            (SHIFT, 0x77, CMD_CORRECT),   // Umschalt+F8
        ]);

        match App::new(hwnd) {
            Ok(app) => APP.with(|cell| *cell.borrow_mut() = Some(app)),
            Err(e) => {
                let text = HSTRING::from(format!(
                    "{}\n\n{e}",
                    t("app.windows_speech_output_sapi")
                ));
                MessageBoxW(None, &text, APP_NAME, MB_ICONERROR);
                return;
            }
        }

        with_app(|app| app.restore_session());
        // Von Hand gestartet öffnet sich das Fenster; der Autostart bleibt still im Infobereich.
        if !std::env::args().any(|arg| arg == ARG_TRAY) {
            window::show();
        } else {
            with_app(|app| app.update_pill());
        }
        // „Öffnen mit Vorleser“ bzw. vorleser.exe <Datei>
        if let Some(path) = file_argument() {
            load_file(path);
        }
        // Ergänzt bei einem Autostart-Eintrag für genau diese Datei das Argument --tray (ältere
        // Versionen trugen es nicht ein). Einträge für andere Kopien bleiben unangetastet.
        if let (Some(command), Ok(exe), false) = (autostart_command(), std::env::current_exe(), test) {
            let own = command.to_lowercase().contains(&exe.display().to_string().to_lowercase());
            if own && !command.contains(ARG_TRAY) {
                set_autostart(true);
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if window::translate_accelerator(&msg) {
                continue;
            }
            // Tab, Eingabe und Esc im Fenster wie in einem Dialog behandeln.
            let handled = settings::handle().is_some_and(|s| IsDialogMessageW(s, &msg).as_bool())
                || aivoices::handle().is_some_and(|s| IsDialogMessageW(s, &msg).as_bool())
                || aisettings::handle().is_some_and(|s| IsDialogMessageW(s, &msg).as_bool())
                || dictsetup::handle().is_some_and(|s| IsDialogMessageW(s, &msg).as_bool())
                || aisetup::handle().is_some_and(|s| IsDialogMessageW(s, &msg).as_bool())
                || IsDialogMessageW(dialog, &msg).as_bool();
            if !handled {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // Erst nach der Schleife freigeben, solange COM noch initialisiert ist.
        APP.with(|cell| cell.borrow_mut().take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortens_voice_names() {
        assert_eq!(short_voice_name("Microsoft Hedda Desktop - German"), "Hedda");
        assert_eq!(short_voice_name("Microsoft Katja - German (Germany)"), "Katja");
        assert_eq!(short_voice_name("Thorsten HD (KI-Stimme)"), "Thorsten HD");
    }

    #[test]
    fn labels_speeds() {
        i18n::init("de");
        assert_eq!(speed_label(0), "1,0×");
        assert_eq!(speed_label(2), "1,2×");
        assert_eq!(speed_label(-6), "0,5×");
    }
}
