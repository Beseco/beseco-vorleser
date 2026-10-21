//! Bildschirmausschnitt wählen: Der Bildschirm wird eingefroren und abgedunkelt gezeigt, man
//! zieht mit der Maus ein Rechteck auf, und dessen Bildpunkte gehen an die Texterkennung.
//! Esc oder ein Rechtsklick bricht ab.

use std::cell::{Cell, RefCell};

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AlphaBlend, BeginPaint, BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, CreateSolidBrush,
    DeleteDC, DeleteObject, DrawTextW, EndPaint, FillRect, FrameRect, GetDC, GetDIBits, InvalidateRect,
    ReleaseDC, SelectObject, SetBkMode, SetTextColor, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    DIB_RGB_COLORS, DT_CENTER, DT_SINGLELINE, DT_VCENTER, HBITMAP, HDC, PAINTSTRUCT, SRCCOPY, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics, LoadCursorW, RegisterClassW,
    SetForegroundWindow, ShowWindow, IDC_CROSS, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN,
    SM_YVIRTUALSCREEN, SW_SHOW, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_PAINT,
    WM_RBUTTONUP, WNDCLASSW, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
};

use crate::i18n::t;

/// Ausgewählter Ausschnitt: Bildpunkte als BGRA, Breite, Höhe.
pub type Selection = (Vec<u8>, u32, u32);

struct State {
    window: HWND,
    size: (i32, i32),
    bright: HBITMAP,
    dim: HBITMAP,
    start: Option<(i32, i32)>,
    current: (i32, i32),
    on_done: fn(Option<Selection>),
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static REGISTERED: Cell<bool> = const { Cell::new(false) };
}

pub fn is_open() -> bool {
    STATE.with(|s| s.borrow().is_some())
}

/// Zeigt die Auswahl über allen Bildschirmen. `on_done` erhält den Ausschnitt oder `None`.
pub fn start(on_done: fn(Option<Selection>)) {
    if is_open() {
        return;
    }
    unsafe {
        let origin = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
        let size = (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN));

        // Bildschirm einfrieren: eine helle und eine abgedunkelte Kopie.
        let screen = GetDC(None);
        let bright = CreateCompatibleBitmap(screen, size.0, size.1);
        let dim = CreateCompatibleBitmap(screen, size.0, size.1);
        let dc = CreateCompatibleDC(Some(screen));
        let old = SelectObject(dc, bright.into());
        let _ = BitBlt(dc, 0, 0, size.0, size.1, Some(screen), origin.0, origin.1, SRCCOPY);
        SelectObject(dc, dim.into());
        let _ = BitBlt(dc, 0, 0, size.0, size.1, Some(screen), origin.0, origin.1, SRCCOPY);
        darken(dc, screen, size);
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        ReleaseDC(None, screen);

        let Ok(instance) = GetModuleHandleW(None) else { return };
        if !REGISTERED.with(|r| r.replace(true)) {
            let class = WNDCLASSW {
                lpfnWndProc: Some(wndproc),
                hInstance: instance.into(),
                lpszClassName: w!("VorleserAusschnitt"),
                hCursor: LoadCursorW(None, IDC_CROSS).unwrap_or_default(),
                ..Default::default()
            };
            RegisterClassW(&class);
        }
        let Ok(window) = CreateWindowExW(
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
            w!("VorleserAusschnitt"),
            w!("Vorleser"),
            WS_POPUP,
            origin.0,
            origin.1,
            size.0,
            size.1,
            None,
            None,
            Some(instance.into()),
            None,
        ) else {
            let _ = DeleteObject(bright.into());
            let _ = DeleteObject(dim.into());
            on_done(None);
            return;
        };
        STATE.with(|s| {
            *s.borrow_mut() = Some(State { window, size, bright, dim, start: None, current: (0, 0), on_done });
        });
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
    }
}

/// Legt ein halbdurchsichtiges Schwarz über das Bild in `dc`.
unsafe fn darken(dc: HDC, screen: HDC, size: (i32, i32)) {
    let black_dc = CreateCompatibleDC(Some(screen));
    let black = CreateCompatibleBitmap(screen, 1, 1);
    let old = SelectObject(black_dc, black.into());
    let brush = CreateSolidBrush(COLORREF(0));
    FillRect(black_dc, &RECT { left: 0, top: 0, right: 1, bottom: 1 }, brush);
    let blend = BLENDFUNCTION { BlendOp: 0, BlendFlags: 0, SourceConstantAlpha: 120, AlphaFormat: 0 };
    let _ = AlphaBlend(dc, 0, 0, size.0, size.1, black_dc, 0, 0, 1, 1, blend);
    SelectObject(black_dc, old);
    let _ = DeleteObject(brush.into());
    let _ = DeleteObject(black.into());
    let _ = DeleteDC(black_dc);
}

fn selection_rect(state: &State) -> Option<RECT> {
    let (x0, y0) = state.start?;
    let (x1, y1) = state.current;
    Some(RECT { left: x0.min(x1), top: y0.min(y1), right: x0.max(x1), bottom: y0.max(y1) })
}

