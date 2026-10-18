//! Vorleser: liest markierten Text mit den Windows-Stimmen vor.
//! Ein Thread, ein verstecktes Fenster, ein Symbol im Infobereich.

#![windows_subsystem = "windows"]

mod config;
mod mouse;
mod selection;
mod settings;
mod textprep;
mod tray;
mod tts;
mod window;

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
use windows::Win32::UI::Controls::Dialogs::{
    GetSaveFileNameW, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_NOREPEAT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AllowSetForegroundWindow, AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu,
    DestroyWindow, DispatchMessageW, FindWindowW, GetMessageW, IsDialogMessageW, LoadCursorW,
    MessageBoxW, SetCursor, IDC_WAIT,
    PostMessageW, PostQuitMessage, RegisterClassW,
    RegisterWindowMessageW, TranslateMessage, HMENU, MB_ICONERROR, MB_ICONINFORMATION,
    MB_SETFOREGROUND, MENU_ITEM_FLAGS, MF_BYCOMMAND,
    MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING, MSG, WINDOW_EX_STYLE, WM_APP,
    WM_DESTROY, WM_HOTKEY, WM_LBUTTONUP, WM_RBUTTONUP, WNDCLASSW, WS_OVERLAPPED,
};

use config::Config;
use tts::Tts;

const APP_NAME: PCWSTR = w!("Vorleser");
const RUN_KEY: PCWSTR = w!(r"Software\Microsoft\Windows\CurrentVersion\Run");

const WM_TRAY: u32 = WM_APP + 1;
const WM_TTS: u32 = WM_APP + 2;
const WM_MOUSE_MENU: u32 = WM_APP + 3;
/// Von einer zweiten Instanz gesendet: Fenster zeigen.
const WM_SHOW_WINDOW: u32 = WM_APP + 4;

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
const CMD_RATE: u32 = 100;
const CMD_VOICE: u32 = 1000;

