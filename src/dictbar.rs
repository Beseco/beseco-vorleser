//! Diktier-Pille: eine kleine schwebende Leiste unten am Bildschirm, solange diktiert wird.
//! Sie zeigt den Pegel des Mikrofons, was gerade passiert und wohin der Text geht, und nimmt
//! beim Anklicken nicht den Fokus, damit der Text weiter im Zielprogramm landet.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, ScreenToClient, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetForegroundWindow, GetWindowRect, KillTimer, LoadCursorW,
    RegisterClassW, SetTimer, SetWindowPos, ShowWindow, HTCAPTION, HTCLIENT, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE,
    SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST,
    WM_TIMER, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
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
    /// Diktat beenden bzw. neu starten.
    Toggle,
    Correct,
    Close,
    /// Regelmäßig, solange Text auf das Zielprogramm wartet.
    Poll,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Record,
    Correct,
    Close,
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
}

thread_local! {
    static BAR: RefCell<Option<Bar>> = const { RefCell::new(None) };
}

/// Rand für den Schatten rund um die Pille.
const MARGIN: f32 = 16.0;
const HEIGHT: f32 = 60.0;
const BARS: usize = 9;

/// Lage der Bedienelemente (geräteunabhängige Pixel, inklusive Rand).
struct Layout {
    width: f32,
    record: (f32, f32, f32),
    wave_x: f32,
    text_x: f32,
    text_w: f32,
    correct: Option<(f32, f32, f32, f32)>,
    close: (f32, f32, f32),
}

fn layout(can_correct: bool) -> Layout {
    let cy = MARGIN + HEIGHT / 2.0;
    let record = (MARGIN + 31.0, cy, 21.0);
    let wave_x = MARGIN + 64.0;
    let text_x = wave_x + 84.0;
    let text_w = 196.0;
    let mut x = text_x + text_w + 8.0;
    let correct_w = paint::measure(t("dictbar.correct"), 13.0, false) + 44.0;
    let correct = can_correct.then(|| {
        let rect = (x, cy - 18.0, correct_w, 36.0);
        x += correct_w + 8.0;
        rect
    });
    let close = (x + 18.0, cy, 18.0);
    Layout { width: x + 36.0 + 10.0 + MARGIN, record, wave_x, text_x, text_w, correct, close }
}

fn hit(layout: &Layout, x: f32, y: f32) -> Option<Hit> {
    let in_circle = |(cx, cy, r): (f32, f32, f32)| (x - cx).powi(2) + (y - cy).powi(2) <= r * r;
    if in_circle(layout.record) {
        return Some(Hit::Record);
    }
    if in_circle(layout.close) {
        return Some(Hit::Close);
    }
    if let Some((rx, ry, rw, rh)) = layout.correct {
        if x >= rx && x <= rx + rw && y >= ry && y <= ry + rh {
            return Some(Hit::Correct);
        }
    }
    None
}

fn running(state: State) -> bool {
    !matches!(state, State::Finished)
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
    let fresh = existing.is_none();
    BAR.with(|b| {
        let mut b = b.borrow_mut();
        *b = Some(Bar {
            hwnd,
            state: State::Preparing,
            level,
            history: VecDeque::from(vec![0.0; BARS]),
            target: target.to_string(),
            can_correct,
            hover: None,
            on_action,
            hide_at: None,
        });
    });
    if fresh {
        place(hwnd, can_correct);
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
            bar.hide_at = Some(Instant::now() + Duration::from_secs(8));
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
            w!("Vorleser – Diktat"),
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

/// Unten mittig auf dem Bildschirm mit dem Programm im Vordergrund.
fn place(hwnd: HWND, can_correct: bool) {
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let work = info.rcWork;
        let width = (layout(can_correct).width * scale) as i32;
        let height = ((HEIGHT + 2.0 * MARGIN) * scale) as i32;
        let x = (work.left + work.right - width) / 2;
        let y = work.bottom - height - (12.0 * scale) as i32;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
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
        let l = layout(bar.can_correct);
        paint::draw_layered(bar.hwnd, rect.left, rect.top, l.width, HEIGHT + 2.0 * MARGIN, dpi, |c| draw(c, bar, &l));
    });
}