unsafe fn paint(state: &State) {
    let mut ps = PAINTSTRUCT::default();
    let target = BeginPaint(state.window, &mut ps);
    let (w, h) = state.size;
    // Doppelt gepuffert, damit beim Ziehen nichts flackert.
    let buffer_dc = CreateCompatibleDC(Some(target));
    let buffer = CreateCompatibleBitmap(target, w, h);
    let old_buffer = SelectObject(buffer_dc, buffer.into());
    let source = CreateCompatibleDC(Some(target));
    let old_source = SelectObject(source, state.dim.into());
    let _ = BitBlt(buffer_dc, 0, 0, w, h, Some(source), 0, 0, SRCCOPY);
    if let Some(r) = selection_rect(state) {
        SelectObject(source, state.bright.into());
        let _ = BitBlt(buffer_dc, r.left, r.top, r.right - r.left, r.bottom - r.top, Some(source), r.left, r.top, SRCCOPY);
        let blue = CreateSolidBrush(COLORREF(0x00D7_7800));
        for inset in 0..2 {
            let frame = RECT { left: r.left - inset, top: r.top - inset, right: r.right + inset, bottom: r.bottom + inset };
            FrameRect(buffer_dc, &frame, blue);
        }
        let _ = DeleteObject(blue.into());
    } else {
        // Hinweis oben in der Mitte.
        SetBkMode(buffer_dc, TRANSPARENT);
        SetTextColor(buffer_dc, COLORREF(0x00FF_FFFF));
        let mut text: Vec<u16> = t("snip.hint").encode_utf16().collect();
        let mut area = RECT { left: 0, top: 20, right: w, bottom: 70 };
        DrawTextW(buffer_dc, &mut text, &mut area, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
    let _ = BitBlt(target, 0, 0, w, h, Some(buffer_dc), 0, 0, SRCCOPY);
    SelectObject(source, old_source);
    SelectObject(buffer_dc, old_buffer);
    let _ = DeleteObject(buffer.into());
    let _ = DeleteDC(buffer_dc);
    let _ = DeleteDC(source);
    let _ = EndPaint(state.window, &ps);
}

/// Bildpunkte eines Rechtecks aus der hellen Kopie, als BGRA von oben nach unten.
unsafe fn crop(state: &State, r: RECT) -> Selection {
    let (w, h) = (r.right - r.left, r.bottom - r.top);
    let screen = GetDC(None);
    let source = CreateCompatibleDC(Some(screen));
    let old_source = SelectObject(source, state.bright.into());
    let part_dc = CreateCompatibleDC(Some(screen));
    let part = CreateCompatibleBitmap(screen, w, h);
    let old_part = SelectObject(part_dc, part.into());
    let _ = BitBlt(part_dc, 0, 0, w, h, Some(source), r.left, r.top, SRCCOPY);
    SelectObject(part_dc, old_part);
    let mut bgra = vec![0u8; (w * h * 4) as usize];
    let mut info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w,
            biHeight: -h,
            biPlanes: 1,
            biBitCount: 32,
            ..Default::default()
        },
        ..Default::default()
    };
    GetDIBits(part_dc, part, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
    for pixel in bgra.chunks_exact_mut(4) {
        pixel[3] = 255;
    }
    SelectObject(source, old_source);
    let _ = DeleteObject(part.into());
    let _ = DeleteDC(part_dc);
    let _ = DeleteDC(source);
    ReleaseDC(None, screen);
    (bgra, w as u32, h as u32)
}

/// Schließt die Auswahl und meldet das Ergebnis.
fn finish(selection: bool) {
    let Some(state) = STATE.with(|s| s.borrow_mut().take()) else { return };
    let result = selection_rect(&state)
        .filter(|r| selection && r.right - r.left >= 6 && r.bottom - r.top >= 6)
        .map(|r| unsafe { crop(&state, r) });
    unsafe {
        let _ = ReleaseCapture();
        let _ = DestroyWindow(state.window);
        let _ = DeleteObject(state.bright.into());
        let _ = DeleteObject(state.dim.into());
    }
    (state.on_done)(result);
}

fn point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xffff) as i16 as i32, ((lparam.0 >> 16) & 0xffff) as i16 as i32)
}

extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let ours = STATE.with(|s| s.borrow().as_ref().is_some_and(|st| st.window == hwnd));
    if !ours {
        return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
    }
    unsafe {
        match msg {
            WM_PAINT => {
                STATE.with(|s| {
                    if let Some(state) = s.borrow().as_ref() {
                        paint(state);
                    }
                });
            }
            WM_LBUTTONDOWN => {
                let at = point(lparam);
                STATE.with(|s| {
                    if let Some(state) = s.borrow_mut().as_mut() {
                        state.start = Some(at);
                        state.current = at;
                    }
                });
                SetCapture(hwnd);
            }
            WM_MOUSEMOVE => {
                let at = point(lparam);
                let dragging = STATE.with(|s| {
                    let mut s = s.borrow_mut();
                    let state = s.as_mut()?;
                    state.start?;
                    state.current = at;
                    Some(())
                });
                if dragging.is_some() {
                    let _ = InvalidateRect(Some(hwnd), None, false);
                }
            }
            WM_LBUTTONUP => {
                STATE.with(|s| {
                    if let Some(state) = s.borrow_mut().as_mut() {
                        state.current = point(lparam);
                    }
                });
                finish(true);
            }
            WM_RBUTTONUP => finish(false),
            WM_KEYDOWN if wparam.0 == VK_ESCAPE.0 as usize => finish(false),
            _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
        }
    }
    LRESULT(0)
}

