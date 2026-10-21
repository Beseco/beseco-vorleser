//! Diktieren mit lokaler Spracherkennung (whisper.cpp, MIT-Lizenz). Programm und Modell werden
//! auf Wunsch geladen, per SHA-256 geprüft und unter `%LOCALAPPDATA%\Vorleser\whisper` abgelegt;
//! erkannt wird danach ohne Internet.
//!
//! Ablauf: Das Mikrofon nimmt mit 16 kHz auf, eine einfache Sprechpausen-Erkennung schneidet die
//! Aufnahme in Abschnitte, und jeder Abschnitt geht an einen lokal laufenden `whisper-server`,
//! der das Modell im Speicher behält. So erscheint der Text kurz nach jeder Sprechpause.

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering::Relaxed};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    waveInAddBuffer, waveInClose, waveInGetDevCapsW, waveInGetNumDevs, waveInOpen, waveInPrepareHeader, waveInReset,
    waveInStart, waveInStop, waveInUnprepareHeader, CALLBACK_NULL, HWAVEIN, WAVEFORMATEX, WAVEHDR, WAVEINCAPSW,
    WAVE_FORMAT_PCM, WAVE_MAPPER, WHDR_DONE,
};

use crate::http;
use crate::i18n::t;
use crate::json;
use crate::piper::{unpack, verify};

/// whisper.cpp für Windows (CPU), Build b5454.
const ENGINE_URL: &str = "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip";
const ENGINE_SHA256: &str = "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb";
pub const ENGINE_SIZE: u64 = 8_928_640;
/// Modelle auf Hugging Face, auf einen festen Stand gepinnt.
const MODELS_URL: &str = "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const RATE: u32 = 16_000;

pub struct Model {
    pub key: &'static str,
    file: &'static str,
    sha256: &'static str,
    pub size: u64,
}

