//! Zeichnen mit Direct2D und DirectWrite: weiche Kanten, runde Formen, Symbole und Text für die
//! schwebenden Leisten (Diktat, Mini-Player) und die eigenen Schaltflächen im Fenster.
//! Koordinaten sind geräteunabhängige Pixel (96 dpi); skaliert wird beim Zeichnen.

use std::cell::OnceCell;

use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW,
    D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_F,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Factory, ID2D1RenderTarget, ID2D1SolidColorBrush, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_SMALL,
    D2D1_DRAW_TEXT_OPTIONS_CLIP, D2D1_ELLIPSE, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_FEATURE_LEVEL_DEFAULT,
    D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT, D2D1_RENDER_TARGET_USAGE_NONE,
    D2D1_ROUNDED_RECT, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL,
    DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_WEIGHT_NORMAL, DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_MEASURING_MODE_NATURAL,
    DWRITE_PARAGRAPH_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_CENTER, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER,
    BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, DIB_RGB_COLORS, HDC,
};
use windows::Win32::UI::WindowsAndMessaging::{UpdateLayeredWindow, ULW_ALPHA};
use windows_numerics::Vector2;

/// Farbe als 0xRRGGBB mit Deckkraft.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Color(pub u32, pub f32);

pub const fn rgb(hex: u32) -> Color {
    Color(hex, 1.0)
}

impl Color {
    pub const fn alpha(self, alpha: f32) -> Color {
        Color(self.0, alpha)
    }

    fn d2d(self) -> D2D1_COLOR_F {
        let channel = |shift: u32| ((self.0 >> shift) & 0xff) as f32 / 255.0;
        D2D1_COLOR_F { r: channel(16), g: channel(8), b: channel(0), a: self.1 }
    }

    /// Für GDI (Hintergrund von Textfeldern usw.).
    pub fn colorref(self) -> COLORREF {
        let (r, g, b) = ((self.0 >> 16) & 0xff, (self.0 >> 8) & 0xff, self.0 & 0xff);
        COLORREF(r | g << 8 | b << 16)
    }

    /// Mischt mit `other`; `amount` 0 = diese Farbe, 1 = `other`.
    pub fn mix(self, other: Color, amount: f32) -> Color {
        let channel = |shift: u32| {
            let a = ((self.0 >> shift) & 0xff) as f32;
            let b = ((other.0 >> shift) & 0xff) as f32;
            ((a + (b - a) * amount).round() as u32) << shift
        };
        Color(channel(16) | channel(8) | channel(0), self.1)
    }
}

/// Ausrichtung von Text im Rechteck.
#[derive(Clone, Copy, PartialEq)]
pub enum Align {
    Left,
    Center,
    Right,
}

/// Gezeichnete Symbole, alle als Linien und Flächen ohne Schriftart.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Icon {
    Mic,
    Stop,
    Play,
    Pause,
    Prev,
    Next,
    Close,
    Check,
    Warning,
    Open,
    Chevron,
    Speaker,
    Gear,
}

struct Factories {
    d2d: ID2D1Factory,
    dwrite: IDWriteFactory,
}

thread_local! {
    static FACTORIES: OnceCell<Option<Factories>> = const { OnceCell::new() };
}

fn with_factories<R>(f: impl FnOnce(&Factories) -> R) -> Option<R> {
    FACTORIES.with(|cell| {
        cell.get_or_init(|| unsafe {
            let d2d = D2D1CreateFactory::<ID2D1Factory>(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).ok()?;
            let dwrite = DWriteCreateFactory::<IDWriteFactory>(DWRITE_FACTORY_TYPE_SHARED).ok()?;
            Some(Factories { d2d, dwrite })
        })
        .as_ref()
        .map(f)
    })
}

/// Eine Zeichenfläche während eines Zeichenvorgangs.
pub struct Canvas<'a> {
    rt: &'a ID2D1RenderTarget,
    f: &'a Factories,
}

