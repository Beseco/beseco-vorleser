//! KI-Stimmen mit Piper (https://github.com/rhasspy/piper, MIT). Programm und Stimmen werden
//! erst auf Wunsch geladen, per SHA-256 geprüft und unter `%LOCALAPPDATA%\Vorleser\piper`
//! abgelegt. Danach läuft alles ohne Internet.
//!
//! Beim Vorlesen läuft `piper.exe` als eigener Prozess: Vorleser schickt ihm Satz für Satz,
//! Piper schreibt je Satz eine WAV-Datei, und ein Hintergrund-Thread spielt sie nacheinander über
//! `waveOut` ab. So kann der nächste Satz entstehen, während der aktuelle läuft, und das Fenster
//! markiert beim Mitlesen den gerade gesprochenen Satz.

use std::collections::VecDeque;
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering::Relaxed};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::Media::Audio::{
    waveOutClose, waveOutOpen, waveOutPause, waveOutPrepareHeader, waveOutReset, waveOutRestart,
    waveOutSetVolume, waveOutUnprepareHeader, waveOutWrite, CALLBACK_NULL, HWAVEOUT, WAVEFORMATEX,
    WAVEHDR, WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
};
use windows::Win32::Security::Cryptography::{
    BCryptCloseAlgorithmProvider, BCryptCreateHash, BCryptDestroyHash, BCryptFinishHash,
    BCryptHashData, BCryptOpenAlgorithmProvider, BCRYPT_ALG_HANDLE, BCRYPT_HASH_HANDLE,
    BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS, BCRYPT_SHA256_ALGORITHM,
};
use windows::Win32::UI::WindowsAndMessaging::PostMessageW;

use crate::http;
use crate::textprep::{split_sentences, Sentence};
use crate::i18n::t;

/// Piper für Windows, letzte Ausgabe des Projekts (2023, MIT-Lizenz).
const ENGINE_URL: &str = "https://github.com/rhasspy/piper/releases/download/2023.11.14-2/piper_windows_amd64.zip";
const ENGINE_SHA256: &str = "f3c58906402b24f3a96d92145f58acba6d86c9b5db896d207f78dc80811efcea";
pub const ENGINE_SIZE: u64 = 22_477_236;

/// Stimmen-Archiv auf Hugging Face, auf einen festen Stand gepinnt.
const VOICES_URL: &str = "https://huggingface.co/rhasspy/piper-voices/resolve/5041a7a49ae5b3a0f30e6837769eb03fdbb231bd/";

/// Startet Prozesse ohne Konsolenfenster.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct CatalogVoice {
    /// Dateiname ohne Endung, z. B. "de_DE-thorsten-medium".
    pub key: &'static str,
    /// Pfad im Stimmen-Archiv ohne Endung.
    path: &'static str,
    pub name: &'static str,
    /// Englische Beschreibung; übersetzt steht sie in den Sprachdateien unter `ai.desc.<key>`.
    pub description_en: &'static str,
    pub lang_id: u16,
    sha256: &'static str,
    pub size: u64,
    pub license: &'static str,
}

/// Nur Stimmen, deren Trainingsdaten frei nutzbar sind (gemeinfrei bzw. CC0).
pub const CATALOG: [CatalogVoice; 5] = [
    CatalogVoice {
        key: "de_DE-thorsten-medium",
        path: "de/de_DE/thorsten/medium/de_DE-thorsten-medium",
        name: "Thorsten",
        description_en: "German, male",
        lang_id: 0x0407,
        sha256: "7e64762d8e5118bb578f2eea6207e1a35a8e0c30595010b666f983fc87bb7819",
        size: 63_201_294,
        license: "CC0",
    },
    CatalogVoice {
        key: "de_DE-thorsten-high",
        path: "de/de_DE/thorsten/high/de_DE-thorsten-high",
        name: "Thorsten HD",
        description_en: "German, male, highest quality",
        lang_id: 0x0407,
        sha256: "9df1c43c61149ef9b39e618e2b861fbe41e1fcea9390b2dac62e8761573ea4f1",
        size: 113_895_201,
        license: "CC0",
    },
    CatalogVoice {
        key: "de_DE-kerstin-low",
        path: "de/de_DE/kerstin/low/de_DE-kerstin-low",
        name: "Kerstin",
        description_en: "German, female",
        lang_id: 0x0407,
        sha256: "d352a7641892cebf2903859af94e9ba81a141110215fe3943bcda7f7da401b7a",
        size: 63_104_526,
        license: "CC0",
    },
    CatalogVoice {
        key: "en_US-ljspeech-high",
        path: "en/en_US/ljspeech/high/en_US-ljspeech-high",
        name: "Linda",
        description_en: "English, female, highest quality",
        lang_id: 0x0409,
        sha256: "5d4f08ba6a2a48c44592eed3ce56bf85e9de3dd4e20df90541ae68a8310c029a",
        size: 114_199_011,
        license: "Public Domain (LJ Speech)",
    },
    CatalogVoice {
        key: "en_US-kristin-medium",
        path: "en/en_US/kristin/medium/en_US-kristin-medium",
        name: "Kristin",
        description_en: "English, female",
        lang_id: 0x0409,
        sha256: "5849957f929cbf720c258f8458692d6103fff2f0e3d3b19c8259474bb06a18d4",
        size: 63_531_379,
        license: "Public Domain (LibriVox)",
    },
];

