//! Fenster „KI-Einstellungen“ des Officeassistenten: KI-Check dieses Rechners, Wahl des
//! Anbieters, Adresse, Schlüssel, Modell und Verbindungstest.

use std::cell::{Cell, RefCell};
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
    BS_DEFPUSHBUTTON, CBN_SELCHANGE, CBS_DROPDOWN, CBS_DROPDOWNLIST, CB_ADDSTRING, CB_GETCURSEL,
    CB_RESETCONTENT, CB_SETCURSEL, CW_USEDEFAULT, ES_AUTOHSCROLL, ES_PASSWORD, HMENU, IDC_ARROW, SW_SHOW,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLOSE, WM_COMMAND, WM_DESTROY, WM_SETFONT, WNDCLASSW, WS_CAPTION,
    WS_CHILD, WS_EX_CLIENTEDGE, WS_EX_CONTROLPARENT, WS_OVERLAPPED, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

use crate::ai::{Kind, Provider};
use crate::i18n::t;
use crate::window::message_font;

const ID_SAVE: u32 = 1;
const ID_CANCEL: u32 = 2;
const ID_KIND: u32 = 10;
const ID_MODELS: u32 = 11;
const ID_TEST: u32 = 12;
const COLOR_BTNFACE: usize = 15;
const EM_SETCUEBANNER: u32 = 0x1501;
const CB_SETEDITSEL: u32 = 0x0142;

/// Ergebnis eines Hintergrundauftrags: `wparam` 0 = Modelle, 1 = Test.
const WM_RESULT: u32 = WM_APP + 1;

#[derive(Clone, Copy)]
struct Controls {
    window: HWND,
    kind: HWND,
    url: HWND,
    key: HWND,
    model: HWND,
    models: HWND,
    test: HWND,
    status: HWND,
    font: HFONT,
}

thread_local! {
    static CONTROLS: Cell<Option<Controls>> = const { Cell::new(None) };
    static ON_SAVE: Cell<Option<fn(Provider)>> = const { Cell::new(None) };
    static LAST_KIND: Cell<usize> = const { Cell::new(0) };
    static ORIGINAL: RefCell<Option<Provider>> = const { RefCell::new(None) };
}

static MODELS: Mutex<Option<Result<Vec<String>, String>>> = Mutex::new(None);
static TEST: Mutex<Option<Result<(u128, String), String>>> = Mutex::new(None);

pub fn handle() -> Option<HWND> {
    CONTROLS.with(|c| c.get()).map(|c| c.window)
}

fn kind_label(kind: Kind) -> &'static str {
    match kind {
        Kind::None => t("aiset.kind_none"),
        Kind::Ollama => t("aiset.kind_ollama"),
        Kind::OpenAi => t("aiset.kind_openai"),
    }
}

