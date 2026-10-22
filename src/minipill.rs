//! Mini-Pille: ein kleiner Strich unten am Bildschirm, immer im Vordergrund, solange das
//! Vorleser-Fenster zu ist (wenn so eingestellt). Unter der Maus wird sie größer und zeigt zwei
//! Knöpfe: Mikrofon (in das Programm im Vordergrund diktieren) und den sprechenden Kopf (den
//! dort markierten Text vorlesen). Darüber steht ein Hinweis mit den Tastenkombinationen.
//! Sie wandert mit auf den Bildschirm, auf dem gerade gearbeitet wird, und lässt sich mit der
//! Maus verschieben; dann bleibt sie an der gewählten Stelle.
//! Rechtsklick: Fenster öffnen, Position zurücksetzen oder die Pille ausblenden.

use std::cell::RefCell;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONULL};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, GetCursorPos,
    GetWindowRect, LoadCursorW, RegisterClassW, SetForegroundWindow, SetWindowPos, ShowWindow, TrackPopupMenu,
    HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, MF_STRING, SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN,
    KillTimer, SetTimer, TPM_NONOTIFY, TPM_RETURNCMD, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW,
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
    /// Die Pille wurde an diese Stelle (Pixel, linke obere Ecke des Fensters) gezogen.
    Moved(i32, i32),
    /// Zurück unten in die Mitte, wieder mitwandernd.
    ResetPosition,
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
    /// Selbst gewählte Stelle; ohne sie wandert die Pille mit.
    custom: Option<(i32, i32)>,
    /// Beim Ziehen: Mauspunkt und Fensterecke beim Drücken, und ob schon bewegt wurde.
    drag: Option<(POINT, POINT, bool)>,
}

thread_local! {
    static PILL: RefCell<Option<Pill>> = const { RefCell::new(None) };
}

const WIDTH: f32 = 380.0;
const HEIGHT: f32 = 124.0;
/// Eingeklappt: nur ein Strich.
const SMALL_W: f32 = 64.0;
const SMALL_H: f32 = 14.0;
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

/// Liegt eine gespeicherte Stelle noch auf einem Bildschirm? (Bildschirme können wegfallen.)
fn on_screen(hwnd: HWND, (x, y): (i32, i32)) -> bool {
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    let center = POINT { x: x + (WIDTH * scale / 2.0) as i32, y: y + ((HEIGHT - 20.0) * scale) as i32 };
    !unsafe { MonitorFromPoint(center, MONITOR_DEFAULTTONULL) }.is_invalid()
}

/// Wohin die Pille gehört: an die gewählte Stelle, sonst unten mittig auf den Bildschirm, auf
/// dem gerade gearbeitet wird.
fn target(hwnd: HWND, custom: Option<(i32, i32)>) -> Option<(i32, i32)> {
    custom.filter(|pos| on_screen(hwnd, *pos)).or_else(|| crate::dictbar::bottom_center(hwnd, WIDTH, HEIGHT))
}