pub fn find(key: &str) -> Option<&'static CatalogVoice> {
    CATALOG.iter().find(|v| v.key == key)
}

fn base_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    local.join("Vorleser").join("piper")
}

/// Ordner, den die Deinstallation entfernt (siehe Installer).
fn engine_dir() -> PathBuf {
    base_dir().join("engine")
}

fn engine_exe() -> PathBuf {
    engine_dir().join("piper").join("piper.exe")
}

fn model_path(key: &str) -> PathBuf {
    base_dir().join("voices").join(format!("{key}.onnx"))
}

fn config_path(key: &str) -> PathBuf {
    base_dir().join("voices").join(format!("{key}.onnx.json"))
}

pub fn engine_installed() -> bool {
    engine_exe().is_file()
}

pub fn is_installed(key: &str) -> bool {
    engine_installed() && model_path(key).is_file() && config_path(key).is_file()
}

pub fn installed() -> impl Iterator<Item = &'static CatalogVoice> {
    CATALOG.iter().filter(|v| is_installed(v.key))
}

/// Abtastrate aus der Modellbeschreibung, z. B. `"sample_rate": 22050`.
fn sample_rate(key: &str) -> u32 {
    let json = std::fs::read_to_string(config_path(key)).unwrap_or_default();
    json.find("\"sample_rate\"")
        .and_then(|i| {
            let rest = json[i + 13..].trim_start().strip_prefix(':')?.trim_start();
            rest[..rest.find(|c: char| !c.is_ascii_digit())?].parse().ok()
        })
        .unwrap_or(22_050)
}

/// Was [`install`] gerade lädt.
#[derive(Clone, Copy, PartialEq)]
pub enum Stage {
    Engine,
    Voice,
}

