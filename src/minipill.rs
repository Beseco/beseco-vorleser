//! Mini-Pille: ein kleiner Strich unten am Bildschirm, immer im Vordergrund, solange das
//! Vorleser-Fenster zu ist (wenn so eingestellt). Unter der Maus wird sie größer und zeigt zwei
//! Knöpfe: Mikrofon (in das Programm im Vordergrund diktieren) und den sprechenden Kopf (den
//! dort markierten Text vorlesen). Darüber steht ein Hinweis mit den Tastenkombinationen.
//! Rechtsklick: Fenster öffnen oder die Pille ausblenden.

use std::cell::RefCell;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos,
    GetWindowRect, LoadCursorW, RegisterClassW, SetForegroundWindow, SetWindowPos, ShowWindow, TrackPopupMenu,
    HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, MF_STRING, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN,
    TPM_NONOTIFY, TPM_RETURNCMD, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_RBUTTONUP, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::i18n::t;
use crate::paint::{self, Align, Canvas, Icon};
use crate::theme;

const WM_MOUSELEAVE: u32 = 0x02A3;

/// Klick in der Mini-Pille.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Dictate,
    Read,
    OpenWindow,
    /// Pille ausblenden (Einstellung „Beim Schließen …“ zurück auf den Infobereich).
    Hide,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Dictate,
    Read,
    Body,
}

struct Pill {
    hwnd: HWND,
    expanded: bool,
    hover: Option<Hit>,
    /// Hinweis mit den Tastenkombinationen.
    hint: String,
    on_action: fn(Action),
}

thread_local! {
    static PILL: RefCell<Option<Pill>> = const { RefCell::new(None) };
}

const WIDTH: f32 = 380.0;
const HEIGHT: f32 = 124.0;
/// Eingeklappt: nur ein Strich.
const SMALL_W: f32 = 46.0;
const SMALL_H: f32 = 10.0;
/// Aufgeklappt: zwei runde Knöpfe.
const BIG_W: f32 = 104.0;
const BIG_H: f32 = 44.0;

fn body(expanded: bool) -> (f32, f32, f32, f32) {
    let (w, h) = if expanded { (BIG_W, BIG_H) } else { (SMALL_W, SMALL_H) };
    // Unten bündig mit der aufgeklappten Pille, damit sie beim Aufklappen nach oben wächst.
    ((WIDTH - w) / 2.0, HEIGHT - 12.0 - h, w, h)
}

fn buttons() -> [(Hit, f32, f32, f32); 2] {
    let (x, y, w, h) = body(true);
    let cy = y + h / 2.0;
    [(Hit::Dictate, x + w / 2.0 - 24.0, cy, 17.0), (Hit::Read, x + w / 2.0 + 24.0, cy, 17.0)]
}

fn hit(expanded: bool, x: f32, y: f32) -> Option<Hit> {
    if expanded {
        if let Some((hit, ..)) = buttons().iter().find(|(_, cx, cy, r)| (x - cx).powi(2) + (y - cy).powi(2) <= r * r) {
            return Some(*hit);
        }
    }
    // Eingeklappt etwas großzügiger, damit der schmale Strich leicht zu treffen ist.
    let (bx, by, bw, bh) = body(expanded);
    let grow = if expanded { 0.0 } else { 8.0 };
    (x >= bx - grow && x <= bx + bw + grow && y >= by - grow && y <= by + bh + grow).then_some(Hit::Body)
}

/// Zeigt die Pille (eingeklappt). `hint` steht beim Aufklappen darüber.
pub fn show(hint: &str, on_action: fn(Action)) {
    if PILL.with(|p| p.borrow().is_some()) {
        PILL.with(|p| {
            if let Some(pill) = p.borrow_mut().as_mut() {
                pill.hint = hint.to_string();
            }
        });
        return;
    }
    let Some(hwnd) = create() else { return };
    PILL.with(|p| *p.borrow_mut() = Some(Pill { hwnd, expanded: false, hover: None, hint: hint.to_string(), on_action }));
    if let Some((x, y)) = crate::dictbar::bottom_center(hwnd, WIDTH, HEIGHT) {
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
    }
    repaint();
}

pub fn hide() {
    if let Some(hwnd) = PILL.with(|p| p.borrow_mut().take().map(|pill| pill.hwnd)) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

fn create() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserMiniPille"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.lpszClassName,
            w!("Vorleser"),
            WS_POPUP,
            0,
            0,
            10,
            10,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()
    }
}

