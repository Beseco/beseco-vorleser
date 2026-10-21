//! Texterkennung (OCR) mit der in Windows eingebauten Texterkennung (`Windows.Media.Ocr`):
//! kostenlos, offline, für alle installierten Sprachen. Gelesen werden Bilder, PDF-Seiten (über
//! die PDF-Darstellung von Windows) und Bildschirmausschnitte.
//!
//! Alles hier blockiert und ist für einen Hintergrund-Thread gedacht, auf dem COM im
//! Mehrfach-Thread-Modell läuft (siehe [`init_thread`]).

use windows::core::{Interface, HSTRING};
use windows::Data::Pdf::{PdfDocument, PdfPageRenderOptions};
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Storage::Streams::InMemoryRandomAccessStream;
use windows::Storage::{FileAccessMode, StorageFile};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};

use crate::i18n::t;

/// Breite, auf die PDF-Seiten für die Texterkennung gerendert werden (bei A4 etwa 200 dpi).
const PDF_WIDTH: u32 = 1700;

pub fn init_thread() {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    }
}

fn error(e: windows::core::Error) -> String {
    e.message()
}

/// Die Texterkennung in der Sprache der Oberfläche, sonst in einer der Windows-Sprachen.
fn engine() -> Result<OcrEngine, String> {
    let preferred = crate::i18n::current();
    if let Ok(languages) = OcrEngine::AvailableRecognizerLanguages() {
        for language in languages {
            let tag = language.LanguageTag().map(|t| t.to_string().to_lowercase()).unwrap_or_default();
            if tag.split('-').next() == Some(preferred) {
                if let Ok(engine) = OcrEngine::TryCreateFromLanguage(&language) {
                    return Ok(engine);
                }
            }
        }
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| t("err.ocr_missing").to_string())
}

/// Erkennt den Text in einem Bild; die Zeilen werden durch Zeilenumbrüche getrennt.
fn recognize(engine: &OcrEngine, bitmap: &SoftwareBitmap) -> Result<String, String> {
    // Die Texterkennung verlangt Bgra8 oder Gray8.
    let bitmap = if bitmap.BitmapPixelFormat().map_err(error)? == BitmapPixelFormat::Bgra8 {
        bitmap.clone()
    } else {
        SoftwareBitmap::ConvertWithAlpha(bitmap, BitmapPixelFormat::Bgra8, BitmapAlphaMode::Premultiplied).map_err(error)?
    };
    let result = engine.RecognizeAsync(&bitmap).map_err(error)?.join().map_err(error)?;
    let mut text = String::new();
    for line in result.Lines().map_err(error)? {
        text.push_str(&line.Text().map_err(error)?.to_string());
        text.push('\n');
    }
    Ok(text)
}

fn too_large(bitmap: &SoftwareBitmap) -> bool {
    let limit = OcrEngine::MaxImageDimension().unwrap_or(10_000) as i32;
    bitmap.PixelWidth().unwrap_or(0) > limit || bitmap.PixelHeight().unwrap_or(0) > limit
}

/// Text aus einer Bilddatei (PNG, JPG, BMP, TIFF, GIF …).
pub fn image_file(path: &str) -> Result<String, String> {
    let engine = engine()?;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path)).map_err(error)?.join().map_err(error)?;
    let stream = file.OpenAsync(FileAccessMode::Read).map_err(error)?.join().map_err(error)?;
    let decoder = BitmapDecoder::CreateAsync(&stream).map_err(error)?.join().map_err(error)?;
    let bitmap = decoder.GetSoftwareBitmapAsync().map_err(error)?.join().map_err(error)?;
    if too_large(&bitmap) {
        return Err(t("err.image_too_large").to_string());
    }
    recognize(&engine, &bitmap)
}

/// Text aus einer PDF-Datei, Seite für Seite über die Texterkennung. `progress` erhält die
/// aktuelle Seite und die Seitenzahl und bricht mit `false` ab.
pub fn pdf_file(path: &str, mut progress: impl FnMut(u32, u32) -> bool) -> Result<String, String> {
    let engine = engine()?;
    let file = StorageFile::GetFileFromPathAsync(&HSTRING::from(path)).map_err(error)?.join().map_err(error)?;
    let document = PdfDocument::LoadFromFileAsync(&file)
        .and_then(|operation| operation.join())
        .map_err(|_| t("err.pdf_unreadable").to_string())?;
    let pages = document.PageCount().map_err(error)?;
    let mut text = String::new();
    for index in 0..pages {
        if !progress(index + 1, pages) {
            return Err(t("err.cancelled").to_string());
        }
        let page = document.GetPage(index).map_err(error)?;
        let options = PdfPageRenderOptions::new().map_err(error)?;
        options.SetDestinationWidth(PDF_WIDTH).map_err(error)?;
        let stream = InMemoryRandomAccessStream::new().map_err(error)?;
        page.RenderWithOptionsToStreamAsync(&stream, &options).map_err(error)?.join().map_err(error)?;
        let decoder = BitmapDecoder::CreateAsync(&stream).map_err(error)?.join().map_err(error)?;
        let bitmap = decoder.GetSoftwareBitmapAsync().map_err(error)?.join().map_err(error)?;
        text.push_str(&recognize(&engine, &bitmap)?);
        // Leerzeile zwischen den Seiten, damit die Bereinigung sie als Absatz behandelt.
        text.push('\n');
    }
    Ok(text)
}