/// Lädt Piper (falls noch nicht vorhanden) und die Stimme. `progress` erhält Abschnitt,
/// geladene und erwartete Bytes und bricht mit `false` ab.
pub fn install(voice: &CatalogVoice, mut progress: impl FnMut(Stage, u64, u64) -> bool) -> Result<(), String> {
    let voices = base_dir().join("voices");
    std::fs::create_dir_all(&voices).map_err(|e| e.to_string())?;
    if !engine_installed() {
        let zip = base_dir().join("piper.zip.part");
        http::download(ENGINE_URL, &zip, |done, total| progress(Stage::Engine, done, total))?;
        verify(&zip, ENGINE_SHA256)?;
        let target = engine_dir();
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        // tar.exe gehört seit Windows 10 (1803) zum System und entpackt auch ZIP-Dateien.
        let tar = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .join("System32")
            .join("tar.exe");
        let status = Command::new(tar)
            .arg("-xf")
            .arg(&zip)
            .arg("-C")
            .arg(&target)
            .creation_flags(CREATE_NO_WINDOW)
            .status()
            .map_err(|e| e.to_string())?;
        let _ = std::fs::remove_file(&zip);
        if !status.success() || !engine_installed() {
            let _ = std::fs::remove_dir_all(&target);
            return Err(t("err.unpack").to_string());
        }
    }

    let part = voices.join(format!("{}.onnx.part", voice.key));
    let url = format!("{VOICES_URL}{}.onnx", voice.path);
    http::download(&url, &part, |done, total| progress(Stage::Voice, done, total))?;
    if let Err(e) = verify(&part, voice.sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    let config = http::get_text(&format!("{VOICES_URL}{}.onnx.json", voice.path), None)
        .filter(|json| json.contains("\"sample_rate\""))
        .ok_or_else(|| t("err.model_config").to_string())?;
    std::fs::write(config_path(voice.key), config).map_err(|e| e.to_string())?;
    std::fs::rename(&part, model_path(voice.key)).map_err(|e| e.to_string())?;
    Ok(())
}

/// Löscht eine Stimme. Piper selbst bleibt, bis keine Stimme mehr übrig ist.
pub fn remove(key: &str) {
    let _ = std::fs::remove_file(model_path(key));
    let _ = std::fs::remove_file(config_path(key));
    if installed().next().is_none() {
        let _ = std::fs::remove_dir_all(engine_dir());
    }
}

fn verify(path: &Path, expected: &str) -> Result<(), String> {
    match sha256_file(path) {
        Some(hash) if hash == expected => Ok(()),
        _ => {
            let _ = std::fs::remove_file(path);
            Err(t("err.checksum").to_string())
        }
    }
}

fn sha256_file(path: &Path) -> Option<String> {
    let mut file = File::open(path).ok()?;
    unsafe {
        let mut algorithm = BCRYPT_ALG_HANDLE::default();
        BCryptOpenAlgorithmProvider(&mut algorithm, BCRYPT_SHA256_ALGORITHM, PCWSTR::null(), BCRYPT_OPEN_ALGORITHM_PROVIDER_FLAGS(0))
            .ok()
            .ok()?;
        let mut hash = BCRYPT_HASH_HANDLE::default();
        let created = BCryptCreateHash(algorithm, &mut hash, None, None, 0).is_ok();
        let mut digest = [0u8; 32];
        let mut ok = created;
        let mut buffer = vec![0u8; 1 << 20];
        while ok {
            match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => ok = BCryptHashData(hash, &buffer[..n], 0).is_ok(),
                Err(_) => ok = false,
            }
        }
        ok = ok && BCryptFinishHash(hash, &mut digest, 0).is_ok();
        if created {
            let _ = BCryptDestroyHash(hash);
        }
        let _ = BCryptCloseAlgorithmProvider(algorithm, 0);
        ok.then(|| digest.iter().map(|b| format!("{b:02x}")).collect())
    }
}

/// SAPI-Geschwindigkeit (-10 bis 10) als Piper-Lautlänge: 10 entspricht etwa dreifachem Tempo.
fn length_scale(rate: i32) -> f32 {
    3f32.powf(-(rate.clamp(-10, 10) as f32) / 10.0)
}

#[derive(Default)]
struct Status {
    /// Gerade gesprochener Satz (Anfang, Länge) und ob er seit der letzten Abfrage neu ist.
    current: Option<(u32, u32)>,
    changed: bool,
    finished: Option<Result<(), String>>,
}

struct Shared {
    cancel: AtomicBool,
    paused: AtomicBool,
    device: AtomicIsize,
    status: Mutex<Status>,
}

/// Eine laufende Ausgabe mit einer KI-Stimme.
pub struct Session {
    shared: Arc<Shared>,
}

/// Was seit der letzten Abfrage passiert ist.
pub struct Progress {
    pub sentence: Option<(u32, u32)>,
    pub finished: Option<Result<(), String>>,
}

impl Session {
    /// Beginnt mit dem Vorlesen. `hwnd` erhält `msg`, sobald ein neuer Satz beginnt und am Ende.
    pub fn start(key: &str, text: &str, rate: i32, volume: u32, hwnd: HWND, msg: u32) -> Session {
        let shared = Arc::new(Shared {
            cancel: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            device: AtomicIsize::new(0),
            status: Mutex::new(Status::default()),
        });
        let worker = shared.clone();
        let key = key.to_string();
        let sentences = split_sentences(text);
        let target = hwnd.0 as isize;
        std::thread::spawn(move || {
            let notify = || unsafe {
                let _ = PostMessageW(Some(HWND(target as *mut _)), msg, WPARAM(0), LPARAM(0));
            };
            let result = play(&worker, &key, &sentences, rate, volume, &notify);
            worker.status.lock().unwrap().finished = Some(result);
            notify();
        });
        Session { shared }
    }

    pub fn pause(&self) {
        self.shared.paused.store(true, Relaxed);
        if let Some(device) = self.device() {
            unsafe {
                waveOutPause(device);
            }
        }
    }

