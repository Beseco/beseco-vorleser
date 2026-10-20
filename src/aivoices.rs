//! Fenster „KI-Stimmen“: zeigt die angebotenen Piper-Stimmen, lädt sie im Hintergrund herunter
//! und entfernt sie wieder.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};
use std::sync::Mutex;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{DeleteObject, HBRUSH, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{AdjustWindowRectExForDpi, GetDpiForSystem};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, LoadCursorW, MessageBoxW, PostMessageW,
    RegisterClassW, SendMessageW, SetForegroundWindow, SetWindowTextW, ShowWindow, BS_DEFPUSHBUTTON,
    CW_USEDEFAULT, HMENU, IDC_ARROW, MB_ICONQUESTION, MB_YESNO, IDYES, SW_SHOW, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_BORDER,
    WS_CAPTION, WS_CHILD, WS_EX_CONTROLPARENT, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use crate::i18n::{self, t};
use crate::piper::{self, Stage, CATALOG};
use crate::window::message_font;

const ID_LIST: u32 = 10;
const ID_DOWNLOAD: u32 = 11;
const ID_REMOVE: u32 = 12;
// IDCANCEL: Esc schließt das Fenster.
const ID_CLOSE: u32 = 2;

const LB_ADDSTRING: u32 = 0x0180;
const LB_RESETCONTENT: u32 = 0x0184;
const LB_SETCURSEL: u32 = 0x0186;
const LB_GETCURSEL: u32 = 0x0188;
const LBN_SELCHANGE: usize = 1;
const LBS_NOTIFY: u32 = 0x0001;
const COLOR_BTNFACE: usize = 15;

/// Fortschritt: `wparam` = Abschnitt (0 Programm, 1 Stimme), `lparam` = Prozent.
const WM_PROGRESS: u32 = WM_APP + 1;
/// Download fertig: `wparam` = 1 bei Erfolg.
const WM_DONE: u32 = WM_APP + 2;

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    list: HWND,
    status: HWND,
    download: HWND,
    remove: HWND,
    font: HFONT,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static ON_CHANGE: Cell<Option<fn(Option<String>)>> = const { Cell::new(None) };
    /// Index in [`CATALOG`] der Stimme, die gerade geladen wird.
    static LOADING: Cell<Option<usize>> = const { Cell::new(None) };
}

static CANCEL: AtomicBool = AtomicBool::new(false);
/// Ein Download-Thread läuft noch (auch nach dem Schließen des Fensters, bis er aufgeräumt hat).
static BUSY: AtomicBool = AtomicBool::new(false);
static ERROR: Mutex<String> = Mutex::new(String::new());

pub fn handle() -> Option<HWND> {
    CONTROLS.with(|c| c.get()).map(|c| c.window)
}

/// Öffnet das Fenster. `on_change` erhält nach einem Download die Kennung der neuen Stimme,
/// nach dem Entfernen `None`; danach sollte die Stimmenliste neu eingelesen werden.
pub fn open(owner: Option<HWND>, on_change: fn(Option<String>)) {
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
            lpszClassName: w!("VorleserKiStimmen"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: crate::tray::load_icon(),
            hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&class);

        let dpi = GetDpiForSystem();
        let px = |value: i32| value * dpi as i32 / 96;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT { left: 0, top: 0, right: px(540), bottom: px(340) };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_CONTROLPARENT, dpi);
        let Ok(window) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            &HSTRING::from(t("ai.vorleser_ai_voices")),
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
        child(
            w!("STATIC"),
            t("ai.ai_voices_sound_much"),
            0, 0, 16, 12, 508, 44,
        );
        let list = child(w!("LISTBOX"), "", tab | LBS_NOTIFY | WS_VSCROLL.0 | WS_BORDER.0, ID_LIST, 16, 62, 508, 140);
        let status = child(w!("STATIC"), "", 0, 0, 16, 212, 508, 62);
        let download = child(w!("BUTTON"), t("ai.download"), tab | BS_DEFPUSHBUTTON as u32, ID_DOWNLOAD, 16, 290, 140, 30);
        let remove = child(w!("BUTTON"), t("ai.remove"), tab, ID_REMOVE, 164, 290, 120, 30);
        child(w!("BUTTON"), t("ai.close"), tab, ID_CLOSE, 404, 290, 120, 30);

        let c = Controls { window, list, status, download, remove, font };
        CONTROLS.with(|cell| cell.set(Some(c)));
        ON_CHANGE.with(|cell| cell.set(Some(on_change)));
        fill(&c, Some(0));
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(list));
    }
}

fn megabytes(bytes: u64) -> u64 {
    (bytes + 500_000) / 1_000_000
}

unsafe fn fill(c: &Controls, select: Option<usize>) {
    SendMessageW(c.list, LB_RESETCONTENT, None, None);
    for voice in &CATALOG {
        let description = i18n::lookup(&format!("ai.desc.{}", voice.key)).unwrap_or(voice.description_en);
        let state = if piper::is_installed(voice.key) {
            t("ai.installed").to_string()
        } else {
            format!("{} MB", megabytes(voice.size))
        };
        let line = HSTRING::from(format!("{} – {description} – {state}", voice.name));
        SendMessageW(c.list, LB_ADDSTRING, None, Some(LPARAM(line.as_ptr() as isize)));
    }
    if let Some(row) = select {
        SendMessageW(c.list, LB_SETCURSEL, Some(WPARAM(row)), None);
    }
    update(c);
}

