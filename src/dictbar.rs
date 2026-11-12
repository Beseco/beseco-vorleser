//! Diktier-Pille: eine kleine schwebende Leiste unten am Bildschirm, solange diktiert wird:
//! ✕ (beenden), der Pegel des Mikrofons, Vorlesen (das ganze Diktat), ✓ (fertig, Text bleibt wie
//! erkannt) und, wenn eine KI eingerichtet ist, ✨ (fertig und mit KI korrigieren). Was gerade
//! passiert und wohin der Text geht, steht in einem Hinweis darüber, sobald die Maus auf der
//! Pille ist (und immer, wenn etwas nicht stimmt). Sie nimmt beim Anklicken nicht den Fokus,
//! damit der Text weiter im Zielprogramm landet.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, GetWindowRect, KillTimer, LoadCursorW,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, SWP_NOACTIVATE,
    SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_TIMER, WNDCLASSW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::i18n::t;
use crate::paint::{self, Align, Canvas, Icon};
use crate::theme;

const WM_MOUSELEAVE: u32 = 0x02A3;

/// Was die Pille gerade zeigt.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum State {
    Preparing,
    Listening,
    Silent,
    Writing,
    Correcting,
    /// Erkannter Text wartet, weil das Zielprogramm nicht im Vordergrund ist.
    Waiting,
    Finished,
}

/// Klick in der Pille.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    /// ✓: Diktat beenden, ohne die KI zu fragen (auch nicht automatisch).
    Finish,
    Correct,
    /// Das ganze Diktat vorlesen (läuft es noch, wird es vorher beendet); während des
    /// Vorlesens: aufhören.
    ReadBack,
    Close,
    /// Regelmäßig, solange Text auf das Zielprogramm wartet.
    Poll,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Close,
    ReadBack,
    Done,
    Correct,
    Body,
}

struct Bar {
    hwnd: HWND,
    state: State,
    level: Option<Arc<AtomicU32>>,
    history: VecDeque<f32>,
    target: String,
    can_correct: bool,
    hover: Option<Hit>,
    on_action: fn(Action),
    hide_at: Option<Instant>,
    tick: u32,
}

thread_local! {
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

/// Fenstergröße (geräteunabhängige Pixel): oben Platz für den Hinweis, unten die Pille.
const WIDTH: f32 = 340.0;
const HEIGHT: f32 = 124.0;
const PILL_H: f32 = 40.0;
const BARS: usize = 9;
/// Abstand der Knopfmitten rechts in der Pille.
const BUTTON_GAP: f32 = 36.0;

/// Knöpfe rechts, von außen nach innen: ✨ (nur mit KI), ✓, Vorlesen.
fn right_buttons(can_correct: bool) -> &'static [Hit] {
    if can_correct { &[Hit::Correct, Hit::Done, Hit::ReadBack] } else { &[Hit::Done, Hit::ReadBack] }
}

/// Je Knopf rechts über den ersten hinaus wird die Pille breiter.
fn pill_w(can_correct: bool) -> f32 {
    156.0 + BUTTON_GAP * (right_buttons(can_correct).len() - 1) as f32
}

fn pill(can_correct: bool) -> (f32, f32) {
    ((WIDTH - pill_w(can_correct)) / 2.0, HEIGHT - 12.0 - PILL_H)
}

/// Mitte und Radius eines Knopfes.
fn button(hit: Hit, can_correct: bool) -> Option<(f32, f32, f32)> {
    let (x, y) = pill(can_correct);
    let cy = y + PILL_H / 2.0;
    if hit == Hit::Close {
        return Some((x + 21.0, cy, 15.0));
    }
    let slot = right_buttons(can_correct).iter().position(|h| *h == hit)?;
    Some((x + pill_w(can_correct) - 21.0 - BUTTON_GAP * slot as f32, cy, 15.0))
}