/// Vom schnellsten zum genauesten.
pub const MODELS: [Model; 3] = [
    Model { key: "base", file: "ggml-base.bin", sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe", size: 147_951_465 },
    Model { key: "small", file: "ggml-small.bin", sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b", size: 487_601_967 },
    Model { key: "turbo", file: "ggml-large-v3-turbo-q5_0.bin", sha256: "394221709cd5ad1f40c46e6031ca61bce88931e6e088c188294c6d5a55ffa7e2", size: 574_041_195 },
];

fn base_dir() -> PathBuf {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    local.join("Vorleser").join("whisper")
}

fn engine_dir() -> PathBuf {
    base_dir().join("engine")
}

fn server_exe() -> PathBuf {
    engine_dir().join("Release").join("whisper-server.exe")
}

fn model_path(model: &Model) -> PathBuf {
    base_dir().join("models").join(model.file)
}

pub fn engine_installed() -> bool {
    server_exe().is_file()
}

pub fn is_installed(model: &Model) -> bool {
    engine_installed() && model_path(model).is_file()
}

/// Das genaueste installierte Modell.
pub fn best_installed() -> Option<&'static Model> {
    MODELS.iter().rev().find(|m| is_installed(m))
}

/// Lädt whisper.cpp (falls nötig) und das Modell. `progress`: Programm (false) oder Modell
/// (true), geladene und erwartete Bytes; `false` bricht ab.
pub fn install(model: &Model, mut progress: impl FnMut(bool, u64, u64) -> bool) -> Result<(), String> {
    let models = base_dir().join("models");
    std::fs::create_dir_all(&models).map_err(|e| e.to_string())?;
    if !engine_installed() {
        let zip = base_dir().join("whisper.zip.part");
        http::download(ENGINE_URL, &zip, |done, total| progress(false, done, total))?;
        verify(&zip, ENGINE_SHA256)?;
        let target = engine_dir();
        let _ = std::fs::remove_dir_all(&target);
        std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
        let unpacked = unpack(&zip, &target);
        let _ = std::fs::remove_file(&zip);
        if !unpacked || !engine_installed() {
            let _ = std::fs::remove_dir_all(&target);
            return Err(t("err.unpack").to_string());
        }
    }
    let part = models.join(format!("{}.part", model.file));
    http::download(&format!("{MODELS_URL}{}", model.file), &part, |done, total| progress(true, done, total))?;
    verify(&part, model.sha256)?;
    std::fs::rename(&part, model_path(model)).map_err(|e| e.to_string())
}

pub fn remove(model: &Model) {
    let _ = std::fs::remove_file(model_path(model));
    if best_installed().is_none() {
        let _ = std::fs::remove_dir_all(engine_dir());
    }
}

/// Ein laufender `whisper-server` mit geladenem Modell.
pub struct Server {
    child: Child,
    port: u16,
}

impl Server {
    /// Startet den Server und wartet, bis das Modell geladen ist (einige Sekunden).
    pub fn start(model: &Model, language: &str) -> Result<Server, String> {
        // Einen freien Port suchen.
        let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).and_then(|l| l.local_addr()).map(|a| a.port()).map_err(|e| e.to_string())?;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).clamp(2, 8);
        let child = Command::new(server_exe())
            .arg("-m")
            .arg(model_path(model))
            .args(["--host", "127.0.0.1", "--port", &port.to_string(), "-l", language, "-t", &threads.to_string(), "-nt"])
            .current_dir(server_exe().parent().unwrap_or(&engine_dir()))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(CREATE_NO_WINDOW)
            .spawn()
            .map_err(|e| format!("{} ({e})", t("err.dictation_start")))?;
        let mut server = Server { child, port };
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
        let deadline = Instant::now() + Duration::from_secs(60);
        loop {
            if TcpStream::connect_timeout(&address, Duration::from_millis(300)).is_ok() {
                return Ok(server);
            }
            if server.child.try_wait().ok().flatten().is_some() || Instant::now() > deadline {
                return Err(t("err.dictation_start").to_string());
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    /// Erkennt den Text in 16-kHz-Audio. Läuft über eine direkte Verbindung zu 127.0.0.1,
    /// damit kein Proxy dazwischenkommt.
    pub fn transcribe(&self, samples: &[i16]) -> Result<String, String> {
        let wav = wav_bytes(samples);
        let boundary = "----VorleserDiktat7d3a";
        let mut body = Vec::new();
        body.extend(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"diktat.wav\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
        body.extend(&wav);
        body.extend(format!("\r\n--{boundary}\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n--{boundary}--\r\n").as_bytes());
        let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)).map_err(|e| e.to_string())?;
        stream.set_read_timeout(Some(Duration::from_secs(300))).map_err(|e| e.to_string())?;
        let head = format!(
            "POST /inference HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            self.port,
            body.len()
        );
        stream.write_all(head.as_bytes()).and_then(|()| stream.write_all(&body)).map_err(|e| e.to_string())?;
        let mut answer = Vec::new();
        stream.read_to_end(&mut answer).map_err(|e| e.to_string())?;
        let answer = String::from_utf8_lossy(&answer);
        let json_text = answer.split_once("\r\n\r\n").map_or("", |(_, b)| b);
        // Bei „chunked“ stehen Längenangaben vor dem JSON; das JSON selbst beginnt mit „{“.
        let json_text = &json_text[json_text.find('{').unwrap_or(0)..json_text.rfind('}').map_or(json_text.len(), |i| i + 1)];
        let text = json::parse(json_text)
            .and_then(|v| v.get("text").and_then(json::Value::as_str).map(str::to_string))
            .ok_or_else(|| t("err.dictation_answer").to_string())?;
        Ok(clean_transcript(&text))
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Fügt die Zeilen zusammen und verwirft typische Einbildungen des Modells bei Stille.
pub fn clean_transcript(text: &str) -> String {
    const HALLUCINATIONS: [&str; 8] = [
        "untertitel",
        "copyright",
        "vielen dank fürs zuschauen",
        "danke fürs zuschauen",
        "thanks for watching",
        "[musik]",
        "(musik)",
        "[blank_audio]",
    ];
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let lower = joined.to_lowercase();
    if HALLUCINATIONS.iter().any(|h| lower.contains(h)) && joined.chars().count() < 80 {
        return String::new();
    }
    joined
}

fn wav_bytes(samples: &[i16]) -> Vec<u8> {
    let data = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data as usize);
    out.extend(b"RIFF");
    out.extend((36 + data).to_le_bytes());
    out.extend(b"WAVEfmt ");
    out.extend(16u32.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(1u16.to_le_bytes());
    out.extend(RATE.to_le_bytes());
    out.extend((RATE * 2).to_le_bytes());
    out.extend(2u16.to_le_bytes());
    out.extend(16u16.to_le_bytes());
    out.extend(b"data");
    out.extend(data.to_le_bytes());
    for s in samples {
        out.extend(s.to_le_bytes());
    }
    out
}

/// Teilt laufendes Audio an Sprechpausen in Abschnitte.
pub struct Splitter {
    frame: Vec<i16>,
    segment: Vec<i16>,
    preroll: VecDeque<Vec<i16>>,
    noise: f64,
    speaking: bool,
    silent_frames: usize,
    speech_frames: usize,
}

/// 20 ms je Rahmen.
const FRAME: usize = (RATE / 50) as usize;
/// So viel Stille beendet einen Abschnitt.
const PAUSE_FRAMES: usize = 40; // 0,8 s
/// Längere Abschnitte werden an der nächsten leisen Stelle geteilt.
const MAX_FRAMES: usize = 50 * 20; // 20 s
const PREROLL_FRAMES: usize = 15; // 0,3 s vor dem ersten lauten Rahmen
const MIN_SPEECH_FRAMES: usize = 15;

impl Default for Splitter {
    fn default() -> Self {
        Splitter { frame: Vec::new(), segment: Vec::new(), preroll: VecDeque::new(), noise: 300.0, speaking: false, silent_frames: 0, speech_frames: 0 }
    }
}

impl Splitter {
    /// Nimmt Audio auf; gibt fertige Abschnitte zurück.
    pub fn push(&mut self, samples: &[i16]) -> Vec<Vec<i16>> {
        let mut done = Vec::new();
        for &sample in samples {
            self.frame.push(sample);
            if self.frame.len() == FRAME {
                let frame = std::mem::take(&mut self.frame);
                if let Some(segment) = self.frame_done(frame) {
                    done.push(segment);
                }
            }
        }
        done
    }

    fn frame_done(&mut self, frame: Vec<i16>) -> Option<Vec<i16>> {
        let rms = (frame.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / frame.len() as f64).sqrt();
        // Grundrauschen langsam nachführen, nur in leisen Rahmen.
        let threshold = (self.noise * 3.0).max(350.0);
        let loud = rms > threshold;
        if !loud {
            self.noise = self.noise * 0.98 + rms * 0.02;
        }
        if !self.speaking {
            self.preroll.push_back(frame);
            if self.preroll.len() > PREROLL_FRAMES {
                self.preroll.pop_front();
            }
            if loud {
                self.speaking = true;
                self.segment = self.preroll.drain(..).flatten().collect();
                self.silent_frames = 0;
                self.speech_frames = 1;
            }
            return None;
        }
        self.segment.extend(&frame);
        if loud {
            self.silent_frames = 0;
            self.speech_frames += 1;
        } else {
            self.silent_frames += 1;
        }
        let frames = self.segment.len() / FRAME;
        let pause = self.silent_frames >= PAUSE_FRAMES;
        let too_long = frames >= MAX_FRAMES && !loud;
        (pause || too_long).then(|| self.finish()).flatten()
    }

    /// Schließt den laufenden Abschnitt ab (auch beim Beenden des Diktats).
    pub fn finish(&mut self) -> Option<Vec<i16>> {
        self.speaking = false;
        let segment = std::mem::take(&mut self.segment);
        let speech = std::mem::take(&mut self.speech_frames);
        self.silent_frames = 0;
        (speech >= MIN_SPEECH_FRAMES).then_some(segment)
    }
}

/// Eine laufende Mikrofonaufnahme. Fertige Abschnitte gehen an den Kanal aus [`Recorder::start`].
/// Ein leerer Abschnitt meldet, dass das Mikrofon nach drei Sekunden noch völlig stumm ist
/// (stummgeschaltet oder falsches Gerät).
pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    level: Arc<AtomicU32>,
}

/// Namen der Aufnahmegeräte (Mikrofone), wie Windows sie nennt. Der Index ist die Geräte-ID.
pub fn microphones() -> Vec<String> {
    let count = unsafe { waveInGetNumDevs() };
    (0..count)
        .filter_map(|id| unsafe {
            let mut caps = WAVEINCAPSW::default();
            (waveInGetDevCapsW(id as usize, &mut caps, std::mem::size_of::<WAVEINCAPSW>() as u32) == 0).then(|| {
                let name = caps.szPname;
                let len = name.iter().position(|&c| c == 0).unwrap_or(name.len());
                String::from_utf16_lossy(&name[..len])
            })
        })
        .collect()
}

/// Geräte-ID des Mikrofons `name`; unbekannt oder leer: das Standardgerät von Windows.
fn device_id(name: &str) -> u32 {
    if name.is_empty() {
        return WAVE_MAPPER;
    }
    microphones().iter().position(|m| m == name).map_or(WAVE_MAPPER, |i| i as u32)
}

impl Recorder {
    /// Startet die Aufnahme mit dem Mikrofon `microphone` (leer: Windows-Standard).
    pub fn start(segments: mpsc::Sender<Vec<i16>>, microphone: &str) -> Result<Recorder, String> {
        if unsafe { waveInGetNumDevs() } == 0 {
            return Err(t("err.no_microphone").to_string());
        }
        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let (ready_tx, ready_rx) = mpsc::channel();
        let (flag, meter) = (stop.clone(), level.clone());
        let device = device_id(microphone);
        // Im Testmodus kann eine WAV-Datei das Mikrofon ersetzen.
        let replay = std::env::var_os("VORLESER_TEST").and(std::env::var_os("VORLESER_DICT_WAV"));
        let thread = match replay {
            Some(path) => std::thread::spawn(move || replay_wav(path.into(), flag, segments, ready_tx, meter)),
            None => std::thread::spawn(move || record(flag, segments, ready_tx, device, meter)),
        };
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Recorder { stop, thread: Some(thread), level }),
            Ok(Err(e)) => Err(e),
            Err(_) => Err(t("err.no_microphone").to_string()),
        }
    }

    /// Lautstärke der letzten 100 ms (Effektivwert, 0 bis 32767), für die Pegelanzeige.
    pub fn level_meter(&self) -> Arc<AtomicU32> {
        self.level.clone()
    }
}