fn selected(c: &Controls) -> Option<usize> {
    let row = unsafe { SendMessageW(c.list, LB_GETCURSEL, None, None).0 };
    (row >= 0 && (row as usize) < CATALOG.len()).then_some(row as usize)
}

fn set_status(c: &Controls, text: &str) {
    unsafe {
        let _ = SetWindowTextW(c.status, &HSTRING::from(text));
    }
}

/// Schaltflächen und Statuszeile passend zur Auswahl.
fn update(c: &Controls) {
    let loading = LOADING.with(|l| l.get()).is_some();
    let voice = selected(c).map(|i| &CATALOG[i]);
    let installed = voice.is_some_and(|v| piper::is_installed(v.key));
    unsafe {
        let _ = EnableWindow(c.download, !loading && voice.is_some() && !installed);
        let _ = EnableWindow(c.remove, !loading && installed);
    }
    if loading {
        return;
    }
    let Some(voice) = voice else {
        set_status(c, "");
        return;
    };
    let text = if installed {
        t("ai.voice_installed_chosen_vorleser")
        .to_string()
    } else {
        let mut size = megabytes(voice.size);
        let mut note = String::new();
        if !piper::engine_installed() {
            size += megabytes(piper::ENGINE_SIZE);
            note = format!(" {}", t("ai.including_piper_speech_engine"));
        }
        format!(
            "{} {size} MB{note}. {}: {}.",
            t("ai.download_2"),
            t("ai.voice_license"),
            voice.license
        )
    };
    set_status(c, &text);
}

fn start_download(c: &Controls) {
    let Some(index) = selected(c) else { return };
    if BUSY.swap(true, Relaxed) {
        set_status(c, t("ai.previous_download_still_stopping"));
        return;
    }
    LOADING.with(|l| l.set(Some(index)));
    CANCEL.store(false, Relaxed);
    update(c);
    set_status(c, t("ai.connecting"));
    let target = c.window.0 as isize;
    std::thread::spawn(move || {
        let post = |msg: u32, wparam: usize, lparam: isize| unsafe {
            let _ = PostMessageW(Some(HWND(target as *mut _)), msg, WPARAM(wparam), LPARAM(lparam));
        };
        let mut last = (2usize, -1isize);
        let result = piper::install(&CATALOG[index], |stage, done, total| {
            let stage = (stage == Stage::Voice) as usize;
            let percent = if total > 0 { (done * 100 / total) as isize } else { 0 };
            if (stage, percent) != last {
                last = (stage, percent);
                post(WM_PROGRESS, stage, percent);
            }
            !CANCEL.load(Relaxed)
        });
        if let Err(e) = &result {
            *ERROR.lock().unwrap() = e.clone();
        }
        BUSY.store(false, Relaxed);
        post(WM_DONE, result.is_ok() as usize, 0);
    });
}

fn on_done(c: &Controls, ok: bool) {
    let index = LOADING.with(|l| l.take());
    if ok {
        let id = index.map(|i| format!("piper:{}", CATALOG[i].key));
        unsafe { fill(c, index) };
        set_status(
            c,
            t("ai.done_voice_now_selected"),
        );
        if let Some(on_change) = ON_CHANGE.with(|cell| cell.get()) {
            on_change(id);
        }
    } else {
        update(c);
        let error = ERROR.lock().unwrap().clone();
        if !CANCEL.load(Relaxed) {
            let text = format!("{} {error}", t("ai.download_failed"));
            set_status(c, &text);
        }
    }
}

fn remove_selected(c: &Controls) {
    let Some(index) = selected(c) else { return };
    let voice = &CATALOG[index];
    let question = HSTRING::from(t("ai.remove_ai_voice"));
    unsafe {
        if MessageBoxW(Some(c.window), &question, w!("Vorleser"), MB_YESNO | MB_ICONQUESTION) != IDYES {
            return;
        }
    }
    piper::remove(voice.key);
    unsafe { fill(c, Some(index)) };
    if let Some(on_change) = ON_CHANGE.with(|cell| cell.get()) {
        on_change(None);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let controls = CONTROLS.with(|c| c.get());
    unsafe {
        match (msg, controls) {
            (WM_COMMAND, Some(c)) => {
                let (id, code) = ((wparam.0 & 0xffff) as u32, (wparam.0 >> 16) & 0xffff);
                match id {
                    ID_LIST if code == LBN_SELCHANGE => update(&c),
                    ID_DOWNLOAD => start_download(&c),
                    ID_REMOVE => remove_selected(&c),
                    ID_CLOSE => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            (WM_PROGRESS, Some(c)) => {
                let what = if wparam.0 == 0 { t("ai.downloading_piper_speech_engine") } else { t("ai.downloading_voice") };
                set_status(&c, &format!("{what} … {} %", lparam.0));
            }
            (WM_DONE, Some(c)) => on_done(&c, wparam.0 == 1),
            (WM_CLOSE, _) => {
                let _ = DestroyWindow(hwnd);
            }
            (WM_DESTROY, _) => {
                // Ein laufender Download bricht ab; der Thread räumt die halbe Datei selbst weg.
                CANCEL.store(true, Relaxed);
                LOADING.with(|l| l.set(None));
                if let Some(c) = CONTROLS.with(|cell| cell.take()) {
                    let _ = DeleteObject(c.font.into());
                }
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}