fn point(x: f32, y: f32) -> Vector2 {
    Vector2 { X: x, Y: y }
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> D2D_RECT_F {
    D2D_RECT_F { left: x, top: y, right: x + w, bottom: y + h }
}

impl Canvas<'_> {
    fn brush(&self, color: Color) -> Option<ID2D1SolidColorBrush> {
        unsafe { self.rt.CreateSolidColorBrush(&color.d2d(), None).ok() }
    }

    pub fn fill_round(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color) {
        let Some(brush) = self.brush(color) else { return };
        let shape = D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillRoundedRectangle(&shape, &brush) };
    }

    pub fn stroke_round(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, color: Color, width: f32) {
        let Some(brush) = self.brush(color) else { return };
        let half = width / 2.0;
        let shape = D2D1_ROUNDED_RECT { rect: rect(x + half, y + half, w - width, h - width), radiusX: radius, radiusY: radius };
        unsafe { self.rt.DrawRoundedRectangle(&shape, &brush, width, None) };
    }

    pub fn circle(&self, cx: f32, cy: f32, radius: f32, color: Color) {
        let Some(brush) = self.brush(color) else { return };
        let shape = D2D1_ELLIPSE { point: point(cx, cy), radiusX: radius, radiusY: radius };
        unsafe { self.rt.FillEllipse(&shape, &brush) };
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, color: Color, width: f32) {
        let Some(brush) = self.brush(color) else { return };
        unsafe { self.rt.DrawLine(point(x1, y1), point(x2, y2), &brush, width, None) };
    }

    /// Weicher Schatten unter einer runden Fläche.
    pub fn shadow(&self, x: f32, y: f32, w: f32, h: f32, radius: f32, spread: f32, strength: f32) {
        let steps = spread.max(1.0) as usize;
        for i in (1..=steps).rev() {
            let grow = i as f32;
            let alpha = strength * (1.0 - grow / (steps as f32 + 1.0)) / steps as f32 * 2.0;
            self.fill_round(x - grow, y - grow + 3.0, w + 2.0 * grow, h + 2.0 * grow, radius + grow, rgb(0x0A0F19).alpha(alpha));
        }
    }

    /// Fläche oder Linienzug durch `points`.
    fn path(&self, points: &[(f32, f32)], filled: bool, closed: bool, color: Color, width: f32) {
        let Some(brush) = self.brush(color) else { return };
        unsafe {
            let Ok(geometry) = self.f.d2d.CreatePathGeometry() else { return };
            let Ok(sink) = geometry.Open() else { return };
            let (x, y) = points[0];
            sink.BeginFigure(point(x, y), if filled { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
            for &(x, y) in &points[1..] {
                sink.AddLine(point(x, y));
            }
            sink.EndFigure(if closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
            if sink.Close().is_err() {
                return;
            }
            if filled {
                self.rt.FillGeometry(&geometry, &brush, None);
            } else {
                self.rt.DrawGeometry(&geometry, &brush, width, None);
            }
        }
    }

    /// Bogen von `from` nach `to` mit Radius `radius`, im Uhrzeigersinn oder dagegen.
    fn arc(&self, from: (f32, f32), to: (f32, f32), radius: f32, clockwise: bool, color: Color, width: f32) {
        let Some(brush) = self.brush(color) else { return };
        unsafe {
            let Ok(geometry) = self.f.d2d.CreatePathGeometry() else { return };
            let Ok(sink) = geometry.Open() else { return };
            sink.BeginFigure(point(from.0, from.1), D2D1_FIGURE_BEGIN_HOLLOW);
            sink.AddArc(&D2D1_ARC_SEGMENT {
                point: point(to.0, to.1),
                size: D2D_SIZE_F { width: radius, height: radius },
                rotationAngle: 0.0,
                sweepDirection: if clockwise { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                arcSize: D2D1_ARC_SIZE_SMALL,
            });
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            if sink.Close().is_ok() {
                self.rt.DrawGeometry(&geometry, &brush, width, None);
            }
        }
    }

    /// Symbol mit Mittelpunkt `cx`, `cy`; `size` ist die Kantenlänge (z. B. 20).
    pub fn icon(&self, icon: Icon, cx: f32, cy: f32, size: f32, color: Color) {
        let s = size / 24.0;
        let p = |x: f32, y: f32| (cx + (x - 12.0) * s, cy + (y - 12.0) * s);
        let stroke = 2.0 * s.max(0.75);
        match icon {
            Icon::Mic => {
                let (x, y) = p(9.0, 3.0);
                self.stroke_round(x, y, 6.0 * s + stroke, 11.0 * s + stroke, 3.0 * s + stroke / 2.0, color, stroke);
                // Der Bügel unter der Kapsel: von links über unten nach rechts.
                self.arc(p(5.0, 11.0), p(19.0, 11.0), 7.0 * s, false, color, stroke);
                let (x1, y1) = p(12.0, 18.0);
                let (x2, y2) = p(12.0, 21.0);
                self.line(x1, y1, x2, y2, color, stroke);
            }
            Icon::Stop => {
                let (x, y) = p(6.0, 6.0);
                self.fill_round(x, y, 12.0 * s, 12.0 * s, 2.0 * s, color);
            }
            Icon::Play => self.path(&[p(8.0, 5.0), p(19.0, 12.0), p(8.0, 19.0)], true, true, color, 0.0),
            Icon::Pause => {
                let (x, y) = p(6.0, 5.0);
                self.fill_round(x, y, 4.0 * s, 14.0 * s, 1.0 * s, color);
                let (x, y) = p(14.0, 5.0);
                self.fill_round(x, y, 4.0 * s, 14.0 * s, 1.0 * s, color);
            }
            Icon::Prev => {
                self.path(&[p(19.0, 5.0), p(9.0, 12.0), p(19.0, 19.0)], true, true, color, 0.0);
                let (x1, y1) = p(6.0, 5.0);
                let (x2, y2) = p(6.0, 19.0);
                self.line(x1, y1, x2, y2, color, stroke);
            }
            Icon::Next => {
                self.path(&[p(5.0, 5.0), p(15.0, 12.0), p(5.0, 19.0)], true, true, color, 0.0);
                let (x1, y1) = p(18.0, 5.0);
                let (x2, y2) = p(18.0, 19.0);
                self.line(x1, y1, x2, y2, color, stroke);
            }
            Icon::Close => {
                self.path(&[p(6.0, 6.0), p(18.0, 18.0)], false, false, color, stroke);
                self.path(&[p(18.0, 6.0), p(6.0, 18.0)], false, false, color, stroke);
            }
            Icon::Check => self.path(&[p(5.0, 12.5), p(10.0, 17.5), p(19.5, 7.0)], false, false, color, stroke),
            Icon::Warning => {
                self.path(&[p(12.0, 3.5), p(21.5, 20.0), p(2.5, 20.0)], false, true, color, stroke);
                self.path(&[p(12.0, 9.5), p(12.0, 14.0)], false, false, color, stroke);
                let (x, y) = p(12.0, 17.0);
                self.circle(x, y, 1.2 * s, color);
            }
            Icon::Open => {
                self.path(&[p(14.0, 4.0), p(20.0, 4.0), p(20.0, 10.0)], false, false, color, stroke);
                self.path(&[p(20.0, 4.0), p(12.0, 12.0)], false, false, color, stroke);
                self.path(&[p(18.0, 14.0), p(18.0, 20.0), p(4.0, 20.0), p(4.0, 6.0), p(10.0, 6.0)], false, false, color, stroke);
            }
            Icon::Chevron => self.path(&[p(6.0, 9.0), p(12.0, 15.0), p(18.0, 9.0)], false, false, color, stroke),
            Icon::Speaker => {
                self.path(&[p(4.0, 9.0), p(8.0, 9.0), p(13.0, 5.0), p(13.0, 19.0), p(8.0, 15.0), p(4.0, 15.0)], false, true, color, stroke);
                self.arc(p(16.5, 8.5), p(16.5, 15.5), 5.0 * s, true, color, stroke);
            }
            Icon::Gear => {
                let (x, y) = p(12.0, 12.0);
                for i in 0..8 {
                    let angle = i as f32 * std::f32::consts::FRAC_PI_4;
                    let (sin, cos) = angle.sin_cos();
                    self.line(x + cos * 6.0 * s, y + sin * 6.0 * s, x + cos * 9.5 * s, y + sin * 9.5 * s, color, stroke * 1.3);
                }
                self.stroke_round(x - 6.0 * s, y - 6.0 * s, 12.0 * s, 12.0 * s, 6.0 * s, color, stroke * 1.4);
            }
        }
    }

    fn format(&self, size: f32, bold: bool, align: Align) -> Option<IDWriteTextFormat> {
        unsafe {
            let weight = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
            let format = self
                .f
                .dwrite
                .CreateTextFormat(w!("Segoe UI"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!(""))
                .ok()?;
            let alignment = match align {
                Align::Left => DWRITE_TEXT_ALIGNMENT_LEADING,
                Align::Center => DWRITE_TEXT_ALIGNMENT_CENTER,
                Align::Right => DWRITE_TEXT_ALIGNMENT_TRAILING,
            };
            let _ = format.SetTextAlignment(alignment);
            let _ = format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_CENTER);
            let _ = format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP);
            // Zu langer Text endet mit „…“.
            if let Ok(sign) = self.f.dwrite.CreateEllipsisTrimmingSign(&format) {
                let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, delimiter: 0, delimiterCount: 0 };
                let _ = format.SetTrimming(&trimming, &sign);
            }
            Some(format)
        }
    }

    /// Einzeiliger Text, senkrecht mittig im Rechteck.
    pub fn text(&self, text: &str, x: f32, y: f32, w: f32, h: f32, size: f32, bold: bool, color: Color, align: Align) {
        let (Some(format), Some(brush)) = (self.format(size, bold, align), self.brush(color)) else { return };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            self.rt.DrawText(&wide, &format, &rect(x, y, w, h), &brush, D2D1_DRAW_TEXT_OPTIONS_CLIP, DWRITE_MEASURING_MODE_NATURAL);
        }
    }

    /// Breite eines einzeiligen Textes.
    pub fn text_width(&self, text: &str, size: f32, bold: bool) -> f32 {
        let Some(format) = self.format(size, bold, Align::Left) else { return 0.0 };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let Ok(layout) = self.f.dwrite.CreateTextLayout(&wide, &format, 10_000.0, 100.0) else { return 0.0 };
            let mut metrics = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut metrics).is_ok() {
                metrics.widthIncludingTrailingWhitespace
            } else {
                0.0
            }
        }
    }
}

