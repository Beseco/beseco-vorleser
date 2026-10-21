//! Einrichtungsassistent des Officeassistenten: eine Frage („Wo läuft die KI?“), ein Knopf.
//! Auf diesem PC installiert er bei Bedarf Ollama und lädt ein passendes Modell; für einen
//! Server im Firmennetz oder einen Cloud-Anbieter sucht er das Modell aus und prüft die Verbindung.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBRUSH, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetWindowTextLengthW, GetWindowTextW, LoadCursorW,
    PostMessageW, RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow,
    BS_DEFPUSHBUTTON, CW_USEDEFAULT, ES_AUTOHSCROLL, ES_PASSWORD, HMENU, IDC_ARROW, SW_HIDE, SW_SHOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WM_USER, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_GROUP, WS_OVERLAPPED, WS_SYSMENU,
    WS_TABSTOP, WS_VISIBLE,
};

use crate::ai::{Kind, Provider};
use crate::aicheck::{self, Rating};
use crate::i18n::t;
use crate::ollama;
use crate::window::message_font;

const ID_SETUP: u32 = 1;
const ID_CANCEL: u32 = 2;
const ID_ADVANCED: u32 = 20;
const ID_CHOICES: [u32; 4] = [30, 31, 32, 33];
const COLOR_BTNFACE: usize = 15;
const EM_SETCUEBANNER: u32 = 0x1501;
const BS_AUTORADIOBUTTON: u32 = 9;
const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const PBM_SETPOS: u32 = WM_USER + 2;
const PBM_SETRANGE32: u32 = WM_USER + 6;

/// Fortschritt: `wparam` Promille (oder `usize::MAX` ohne Balken), Text in `STATUS`.
const WM_PROGRESS: u32 = WM_APP + 1;
/// Fertig, Ergebnis in `RESULT`.
const WM_DONE: u32 = WM_APP + 2;

/// Wo die KI läuft.
#[derive(Clone, Copy, PartialEq)]
enum Choice {
    ThisPc,
    Server,
    Cloud,
    Without,
}

const CHOICES: [Choice; 4] = [Choice::ThisPc, Choice::Server, Choice::Cloud, Choice::Without];

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    choices: [HWND; 4],
    server: HWND,
    cloud_url: HWND,
    cloud_key: HWND,
    status: HWND,
    progress: HWND,
    setup: HWND,
    cancel: HWND,
    advanced: HWND,
    font: HFONT,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static ON_DONE: Cell<Option<fn(Provider)>> = const { Cell::new(None) };
    static ON_ADVANCED: Cell<Option<fn()>> = const { Cell::new(None) };
    static RUNNING: Cell<bool> = const { Cell::new(false) };
}

static STATUS: Mutex<String> = Mutex::new(String::new());
static RESULT: Mutex<Option<Result<(Provider, u128), String>>> = Mutex::new(None);
static CANCEL: AtomicBool = AtomicBool::new(false);

pub fn handle() -> Option<HWND> {
    CONTROLS.with(|c| c.get()).map(|c| c.window)
}