fn move_to(hwnd: HWND, (x, y): (i32, i32)) {
    let mut rect = RECT::default();
    unsafe {
        let _ = GetWindowRect(hwnd, &mut rect);
        if (rect.left, rect.top) != (x, y) {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

/// Zeigt die Pille (eingeklappt). `hint` steht beim Aufklappen darüber; `position` ist eine
/// selbst gewählte Stelle (sonst wandert sie mit).
pub fn show(hint: &str, position: Option<(i32, i32)>, on_action: fn(Action)) {
    let existing = PILL.with(|p| {
        let mut p = p.borrow_mut();
        let pill = p.as_mut()?;
        pill.hint = hint.to_string();
        let changed = pill.custom != position;
        pill.custom = position;
        Some((pill.hwnd, changed))
    });
    if let Some((hwnd, changed)) = existing {
        if changed {
            if let Some(pos) = target(hwnd, position) {
                move_to(hwnd, pos);
                repaint();
            }
        }
        return;
    }
    let Some(hwnd) = create() else { return };
    PILL.with(|p| {
        *p.borrow_mut() = Some(Pill { hwnd, expanded: false, hover: None, hint: hint.to_string(), on_action, custom: position, drag: None })
    });
    if let Some(pos) = target(hwnd, position) {
        move_to(hwnd, pos);
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        // Einmal je Sekunde: auf den Bildschirm wechseln, auf dem gearbeitet wird.
        SetTimer(Some(hwnd), 2, 1000, None);
    }
    repaint();
}

pub fn hide() {
    if let Some(hwnd) = PILL.with(|p| p.borrow_mut().take().map(|pill| pill.hwnd)) {
        unsafe {
            let _ = KillTimer(Some(hwnd), 2);
            let _ = DestroyWindow(hwnd);
        }
    }
}

/// Mitwandern: nur ohne eigene Stelle, und nicht, während die Maus darauf ist.
fn follow() {
    let Some((hwnd, busy)) = PILL.with(|p| p.borrow().as_ref().map(|pill| (pill.hwnd, pill.custom.is_some() || pill.expanded || pill.drag.is_some()))) else {
        return;
    };
    if busy {
        return;
    }
    if let Some(pos) = crate::dictbar::bottom_center(hwnd, WIDTH, HEIGHT) {
        move_to(hwnd, pos);
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
            w!("Hedgie"),
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
        // Auf jedem Hintergrund gut zu sehen: heller Rand und ein Punkt in Akzentfarbe.
        c.shadow(x, y, w, h, h / 2.0, 4.0, 0.35);
        c.fill_round(x, y, w, h, h / 2.0, p.ground);
        c.stroke_round(x, y, w, h, h / 2.0, paint::rgb(0xC9CDD3).alpha(0.9), 1.2);
        c.circle(x + w / 2.0, y + h / 2.0, 2.6, p.accent);
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
    let custom = PILL.with(|p| p.borrow().as_ref().is_some_and(|pill| pill.custom.is_some()));
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        let _ = AppendMenuW(menu, MF_STRING, 1, &HSTRING::from(t("pill.open")));
        if custom {
            let _ = AppendMenuW(menu, MF_STRING, 3, &HSTRING::from(t("pill.reset_position")));
        }
        let _ = AppendMenuW(menu, MF_STRING, 2, &HSTRING::from(t("pill.hide")));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN, pt.x, pt.y, None, hwnd, None);
        let _ = DestroyMenu(menu);
        match chosen.0 {
            1 => Some(Action::OpenWindow),
            2 => Some(Action::Hide),
            3 => Some(Action::ResetPosition),
            _ => None,
        }
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_LBUTTONDOWN => {
            // Ziehen beginnt; ob es ein Klick war, zeigt sich beim Loslassen.
            let mut cursor = POINT::default();
            let mut rect = RECT::default();
            unsafe {
                let _ = GetCursorPos(&mut cursor);
                let _ = GetWindowRect(hwnd, &mut rect);
                SetCapture(hwnd);
            }
            PILL.with(|p| {
                if let Some(pill) = p.borrow_mut().as_mut() {
                    pill.drag = Some((cursor, POINT { x: rect.left, y: rect.top }, false));
                }
            });
            LRESULT(0)
        }
        WM_MOUSEMOVE if PILL.with(|p| p.borrow().as_ref().is_some_and(|pill| pill.drag.is_some())) => {
            let mut cursor = POINT::default();
            unsafe {
                let _ = GetCursorPos(&mut cursor);
            }
            let target = PILL.with(|p| {
                let mut p = p.borrow_mut();
                let pill = p.as_mut()?;
                let (start, origin, moved) = pill.drag.as_mut()?;
                let (dx, dy) = (cursor.x - start.x, cursor.y - start.y);
                if !*moved && dx.abs() + dy.abs() < 5 {
                    return None;
                }
                *moved = true;
                Some((origin.x + dx, origin.y + dy))
            });
            if let Some(pos) = target {
                move_to(hwnd, pos);
            }
            LRESULT(0)
        }
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
            unsafe {
                let _ = ReleaseCapture();
            }
            let info = PILL.with(|p| {
                let mut p = p.borrow_mut();
                p.as_mut().map(|pill| (pill.on_action, pill.expanded, pill.drag.take().is_some_and(|(_, _, moved)| moved)))
            });
            if let Some((on_action, expanded, moved)) = info {
                if moved {
                    let mut rect = RECT::default();
                    unsafe {
                        let _ = GetWindowRect(hwnd, &mut rect);
                    }
                    PILL.with(|p| {
                        if let Some(pill) = p.borrow_mut().as_mut() {
                            pill.custom = Some((rect.left, rect.top));
                        }
                    });
                    on_action(Action::Moved(rect.left, rect.top));
                } else {
                    match hit(expanded, x, y) {
                        Some(Hit::Dictate) => on_action(Action::Dictate),
                        Some(Hit::Read) => on_action(Action::Read),
                        _ => {}
                    }
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            follow();
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