    pub fn resume(&self) {
        self.shared.paused.store(false, Relaxed);
        if let Some(device) = self.device() {
            unsafe {
                waveOutRestart(device);
            }
        }
    }

    pub fn poll(&self) -> Progress {
        let mut status = self.shared.status.lock().unwrap();
        let sentence = if std::mem::take(&mut status.changed) { status.current } else { None };
        Progress { sentence, finished: status.finished.take() }
    }

    fn device(&self) -> Option<HWAVEOUT> {
        let raw = self.shared.device.load(Relaxed);
        (raw != 0).then(|| HWAVEOUT(raw as _))
    }
}

impl Drop for Session {
    /// Beendet die Ausgabe; der Thread räumt danach selbst auf.
    fn drop(&mut self) {
        self.shared.cancel.store(true, Relaxed);
        if let Some(device) = self.device() {
            unsafe {
                waveOutReset(device);
            }
        }
    }
}

fn start_engine(key: &str, rate: i32, output_dir: &Path) -> std::io::Result<Child> {
    Command::new(engine_exe())
        .arg("--model")
        .arg(model_path(key))
        .arg("--output_dir")
        .arg(output_dir)
        .arg("--length_scale")
        .arg(format!("{:.3}", length_scale(rate)))
        .arg("-q")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
}

/// Liest die Audiodaten (16 Bit, mono) aus einer WAV-Datei von Piper.
fn read_pcm(path: &Path) -> Option<Vec<u8>> {
    let bytes = std::fs::read(path).ok()?;
    let mut pos = 12;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().ok()?) as usize;
        if id == b"data" {
            let end = (pos + 8 + size).min(bytes.len());
            return Some(bytes[pos + 8..end].to_vec());
        }
        pos += 8 + size + (size & 1);
    }
    None
}

/// Ein an `waveOut` übergebener Puffer. Kopf und Daten müssen bis zum Ende der Wiedergabe an
/// derselben Stelle im Speicher bleiben, daher `Box` und ein unverändertes `Vec`.
struct Buffer {
    header: Box<WAVEHDR>,
    _data: Vec<u8>,
    range: (u32, u32),
}