fn hit(x: f32, y: f32, can_correct: bool) -> Option<Hit> {
    let inside = |(cx, cy, r): (f32, f32, f32)| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
    for candidate in [Hit::Close, Hit::ReadBack, Hit::Done, Hit::Correct] {
        if button(candidate, can_correct).is_some_and(inside) {
            return Some(candidate);
        }
    }
    let (px, py) = pill(can_correct);
    (x >= px && x <= px + pill_w(can_correct) && y >= py && y <= py + PILL_H).then_some(Hit::Body)
}

/// Zeigt die Pille (oder aktualisiert sie). `target` ist der Name des Programms, in das
/// geschrieben wird; `can_correct`, ob eine KI eingerichtet ist.
pub fn show(target: &str, can_correct: bool, level: Option<Arc<AtomicU32>>, on_action: fn(Action)) {
    let existing = BAR.with(|b| b.borrow().as_ref().map(|bar| bar.hwnd));
    let hwnd = match existing {
        Some(hwnd) => hwnd,
        None => match create() {
            Some(hwnd) => hwnd,
            None => return,
        },
    };
    BAR.with(|b| {
        *b.borrow_mut() = Some(Bar {
            hwnd,
            state: State::Preparing,
            level,
            history: VecDeque::from(vec![0.0; BARS]),
            target: target.to_string(),
            can_correct,
            hover: None,
            on_action,
            hide_at: None,
            tick: 0,
        });
    });
    if existing.is_none() {
        place(hwnd);
    }
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        SetTimer(Some(hwnd), 1, 50, None);
    }
    repaint();
}

pub fn set_state(state: State) {
    let changed = BAR.with(|b| {
        let mut b = b.borrow_mut();
        let Some(bar) = b.as_mut() else { return false };
        bar.state = state;
        if state == State::Finished {
            bar.level = None;
            bar.hide_at = Some(Instant::now() + Duration::from_millis(1200));
        } else {
            bar.hide_at = None;
        }
        true
    });
    if changed {
        repaint();
    }
}

pub fn hide() {
    if let Some(hwnd) = BAR.with(|b| b.borrow_mut().take().map(|bar| bar.hwnd)) {
        unsafe {
            let _ = KillTimer(Some(hwnd), 1);
            let _ = DestroyWindow(hwnd);
        }
    }
}

pub fn visible() -> bool {
    BAR.with(|b| b.borrow().is_some())
}

fn create() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserDiktatLeiste"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.lpszClassName,
            w!("Hedgie – Diktat"),
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

/// Unten mittig auf dem Bildschirm mit dem Programm im Vordergrund, knapp über der Taskleiste.
pub(crate) fn bottom_center(hwnd: HWND, width: f32, height: f32) -> Option<(i32, i32)> {
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return None;
        }
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let work = info.rcWork;
        let x = (work.left + work.right - (width * scale) as i32) / 2;
        let y = work.bottom - (height * scale) as i32;
        Some((x, y))
    }
}

fn place(hwnd: HWND) {
    if let Some((x, y)) = bottom_center(hwnd, WIDTH, HEIGHT) {
        unsafe {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
        }
    }
}

fn title(state: State) -> &'static str {
    match state {
        State::Preparing => t("dictbar.preparing"),
        State::Listening => t("dictbar.listening"),
        State::Silent => t("dictbar.silent"),
        State::Writing => t("dictbar.writing"),
        State::Correcting => t("dictbar.correcting"),
        State::Waiting => t("dictbar.waiting"),
        State::Finished => t("dictbar.finished"),
    }
}

fn repaint() {
    BAR.with(|b| {
        let b = b.borrow();
        let Some(bar) = b.as_ref() else { return };
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(bar.hwnd, &mut rect);
        }
        let dpi = unsafe { GetDpiForWindow(bar.hwnd) }.max(96);
        paint::draw_layered(bar.hwnd, rect.left, rect.top, WIDTH, HEIGHT, dpi, |c| draw(c, bar));
    });
}

