//! Mini-Player: eine schwebende Leiste unten am Bildschirm, solange Text aus einem anderen
//! Programm vorgelesen wird und das Vorleser-Fenster nicht offen ist. Satz zurück und vor,
//! Pause, Geschwindigkeit, Fenster öffnen und Stopp, ohne das Programm zu verlassen.

use std::cell::RefCell;

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, ScreenToClient, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CheckMenuRadioItem, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
    GetForegroundWindow, GetWindowRect, LoadCursorW, RegisterClassW, SetForegroundWindow, SetWindowPos, ShowWindow,
    TrackPopupMenu, HTCAPTION, HTCLIENT, HWND_TOPMOST, IDC_ARROW, MA_NOACTIVATE, MF_BYCOMMAND, MF_STRING,
    SWP_NOACTIVATE, SWP_NOSIZE, SW_SHOWNOACTIVATE, TPM_BOTTOMALIGN, TPM_CENTERALIGN, TPM_NONOTIFY, TPM_RETURNCMD,
    WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_NCHITTEST, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::i18n::t;
use crate::paint::{self, Align, Canvas, Icon};
use crate::theme;

const WM_MOUSELEAVE: u32 = 0x02A3;

/// Klick im Mini-Player.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Action {
    Prev,
    PlayPause,
    Next,
    /// Neue Geschwindigkeit: Index in der Liste aus [`show`].
    Rate(usize),
    OpenWindow,
    Stop,
}

/// Was der Player anzeigt.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Info {
    pub voice: String,
    /// „Satz 2 von 3“ oder leer.
    pub progress: String,
    pub paused: bool,
    /// Geschwindigkeit als Faktor, z. B. „1,2×“.
    pub speed: String,
    /// Beschriftungen der Geschwindigkeiten fürs Menü und die gewählte.
    pub rates: Vec<String>,
    pub rate: usize,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Prev,
    PlayPause,
    Next,
    Speed,
    Open,
    Stop,
}

struct Player {
    hwnd: HWND,
    info: Info,
    hover: Option<Hit>,
    on_action: fn(Action),
}

thread_local! {
    static PLAYER: RefCell<Option<Player>> = const { RefCell::new(None) };
}

const MARGIN: f32 = 16.0;
const HEIGHT: f32 = 64.0;

struct Layout {
    width: f32,
    avatar: (f32, f32, f32),
    text_x: f32,
    buttons: Vec<(Hit, f32, f32, f32)>,
    speed: (f32, f32, f32, f32),
}

fn layout(speed: &str) -> Layout {
    let cy = MARGIN + HEIGHT / 2.0;
    let avatar = (MARGIN + 31.0, cy, 23.0);
    let text_x = MARGIN + 62.0;
    let mut x = text_x + 132.0;
    let mut buttons = Vec::new();
    for (hit, r) in [(Hit::Prev, 22.0), (Hit::PlayPause, 24.0), (Hit::Next, 22.0)] {
        buttons.push((hit, x + r, cy, r));
        x += 2.0 * r + 4.0;
    }
    let speed_w = paint::measure(speed, 14.0, true) + 28.0;
    let speed_rect = (x + 4.0, cy - 18.0, speed_w, 36.0);
    x += speed_w + 12.0;
    for hit in [Hit::Open, Hit::Stop] {
        buttons.push((hit, x + 20.0, cy, 20.0));
        x += 42.0;
    }
    Layout { width: x + 8.0 + MARGIN, avatar, text_x, buttons, speed: speed_rect }
}

fn hit(l: &Layout, x: f32, y: f32) -> Option<Hit> {
    let (sx, sy, sw, sh) = l.speed;
    if x >= sx && x <= sx + sw && y >= sy && y <= sy + sh {
        return Some(Hit::Speed);
    }
    l.buttons.iter().find(|(_, cx, cy, r)| (x - cx).powi(2) + (y - cy).powi(2) <= r * r).map(|(hit, ..)| *hit)
}

