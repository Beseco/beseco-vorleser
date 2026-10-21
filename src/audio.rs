//! Audiodateien für die Abschrift: entpackt mit Media Foundation, was Windows abspielen kann
//! (MP3, M4A/AAC, WAV, WMA, FLAC, auch Ton aus MP4-Videos), rechnet auf 16 kHz Mono um und
//! schneidet an Sprechpausen in Abschnitte, die Whisper einzeln erkennt.

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, IMFSample, MFAudioFormat_PCM, MFCreateMediaType, MFCreateSourceReaderFromURL, MFMediaType_Audio,
    MFShutdown, MFStartup, MFSTARTUP_LITE, MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_PD_DURATION, MF_SOURCE_READERF_ENDOFSTREAM,
    MF_SOURCE_READER_ALL_STREAMS, MF_SOURCE_READER_FIRST_AUDIO_STREAM, MF_SOURCE_READER_MEDIASOURCE, MF_VERSION,
};

use crate::i18n::t;

/// Abtastrate für Whisper.
pub const RATE: u32 = 16_000;

/// Dateiendungen, die als Aufnahme gelten.
pub const EXTENSIONS: [&str; 11] = ["mp3", "m4a", "aac", "wav", "wma", "flac", "ogg", "opus", "mp4", "m4v", "webm"];

pub fn is_audio(path: &str) -> bool {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_lowercase();
    EXTENSIONS.contains(&ext.as_str())
}

/// Entpackt `path` zu 16-kHz-Mono-Samples. `progress` erhält gelesene und gesamte Sekunden
/// (gesamt 0, wenn unbekannt) und bricht mit `false` ab.
pub fn decode(path: &str, mut progress: impl FnMut(f64, f64) -> bool) -> Result<Vec<i16>, String> {
    let unreadable = || t("audio.err_unreadable").to_string();
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_LITE).map_err(|e| e.message())?;
        let result = (|| {
            let reader = MFCreateSourceReaderFromURL(&HSTRING::from(path), None::<&IMFAttributes>).map_err(|_| unreadable())?;
            let first_audio = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
            let _ = reader.SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false);
            reader.SetStreamSelection(first_audio, true).map_err(|_| t("audio.err_no_sound").to_string())?;
            // PCM in der Abtastrate der Datei; umgerechnet wird danach (das geht überall).
            let wanted = MFCreateMediaType().map_err(|e| e.message())?;
            wanted.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio).map_err(|e| e.message())?;
            wanted.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM).map_err(|e| e.message())?;
            wanted.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16).map_err(|e| e.message())?;
            reader.SetCurrentMediaType(first_audio, None, &wanted).map_err(|_| unreadable())?;
            let actual = reader.GetCurrentMediaType(first_audio).map_err(|_| unreadable())?;
            let channels = actual.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS).unwrap_or(1).max(1) as usize;
            let rate = actual.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND).unwrap_or(RATE).max(1);
            let total = reader
                .GetPresentationAttribute(MF_SOURCE_READER_MEDIASOURCE.0 as u32, &MF_PD_DURATION)
                .ok()
                .and_then(|value| u64::try_from(&value).ok())
                .map_or(0.0, |ticks| ticks as f64 / 10_000_000.0);
            let mut mono = Vec::new();
            loop {
                let mut flags = 0u32;
                let mut sample: Option<IMFSample> = None;
                reader.ReadSample(first_audio, 0, None, Some(&mut flags), None, Some(&mut sample)).map_err(|_| unreadable())?;
                if let Some(sample) = sample {
                    let buffer = sample.ConvertToContiguousBuffer().map_err(|e| e.message())?;
                    let mut data = std::ptr::null_mut();
                    let mut length = 0u32;
                    buffer.Lock(&mut data, None, Some(&mut length)).map_err(|e| e.message())?;
                    let bytes = std::slice::from_raw_parts(data, length as usize);
                    // Kanäle zu Mono mitteln.
                    for frame in bytes.chunks_exact(2 * channels) {
                        let sum: i32 = frame.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]]) as i32).sum();
                        mono.push((sum / channels as i32) as i16);
                    }
                    let _ = buffer.Unlock();
                    if !progress(mono.len() as f64 / rate as f64, total) {
                        return Err(t("err.cancelled").to_string());
                    }
                }
                if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                    break;
                }
            }
            if mono.is_empty() {
                return Err(t("audio.err_no_sound").to_string());
            }
            Ok(resample(&mono, rate, RATE))
        })();
        let _ = MFShutdown();
        result
    }
}

/// Lineare Umrechnung der Abtastrate; für Sprache bei 16 kHz ausreichend. Vorher wird grob
/// tiefpassgefiltert, damit beim Verkleinern keine Störgeräusche entstehen.
fn resample(samples: &[i16], from: u32, to: u32) -> Vec<i16> {
    if from == to {
        return samples.to_vec();
    }
    let ratio = from as f64 / to as f64;
    let smoothed: Vec<f64> = if ratio > 1.0 {
        let width = ratio.round().max(1.0) as usize;
        let mut sum = 0f64;
        let mut out = Vec::with_capacity(samples.len());
        for (i, &s) in samples.iter().enumerate() {
            sum += s as f64;
            if i >= width {
                sum -= samples[i - width] as f64;
            }
            out.push(sum / width.min(i + 1) as f64);
        }
        out
    } else {
        samples.iter().map(|&s| s as f64).collect()
    };
    let count = (samples.len() as f64 / ratio) as usize;
    (0..count)
        .map(|i| {
            let pos = i as f64 * ratio;
            let index = pos as usize;
            let frac = pos - index as f64;
            let a = smoothed[index.min(smoothed.len() - 1)];
            let b = smoothed[(index + 1).min(smoothed.len() - 1)];
            (a + (b - a) * frac).round().clamp(i16::MIN as f64, i16::MAX as f64) as i16
        })
        .collect()
}