fn draw(c: &Canvas, bar: &Bar) {
    let p = theme::glass();
    let (px, py) = pill(bar.can_correct);
    let pill_w = pill_w(bar.can_correct);
    let cy = py + PILL_H / 2.0;

    // Hinweis über der Pille: unter der Maus, und immer, wenn etwas zu tun ist.
    let important = matches!(bar.state, State::Silent | State::Waiting | State::Preparing);
    if bar.hover.is_some() || important {
        let sub = match bar.state {
            State::Silent => t("dictbar.silent_hint").to_string(),
            State::Waiting => t("dictbar.waiting_hint").replace("{}", &bar.target),
            _ if bar.hover == Some(Hit::Close) => t("dictbar.close_hint").to_string(),
            _ if bar.hover == Some(Hit::ReadBack) => t("dictbar.read_back_hint").to_string(),
            _ if bar.hover == Some(Hit::Correct) => t("dictbar.done_correct_hint").to_string(),
            _ if bar.hover == Some(Hit::Done) && bar.can_correct => t("dictbar.done_plain_hint").to_string(),
            _ if bar.hover == Some(Hit::Done) => t("dictbar.done_hint").to_string(),
            _ => t("dictbar.target").replace("{}", &bar.target),
        };
        let heading = title(bar.state);
        let w = (paint::measure(heading, 13.0, true).max(paint::measure(&sub, 11.5, false)) + 32.0).min(WIDTH - 16.0);
        let (bx, by, bh) = ((WIDTH - w) / 2.0, py - 10.0 - 48.0, 48.0);
        c.shadow(bx, by, w, bh, 12.0, 6.0, 0.25);
        c.fill_round(bx, by, w, bh, 12.0, p.ground);
        c.stroke_round(bx, by, w, bh, 12.0, p.border.alpha(0.6), 1.0);
        let color = if bar.state == State::Silent { p.warn } else { p.text };
        c.text(heading, bx + 16.0, by + 6.0, w - 32.0, 20.0, 13.0, true, color, Align::Center);
        c.text(&sub, bx + 16.0, by + 25.0, w - 32.0, 17.0, 11.5, false, p.muted, Align::Center);
    }

    c.shadow(px, py, pill_w, PILL_H, PILL_H / 2.0, 8.0, 0.35);
    c.fill_round(px, py, pill_w, PILL_H, PILL_H / 2.0, p.ground);
    c.stroke_round(px, py, pill_w, PILL_H, PILL_H / 2.0, p.border.alpha(0.7), 1.0);

    // Knöpfe: ✕ links (beenden); rechts Vorlesen, ✓ (fertig, Text bleibt wie erkannt) und mit KI
    // ganz außen ✨ (fertig und korrigieren). ✓ ist der hervorgehobene Hauptknopf.
    let mut middle_right = px + pill_w;
    for hit in [Hit::Close, Hit::ReadBack, Hit::Done, Hit::Correct] {
        let Some((x, y, r)) = button(hit, bar.can_correct) else { continue };
        let hover = bar.hover == Some(hit);
        let plain = if hover { p.raised } else { p.surface };
        match hit {
            Hit::Close => {
                c.circle(x, y, r, plain);
                c.icon(Icon::Close, x, y, 13.0, p.text);
            }
            Hit::ReadBack => {
                middle_right = x;
                c.circle(x, y, r, plain);
                c.icon(Icon::Speak, x, y, 17.0, p.text);
            }
            Hit::Done => {
                let done = if hover { p.accent.mix(paint::rgb(0xFFFFFF), 0.2) } else { p.accent };
                c.circle(x, y, r, done);
                c.icon(Icon::Check, x, y, 15.0, p.on_accent);
            }
            Hit::Correct => {
                c.circle(x, y, r, plain);
                c.icon(Icon::Sparkle, x, y, 16.0, p.accent);
            }
            Hit::Body => {}
        }
    }

    // Mitte zwischen ✕ und den Knöpfen rechts: Pegel, bei stummem Mikrofon ein Warnzeichen,
    // beim Schreiben laufende Punkte.
    let close_x = button(Hit::Close, bar.can_correct).map_or(px, |(x, ..)| x);
    let middle = (close_x + middle_right) / 2.0;
    match bar.state {
        State::Silent => c.icon(Icon::Warning, middle, cy, 17.0, p.warn),
        State::Listening => {
            for (i, level) in bar.history.iter().enumerate() {
                let height = 3.0 + 18.0 * level;
                let x = middle - (BARS as f32 * 7.0) / 2.0 + i as f32 * 7.0;
                c.fill_round(x, cy - height / 2.0, 3.0, height, 1.5, p.text);
            }
        }
        _ => {
            for i in 0..BARS {
                let x = middle - (BARS as f32 * 7.0) / 2.0 + i as f32 * 7.0 + 1.5;
                let lit = matches!(bar.state, State::Writing | State::Correcting | State::Preparing) && (bar.tick as usize / 3) % BARS == i;
                c.circle(x, cy, if lit { 2.4 } else { 1.6 }, if lit { p.accent } else { p.muted });
            }
        }
    }
}

