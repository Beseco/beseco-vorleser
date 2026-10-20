//! MP3-Export mit dem MP3-Encoder, den Windows über Media Foundation mitbringt. Vorleser
//! schreibt zuerst eine WAV-Datei und wandelt sie dann um; so funktioniert es für Windows- und
//! KI-Stimmen gleich.

use std::path::Path;

use windows::core::HSTRING;
use windows::Win32::Media::MediaFoundation::{
    IMFAttributes, MFAudioFormat_MP3, MFAudioFormat_PCM, MFCreateAttributes, MFCreateMediaType,
    MFCreateMemoryBuffer, MFCreateSample, MFCreateSinkWriterFromURL, MFMediaType_Audio, MFShutdown,
    MFStartup, MFTranscodeContainerType_MP3, MFSTARTUP_LITE, MF_MT_AUDIO_AVG_BYTES_PER_SECOND,
    MF_MT_AUDIO_BITS_PER_SAMPLE, MF_MT_AUDIO_BLOCK_ALIGNMENT, MF_MT_AUDIO_NUM_CHANNELS,
    MF_MT_AUDIO_SAMPLES_PER_SECOND, MF_MT_MAJOR_TYPE, MF_MT_SUBTYPE, MF_TRANSCODE_CONTAINERTYPE,
    MF_VERSION,
};

/// Abtastrate der MP3-Datei; der Encoder von Windows nimmt nur 32, 44,1 oder 48 kHz an.
const MP3_RATE: u32 = 44_100;
/// 96 kbit/s, für Sprache mehr als genug.
const MP3_BYTES_PER_SECOND: u32 = 12_000;

/// 16-Bit-PCM-Audio aus einer WAV-Datei.
struct Pcm {
    channels: u16,
    rate: u32,
    samples: Vec<i16>,
}

fn read_wav(bytes: &[u8]) -> Option<Pcm> {
    if bytes.get(0..4)? != b"RIFF" || bytes.get(8..12)? != b"WAVE" {
        return None;
    }
    let (mut format, mut data) = (None, None);
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        let body = &bytes[pos + 8..(pos + 8 + size).min(bytes.len())];
        match id {
            b"fmt " if body.len() >= 16 => format = Some(body),
            b"data" => data = Some(body),
            _ => {}
        }
        pos += 8 + size + (size & 1);
    }
    let (format, data) = (format?, data?);
    let tag = u16::from_le_bytes([format[0], format[1]]);
    let channels = u16::from_le_bytes([format[2], format[3]]);
    let rate = u32::from_le_bytes(format[4..8].try_into().ok()?);
    let bits = u16::from_le_bytes([format[14], format[15]]);
    // 1 = PCM; 0xFFFE = WAVE_FORMAT_EXTENSIBLE, das SAPI je nach Stimme ebenfalls schreibt.
    if !(tag == 1 || tag == 0xFFFE) || bits != 16 || channels == 0 || rate == 0 {
        return None;
    }
    let samples = data.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
    Some(Pcm { channels, rate, samples })
}

/// Rechnet auf `target` Hz um (lineare Interpolation; für Sprache unhörbar).
fn resample(pcm: &Pcm, target: u32) -> Vec<i16> {
    if pcm.rate == target {
        return pcm.samples.clone();
    }
    let channels = pcm.channels as usize;
    let frames = pcm.samples.len() / channels;
    if frames == 0 {
        return Vec::new();
    }
    let out_frames = (frames as u64 * target as u64 / pcm.rate as u64) as usize;
    let step = pcm.rate as f64 / target as f64;
    let mut out = Vec::with_capacity(out_frames * channels);
    for i in 0..out_frames {
        let position = i as f64 * step;
        let index = (position as usize).min(frames - 1);
        let next = (index + 1).min(frames - 1);
        let fraction = position - index as f64;
        for c in 0..channels {
            let a = pcm.samples[index * channels + c] as f64;
            let b = pcm.samples[next * channels + c] as f64;
            out.push((a + (b - a) * fraction).round() as i16);
        }
    }
    out
}

/// Wandelt eine WAV-Datei (16 Bit PCM) in eine MP3-Datei um.
pub fn wav_to_mp3(wav: &Path, mp3: &str) -> Result<(), String> {
    let bytes = std::fs::read(wav).map_err(|e| e.to_string())?;
    let pcm = read_wav(&bytes).ok_or("WAV-Format nicht unterstützt")?;
    let samples = resample(&pcm, MP3_RATE);
    // Media Foundation braucht COM auf diesem Thread; der Aufrufer hat es bereits initialisiert.
    unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_LITE).map_err(|e| e.message())?;
        let result = encode(&samples, pcm.channels as u32, mp3);
        let _ = MFShutdown();
        result.map_err(|e| e.message())
    }
}