/// Öffnet das Fenster; `on_save` erhält den eingestellten Anbieter (Schlüssel im Klartext).
pub fn open(owner: Option<HWND>, provider: Provider, report: String, on_save: fn(Provider)) {
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
            lpszClassName: w!("VorleserKiEinstellungen"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            hIcon: crate::tray::load_icon(),
            hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut _),
            ..Default::default()
        };
        RegisterClassW(&class);
        let dpi = GetDpiForSystem();
        let px = |value: i32| value * dpi as i32 / 96;
        let style = WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU;
        let mut frame = RECT { left: 0, top: 0, right: px(560), bottom: px(470) };
        let _ = AdjustWindowRectExForDpi(&mut frame, style, false, WS_EX_CONTROLPARENT, dpi);
        let Ok(window) = CreateWindowExW(
            WS_EX_CONTROLPARENT,
            class.lpszClassName,
            &HSTRING::from(t("aiset.title")),
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
        let none = WINDOW_EX_STYLE(0);
        let tab = WS_TABSTOP.0;
        let (lx, fx, lw, fw) = (16, 150, 130, 394);

        child(w!("BUTTON"), t("aiset.this_pc"), 0x7 /* BS_GROUPBOX */, none, 0, 10, 8, 540, 116);
        child(w!("STATIC"), &report, 0, none, 0, 22, 30, 520, 90);

        child(w!("STATIC"), t("aiset.intro"), 0, none, 0, lx, 136, 528, 40);
        child(w!("STATIC"), t("aiset.kind"), 0, none, 0, lx, 188, lw, 22);
        let kind = child(w!("COMBOBOX"), "", tab | WS_VSCROLL.0 | CBS_DROPDOWNLIST as u32, none, ID_KIND, fx, 184, fw, 200);
        child(w!("STATIC"), t("aiset.url"), 0, none, 0, lx, 224, lw, 22);
        let url = child(w!("EDIT"), &provider.url, tab | ES_AUTOHSCROLL as u32, WS_EX_CLIENTEDGE, 0, fx, 220, fw, 26);
        child(w!("STATIC"), t("aiset.key"), 0, none, 0, lx, 260, lw, 22);
        let key = child(w!("EDIT"), &provider.key, tab | (ES_AUTOHSCROLL | ES_PASSWORD) as u32, WS_EX_CLIENTEDGE, 0, fx, 256, fw, 26);
        child(w!("STATIC"), t("aiset.model"), 0, none, 0, lx, 296, lw, 22);
        let model = child(w!("COMBOBOX"), &provider.model, tab | WS_VSCROLL.0 | CBS_DROPDOWN as u32, none, 0, fx, 292, fw - 128, 260);
        let models = child(w!("BUTTON"), t("aiset.fetch_models"), tab, none, ID_MODELS, fx + fw - 120, 291, 120, 28);
        let test = child(w!("BUTTON"), t("aiset.test"), tab, none, ID_TEST, fx, 332, 180, 30);
        let status = child(w!("STATIC"), "", 0, none, 0, lx, 372, 528, 52);
        child(w!("BUTTON"), t("settings.save"), tab | BS_DEFPUSHBUTTON as u32, none, ID_SAVE, 338, 430, 100, 30);
        child(w!("BUTTON"), t("settings.cancel"), tab, none, ID_CANCEL, 446, 430, 98, 30);

        for k in Kind::ALL {
            let label = HSTRING::from(kind_label(k));
            SendMessageW(kind, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
        }
        let index = Kind::ALL.iter().position(|k| *k == provider.kind).unwrap_or(0);
        SendMessageW(kind, CB_SETCURSEL, Some(WPARAM(index)), None);
        LAST_KIND.with(|l| l.set(index));
        let cue = HSTRING::from(t("aiset.key_hint"));
        SendMessageW(key, EM_SETCUEBANNER, Some(WPARAM(1)), Some(LPARAM(cue.as_ptr() as isize)));

        let c = Controls { window, kind, url, key, model, models, test, status, font };
        CONTROLS.with(|cell| cell.set(Some(c)));
        ON_SAVE.with(|cell| cell.set(Some(on_save)));
        ORIGINAL.with(|cell| *cell.borrow_mut() = Some(provider));
        update_fields(&c);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(kind));
    }
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

fn current(c: &Controls) -> Provider {
    Provider { kind: selected_kind(c), url: text(c.url), key: text(c.key), model: text(c.model) }
}

/// Felder je nach Anbieter freigeben und einen passenden Hinweis zeigen.
fn update_fields(c: &Controls) {
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
    update_fields(c);
}

fn run_in_background(c: &Controls, kind: usize) {
    let provider = current(c);
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
                    for name in &models {
                        let label = HSTRING::from(name.as_str());
                        SendMessageW(c.model, CB_ADDSTRING, None, Some(LPARAM(label.as_ptr() as isize)));
                    }
                    // Das bisherige Modell behalten, sonst das erste vorschlagen.
                    let keep = models.iter().position(|m| *m == chosen);
                    SendMessageW(c.model, CB_SETCURSEL, Some(WPARAM(keep.unwrap_or(0))), None);
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

fn save(c: &Controls) {
    let provider = current(c);
    if provider.kind != Kind::None && (provider.url.is_empty() || provider.model.is_empty()) {
        set_text(c.status, if provider.url.is_empty() { t("aiset.need_url") } else { t("aiset.need_model") });
        return;
    }
    let on_save = ON_SAVE.with(|cell| cell.get());
    unsafe {
        let _ = DestroyWindow(c.window);
    }
    if let Some(on_save) = on_save {
        on_save(provider);
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let controls = CONTROLS.with(|c| c.get());
    unsafe {
        match (msg, controls) {
            (WM_COMMAND, Some(c)) => {
                let (id, code) = ((wparam.0 & 0xffff) as u32, ((wparam.0 >> 16) & 0xffff) as u32);
                match id {
                    ID_KIND if code == CBN_SELCHANGE => on_kind_changed(&c),
                    ID_MODELS => run_in_background(&c, 0),
                    ID_TEST => run_in_background(&c, 1),
                    ID_SAVE => save(&c),
                    ID_CANCEL => {
                        let _ = DestroyWindow(hwnd);
                    }
                    _ => {}
                }
            }
            (WM_RESULT, Some(c)) => on_result(&c, wparam.0),
            (WM_CLOSE, _) => {
                let _ = DestroyWindow(hwnd);
            }
            (WM_DESTROY, _) => {
                if let Some(c) = CONTROLS.with(|cell| cell.take()) {
                    let _ = DeleteObject(c.font.into());
                }
            }
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}