fn local_point(hwnd: HWND, lparam: LPARAM) -> (f32, f32) {
    let pt = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
    let scale = unsafe { GetDpiForWindow(hwnd) }.max(96) as f32 / 96.0;
    (pt.x as f32 / scale, pt.y as f32 / scale)
}

fn on_timer() {
    let mut close = None;
    let mut poll = None;
    BAR.with(|b| {
        let mut b = b.borrow_mut();
        let Some(bar) = b.as_mut() else { return };
        bar.tick = bar.tick.wrapping_add(1);
        if let Some(level) = &bar.level {
            // Wurzel, damit auch leise Sprache sichtbar ausschlägt.
            let value = (level.load(Ordering::Relaxed) as f32 / 2500.0).sqrt().min(1.0);
            bar.history.pop_front();
            bar.history.push_back(value);
        }
        if bar.hide_at.is_some_and(|at| Instant::now() >= at) && bar.hover.is_none() {
            close = Some(bar.on_action);
        }
        if bar.state == State::Waiting {
            poll = Some(bar.on_action);
        }
    });
    if let Some(on_action) = poll {
        on_action(Action::Poll);
    }
    if let Some(on_action) = close {
        on_action(Action::Close);
    } else {
        repaint();
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_MOUSEMOVE => {
            let (x, y) = local_point(hwnd, lparam);
            let can_correct = BAR.with(|b| b.borrow().as_ref().is_some_and(|bar| bar.can_correct));
            let hover = hit(x, y, can_correct);
            let changed = BAR.with(|b| b.borrow_mut().as_mut().is_some_and(|bar| std::mem::replace(&mut bar.hover, hover) != hover));
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
            BAR.with(|b| {
                if let Some(bar) = b.borrow_mut().as_mut() {
                    bar.hover = None;
                }
            });
            repaint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = local_point(hwnd, lparam);
            let info = BAR.with(|b| b.borrow().as_ref().map(|bar| (bar.on_action, bar.can_correct, bar.state)));
            if let Some((on_action, can_correct, state)) = info {
                let action = match hit(x, y, can_correct) {
                    Some(Hit::Close) => Some(Action::Close),
                    // ✓: nur beenden, der Text bleibt wie erkannt.
                    Some(Hit::Done) if state != State::Finished => Some(Action::Finish),
                    // ✨: beenden und mit KI korrigieren; geht auch kurz nach ✓ noch.
                    Some(Hit::Correct) if state != State::Correcting => Some(Action::Correct),
                    Some(Hit::ReadBack) => Some(Action::ReadBack),
                    _ => None,
                };
                if let Some(action) = action {
                    on_action(action);
                }
            }
            LRESULT(0)
        }
        WM_TIMER => {
            on_timer();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}