/// Text aus Bildpunkten im Format BGRA (4 Bytes je Punkt, Zeile für Zeile von oben).
pub fn pixels(bgra: &[u8], width: u32, height: u32) -> Result<String, String> {
    let engine = engine()?;
    let buffer = CryptographicBuffer::CreateFromByteArray(bgra).map_err(error)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        width as i32,
        height as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(error)?;
    // Kleine Ausschnitte (eine Zeile Text) erkennt die Texterkennung vergrößert deutlich besser.
    let bitmap = if height < 120 { scaled(&bitmap, 3).unwrap_or(bitmap) } else { bitmap };
    if too_large(&bitmap) {
        return Err(t("err.image_too_large").to_string());
    }
    recognize(&engine, &bitmap)
}

/// Vergrößert ein Bild um den Faktor `factor` (über einen Bildkodierer im Speicher).
fn scaled(bitmap: &SoftwareBitmap, factor: u32) -> windows::core::Result<SoftwareBitmap> {
    use windows::Graphics::Imaging::{BitmapEncoder, BitmapInterpolationMode};
    let stream = InMemoryRandomAccessStream::new()?;
    let encoder = BitmapEncoder::CreateAsync(BitmapEncoder::PngEncoderId()?, &stream)?.join()?;
    encoder.SetSoftwareBitmap(bitmap)?;
    let transform = encoder.BitmapTransform()?;
    transform.SetScaledWidth(bitmap.PixelWidth()? as u32 * factor)?;
    transform.SetScaledHeight(bitmap.PixelHeight()? as u32 * factor)?;
    transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
    encoder.FlushAsync()?.join()?;
    let decoder = BitmapDecoder::CreateAsync(&stream.cast::<windows::Storage::Streams::IRandomAccessStream>()?)?.join()?;
    decoder.GetSoftwareBitmapAsync()?.join()
}


#[cfg(test)]
mod tests {
    use super::*;

    /// Eine einseitige PDF-Datei mit Text in Helvetica (von Hand aufgebaut).
    fn pdf(text: &str) -> Vec<u8> {
        let stream = format!("BT /F1 28 Tf 72 700 Td ({text}) Tj ET");
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R /Resources << /Font << /F1 5 0 R >> >> >>".to_string(),
            format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut out = b"%PDF-1.4\n".to_vec();
        let mut offsets = Vec::new();
        for (i, object) in objects.iter().enumerate() {
            offsets.push(out.len());
            out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
        }
        let xref = out.len();
        out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
        for offset in offsets {
            out.extend(format!("{offset:010} 00000 n \n").as_bytes());
        }
        out.extend(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
        out
    }

    #[test]
    fn reads_text_from_pdf() {
        init_thread();
        let path = std::env::temp_dir().join("vorleser_ocr_test.pdf");
        std::fs::write(&path, pdf("Guten Morgen aus Freising")).unwrap();
        let mut pages = Vec::new();
        let text = pdf_file(path.to_str().unwrap(), |page, total| {
            pages.push((page, total));
            true
        });
        let _ = std::fs::remove_file(&path);
        let text = text.unwrap();
        println!("erkannt: {text:?}");
        assert_eq!(pages, vec![(1, 1)]);
        assert!(text.contains("Guten Morgen") && text.contains("Freising"), "{text:?}");
    }

    /// Schwarzer Text auf Weiß, mit GDI in Bildpunkte gezeichnet – wie ein Bildschirmausschnitt.
    #[test]
    fn reads_text_from_pixels() {
        use windows::Win32::Graphics::Gdi::*;
        init_thread();
        let (width, height) = (420i32, 40i32);
        let mut bgra = vec![0u8; (width * height * 4) as usize];
        unsafe {
            let screen = GetDC(None);
            let dc = CreateCompatibleDC(Some(screen));
            let bitmap = CreateCompatibleBitmap(screen, width, height);
            let old = SelectObject(dc, bitmap.into());
            let white = CreateSolidBrush(windows::Win32::Foundation::COLORREF(0x00FF_FFFF));
            FillRect(dc, &windows::Win32::Foundation::RECT { left: 0, top: 0, right: width, bottom: height }, white);
            let mut text: Vec<u16> = "Bildschirmtext zum Vorlesen".encode_utf16().collect();
            let mut rect = windows::Win32::Foundation::RECT { left: 6, top: 8, right: width, bottom: height };
            DrawTextW(dc, &mut text, &mut rect, DT_LEFT | DT_SINGLELINE);
            let mut info = BITMAPINFO {
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
            GetDIBits(dc, bitmap, 0, height as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS);
            SelectObject(dc, old);
            let _ = DeleteObject(bitmap.into());
            let _ = DeleteObject(white.into());
            let _ = DeleteDC(dc);
            ReleaseDC(None, screen);
        }
        for pixel in bgra.chunks_exact_mut(4) {
            pixel[3] = 255;
        }
        let text = pixels(&bgra, width as u32, height as u32).unwrap();
        println!("erkannt: {text:?}");
        assert!(text.contains("Bildschirmtext"), "{text:?}");
    }
}