unsafe fn encode(samples: &[i16], channels: u32, path: &str) -> windows::core::Result<()> {
    let mut attributes: Option<IMFAttributes> = None;
    MFCreateAttributes(&mut attributes, 1)?;
    let attributes = attributes.ok_or_else(windows::core::Error::empty)?;
    attributes.SetGUID(&MF_TRANSCODE_CONTAINERTYPE, &MFTranscodeContainerType_MP3)?;
    let writer = MFCreateSinkWriterFromURL(&HSTRING::from(path), None, &attributes)?;

    let output = MFCreateMediaType()?;
    output.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
    output.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_MP3)?;
    output.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?;
    output.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, MP3_RATE)?;
    output.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MP3_BYTES_PER_SECOND)?;
    let stream = writer.AddStream(&output)?;

    let block = channels * 2;
    let input = MFCreateMediaType()?;
    input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)?;
    input.SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)?;
    input.SetUINT32(&MF_MT_AUDIO_NUM_CHANNELS, channels)?;
    input.SetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND, MP3_RATE)?;
    input.SetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE, 16)?;
    input.SetUINT32(&MF_MT_AUDIO_BLOCK_ALIGNMENT, block)?;
    input.SetUINT32(&MF_MT_AUDIO_AVG_BYTES_PER_SECOND, MP3_RATE * block)?;
    writer.SetInputMediaType(stream, &input, None)?;
    writer.BeginWriting()?;

    // Je eine halbe Sekunde pro Block; Zeiten in 100-Nanosekunden-Einheiten.
    let frames_per_chunk = (MP3_RATE / 2) as usize;
    let mut written = 0u64;
    for chunk in samples.chunks(frames_per_chunk * channels as usize) {
        let bytes = std::slice::from_raw_parts(chunk.as_ptr() as *const u8, chunk.len() * 2);
        let buffer = MFCreateMemoryBuffer(bytes.len() as u32)?;
        let mut target = std::ptr::null_mut();
        buffer.Lock(&mut target, None, None)?;
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), target, bytes.len());
        buffer.Unlock()?;
        buffer.SetCurrentLength(bytes.len() as u32)?;
        let frames = (chunk.len() / channels as usize) as u64;
        let sample = MFCreateSample()?;
        sample.AddBuffer(&buffer)?;
        sample.SetSampleTime((written * 10_000_000 / MP3_RATE as u64) as i64)?;
        sample.SetSampleDuration((frames * 10_000_000 / MP3_RATE as u64) as i64)?;
        writer.WriteSample(stream, &sample)?;
        written += frames;
    }
    writer.Finalize()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wav(rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        let mut out = b"RIFF".to_vec();
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(1u16.to_le_bytes()); // PCM
        out.extend(1u16.to_le_bytes()); // mono
        out.extend(rate.to_le_bytes());
        out.extend((rate * 2).to_le_bytes());
        out.extend(2u16.to_le_bytes());
        out.extend(16u16.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    #[test]
    fn reads_and_resamples_wav() {
        let pcm = read_wav(&wav(22_050, &[0, 1000, 2000, 3000])).unwrap();
        assert_eq!((pcm.channels, pcm.rate, pcm.samples.len()), (1, 22_050, 4));
        let doubled = resample(&pcm, 44_100);
        assert_eq!(doubled.len(), 8);
        assert_eq!(&doubled[..4], &[0, 500, 1000, 1500]);
        assert!(read_wav(b"keine WAV-Datei").is_none());
    }

    /// Ganzer Weg wie im Programm: Windows-Stimme schreibt WAV, daraus wird MP3.
    #[test]
    fn sapi_voice_to_mp3() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let dir = std::env::temp_dir();
        let source = dir.join("vorleser_sapi_test.wav");
        let target = dir.join("vorleser_sapi_test.mp3");
        crate::tts::save_wav(None, 0, "Das ist ein Test für den MP3-Export.", &source.to_string_lossy()).unwrap();
        let format = read_wav(&std::fs::read(&source).unwrap()).map(|p| (p.channels, p.rate));
        wav_to_mp3(&source, target.to_str().unwrap()).unwrap();
        let size = std::fs::metadata(&target).unwrap().len();
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&target);
        assert!(format.is_some(), "SAPI-WAV nicht lesbar");
        assert!(size > 5_000, "MP3 zu klein: {size}");
    }

    /// Echte Umwandlung mit dem Encoder von Windows: zwei Sekunden Ton.
    #[test]
    fn encodes_mp3() {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let tone: Vec<i16> = (0..44_100).map(|i| ((i as f64 * 0.06).sin() * 8000.0) as i16).collect();
        let dir = std::env::temp_dir();
        let source = dir.join("vorleser_mp3_test.wav");
        let target = dir.join("vorleser_mp3_test.mp3");
        std::fs::write(&source, wav(22_050, &tone)).unwrap();
        wav_to_mp3(&source, target.to_str().unwrap()).unwrap();
        let bytes = std::fs::read(&target).unwrap();
        let _ = std::fs::remove_file(&source);
        let _ = std::fs::remove_file(&target);
        // 2 s bei 96 kbit/s sind rund 24 KB; eine MP3-Datei beginnt mit ID3-Kopf oder Rahmen-Sync.
        assert!(bytes.len() > 15_000 && bytes.len() < 40_000, "Größe {}", bytes.len());
        let frame_sync = bytes[0] == 0xFF && bytes[1] & 0xE0 == 0xE0;
        assert!(bytes.starts_with(b"ID3") || frame_sync);
    }
}
