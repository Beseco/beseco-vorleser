//! Vorleser: liest markierten Text mit den Windows-Stimmen vor.
//! Ein Thread, ein verstecktes Fenster, ein Symbol im Infobereich.

#![windows_subsystem = "windows"]

mod aivoices;
mod config;
mod http;
mod i18n;
mod mouse;
mod piper;
mod selection;
mod settings;
mod textprep;
mod tray;
mod tts;
mod update;
mod window;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

use windows::core::{w, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{
    GetLastError, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HWND, LPARAM, LRESULT, WPARAM,
};
use windows::Win32::Globalization::{
    GetUserDefaultUILanguage, MultiByteToWideChar, CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS,
};
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
use windows::Win32::UI::WindowsAndMessaging::{
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
}

impl Follow {
    fn new(base: u32, end: u32, restore: (u32, u32)) -> Follow {
        Follow { base, restore, pos: base, end }
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
        let app = App { hwnd, tts, cfg, state: State::Idle, retry: None, follow: None };
        app.add_icon();
        app.register_hotkeys();
        app.fill_window();
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
            mouse_menu: self.cfg.mouse_menu.clone(),
            volume: self.cfg.volume,
            clean_text: self.cfg.clean_text,
            remember_text: self.cfg.remember_text,
            update_check: self.cfg.update_check,
            language: self.cfg.language.clone(),
            proxy: self.cfg.proxy.clone(),
        }
    }

    fn apply_settings(&mut self, values: settings::Values) {
        self.cfg.hotkey_read = values.hotkey_read;
        self.cfg.hotkey_pause = values.hotkey_pause;
        self.cfg.hotkey_menu = values.hotkey_menu;
        self.cfg.mouse_menu = values.mouse_menu;
        self.cfg.volume = values.volume;
        self.cfg.clean_text = values.clean_text;
        self.cfg.update_check = values.update_check;
        self.cfg.remember_text = values.remember_text;
        self.cfg.proxy = values.proxy;
        http::set_proxy(&self.cfg.proxy);
        if values.language != self.cfg.language {
            self.set_language(&values.language);
        }
        update::set_enabled(values.update_check);
        self.cfg.save();
        for id in [HOTKEY_READ, HOTKEY_PAUSE, HOTKEY_MENU] {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), id);
            }
        }
        self.register_hotkeys();
        self.tts.set_volume(self.cfg.volume);
        tray::set_tip(self.hwnd, &self.tip());
    }

    /// Stellt die Sprache der Oberfläche um; Fenster und Menüs wechseln sofort.
    fn set_language(&mut self, code: &str) {
        self.cfg.language = code.to_string();
        self.cfg.save();
        i18n::init(code);
        window::relabel();
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
        }
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

    fn fill_window(&self) {
        let voices = self.tts.voices();
        let items: Vec<(usize, String)> = self
            .voice_groups()
            .into_iter()
            .flat_map(|(_, indices)| indices)
            .map(|i| (i, voices[i].name.clone()))
            .collect();
        window::set_voices(&items);
        let labels: Vec<&str> = RATES.iter().map(|(_, key)| t(key)).collect();
        window::set_rates(&labels);
        window::set_topmost(self.cfg.always_on_top);
        self.sync_window();
    }

    /// Gleicht die Auswahl im Fenster mit den Einstellungen ab (z. B. nach Änderung über das Menü).
    fn sync_window(&self) {
        window::select_voice(self.current_voice());
        window::select_rate(RATES.iter().position(|(rate, _)| *rate == self.cfg.rate));
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
        let follow = window::visible().map(|_| {
            window::set_text(&text);
            Follow::new(0, text.encode_utf16().count() as u32, (0, 0))
        });
        self.start(text, follow);
    }

    /// Liest den markierten Teil des Textfelds vor; ohne Markierung ab dem Satz, in dem die
    /// Schreibmarke steht, und steht sie am Anfang oder Ende, den ganzen Text.
    fn speak_window(&mut self) {
        // Zuerst die eigene Wortmarkierung zurücknehmen, sonst gälte sie als Auswahl.
        self.end_follow();
        let Some(source) = window::source() else { return };
        let (start, end) = source.selection;
        let len = source.content.len() as u32;
        if start == end && start > 0 && start < len {
            let starts = textprep::sentence_starts(&source.content);
            let from = textprep::sentence_at(&starts, start).unwrap_or(0);
            self.speak_range(&source.content, from, len, (from, from));
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
                window::set_text(&text);
            }
            let len = text.encode_utf16().count() as u32;
            (text, Follow::new(0, len, (0, 0)))
        };
        if text.trim().is_empty() {
            tray::balloon(self.hwnd, "Vorleser", t("app.text_field_empty"));
            return;
        }
        self.start(text, Some(follow));
    }

    /// Liest einen Bereich des Textfelds vor (Weiterlesen, Navigation).
    fn speak_range(&mut self, content: &[u16], start: u32, end: u32, restore: (u32, u32)) {
        let end = end.min(content.len() as u32);
        if start >= end {
            return;
        }
        let text = String::from_utf16_lossy(&content[start as usize..end as usize]);
        self.start(text, Some(Follow::new(start, end, restore)));
    }

    /// Springt einen Satz vor oder zurück. Beim Vorlesen geht es dort weiter, sonst wandert
    /// nur die Schreibmarke (F5 liest dann ab dort).
    fn navigate(&mut self, forward: bool) {
        let Some(source) = window::source() else { return };
        let starts = textprep::sentence_starts(&source.content);
        match self.follow {
            Some(follow) => match textprep::neighbor(&starts, follow.pos, forward) {
                Some(target) if target < follow.end => {
                    self.tts.stop();
                    self.speak_range(&source.content, target, follow.end, follow.restore);
                }
                // Hinter dem letzten Satz: aufhören, wie am Ende des Textes.
                _ if forward => self.stop(),
                _ => {}
            },
            None => {
                if let Some(target) = textprep::neighbor(&starts, source.selection.0, forward) {
                    window::mark(target, target);
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
        let pos = self.follow.map_or(source.selection.0, |f| f.pos);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(&path, String::from_utf16_lossy(&source.content)).is_ok() {
            self.cfg.resume_pos = pos;
            self.cfg.save();
        }
    }

    /// Holt Text und Leseposition vom letzten Mal zurück.
    fn restore_session(&self) {
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
                self.follow = follow;
                self.set_state(State::Speaking);
            }
            Err(_) => tray::balloon(self.hwnd, "Vorleser", t("app.speech_output_failed")),
        }
    }

    /// Markiert das gerade gesprochene Wort im Fenster.
    fn on_word(&mut self, start: u32, len: u32) {
        if let Some(follow) = &mut self.follow {
            follow.pos = follow.base + start;
            window::mark(follow.pos, follow.pos + len);
        }
    }

    /// Beendet das Mitlesen und stellt die ursprüngliche Markierung wieder her.
    fn end_follow(&mut self) {
        if let Some(follow) = self.follow.take() {
            window::mark(follow.restore.0, follow.restore.1);
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
            let starts = window::source().map(|s| textprep::sentence_starts(&s.content)).unwrap_or_default();
            let at = textprep::sentence_at(&starts, follow.pos).unwrap_or(follow.pos);
            window::mark(at, at);
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
            separator(playback);
            let rates = self.build_rate_menu(base);
            let _ = AppendMenuW(playback, MF_POPUP, rates.0 as usize, &HSTRING::from(t("menu.speed")));
            popup(t("menu.playback"), playback);

            let voice = self.build_voice_menu(base);
            separator(voice);
            item(voice, base + CMD_SAMPLE, t("menu.sample"), true, false);
            item(voice, base + CMD_AI_VOICES, t("menu.ai_voices"), true, false);
            popup(t("menu.voice"), voice);

            let tools = CreatePopupMenu().unwrap_or_default();
            item(tools, base + CMD_TOPMOST, t("menu.topmost"), true, self.cfg.always_on_top);
            item(tools, base + CMD_AUTOSTART, t("menu.autostart"), true, autostart_enabled());
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

/// Liest eine Textdatei: UTF-8 oder UTF-16 mit Kennung, sonst UTF-8, sonst die Windows-Codepage.
fn read_text_file(path: &str) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return Some(String::from_utf8_lossy(rest).into_owned());
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        return Some(String::from_utf16_lossy(&units));
    }
    if let Ok(text) = String::from_utf8(bytes.clone()) {
        return Some(text);
    }
    unsafe {
        let len = MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), &bytes, None);
        let mut wide = vec![0u16; len.max(0) as usize];
        MultiByteToWideChar(CP_ACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0), &bytes, Some(&mut wide));
        Some(String::from_utf16_lossy(&wide))
    }
}