/// Zeigt den Player oder aktualisiert ihn.
pub fn show(info: Info, on_action: fn(Action)) {
    let existing = PLAYER.with(|p| p.borrow().as_ref().map(|player| player.hwnd));
    let hwnd = match existing {
        Some(hwnd) => hwnd,
        None => match create() {
            Some(hwnd) => hwnd,
            None => return,
        },
    };
    let speed = info.speed.clone();
    PLAYER.with(|p| {
        let mut p = p.borrow_mut();
        let hover = p.as_ref().and_then(|player| player.hover);
        *p = Some(Player { hwnd, info, hover, on_action });
    });
    if existing.is_none() {
        place(hwnd, &speed);
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
    repaint();
}

pub fn hide() {
    if let Some(hwnd) = PLAYER.with(|p| p.borrow_mut().take().map(|player| player.hwnd)) {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
}

pub fn visible() -> bool {
    PLAYER.with(|p| p.borrow().is_some())
}

fn create() -> Option<HWND> {
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let class = WNDCLASSW {
            lpfnWndProc: Some(wndproc),
            hInstance: instance.into(),
            lpszClassName: w!("VorleserMiniPlayer"),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassW(&class);
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class.lpszClassName,
            w!("Hedgie – Wiedergabe"),
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

fn place(hwnd: HWND, speed: &str) {
    unsafe {
        let monitor = MonitorFromWindow(GetForegroundWindow(), MONITOR_DEFAULTTOPRIMARY);
        let mut info = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return;
        }
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        let work = info.rcWork;
        let width = (layout(speed).width * scale) as i32;
        let height = ((HEIGHT + 2.0 * MARGIN) * scale) as i32;
        let x = (work.left + work.right - width) / 2;
        let y = work.bottom - height - (12.0 * scale) as i32;
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, 0, 0, SWP_NOSIZE | SWP_NOACTIVATE);
    }
}

/// Anfangsbuchstaben für den runden Platzhalter, z. B. „TH“ für „Thorsten HD“.
fn initials(name: &str) -> String {
    let words: Vec<&str> = name.split_whitespace().collect();
    match words.as_slice() {
        [] => String::new(),
        [one] => one.chars().take(2).collect::<String>().to_uppercase(),
        [first, second, ..] => format!("{}{}", first.chars().next().unwrap_or(' '), second.chars().next().unwrap_or(' ')).to_uppercase(),
    }
}

fn repaint() {
    PLAYER.with(|p| {
        let p = p.borrow();
        let Some(player) = p.as_ref() else { return };
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(player.hwnd, &mut rect);
        }
        let dpi = unsafe { GetDpiForWindow(player.hwnd) }.max(96);
        let l = layout(&player.info.speed);
        paint::draw_layered(player.hwnd, rect.left, rect.top, l.width, HEIGHT + 2.0 * MARGIN, dpi, |c| draw(c, player, &l));
    });
}

fn draw(c: &Canvas, player: &Player, l: &Layout) {
    let p = theme::palette();
    let body_w = l.width - 2.0 * MARGIN;
    c.shadow(MARGIN, MARGIN, body_w, HEIGHT, HEIGHT / 2.0, 10.0, if p.dark { 0.4 } else { 0.22 });
    c.fill_round(MARGIN, MARGIN, body_w, HEIGHT, HEIGHT / 2.0, p.surface);
    c.stroke_round(MARGIN, MARGIN, body_w, HEIGHT, HEIGHT / 2.0, p.border, 1.0);

    let (cx, cy, r) = l.avatar;
    c.circle(cx, cy, r, p.accent_soft);
    c.text(&initials(&player.info.voice), cx - r, cy - r, 2.0 * r, 2.0 * r, 14.0, true, if p.dark { paint::rgb(0xB5D2F7) } else { paint::rgb(0x0D47A1) }, Align::Center);
    c.text(&player.info.voice, l.text_x, MARGIN + 12.0, 128.0, 20.0, 13.0, true, p.text, Align::Left);
    let sub = if player.info.paused { t("player.paused").to_string() } else { player.info.progress.clone() };
    c.text(&sub, l.text_x, MARGIN + 32.0, 128.0, 18.0, 11.5, false, p.muted, Align::Left);

    for &(hit, cx, cy, r) in &l.buttons {
        let hover = player.hover == Some(hit);
        match hit {
            Hit::PlayPause => {
                c.circle(cx, cy, r, if hover { p.accent.mix(p.text, 0.15) } else { p.accent });
                c.icon(if player.info.paused { Icon::Play } else { Icon::Pause }, cx, cy, 20.0, p.on_accent);
            }
            _ => {
                if hover {
                    c.circle(cx, cy, r, p.raised);
                }
                let (icon, size) = match hit {
                    Hit::Prev => (Icon::Prev, 20.0),
                    Hit::Next => (Icon::Next, 20.0),
                    Hit::Open => (Icon::Open, 16.0),
                    _ => (Icon::Close, 14.0),
                };
                c.icon(icon, cx, cy, size, if matches!(hit, Hit::Open | Hit::Stop) { p.muted } else { p.text });
            }
        }
    }
    let (x, y, w, h) = l.speed;
    c.fill_round(x, y, w, h, h / 2.0, if player.hover == Some(Hit::Speed) { p.raised } else { p.ground });
    c.stroke_round(x, y, w, h, h / 2.0, p.border, 1.0);
    c.text(&player.info.speed, x, y, w, h, 14.0, true, p.text, Align::Center);
}

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
    let speed = PLAYER.with(|p| p.borrow().as_ref().map(|player| player.info.speed.clone()))?;
    hit(&layout(&speed), x, y)
}