/// Breite eines Textes, ohne zu zeichnen (zum Bemessen von Leisten).
pub fn measure(text: &str, size: f32, bold: bool) -> f32 {
    with_factories(|f| {
        // Eine Zeichenfläche ist zum Messen nicht nötig, nur DirectWrite.
        let format = unsafe {
            let weight = if bold { DWRITE_FONT_WEIGHT_SEMI_BOLD } else { DWRITE_FONT_WEIGHT_NORMAL };
            f.dwrite.CreateTextFormat(w!("Segoe UI"), None, weight, DWRITE_FONT_STYLE_NORMAL, DWRITE_FONT_STRETCH_NORMAL, size, w!(""))
        };
        let Ok(format) = format else { return 0.0 };
        let wide: Vec<u16> = text.encode_utf16().collect();
        unsafe {
            let Ok(layout) = f.dwrite.CreateTextLayout(&wide, &format, 10_000.0, 100.0) else { return 0.0 };
            let mut metrics = DWRITE_TEXT_METRICS::default();
            if layout.GetMetrics(&mut metrics).is_ok() {
                metrics.widthIncludingTrailingWhitespace
            } else {
                0.0
            }
        }
    })
    .unwrap_or(0.0)
}

fn properties() -> D2D1_RENDER_TARGET_PROPERTIES {
    D2D1_RENDER_TARGET_PROPERTIES {
        r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT,
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 0.0,
        dpiY: 0.0,
        usage: D2D1_RENDER_TARGET_USAGE_NONE,
        minLevel: D2D1_FEATURE_LEVEL_DEFAULT,
    }
}