fn draw(c: &Canvas, bar: &Bar, l: &Layout) {
    let p = theme::glass();
    let body_w = l.width - 2.0 * MARGIN;
    c.shadow(MARGIN, MARGIN, body_w, HEIGHT, HEIGHT / 2.0, 10.0, 0.35);
    c.fill_round(MARGIN, MARGIN, body_w, HEIGHT, HEIGHT / 2.0, p.ground);
    c.stroke_round(MARGIN, MARGIN, body_w, HEIGHT, HEIGHT / 2.0, p.border.alpha(0.6), 1.0);

    // Aufnahmeknopf: läuft das Diktat, beendet er es (unter der Maus als Stopp-Symbol).
    let (cx, cy, r) = l.record;
    let hover = bar.hover == Some(Hit::Record);
    if running(bar.state) {
        let color = if hover { p.record.mix(paint::rgb(0xFFFFFF), 0.15) } else { p.record };
        c.circle(cx, cy, r, color);
        c.icon(if hover { Icon::Stop } else { Icon::Mic }, cx, cy, 20.0, paint::rgb(0xFFFFFF));
    } else {
        c.circle(cx, cy, r, if hover { p.raised } else { p.surface });
        c.icon(Icon::Mic, cx, cy, 20.0, p.text);
    }

    // Pegel, bei stummem Mikrofon ein Warnzeichen.
    let wave_y = MARGIN + HEIGHT / 2.0;
    if bar.state == State::Silent {
        c.circle(l.wave_x + 37.0, wave_y, 17.0, p.warn.alpha(0.18));
        c.icon(Icon::Warning, l.wave_x + 37.0, wave_y, 18.0, p.warn);
    } else {
        for (i, level) in bar.history.iter().enumerate() {
            let height = 4.0 + 20.0 * level;
            let x = l.wave_x + i as f32 * 8.0;
            let color = if bar.state == State::Listening { p.accent } else { p.muted.alpha(0.6) };
            c.fill_round(x, wave_y - height / 2.0, 4.0, height, 2.0, color);
        }
    }

    c.text(title(bar.state), l.text_x, MARGIN + 11.0, l.text_w, 20.0, 14.0, true, p.text, Align::Left);
    let sub = if bar.state == State::Silent {
        t("dictbar.silent_hint").to_string()
    } else if bar.state == State::Waiting {
        t("dictbar.waiting_hint").replace("{}", &bar.target)
    } else {
        t("dictbar.target").replace("{}", &bar.target)
    };
    c.text(&sub, l.text_x, MARGIN + 31.0, l.text_w, 17.0, 11.5, false, p.muted, Align::Left);

    if let Some((x, y, w, h)) = l.correct {
        let enabled = matches!(bar.state, State::Listening | State::Silent | State::Writing | State::Finished);
        if bar.hover == Some(Hit::Correct) && enabled {
            c.fill_round(x, y, w, h, h / 2.0, p.raised);
        }
        c.stroke_round(x, y, w, h, h / 2.0, p.border, 1.0);
        let color = if enabled { p.text } else { p.muted.alpha(0.6) };
        c.icon(Icon::Check, x + 20.0, y + h / 2.0, 15.0, color);
        c.text(t("dictbar.correct"), x + 32.0, y, w - 36.0, h, 13.0, false, color, Align::Left);
    }

    let (cx, cy, r) = l.close;
    if bar.hover == Some(Hit::Close) {
        c.circle(cx, cy, r, p.raised);
    }
    c.icon(Icon::Close, cx, cy, 14.0, p.muted.mix(paint::rgb(0xFFFFFF), 0.4));
}

/// Mauspunkt in geräteunabhängigen Pixeln relativ zur Pille.
fn local_point(hwnd: HWND, lparam: LPARAM, screen: bool) -> (f32, f32) {
    let mut pt = POINT { x: (lparam.0 & 0xffff) as i16 as i32, y: ((lparam.0 >> 16) & 0xffff) as i16 as i32 };
    unsafe {
        if screen {
            let _ = ScreenToClient(hwnd, &mut pt);
        }
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        (pt.x as f32 / scale, pt.y as f32 / scale)
    }
}

fn hit_at(hwnd: HWND, lparam: LPARAM, screen: bool) -> Option<Hit> {
    let (x, y) = local_point(hwnd, lparam, screen);
    let can_correct = BAR.with(|b| b.borrow().as_ref().is_some_and(|bar| bar.can_correct));
    hit(&layout(can_correct), x, y)
}

fn on_timer() {
    let mut close = false;
    let mut poll = None;
    BAR.with(|b| {
        let mut b = b.borrow_mut();
        let Some(bar) = b.as_mut() else { return };
        if let Some(level) = &bar.level {
            // Wurzel, damit auch leise Sprache sichtbar ausschlägt.
            let value = (level.load(Ordering::Relaxed) as f32 / 2500.0).sqrt().min(1.0);
            bar.history.pop_front();
            bar.history.push_back(value);
        } else if bar.history.iter().any(|v| *v > 0.0) {
            bar.history.iter_mut().for_each(|v| *v *= 0.6);
        }
        close = bar.hide_at.is_some_and(|at| Instant::now() >= at) && bar.hover.is_none();
        if bar.state == State::Waiting {
            poll = Some(bar.on_action);
        }
    });
    if let Some(on_action) = poll {
        on_action(Action::Poll);
    }
    if close {
        hide();
    } else {
        repaint();
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // Außerhalb der Knöpfe lässt sich die Pille verschieben.
        WM_NCHITTEST => LRESULT(if hit_at(hwnd, lparam, true).is_some() { HTCLIENT } else { HTCAPTION } as isize),
        WM_MOUSEMOVE => {
            let hover = hit_at(hwnd, lparam, false);
            let changed = BAR.with(|b| {
                let mut b = b.borrow_mut();
                b.as_mut().is_some_and(|bar| std::mem::replace(&mut bar.hover, hover) != hover)
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
            BAR.with(|b| {
                if let Some(bar) = b.borrow_mut().as_mut() {
                    bar.hover = None;
                }
            });
            repaint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let action = hit_at(hwnd, lparam, false).map(|hit| match hit {
                Hit::Record => Action::Toggle,
                Hit::Correct => Action::Correct,
                Hit::Close => Action::Close,
            });
            let on_action = BAR.with(|b| b.borrow().as_ref().map(|bar| bar.on_action));
            if let (Some(action), Some(on_action)) = (action, on_action) {
                on_action(action);
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