/// Menü der Geschwindigkeiten über dem Knopf; liefert den gewählten Index.
fn choose_rate(hwnd: HWND) -> Option<usize> {
    let (rates, current) = PLAYER.with(|p| p.borrow().as_ref().map(|player| (player.info.rates.clone(), player.info.rate)))?;
    unsafe {
        let menu = CreatePopupMenu().ok()?;
        for (i, label) in rates.iter().enumerate() {
            let _ = AppendMenuW(menu, MF_STRING, i + 1, &HSTRING::from(label.as_str()));
        }
        let id = current as u32 + 1;
        let _ = CheckMenuRadioItem(menu, id, id, id, MF_BYCOMMAND.0);
        let mut rect = RECT::default();
        let _ = GetWindowRect(hwnd, &mut rect);
        // Ohne Vordergrund schließt sich das Menü nicht beim Klick daneben.
        let _ = SetForegroundWindow(hwnd);
        let chosen = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_NONOTIFY | TPM_BOTTOMALIGN | TPM_CENTERALIGN,
            (rect.left + rect.right) / 2,
            rect.top + 12,
            None,
            hwnd,
            None,
        );
        let _ = DestroyMenu(menu);
        (chosen.0 > 0).then(|| chosen.0 as usize - 1)
    }
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_NCHITTEST => LRESULT(if hit_at(hwnd, lparam, true).is_some() { HTCLIENT } else { HTCAPTION } as isize),
        WM_MOUSEMOVE => {
            let hover = hit_at(hwnd, lparam, false);
            let changed = PLAYER.with(|p| p.borrow_mut().as_mut().is_some_and(|player| std::mem::replace(&mut player.hover, hover) != hover));
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
            PLAYER.with(|p| {
                if let Some(player) = p.borrow_mut().as_mut() {
                    player.hover = None;
                }
            });
            repaint();
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let action = match hit_at(hwnd, lparam, false) {
                Some(Hit::Prev) => Some(Action::Prev),
                Some(Hit::PlayPause) => Some(Action::PlayPause),
                Some(Hit::Next) => Some(Action::Next),
                Some(Hit::Speed) => choose_rate(hwnd).map(Action::Rate),
                Some(Hit::Open) => Some(Action::OpenWindow),
                Some(Hit::Stop) => Some(Action::Stop),
                None => None,
            };
            let on_action = PLAYER.with(|p| p.borrow().as_ref().map(|player| player.on_action));
            if let (Some(action), Some(on_action)) = (action, on_action) {
                on_action(action);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn makes_initials() {
        assert_eq!(initials("Thorsten HD"), "TH");
        assert_eq!(initials("Hedda"), "HE");
        assert_eq!(initials(""), "");
    }
}