/// Lädt eine Datei ins Textfeld (über Datei → Öffnen oder durch Hineinziehen).
fn load_file(path: String) {
    let owner = window::visible();
    let is_text = std::path::Path::new(&path).extension().is_some_and(|e| e.eq_ignore_ascii_case("txt"));
    let message = if !is_text {
        Some(t("app.only_text_files"))
    } else {
        match read_text_file(&path) {
            Some(text) => {
                with_app(App::stop);
                window::set_text(&to_crlf(&text));
                window::show();
                None
            }
            None => Some(t("app.file_unreadable")),
        }
    };
    if let Some(message) = message {
        unsafe {
            MessageBoxW(owner, &HSTRING::from(message), APP_NAME, MB_ICONINFORMATION);
        }
    }
}

fn open_file() {
    let mut file = [0u16; 1024];
    // Filter: "Textdateien (*.txt)\0*.txt\0\0" in der Sprache der Oberfläche.
    let filter: Vec<u16> = format!("{} (*.txt)\0*.txt\0\0", t("app.text_files")).encode_utf16().collect();
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

fn open_settings() {
    if let Some(values) = with_app(|app| app.settings()) {
        settings::open(window::visible(), values, |values| {
            with_app(|app| app.apply_settings(values));
        });
    }
}

/// Fragt nach einem Dateinamen und schreibt den Text des Fensters als WAV-Datei.
fn save_audio() {
    let owner = window::visible();
    let Some(text) = window::text() else {
        unsafe {
            MessageBoxW(owner, &HSTRING::from(t("app.text_field_empty")), APP_NAME, MB_ICONINFORMATION);
        }
        return;
    };
    let mut file = [0u16; 520];
    for (slot, unit) in file.iter_mut().zip("Vorleser.wav".encode_utf16()) {
        *slot = unit;
    }
    let mut dialog = OPENFILENAMEW {
        lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner.unwrap_or_default(),
        lpstrFilter: w!("WAV (*.wav)\0*.wav\0"),
        lpstrFile: PWSTR(file.as_mut_ptr()),
        nMaxFile: file.len() as u32,
        lpstrDefExt: w!("wav"),
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
            tts::save_wav(voice.as_deref(), rate, &text, &path).is_ok()
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
        (window::ID_TOPMOST, window::CLICKED) => {
            with_app(|app| app.set_topmost(window::topmost_checked()));
        }
        (window::ID_PAUSE, window::CLICKED) => {
            with_app(App::toggle_pause);
        }
        (window::ID_STOP, window::CLICKED) => {
            with_app(App::stop);
        }
        (window::ID_SAMPLE, window::CLICKED) => {
            with_app(App::speak_sample);
        }
        (window::ID_VOICE, window::SELECTED) => {
            if let Some(index) = window::selected_voice() {
                with_app(|app| app.select_voice(index));
            }
        }
        (window::ID_AI_VOICES, window::CLICKED) => open_ai_voices(),
        (window::ID_PREV, window::CLICKED) => {
            with_app(|app| app.navigate(false));
        }
        (window::ID_NEXT, window::CLICKED) => {
            with_app(|app| app.navigate(true));
        }
        (window::ID_RATE, window::SELECTED) => {
            if let Some(index) = window::selected_rate() {
                with_app(|app| app.select_rate(index));
            }
        }
        _ => {}
    }
}

fn on_hotkey(hwnd: HWND, id: i32) {
    match id {
        HOTKEY_READ => {
            if with_app(|app| app.state != State::Idle) == Some(true) {
                with_app(App::stop);
            } else {
                // Die Markierung wird außerhalb von `with_app` ermittelt, weil das bis zu
                // einer Sekunde dauern kann und dabei Nachrichten eintreffen können.
                let text = selection::selected_text();
                with_app(|app| app.speak(text, t("app.no_text_selected")));
            }
        }
        HOTKEY_PAUSE => {
            with_app(App::toggle_pause);
        }
        HOTKEY_MENU => show_selection_menu(hwnd),
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
            // Das Programm läuft schon: dessen Fenster zeigen statt ein zweites Mal zu starten.
            if let Ok(running) = FindWindowW(MAIN_CLASS, APP_NAME) {
                let _ = AllowSetForegroundWindow(ANY_PROCESS);
                let _ = PostMessageW(Some(running), WM_SHOW_WINDOW, WPARAM(0), LPARAM(0));
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
        window::set_accelerators(&[
            (CONTROL, b'O' as u16, CMD_OPEN_FILE),
            (CONTROL, b'S' as u16, CMD_SAVE_WAV),
            (CONTROL, b'A' as u16, CMD_SELECT_ALL),
            (KEY, 0x74, CMD_READ_WINDOW), // F5
            (KEY, 0x75, CMD_PAUSE),       // F6
            (KEY, 0x76, CMD_STOP),        // F7
            (ALT, 0x25, CMD_PREV),        // Alt+Links
            (ALT, 0x27, CMD_NEXT),        // Alt+Rechts
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