fn play(shared: &Shared, key: &str, sentences: &[Sentence], rate: i32, volume: u32, notify: &dyn Fn()) -> Result<(), String> {
    let output = std::env::temp_dir().join(format!("vorleser-piper-{}", std::process::id()));
    std::fs::create_dir_all(&output).map_err(|e| e.to_string())?;
    let mut engine = start_engine(key, rate, &output).map_err(|e| e.to_string())?;
    let mut stdin = engine.stdin.take().ok_or_else(|| t("err.piper_failed").to_string())?;
    let mut stdout = BufReader::new(engine.stdout.take().ok_or_else(|| t("err.piper_failed").to_string())?);

    let rate_hz = sample_rate(key);
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: rate_hz,
        nAvgBytesPerSec: rate_hz * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let mut device = HWAVEOUT::default();
    if unsafe { waveOutOpen(Some(&mut device), WAVE_MAPPER, &format, None, None, CALLBACK_NULL) } != 0 {
        let _ = engine.kill();
        return Err(t("err.no_audio_device").to_string());
    }
    let level = volume.min(100) * 0xFFFF / 100;
    unsafe {
        waveOutSetVolume(Some(device), level | (level << 16));
        if shared.paused.load(Relaxed) {
            waveOutPause(device);
        }
    }
    shared.device.store(device.0 as isize, Relaxed);

    let header_size = std::mem::size_of::<WAVEHDR>() as u32;
    let mut queue: VecDeque<Buffer> = VecDeque::new();
    let mut next = 0;
    let mut playing = None;
    let mut result = Ok(());
    while !shared.cancel.load(Relaxed) {
        // Höchstens drei Sätze im Voraus, damit Pausen und Stopp sofort greifen.
        if queue.len() < 3 && next < sentences.len() {
            let sentence = &sentences[next];
            next += 1;
            let line = format!("{}\n", sentence.text);
            let mut path = String::new();
            if stdin.write_all(line.as_bytes()).and_then(|()| stdin.flush()).is_err()
                || stdout.read_line(&mut path).unwrap_or(0) == 0
            {
                result = Err(t("err.piper_stopped").to_string());
                break;
            }
            let path = PathBuf::from(path.trim());
            let data = read_pcm(&path).unwrap_or_default();
            let _ = std::fs::remove_file(&path);
            if !data.is_empty() && !shared.cancel.load(Relaxed) {
                let mut header = Box::new(WAVEHDR {
                    lpData: windows::core::PSTR(data.as_ptr() as *mut u8),
                    dwBufferLength: data.len() as u32,
                    ..Default::default()
                });
                unsafe {
                    waveOutPrepareHeader(device, &mut *header, header_size);
                    waveOutWrite(device, &mut *header, header_size);
                }
                queue.push_back(Buffer { header, _data: data, range: (sentence.start, sentence.len) });
            }
            continue;
        }
        while queue.front().is_some_and(|b| b.header.dwFlags & WHDR_DONE != 0) {
            let mut done = queue.pop_front().unwrap();
            unsafe {
                waveOutUnprepareHeader(device, &mut *done.header, header_size);
            }
        }
        let now = queue.front().map(|b| b.range);
        if now.is_some() && now != playing {
            playing = now;
            let mut status = shared.status.lock().unwrap();
            status.current = now;
            status.changed = true;
            drop(status);
            notify();
        }
        if queue.is_empty() && next >= sentences.len() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    shared.device.store(0, Relaxed);
    unsafe {
        waveOutReset(device);
        for mut buffer in queue {
            waveOutUnprepareHeader(device, &mut *buffer.header, header_size);
        }
        waveOutClose(device);
    }
    drop(stdin);
    let _ = engine.kill();
    let _ = engine.wait();
    result
}

/// Schreibt `text` als WAV-Datei. Läuft synchron, gedacht für einen Hintergrund-Thread.
pub fn save_wav(key: &str, rate: i32, text: &str, path: &str) -> Result<(), String> {
    let lines: Vec<String> = split_sentences(text).into_iter().map(|s| s.text).collect();
    let mut child = Command::new(engine_exe())
        .arg("--model")
        .arg(model_path(key))
        .arg("--output_file")
        .arg(path)
        .arg("--length_scale")
        .arg(format!("{:.3}", length_scale(rate)))
        .arg("-q")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| e.to_string())?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all((lines.join("\n") + "\n").as_bytes()).map_err(|e| e.to_string())?;
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(t("err.piper_failed").to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_rate_to_length_scale() {
        assert!((length_scale(0) - 1.0).abs() < 1e-6);
        assert!(length_scale(10) < 0.34 && length_scale(-10) > 2.9);
    }

    /// Probe von Hand mit echtem Download (rund 85 MB) in einen Testordner, stumm:
    /// `cargo test installs_and_speaks -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn installs_and_speaks() {
        let dir = std::env::temp_dir().join("vorleser-piper-test");
        std::env::set_var("LOCALAPPDATA", &dir);
        let voice = find("de_DE-thorsten-medium").unwrap();
        let start = std::time::Instant::now();
        let mut last = (Stage::Engine, 0u64);
        install(voice, |stage, done, total| {
            if stage != last.0 || done - last.1 > 20_000_000 || done == total {
                println!("{:?} {done}/{total}", stage == Stage::Engine);
                last = (stage, done);
            }
            true
        })
        .unwrap();
        println!("installiert in {:?}", start.elapsed());
        assert!(is_installed(voice.key));
        assert_eq!(sample_rate(voice.key), 22_050);

        let text = "Guten Tag. Das ist ein Test der KI-Stimme. Und ein dritter Satz.";
        let session = Session::start(voice.key, text, 2, 0, HWND::default(), 0);
        let start = std::time::Instant::now();
        let mut seen = Vec::new();
        let result = loop {
            std::thread::sleep(Duration::from_millis(50));
            let progress = session.poll();
            seen.extend(progress.sentence);
            if let Some(result) = progress.finished {
                break result;
            }
            assert!(start.elapsed().as_secs() < 60, "hängt");
        };
        println!("gesprochen in {:?}: {seen:?} {result:?}", start.elapsed());
        assert!(result.is_ok());
        assert_eq!(seen, vec![(0, 10), (11, 31), (43, 21)]);

        let wav = dir.join("test.wav");
        save_wav(voice.key, 0, text, wav.to_str().unwrap()).unwrap();
        assert!(std::fs::metadata(&wav).unwrap().len() > 100_000);
    }

    #[test]
    fn catalog_is_consistent() {
        for voice in &CATALOG {
            assert!(voice.path.ends_with(voice.key));
            assert_eq!(voice.sha256.len(), 64);
        }
    }
}