/// Öffnet den Assistenten. `on_done` erhält den eingerichteten Anbieter (auch „ohne KI“),
/// `on_advanced` öffnet die ausführlichen KI-Einstellungen.
pub fn open(owner: Option<HWND>, current: &Provider, on_done: fn(Provider), on_advanced: fn()) {
    if let Some(window) = handle() {
        unsafe {
            let _ = SetForegroundWindow(window);
        }
        return;
    }
    let hardware = aicheck::detect();
    let local_url = ollama::local_url();
    let suggestion = ollama::suggest(&hardware);
    let local_state = if ollama::is_running(&local_url) {
        t("setup.local_running").to_string()
    } else if ollama::is_installed() {
        t("setup.local_installed").to_string()
    } else {
        t("setup.local_missing").replace("{}", &gigabytes(ollama::SETUP_MB))
    };
    let rating = match hardware.local_llm() {
        Rating::Good => t("setup.local_good"),
        Rating::Slow => t("setup.local_slow"),
        Rating::NotRecommended => t("setup.local_not_recommended"),
    };
    let model = t("setup.local_model").replacen("{}", suggestion.name, 1).replacen("{}", &gigabytes(suggestion.size_mb), 1);
    let local_text = format!("{local_state} {model} {rating}");
    // Vorauswahl: was schon eingestellt ist, sonst dieser PC nur, wenn er dafür taugt.
    let initial = match current.kind {
        Kind::Ollama if is_local(&current.url) => Choice::ThisPc,
        Kind::Ollama => Choice::Server,
        Kind::OpenAi => Choice::Cloud,
        Kind::None if hardware.local_llm() == Rating::Good || ollama::is_installed() => Choice::ThisPc,
        Kind::None => Choice::Server,
    };
    let server_url = if current.kind == Kind::Ollama && !is_local(&current.url) { current.url.as_str() } else { "" };
    let (cloud_url, cloud_key) = if current.kind == Kind::OpenAi {
        (current.url.as_str(), current.key.as_str())
    } else {
        (Kind::OpenAi.default_url(), "")
    };

    unsafe {
        let Ok(instance) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserKiEinrichten"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: crate::tray::load_icon(),
            hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&class);
        let dpi = GetDpiForSystem();
        let px = |value: i32| value * dpi as i32 / 96;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT { left: 0, top: 0, right: px(560), bottom: px(492) };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_CONTROLPARENT, dpi);
        let Ok(window) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            &HSTRING::from(t("setup.title")),
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
        let child = |class: PCWSTR, text: &str, style: u32, ex: WINDOW_EX_STYLE, id: u32, x: i32, y: i32, width: i32, height: i32| {
            let control = CreateWindowExW(
                ex,
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
        let cue = |control: HWND, text: &str| {
            let text = HSTRING::from(text);
            SendMessageW(control, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(text.as_ptr() as isize)));
        };
        let none = WINDOW_EX_STYLE(0);
        let tab = WS_TABSTOP.0;
        let radio = |i: usize, text: &str, y: i32, first: bool| {
            let group = if first { WS_GROUP.0 | tab } else { 0 };
            child(w!("BUTTON"), text, BS_AUTORADIOBUTTON | group, none, ID_CHOICES[i], 16, y, 528, 24)
        };

        child(w!("STATIC"), t("setup.intro"), 0, none, 0, 16, 12, 528, 40);
        let this_pc = radio(0, t("setup.this_pc"), 60, true);
        child(w!("STATIC"), &local_text, 0, none, 0, 36, 84, 508, 54);
        let server_choice = radio(1, t("setup.server"), 146, false);
        let server = child(w!("EDIT"), server_url, tab | ES_AUTOHSCROLL as u32, WS_EX_CLIENTEDGE, 0, 36, 172, 508, 26);
        cue(server, t("setup.server_hint"));
        let cloud_choice = radio(2, t("setup.cloud"), 210, false);
        let cloud_url = child(w!("EDIT"), cloud_url, tab | ES_AUTOHSCROLL as u32, WS_EX_CLIENTEDGE, 0, 36, 236, 508, 26);
        let cloud_key =
            child(w!("EDIT"), cloud_key, tab | (ES_AUTOHSCROLL | ES_PASSWORD) as u32, WS_EX_CLIENTEDGE, 0, 36, 268, 508, 26);
        cue(cloud_key, t("setup.cloud_key_hint"));
        let without = radio(3, t("setup.without"), 306, false);
        let status = child(w!("STATIC"), t("setup.ready_hint"), 0, none, 0, 16, 344, 528, 58);
        let progress = child(w!("msctls_progress32"), "", 0, none, 0, 16, 406, 528, 16);
        SendMessageW(progress, PBM_SETRANGE32, Some(WPARAM(0)), Some(LPARAM(1000)));
        let _ = ShowWindow(progress, SW_HIDE);
        let advanced = child(w!("BUTTON"), t("setup.advanced"), tab, none, ID_ADVANCED, 16, 446, 190, 30);
        let setup = child(w!("BUTTON"), t("setup.start"), tab | BS_DEFPUSHBUTTON as u32, none, ID_SETUP, 318, 446, 120, 30);
        let cancel = child(w!("BUTTON"), t("settings.cancel"), tab, none, ID_CANCEL, 446, 446, 98, 30);

        let choices = [this_pc, server_choice, cloud_choice, without];
        let index = CHOICES.iter().position(|c| *c == initial).unwrap_or(0);
        SendMessageW(choices[index], BM_SETCHECK, Some(WPARAM(1)), None);
        let c = Controls { window, choices, server, cloud_url, cloud_key, status, progress, setup, cancel, advanced, font };
        CONTROLS.with(|cell| cell.set(Some(c)));
        ON_DONE.with(|cell| cell.set(Some(on_done)));
        ON_ADVANCED.with(|cell| cell.set(Some(on_advanced)));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(choices[index]));
    }
}