/// Zeichnet `hdc` im Bereich `area` (Pixel) neu, z. B. eine eigene Schaltfläche.
pub fn draw_dc(hdc: HDC, area: RECT, dpi: u32, background: Color, draw: impl FnOnce(&Canvas, f32, f32)) {
    with_factories(|f| unsafe {
        let Ok(target) = f.d2d.CreateDCRenderTarget(&properties()) else { return };
        if target.BindDC(hdc, &area).is_err() {
            return;
        }
        target.SetDpi(dpi as f32, dpi as f32);
        target.BeginDraw();
        target.Clear(Some(&background.d2d()));
        let scale = 96.0 / dpi as f32;
        let canvas = Canvas { rt: &target, f };
        draw(&canvas, (area.right - area.left) as f32 * scale, (area.bottom - area.top) as f32 * scale);
        let _ = target.EndDraw(None, None);
    });
}

/// Zeichnet ein durchscheinendes, rahmenloses Fenster (`WS_EX_LAYERED`) an `x`, `y` (Pixel) in
/// der Größe `w`×`h` (geräteunabhängige Pixel).
pub fn draw_layered(hwnd: HWND, x: i32, y: i32, w: f32, h: f32, dpi: u32, draw: impl FnOnce(&Canvas)) {
    let width = (w * dpi as f32 / 96.0).ceil() as i32;
    let height = (h * dpi as f32 / 96.0).ceil() as i32;
    with_factories(|f| unsafe {
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
                biPlanes: 1,
                biBitCount: 32,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let Ok(bitmap) = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0) else { return };
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, bitmap.into());
        if let Ok(target) = f.d2d.CreateDCRenderTarget(&properties()) {
            let area = RECT { left: 0, top: 0, right: width, bottom: height };
            if target.BindDC(dc, &area).is_ok() {
                target.SetDpi(dpi as f32, dpi as f32);
                target.BeginDraw();
                target.Clear(Some(&D2D1_COLOR_F::default()));
                draw(&Canvas { rt: &target, f });
                let _ = target.EndDraw(None, None);
                let blend = BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: 255, AlphaFormat: AC_SRC_ALPHA as u8 };
                let _ = UpdateLayeredWindow(
                    hwnd,
                    None,
                    Some(&POINT { x, y }),
                    Some(&SIZE { cx: width, cy: height }),
                    Some(dc),
                    Some(&POINT { x: 0, y: 0 }),
                    COLORREF(0),
                    Some(&blend),
                    ULW_ALPHA,
                );
            }
        }
        SelectObject(dc, old);
        let _ = DeleteObject(bitmap.into());
        let _ = DeleteDC(dc);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixes_colors() {
        assert_eq!(rgb(0x000000).mix(rgb(0xFFFFFF), 0.5), rgb(0x808080));
        assert_eq!(rgb(0x1565C0).colorref(), COLORREF(0xC06515));
    }
}