/// 20 ms je Rahmen.
const FRAME: usize = (RATE / 50) as usize;

/// Teilt eine Aufnahme in Abschnitte von 15 bis 28 Sekunden, jeweils an der leisesten Stelle
/// (meist einer Sprechpause). Ganz stille Abschnitte fallen weg. Liefert Anfang und Ende.
pub fn sections(samples: &[i16]) -> Vec<(usize, usize)> {
    let rms: Vec<f32> = samples
        .chunks(FRAME)
        .map(|f| (f.iter().map(|&s| (s as f32) * (s as f32)).sum::<f32>() / f.len() as f32).sqrt())
        .collect();
    // Über 300 ms geglättet, damit eine einzelne leise Silbe nicht als Pause zählt.
    let smooth: Vec<f32> = (0..rms.len())
        .map(|i| {
            let from = i.saturating_sub(7);
            let to = (i + 8).min(rms.len());
            rms[from..to].iter().sum::<f32>() / (to - from) as f32
        })
        .collect();
    // Sprache enthält ein Abschnitt, wenn er irgendwo deutlich über Rauschen liegt (fest und im
    // Verhältnis zur lautesten Stelle; eine Schätzung des Grundrauschens würde bei Aufnahmen
    // ohne Pausen die Sprache selbst für Rauschen halten).
    let peak = rms.iter().copied().fold(0.0f32, f32::max);
    let silence = (peak * 0.05).max(250.0);
    let (min, max) = (15 * 50, 28 * 50);
    let mut result = Vec::new();
    let mut start = 0;
    while start < rms.len() {
        let end = if rms.len() - start <= max {
            rms.len()
        } else {
            let window = start + min..start + max;
            window.clone().min_by(|a, b| smooth[*a].total_cmp(&smooth[*b])).unwrap_or(start + max)
        };
        if rms[start..end].iter().any(|v| *v > silence) {
            result.push((start * FRAME, (end * FRAME).min(samples.len())));
        }
        start = end;
    }
    result
}

/// Zeitmarke wie „01:23“ bzw. „1:02:03“.
pub fn timestamp(seconds: f64) -> String {
    let total = seconds.max(0.0) as u64;
    let (h, m, s) = (total / 3600, total / 60 % 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m:02}:{s:02}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn speech(seconds: f64) -> Vec<i16> {
        (0..(seconds * RATE as f64) as usize).map(|i| ((i as f64 * 0.2).sin() * 3000.0) as i16).collect()
    }

    #[test]
    fn cuts_at_pauses() {
        let mut samples = speech(20.0);
        samples.extend(vec![0i16; RATE as usize]);
        samples.extend(speech(20.0));
        let parts = sections(&samples);
        assert_eq!(parts.len(), 2, "{parts:?}");
        // Geschnitten wird in der Pause nach 20 Sekunden.
        let cut = parts[0].1 as f64 / RATE as f64;
        assert!((20.0..21.0).contains(&cut), "{cut}");
        assert_eq!(parts[1].1, samples.len());
    }

    #[test]
    fn drops_silence_and_cuts_long_speech() {
        let mut samples = vec![0i16; 40 * RATE as usize];
        samples.extend(speech(70.0));
        let parts = sections(&samples);
        assert!(parts.iter().all(|(a, b)| (b - a) as f64 / RATE as f64 <= 28.01));
        assert!(parts.first().is_some_and(|(a, _)| *a >= 28 * RATE as usize), "{parts:?}");
    }

    #[test]
    fn resamples() {
        let input: Vec<i16> = (0..48_000).map(|i| (i % 100) as i16).collect();
        assert_eq!(resample(&input, 48_000, 16_000).len(), 16_000);
        assert_eq!(resample(&input, 16_000, 16_000).len(), 48_000);
        assert_eq!(timestamp(83.0), "01:23");
        assert_eq!(timestamp(3723.0), "1:02:03");
        assert!(is_audio(r"C:\Aufnahmen\Besprechung.M4A"));
        assert!(!is_audio("brief.docx"));
    }

    /// Entpackt eine echte Datei: `VORLESER_AUDIO=<Datei>`, dann
    /// `cargo test decodes_file -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn decodes_file() {
        let path = std::env::var("VORLESER_AUDIO").unwrap();
        let start = std::time::Instant::now();
        let samples = decode(&path, |_, _| true).unwrap();
        println!("{:.1} s Audio in {:?}, {} Abschnitte", samples.len() as f64 / RATE as f64, start.elapsed(), sections(&samples).len());
    }

    /// Dieselbe Aufnahme als MP3 (mit dem MP3-Export des Vorlesers) muss gleich lang sein.
    #[test]
    #[ignore]
    fn decodes_mp3() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let wav = std::env::var("VORLESER_AUDIO").unwrap();
        let mp3 = std::env::temp_dir().join("vorleser-test.mp3");
        crate::mp3::wav_to_mp3(Path::new(&wav), &mp3.display().to_string()).unwrap();
        let a = decode(&wav, |_, _| true).unwrap();
        let b = decode(&mp3.display().to_string(), |_, _| true).unwrap();
        println!("WAV {:.2} s, MP3 {:.2} s", a.len() as f64 / RATE as f64, b.len() as f64 / RATE as f64);
        assert!((a.len() as f64 - b.len() as f64).abs() / (RATE as f64) < 0.2);
        let _ = std::fs::remove_file(mp3);
    }
}