fn is_local(url: &str) -> bool {
    url.contains("://localhost") || url.contains("://127.0.0.1")
}

/// „1,7“ für 1700 MB.
fn gigabytes(mb: u64) -> String {
    format!("{:.1}", mb as f64 / 1000.0).replace('.', t("number.decimal"))
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

fn selected(c: &Controls) -> Choice {
    let index = c.choices.iter().position(|b| unsafe { SendMessageW(*b, BM_GETCHECK, None, None).0 == 1 });
    CHOICES[index.unwrap_or(0)]
}

/// Ergänzt eine knapp eingegebene Serveradresse: „ki-server“ wird zu `http://ki-server:11434`.
fn server_address(input: &str) -> String {
    let input = input.trim().trim_end_matches('/');
    let with_scheme = if input.contains("://") { input.to_string() } else { format!("http://{input}") };
    let host = with_scheme.split("://").nth(1).unwrap_or_default();
    if host.contains(':') || host.contains('/') {
        with_scheme
    } else {
        format!("{with_scheme}:11434")
    }
}

fn report(hwnd: isize, promille: Option<u64>, status: String) {
    *STATUS.lock().unwrap() = status;
    let wparam = promille.map_or(usize::MAX, |p| p.min(1000) as usize);
    unsafe {
        let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_PROGRESS, WPARAM(wparam), LPARAM(0));
    }
}

/// Fortschritt eines Downloads als Text und Promille.
fn download_progress(hwnd: isize, what: &str, done: u64, total: u64) -> bool {
    let mb = |bytes: u64| bytes / 1_000_000;
    let text = t("setup.downloading").replacen("{}", what, 1).replacen("{}", &mb(done).to_string(), 1).replacen("{}", &mb(total).to_string(), 1);
    report(hwnd, (total > 0).then(|| done * 1000 / total), text);
    !CANCEL.load(Ordering::Relaxed)
}

/// Richtet die gewählte KI ein (im Hintergrund).
fn run(choice: Choice, server: String, cloud_url: String, cloud_key: String, hwnd: isize) -> Result<(Provider, u128), String> {
    let mut provider = match choice {
        Choice::Without => return Ok((Provider { kind: Kind::None, url: String::new(), key: String::new(), model: String::new() }, 0)),
        Choice::ThisPc => Provider { kind: Kind::Ollama, url: ollama::local_url(), key: String::new(), model: String::new() },
        Choice::Server if server.is_empty() => return Err(t("setup.need_server").to_string()),
        Choice::Server => Provider { kind: Kind::Ollama, url: server_address(&server), key: String::new(), model: String::new() },
        Choice::Cloud if cloud_url.is_empty() || cloud_key.is_empty() => return Err(t("setup.need_key").to_string()),
        Choice::Cloud => Provider { kind: Kind::OpenAi, url: cloud_url, key: cloud_key, model: String::new() },
    };
    if choice == Choice::ThisPc && !ollama::is_running(&provider.url) {
        if !ollama::is_installed() {
            ollama::install(&provider.url, |done, total| download_progress(hwnd, "Ollama", done, total))?;
        } else {
            report(hwnd, None, t("setup.starting").to_string());
            ollama::start(&provider.url)?;
        }
    }
    report(hwnd, None, t("setup.connecting").to_string());
    let models = provider.models()?;
    provider.model = match ollama::choose_model(&models) {
        Some(model) => model,
        None if choice == Choice::ThisPc => {
            let suggestion = ollama::suggest(&aicheck::detect());
            ollama::pull(&provider.url, suggestion.name, |done, total| download_progress(hwnd, suggestion.name, done, total))?;
            suggestion.name.to_string()
        }
        None => return Err(t("setup.no_model").to_string()),
    };
    if CANCEL.load(Ordering::Relaxed) {
        return Err(t("err.cancelled").to_string());
    }
    report(hwnd, None, t("setup.testing").replace("{}", &provider.model));
    let (ms, _) = provider.test()?;
    Ok((provider, ms))
}