const RATES: [(i32, &str); 9] = [
    (-6, "Sehr langsam"),
    (-4, "Langsam"),
    (-2, "Etwas langsamer"),
    (0, "Normal"),
    (2, "Etwas schneller"),
    (4, "Schnell"),
    (6, "Schneller"),
    (8, "Sehr schnell"),
    (10, "Maximal"),
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
/// Instanzsperre, keine globalen Tastenkombinationen, kein Maus-Auslöser, kein Ton.
static TEST_MODE: AtomicBool = AtomicBool::new(false);

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
        let tts = Tts::new()?;
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
        Ok(app)
    }

    fn add_icon(&self) {
        tray::add(self.hwnd, tray::load_icon(), WM_TRAY, &self.tip());
    }

    fn tip(&self) -> String {
        match self.state {
            State::Idle => format!("Vorleser – {} liest markierten Text vor", self.cfg.hotkey_read),
            State::Speaking => "Vorleser – liest vor".to_string(),
            State::Paused => "Vorleser – pausiert".to_string(),
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
            let text = format!("Nicht verfügbar oder ungültig: {}", failed.join(", "));
            tray::balloon(self.hwnd, "Tastenkombination", &text);
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
        }
    }

    fn apply_settings(&mut self, values: settings::Values) {
        self.cfg.hotkey_read = values.hotkey_read;
        self.cfg.hotkey_pause = values.hotkey_pause;
        self.cfg.hotkey_menu = values.hotkey_menu;
        self.cfg.mouse_menu = values.mouse_menu;
        self.cfg.volume = values.volume;
        self.cfg.clean_text = values.clean_text;
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
        let labels: Vec<&str> = RATES.iter().map(|(_, label)| *label).collect();
        window::set_rates(&labels);
        window::set_topmost(self.cfg.always_on_top);
        self.sync_window();
    }

    /// Gleicht die Auswahl im Fenster mit den Einstellungen ab (z. B. nach Änderung über das Menü).
    fn sync_window(&self) {
        window::select_voice(self.current_voice());
        window::select_rate(RATES.iter().position(|(rate, _)| *rate == self.cfg.rate));
        window::set_playback(self.state != State::Idle, self.state == State::Paused);
    }

    fn select_voice(&mut self, index: usize) {
        if let Some(voice) = self.tts.voices().get(index) {
            let id = voice.id.clone();
            if self.tts.set_voice(&id) {
                self.cfg.voice = id;
                self.cfg.save();
            } else {
                tray::balloon(self.hwnd, "Vorleser", "Diese Stimme lässt sich nicht verwenden.");
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
            Follow { base: 0, restore: (0, 0) }
        });
        self.start(text, follow);
    }

    /// Liest den markierten Teil des Textfelds vor, sonst den ganzen Text.
    fn speak_window(&mut self) {
        // Zuerst die eigene Wortmarkierung zurücknehmen, sonst gälte sie als Auswahl.
        self.end_follow();
        let Some(source) = window::source() else { return };
        let (start, end) = source.selection;
        let (text, follow) = if start < end {
            let part = String::from_utf16_lossy(&source.content[start as usize..end as usize]);
            (part, Follow { base: start, restore: (start, end) })
        } else {
            // Die Bereinigung wird sichtbar ins Textfeld übernommen: gelesen wird, was dort steht.
            let shown = String::from_utf16_lossy(&source.content);
            let text = to_crlf(&self.prepare(shown.clone()));
            if text != shown {
                window::set_text(&text);
            }
            (text, Follow { base: 0, restore: (0, 0) })
        };
        if text.trim().is_empty() {
            tray::balloon(self.hwnd, "Vorleser", "Das Textfeld ist leer.");
            return;
        }
        self.start(text, Some(follow));
    }

    fn start(&mut self, text: String, follow: Option<Follow>) {
        match self.tts.speak(&text) {
            Ok(()) => {
                self.retry = Some(text);
                self.follow = follow;
                self.set_state(State::Speaking);
            }
            Err(_) => tray::balloon(self.hwnd, "Vorleser", "Die Sprachausgabe ist fehlgeschlagen."),
        }
    }

    /// Markiert das gerade gesprochene Wort im Fenster.
    fn on_word(&self, start: u32, len: u32) {
        if let Some(follow) = self.follow {
            window::mark(follow.base + start, follow.base + start + len);
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
    }

    /// Die Ausgabe ist zu Ende; `result` ist ein Fehler, wenn die Stimme abgebrochen hat.
    fn on_finished(&mut self, result: windows::core::Result<()>) {
        let retry = self.retry.take();
        if result.is_err() {
            // Online-Stimmen scheitern gelegentlich an ihrem Dienst; ein zweiter Versuch hilft oft.
            if retry.is_some_and(|text| self.tts.speak(&text).is_ok()) {
                return;
            }
            let hint = "Die Stimme konnte nicht sprechen. Online-Stimmen brauchen eine Internetverbindung.";
            tray::balloon(self.hwnd, "Vorleser", hint);
        }
        self.end_follow();
        self.set_state(State::Idle);
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

    fn stop(&mut self) {
        self.retry = None;
        self.end_follow();
        self.tts.stop();
        self.set_state(State::Idle);
    }

    /// `selection`: `None` lässt den Eintrag für markierten Text weg (Menü am Symbol),
    /// sonst gibt der Wert an, ob eine Markierung gefunden wurde.
    fn build_menu(&self, selection: Option<bool>) -> HMENU {
        unsafe {
            let menu = CreatePopupMenu().unwrap_or_default();
            let active = self.state != State::Idle;
            item(menu, CMD_OPEN, "Vorleser öffnen", true, false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            if let Some(found) = selection {
                item(menu, CMD_READ_SELECTION, "Markierten Text vorlesen", found, false);
            }
            item(menu, CMD_READ_CLIPBOARD, "Zwischenablage vorlesen", true, false);
            let pause = if self.state == State::Paused { "Fortsetzen" } else { "Pause" };
            item(menu, CMD_PAUSE, &format!("{pause}\t{}", self.cfg.hotkey_pause), active, false);
            item(menu, CMD_STOP, "Stopp", active, false);
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());

            let voices = self.build_voice_menu();
            let _ = AppendMenuW(menu, MF_POPUP, voices.0 as usize, w!("Stimme"));

            let rates = CreatePopupMenu().unwrap_or_default();
            for (i, (rate, label)) in RATES.iter().enumerate() {
                let id = CMD_RATE + i as u32;
                item(rates, id, label, true, false);
                if *rate == self.cfg.rate {
                    let last = CMD_RATE + RATES.len() as u32;
                    let _ = CheckMenuRadioItem(rates, CMD_RATE, last, id, MF_BYCOMMAND.0);
                }
            }
            let _ = AppendMenuW(menu, MF_POPUP, rates.0 as usize, w!("Geschwindigkeit"));

            item(menu, CMD_AUTOSTART, "Mit Windows starten", true, autostart_enabled());
            let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            item(menu, CMD_SETTINGS, "Einstellungen …", true, false);
            item(menu, CMD_ABOUT, "Über Vorleser …", true, false);
            item(menu, CMD_QUIT, "Beenden", true, false);
            menu
        }
    }

    /// Ein Untermenü je Sprache; bei nur einer Sprache eine einfache Liste.
    fn build_voice_menu(&self) -> HMENU {
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
                    let id = CMD_VOICE + i as u32;
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
            CMD_READ_SELECTION => self.speak(selection, "Kein Text markiert."),
            CMD_READ_CLIPBOARD => {
                self.speak(selection::clipboard_text(), "Die Zwischenablage enthält keinen Text.")
            }
            CMD_PAUSE => self.toggle_pause(),
            CMD_STOP => self.stop(),
            CMD_AUTOSTART => set_autostart(!autostart_enabled()),
            CMD_OPEN => window::show(),
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
        if cmd == CMD_QUIT {
            let _ = DestroyWindow(hwnd);
            return;
        }
    }
    if cmd == CMD_ABOUT {
        show_about();
    } else if cmd == CMD_SETTINGS {
        open_settings();
    } else if cmd != 0 {
        with_app(|app| app.command(cmd, selection.flatten()));
    }
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
            MessageBoxW(owner, w!("Das Textfeld ist leer."), APP_NAME, MB_ICONINFORMATION);
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
        lpstrFilter: w!("WAV-Audio (*.wav)\0*.wav\0"),
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
    // Die Sprachsynthese läuft synchron; die Sanduhr zeigt, dass gearbeitet wird.
    let saved = unsafe {
        let previous = SetCursor(LoadCursorW(None, IDC_WAIT).ok());
        let saved = with_app(|app| app.tts.save_wav(&app.prepare(text), &path));
        SetCursor(Some(previous));
        saved
    };
    let (message, icon) = match saved {
        Some(Ok(())) => (format!("Gespeichert:\n{path}"), MB_ICONINFORMATION),
        _ => ("Die Audiodatei konnte nicht geschrieben werden.".to_string(), MB_ICONERROR),
    };
    unsafe {
        MessageBoxW(owner, &HSTRING::from(message), APP_NAME, icon);
    }
}

/// Version und Hersteller. Läuft als eigenes Meldungsfenster, deshalb außerhalb von `with_app`.
fn show_about() {
    let text = HSTRING::from(format!(
        "Vorleser {}\n\
         Liest markierten Text mit den Windows-Stimmen vor.\n\
         Freie Software unter der GNU General Public License, Version 3.\n\n\
         beseco – Beubl IT Systems\n\
         Florian Beubl\n\
         Veit-Adam 15, 85354 Freising\n\n\
         Telefon: 08161 185150-0\n\
         E-Mail: info@beseco.com\n\
         Web: https://beseco.com\n\
         Quellcode: https://github.com/Beseco/beseco-vorleser",
        env!("CARGO_PKG_VERSION")
    ));
    unsafe {
        MessageBoxW(window::visible(), &text, w!("Über Vorleser"), MB_ICONINFORMATION | MB_SETFOREGROUND);
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

/// Befehle aus dem Fenster (Schaltflächen und Auswahllisten).
fn on_window_command(id: u32, code: u32) {
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
        (window::ID_ABOUT, window::CLICKED) => show_about(),
        (window::ID_SETTINGS, window::CLICKED) => open_settings(),
        (window::ID_SAVE_AUDIO, window::CLICKED) => save_audio(),
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
                with_app(|app| app.speak(text, "Kein Text markiert."));
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
        WM_DESTROY => {
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

        window::enable_visual_styles();
        let dialog = window::create(tray::load_icon(), on_window_command).unwrap_or_default();

        match App::new(hwnd) {
            Ok(app) => APP.with(|cell| *cell.borrow_mut() = Some(app)),
            Err(e) => {
                let text = HSTRING::from(format!(
                    "Die Windows-Sprachausgabe (SAPI) konnte nicht gestartet werden.\n\n{e}"
                ));
                MessageBoxW(None, &text, APP_NAME, MB_ICONERROR);
                return;
            }
        }

        // Von Hand gestartet öffnet sich das Fenster; der Autostart bleibt still im Infobereich.
        if !std::env::args().any(|arg| arg == ARG_TRAY) {
            window::show();
        }
        // Frischt Pfad und Argument eines vorhandenen Autostart-Eintrags auf.
        if autostart_enabled() {
            set_autostart(true);
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            // Tab, Eingabe und Esc im Fenster wie in einem Dialog behandeln.
            let handled = settings::handle().is_some_and(|s| IsDialogMessageW(s, &msg).as_bool())
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