fn repaint() {
    PILL.with(|p| {
        let p = p.borrow();
        let Some(pill) = p.as_ref() else { return };
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(pill.hwnd, &mut rect);
        }
        let dpi = unsafe { GetDpiForWindow(pill.hwnd) }.max(96);
        paint::draw_layered(pill.hwnd, rect.left, rect.top, WIDTH, HEIGHT, dpi, |c| draw(c, pill));
    });
}

fn draw(c: &Canvas, pill: &Pill) {
    let p = theme::glass();
    let (x, y, w, h) = body(pill.expanded);
    if !pill.expanded {
        c.shadow(x, y, w, h, h / 2.0, 4.0, 0.3);
        c.fill_round(x, y, w, h, h / 2.0, p.ground);
        c.stroke_round(x, y, w, h, h / 2.0, p.border.alpha(0.9), 1.0);
        return;
    }
    // Hinweis darüber: was der Knopf unter der Maus tut, sonst die Tastenkombinationen.
    let hint = match pill.hover {
        Some(Hit::Dictate) => t("pill.dictate"),
        Some(Hit::Read) => t("pill.read"),
        _ => pill.hint.as_str(),
    };
    let hw = (paint::measure(hint, 13.0, true) + 32.0).min(WIDTH - 12.0);
    let (hx, hy, hh) = ((WIDTH - hw) / 2.0, y - 10.0 - 38.0, 38.0);
    c.shadow(hx, hy, hw, hh, hh / 2.0, 6.0, 0.25);
    c.fill_round(hx, hy, hw, hh, hh / 2.0, p.ground);
    c.stroke_round(hx, hy, hw, hh, hh / 2.0, p.border.alpha(0.6), 1.0);
    c.text(hint, hx + 16.0, hy, hw - 32.0, hh, 13.0, true, p.text, Align::Center);

    c.shadow(x, y, w, h, h / 2.0, 8.0, 0.35);
    c.fill_round(x, y, w, h, h / 2.0, p.ground);
    c.stroke_round(x, y, w, h, h / 2.0, p.border.alpha(0.7), 1.0);
    for (hit, cx, cy, r) in buttons() {
        let hover = pill.hover == Some(hit);
        let fill = match (hit, hover) {
            (Hit::Dictate, true) => p.record,
            (_, true) => p.accent,
            _ => p.surface,
        };
        c.circle(cx, cy, r, fill);
        let icon = if hit == Hit::Dictate { Icon::Mic } else { Icon::Speak };
        let color = if hover && hit == Hit::Read { p.on_accent } else { paint::rgb(0xFFFFFF) };
        c.icon(icon, cx, cy, 19.0, color);
    }
}

fn local_point(hwnd: HWND, lparam: LPARAM) -> (f32, f32) {
    let pt = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    (pt.x as f32 / scale, pt.y as f32 / scale)
}

/// Kleines Menü per Rechtsklick.
fn context_menu(hwnd: HWND) -> Option<Action> {
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let _ = AppendMenuW(menu, MF_STRING, 1, &HSTRING::from(t("pill.open")));
        let _ = AppendMenuW(menu, MF_STRING, 2, &HSTRING::from(t("pill.hide")));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        match chosen.0 {
            1 => Some(Action::OpenWindow),
            2 => Some(Action::Hide),
            _ => None,
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = local_point(hwnd, lparam);
            let changed = PILL.with(|p| {
                let mut p = p.borrow_mut();
                let Some(pill) = p.as_mut() else { return false };
                let hover = hit(pill.expanded || hit(false, x, y).is_some(), x, y);
                let expanded = hover.is_some();
                let changed = pill.hover != hover || pill.expanded != expanded;
                pill.hover = hover;
                pill.expanded = expanded;
                changed
            });
            unsafe {
                let mut track = TRACKMOUSEEVENT { cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                let _ = TrackMouseEvent(&mut track);
            }
            if changed {
                repaint();
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            PILL.with(|p| {
                if let Some(pill) = p.borrow_mut().as_mut() {
                    pill.hover = None;
                    pill.expanded = false;
                }
            });
            repaint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = local_point(hwnd, lparam);
            let info = PILL.with(|p| p.borrow().as_ref().map(|pill| (pill.on_action, pill.expanded)));
            if let Some((on_action, expanded)) = info {
                match hit(expanded, x, y) {
                    Some(Hit::Dictate) => on_action(Action::Dictate),
                    Some(Hit::Read) => on_action(Action::Read),
                    _ => {}
                }
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            let on_action = PILL.with(|p| p.borrow().as_ref().map(|pill| pill.on_action));
            if let (Some(on_action), Some(action)) = (on_action, context_menu(hwnd)) {
                on_action(action);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