fn start(c: &Controls) {
    let choice = selected(c);
    let (server, cloud_url, cloud_key) = (text(c.server), text(c.cloud_url), text(c.cloud_key));
    CANCEL.store(false, Ordering::Relaxed);
    RUNNING.with(|r| r.set(true));
    unsafe {
        for control in c.choices.iter().chain([&c.server, &c.cloud_url, &c.cloud_key, &c.setup, &c.advanced]) {
            let _ = EnableWindow(*control, false);
        }
    }
    set_text(c.status, t("setup.working"));
    let hwnd = c.window.0 as isize;
    std::thread::spawn(move || {
        let result = run(choice, server, cloud_url, cloud_key, hwnd);
        *RESULT.lock().unwrap() = Some(result);
        unsafe {
            let _ = PostMessageW(Some(HWND(hwnd as *mut _)), WM_DONE, WPARAM(0), LPARAM(0));
        }
    });
}

fn on_progress(c: &Controls, promille: usize) {
    set_text(c.status, &STATUS.lock().unwrap());
    unsafe {
        if promille == usize::MAX {
            let _ = ShowWindow(c.progress, SW_HIDE);
        } else {
            let _ = ShowWindow(c.progress, SW_SHOW);
            SendMessageW(c.progress, PBM_SETPOS, Some(WPARAM(promille)), None);
        }
    }
}

fn on_done(c: &Controls) {
    RUNNING.with(|r| r.set(false));
    unsafe {
        let _ = ShowWindow(c.progress, SW_HIDE);
        for control in c.choices.iter().chain([&c.server, &c.cloud_url, &c.cloud_key, &c.setup, &c.advanced]) {
            let _ = EnableWindow(*control, true);
        }
    }
    match RESULT.lock().unwrap().take() {
        Some(Ok((provider, ms))) => {
            let message = if provider.kind == Kind::None {
                t("setup.done_without").to_string()
            } else {
                let seconds = format!("{:.1}", ms as f64 / 1000.0).replace('.', t("number.decimal"));
                t("setup.done").replacen("{}", &provider.model, 1).replacen("{}", &seconds, 1)
            };
            set_text(c.status, &message);
            set_text(c.cancel, t("setup.close"));
            if let Some(on_done) = ON_DONE.with(|cell| cell.get()) {
                on_done(provider);
            }
        }
        Some(Err(error)) => set_text(c.status, &format!("{} {error}", t("setup.failed"))),
        None => {}
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let controls = CONTROLS.with(|c| c.get());
    unsafe {
        match (msg, controls) {
            (WM_COMMAND, Some(c)) => match (wparam.0 & 0xffff) as u32 {
                ID_SETUP if !RUNNING.with(|r| r.get()) => start(&c),
                ID_CANCEL => {
                    // Ein laufender Download bricht ab, das Fenster schließt sich gleich.
                    CANCEL.store(true, Ordering::Relaxed);
                    let _ = DestroyWindow(hwnd);
                }
                ID_ADVANCED => {
                    let _ = DestroyWindow(hwnd);
                    if let Some(on_advanced) = ON_ADVANCED.with(|cell| cell.get()) {
                        on_advanced();
                    }
                }
                _ => {}
            },
            (WM_PROGRESS, Some(c)) => on_progress(&c, wparam.0),
            (WM_DONE, Some(c)) => on_done(&c),
            (WM_CLOSE, _) => {
                CANCEL.store(true, Ordering::Relaxed);
                let _ = DestroyWindow(hwnd);
            }
            (WM_DESTROY, _) => {
                RUNNING.with(|r| r.set(false));
                if let Some(c) = CONTROLS.with(|cell| cell.take()) {
                    let _ = DeleteObject(c.font.into());
                }
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
    fn completes_server_addresses() {
        assert_eq!(server_address("ki-server"), "http://ki-server:11434");
        assert_eq!(server_address("http://ki-server:8080/"), "http://ki-server:8080");
        assert_eq!(server_address("10.0.0.5"), "http://10.0.0.5:11434");
        assert_eq!(server_address("https://ki.firma.de/ollama"), "https://ki.firma.de/ollama");
    }
}