fn rms(samples: &[i16]) -> u32 {
    if samples.is_empty() {
        return 0;
    }
    ((samples.iter().map(|&s| (s as f64) * (s as f64)).sum::<f64>() / samples.len() as f64).sqrt()) as u32
}

impl Drop for Recorder {
    /// Hält die Aufnahme an; der letzte Abschnitt wird noch weitergegeben.
    fn drop(&mut self) {
        self.stop.store(true, Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// Spielt eine 16-kHz-Mono-WAV in Echtzeit ein, als käme sie vom Mikrofon (nur für Tests).
fn replay_wav(path: PathBuf, stop: Arc<AtomicBool>, segments: mpsc::Sender<Vec<i16>>, ready: mpsc::Sender<Result<(), String>>, level: Arc<AtomicU32>) {
    let Ok(wav) = std::fs::read(path) else {
        let _ = ready.send(Err(t("err.no_microphone").to_string()));
        return;
    };
    let _ = ready.send(Ok(()));
    let samples = read_wav(&wav);
    let chunk = (RATE / 10) as usize;
    let silence = vec![0i16; chunk];
    let mut splitter = Splitter::default();
    let mut chunks = samples.chunks(chunk);
    while !stop.load(Relaxed) {
        let chunk = chunks.next().unwrap_or(&silence);
        level.store(rms(chunk), Relaxed);
        for segment in splitter.push(chunk) {
            let _ = segments.send(segment);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(segment) = splitter.finish() {
        let _ = segments.send(segment);
    }
}

fn read_wav(wav: &[u8]) -> Vec<i16> {
    wav.get(44..).unwrap_or_default().chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect()
}

fn record(stop: Arc<AtomicBool>, segments: mpsc::Sender<Vec<i16>>, ready: mpsc::Sender<Result<(), String>>, id: u32, level: Arc<AtomicU32>) {
    let format = WAVEFORMATEX {
        wFormatTag: WAVE_FORMAT_PCM as u16,
        nChannels: 1,
        nSamplesPerSec: RATE,
        nAvgBytesPerSec: RATE * 2,
        nBlockAlign: 2,
        wBitsPerSample: 16,
        cbSize: 0,
    };
    let mut device = HWAVEIN::default();
    if unsafe { waveInOpen(Some(&mut device), id, &format, None, None, CALLBACK_NULL) } != 0 {
        let _ = ready.send(Err(t("err.no_microphone").to_string()));
        return;
    }
    // Acht Puffer zu je 100 ms.
    let size = (RATE / 10) as usize;
    let header_size = std::mem::size_of::<WAVEHDR>() as u32;
    let mut buffers: Vec<(Box<WAVEHDR>, Vec<i16>)> = (0..8).map(|_| (Box::new(WAVEHDR::default()), vec![0i16; size])).collect();
    unsafe {
        for (header, data) in buffers.iter_mut() {
            header.lpData = windows::core::PSTR(data.as_mut_ptr() as *mut u8);
            header.dwBufferLength = (size * 2) as u32;
            waveInPrepareHeader(device, &mut **header, header_size);
            waveInAddBuffer(device, &mut **header, header_size);
        }
        waveInStart(device);
    }
    let _ = ready.send(Ok(()));
    let mut splitter = Splitter::default();
    let mut next = 0;
    let mut heard = 0usize;
    let mut loudest = 0u16;
    while !stop.load(Relaxed) {
        let (header, data) = &mut buffers[next];
        // Der Treiber setzt das Flag aus einem anderen Thread.
        std::sync::atomic::fence(std::sync::atomic::Ordering::SeqCst);
        if unsafe { std::ptr::addr_of!(header.dwFlags).read_unaligned() } & WHDR_DONE == 0 {
            std::thread::sleep(Duration::from_millis(15));
            continue;
        }
        let recorded = header.dwBytesRecorded as usize / 2;
        level.store(rms(&data[..recorded]), Relaxed);
        if heard < RATE as usize * 3 {
            loudest = data[..recorded].iter().map(|s| s.unsigned_abs()).max().unwrap_or(0).max(loudest);
            heard += recorded;
            if heard >= RATE as usize * 3 && loudest < 4 {
                let _ = segments.send(Vec::new());
            }
        }
        for segment in splitter.push(&data[..recorded]) {
            let _ = segments.send(segment);
        }
        unsafe {
            header.dwFlags &= !WHDR_DONE;
            waveInAddBuffer(device, &mut **header, header_size);
        }
        next = (next + 1) % buffers.len();
    }
    unsafe {
        waveInStop(device);
        waveInReset(device);
        // Restliche Puffer in Aufnahmereihenfolge, beginnend beim ältesten.
        let count = buffers.len();
        for i in 0..count {
            let (header, data) = &mut buffers[(next + i) % count];
            if header.dwBytesRecorded > 0 && header.dwFlags & WHDR_DONE != 0 {
                let recorded = header.dwBytesRecorded as usize / 2;
                for segment in splitter.push(&data[..recorded]) {
                    let _ = segments.send(segment);
                }
            }
            waveInUnprepareHeader(device, &mut **header, header_size);
        }
        waveInClose(device);
    }
    if let Some(segment) = splitter.finish() {
        let _ = segments.send(segment);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(ms: usize, amplitude: f64) -> Vec<i16> {
        (0..RATE as usize * ms / 1000).map(|i| ((i as f64 * 0.12).sin() * amplitude) as i16).collect()
    }

    /// Nimmt fünf Sekunden auf und zeigt die Abschnitte:
    /// `cargo test records_microphone -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn records_microphone() {
        let (tx, rx) = mpsc::channel();
        println!("Mikrofone: {:?}", microphones());
        let recorder = Recorder::start(tx, &std::env::var("VORLESER_MIC").unwrap_or_default()).unwrap();
        let meter = recorder.level_meter();
        let mut peak = 0;
        for _ in 0..30 {
            std::thread::sleep(Duration::from_millis(100));
            peak = peak.max(meter.load(Relaxed));
        }
        println!("höchster Pegel: {peak}");
        drop(recorder);
        for segment in rx {
            let peak = segment.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0);
            println!("Abschnitt: {:.1} s, Spitze {peak}", segment.len() as f64 / RATE as f64);
        }
    }

    #[test]
    fn splits_at_pauses() {
        let mut splitter = Splitter::default();
        let mut audio = tone(500, 50.0); // Stille (leises Rauschen)
        audio.extend(tone(1200, 6000.0)); // „Sprechen“
        audio.extend(tone(1000, 50.0)); // Pause → Abschnitt fertig
        audio.extend(tone(900, 6000.0));
        let mut segments = splitter.push(&audio);
        assert_eq!(segments.len(), 1, "nach der Pause genau ein Abschnitt");
        let first = segments.remove(0).len() as f64 / RATE as f64;
        assert!(first > 1.4 && first < 2.4, "Länge {first}");
        let last = splitter.finish().expect("zweiter Abschnitt beim Beenden");
        assert!(last.len() as f64 / RATE as f64 > 0.8);
        // Kurzes Knacken ist kein Abschnitt.
        assert!(splitter.push(&tone(100, 6000.0)).is_empty() && splitter.finish().is_none());
    }

    #[test]
    fn drops_silence_hallucinations() {
        assert_eq!(clean_transcript(" Untertitel im Auftrag des ZDF, 2021\n"), "");
        assert_eq!(clean_transcript(" Guten Morgen,\n wie geht es? "), "Guten Morgen, wie geht es?");
    }

    #[test]
    fn writes_wav() {
        let wav = wav_bytes(&[0, 1000, -1000]);
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 6);
    }

    /// Probe von Hand mit installiertem whisper.cpp (Testordner über LOCALAPPDATA):
    /// `VORLESER_WAV=<16-kHz-WAV>`, dann `cargo test whisper_server -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn whisper_server() {
        if best_installed().is_none() {
            let start = Instant::now();
            install(&MODELS[0], |_, _, _| true).unwrap();
            println!("installiert in {:?}", start.elapsed());
        }
        let model = best_installed().expect("kein Modell installiert");
        let start = Instant::now();
        let server = Server::start(model, "de").unwrap();
        println!("Server bereit nach {:?}", start.elapsed());
        let samples = read_wav(&std::fs::read(std::env::var("VORLESER_WAV").unwrap()).unwrap());
        let start = Instant::now();
        let text = server.transcribe(&samples).unwrap();
        println!("erkannt in {:?}: {text}", start.elapsed());
        assert!(!text.is_empty());
    }
}
