//! Sprachausgabe über SAPI 5 (`ISpVoice`).

use windows::core::{w, Result, BSTR, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, VARIANT_FALSE, WPARAM};
use windows::Win32::Globalization::{
    GetLocaleInfoEx, LCIDToLocaleName, LOCALE_SLOCALIZEDDISPLAYNAME,
};
use windows::Win32::Media::Speech::{
    ISpObjectToken, ISpObjectTokenCategory, ISpVoice, ISpeechFileStream, SSFMCreateForWrite,
    SpFileStream, SpObjectTokenCategory, SpVoice,
    SPEI_END_INPUT_STREAM, SPEI_RESERVED1, SPEI_RESERVED2, SPEI_WORD_BOUNDARY, SPEVENT, SPF_ASYNC, SPF_IS_NOT_XML,
    SPF_PURGEBEFORESPEAK, SPVOICESTATUS,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};

/// Klassische SAPI-Stimmen und die OneCore-Stimmen (u. a. Katja/Stefan).
const VOICE_CATEGORIES: [&str; 2] = [
    r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech\Voices",
    r"HKEY_LOCAL_MACHINE\SOFTWARE\Microsoft\Speech_OneCore\Voices",
];

// Ende der Ausgabe und Wortgrenzen (zum Mitlesen); entspricht dem C-Makro SPFEI(...).
const INTEREST: u64 = (1 << SPEI_END_INPUT_STREAM.0)
    | (1 << SPEI_WORD_BOUNDARY.0)
    | (1 << SPEI_RESERVED1.0)
    | (1 << SPEI_RESERVED2.0);

pub struct Voice {
    pub id: String,
    pub name: String,
    /// Sprachkennung (LANGID) der Stimme, 0 = unbekannt.
    pub lang_id: u16,
    /// Anzeigename der Sprache, z. B. "Deutsch (Deutschland)".
    pub language: String,
    token: ISpObjectToken,
}

/// Was seit dem letzten Aufruf von [`Tts::poll`] passiert ist.
pub struct Progress {
    /// Zuletzt begonnenes Wort: Position und Länge im gesprochenen Text, in UTF-16-Einheiten.
    pub word: Option<(u32, u32)>,
    /// `Some`, wenn die Ausgabe zu Ende ist; ein Fehler z. B., wenn eine Online-Stimme ihren
    /// Dienst nicht erreicht hat.
    pub finished: Option<Result<()>>,
}

pub struct Tts {
    voice: ISpVoice,
    voices: Vec<Voice>,
    /// Nummer des zuletzt gestarteten Sprachstroms; 0 = keiner.
    stream: u32,
    paused: bool,
}

impl Tts {
    /// COM muss auf dem aufrufenden Thread bereits initialisiert sein.
    pub fn new() -> Result<Tts> {
        let voice: ISpVoice = unsafe { CoCreateInstance(&SpVoice, None, CLSCTX_ALL)? };
        let mut voices = Vec::new();
        for category in VOICE_CATEGORIES {
            // Fehlt eine Kategorie (z. B. keine OneCore-Stimmen), wird sie übersprungen.
            let _ = enumerate(category, &mut voices);
        }
        Ok(Tts { voice, voices, stream: 0, paused: false })
    }

    pub fn voices(&self) -> &[Voice] {
        &self.voices
    }

    /// Meldet das Ende eines Sprachstroms als Fensternachricht `msg` an `hwnd`.
    pub fn notify_window(&self, hwnd: HWND, msg: u32) -> Result<()> {
        unsafe {
            self.voice.SetNotifyWindowMessage(hwnd, msg, WPARAM(0), LPARAM(0))?;
            self.voice.SetInterest(INTEREST, INTEREST)
        }
    }

    /// Gibt `false` zurück, wenn die Stimme nicht (mehr) vorhanden oder nicht nutzbar ist.
    pub fn set_voice(&self, id: &str) -> bool {
        match self.voices.iter().find(|v| v.id == id) {
            Some(v) => unsafe { self.voice.SetVoice(&v.token).is_ok() },
            None => false,
        }
    }

    pub fn current_voice_id(&self) -> Option<String> {
        unsafe { self.voice.GetVoice().and_then(|t| t.GetId()).ok().map(take_string) }
    }

    pub fn set_rate(&self, rate: i32) {
        unsafe {
            let _ = self.voice.SetRate(rate.clamp(-10, 10));
        }
    }

    /// Lautstärke in Prozent.
    pub fn set_volume(&self, volume: u32) {
        unsafe {
            let _ = self.voice.SetVolume(volume.min(100) as u16);
        }
    }

    pub fn current_rate(&self) -> i32 {
        let mut rate = 0;
        unsafe {
            let _ = self.voice.GetRate(&mut rate);
        }
        rate
    }

    /// Bricht laufende Ausgabe ab und liest `text` vor.
    pub fn speak(&mut self, text: &str) -> Result<()> {
        let text = HSTRING::from(text);
        let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0 | SPF_IS_NOT_XML.0) as u32;
        let mut stream = 0;
        unsafe { self.voice.Speak(PCWSTR(text.as_ptr()), flags, Some(&mut stream))? };
        self.stream = stream;
        self.unpause();
        Ok(())
    }

    pub fn pause(&mut self) {
        if !self.paused && unsafe { self.voice.Pause().is_ok() } {
            self.paused = true;
        }
    }

    pub fn resume(&mut self) {
        self.unpause();
    }

    pub fn stop(&mut self) {
        self.stream = 0;
        let flags = (SPF_ASYNC.0 | SPF_PURGEBEFORESPEAK.0) as u32;
        unsafe {
            let _ = self.voice.Speak(PCWSTR::null(), flags, None);
        }
        self.unpause();
    }

    /// Nach der Fensternachricht aufrufen. Ereignisse abgebrochener, älterer Sprachströme
    /// zählen nicht.
    pub fn poll(&mut self) -> Progress {
        let mut finished = false;
        let mut word = None;
        loop {
            let mut event = SPEVENT::default();
            let mut fetched = 0;
            let ok = unsafe { self.voice.GetEvents(1, &mut event, &mut fetched).is_ok() };
            if !ok || fetched == 0 {
                break;
            }
            if self.stream == 0 || event.ulStreamNum != self.stream {
                continue;
            }
            let id = event._bitfield & 0xffff;
            if id == SPEI_END_INPUT_STREAM.0 {
                finished = true;
            } else if id == SPEI_WORD_BOUNDARY.0 {
                word = Some((event.lParam.0 as u32, event.wParam.0 as u32));
            }
        }
        if !finished {
            return Progress { word, finished: None };
        }
        self.stream = 0;
        let mut status = SPVOICESTATUS::default();
        let result = unsafe {
            self.voice
                .GetStatus(&mut status, std::ptr::null_mut())
                .and_then(|()| status.hrLastResult.ok())
        };
        Progress { word, finished: Some(result) }
    }

    // SAPI zählt Pausen mit, deshalb genau ein Resume pro Pause.
    fn unpause(&mut self) {
        if self.paused {
            unsafe {
                let _ = self.voice.Resume();
            }
            self.paused = false;
        }
    }
}

/// Schreibt `text` mit der Stimme `voice_id` (sonst der Standardstimme) als WAV-Datei.
/// Läuft synchron mit einer eigenen Stimme und ist für einen Hintergrund-Thread gedacht, auf dem
/// COM bereits initialisiert ist; die laufende Ausgabe bleibt davon unberührt.
pub fn save_wav(voice_id: Option<&str>, rate: i32, text: &str, path: &str) -> Result<()> {
    unsafe {
        let file: ISpeechFileStream = CoCreateInstance(&SpFileStream, None, CLSCTX_ALL)?;
        file.Open(&BSTR::from(path), SSFMCreateForWrite, VARIANT_FALSE)?;
        let writer: ISpVoice = CoCreateInstance(&SpVoice, None, CLSCTX_ALL)?;
        writer.SetOutput(&file, true)?;
        if let Some(id) = voice_id {
            // COM-Objekte gehören zu ihrem Thread, deshalb die Stimme hier neu suchen.
            let mut voices = Vec::new();
            for category in VOICE_CATEGORIES {
                let _ = enumerate(category, &mut voices);
            }
            if let Some(voice) = voices.iter().find(|v| v.id == id) {
                writer.SetVoice(&voice.token)?;
            }
        }
        let _ = writer.SetRate(rate.clamp(-10, 10));
        let spoken = writer.Speak(&HSTRING::from(text), SPF_IS_NOT_XML.0 as u32, None);
        drop(writer);
        file.Close()?;
        spoken
    }
}

fn enumerate(category_id: &str, out: &mut Vec<Voice>) -> Result<()> {
    unsafe {
        let category: ISpObjectTokenCategory =
            CoCreateInstance(&SpObjectTokenCategory, None, CLSCTX_ALL)?;
        category.SetId(&HSTRING::from(category_id), false)?;
        let tokens = category.EnumTokens(PCWSTR::null(), PCWSTR::null())?;
        let mut count = 0;
        tokens.GetCount(&mut count)?;
        for i in 0..count {
            let token = tokens.Item(i)?;
            let Ok(id) = token.GetId().map(take_string) else { continue };
            // Der Standardwert des Tokens ist der Anzeigename.
            let name = token.GetStringValue(PCWSTR::null()).map(take_string).unwrap_or_else(|_| id.clone());
            // Online-Stimmen (z. B. über den NaturalVoiceSAPIAdapter) brechen je nach Netz und Dienst
            // ab und schicken den Text an einen fremden Anbieter; Vorleser bietet sie nicht an.
            if is_online(&name) {
                continue;
            }
            let lang_id = language_id(&token).unwrap_or(0);
            let language = language_name(lang_id).unwrap_or_else(|| "Andere".to_string());
            out.push(Voice { id, name, lang_id, language, token });
        }
    }
    Ok(())
}

fn is_online(name: &str) -> bool {
    name.contains(" Online")
}

/// Liest das Attribut "Language" der Stimme, z. B. "407" oder "409;9" (hexadezimal).
fn language_id(token: &ISpObjectToken) -> Option<u16> {
    let value = unsafe {
        let attributes = token.OpenKey(w!("Attributes")).ok()?;
        take_string(attributes.GetStringValue(w!("Language")).ok()?)
    };
    u16::from_str_radix(value.split(';').next()?.trim(), 16).ok()
}

fn language_name(lang_id: u16) -> Option<String> {
    let mut locale = [0u16; 85];
    let mut name = [0u16; 128];
    unsafe {
        if LCIDToLocaleName(lang_id as u32, Some(&mut locale), 0) == 0 {
            return None;
        }
        let len = GetLocaleInfoEx(PCWSTR(locale.as_ptr()), LOCALE_SLOCALIZEDDISPLAYNAME, Some(&mut name));
        // Die Länge schließt das Nullzeichen ein.
        (len > 1).then(|| String::from_utf16_lossy(&name[..len as usize - 1]))
    }
}

/// Übernimmt einen von COM allozierten String und gibt den Speicher frei.
fn take_string(p: PWSTR) -> String {
    unsafe {
        let s = p.to_string().unwrap_or_default();
        CoTaskMemFree(Some(p.0 as *const _));
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};

    /// Machbarkeitsprobe von Hand: listet alle Stimmen und lässt jede stumm (Lautstärke 0) ein
    /// Wort synthetisieren. Dauert je nach Anzahl der Stimmen über eine Minute, und Online-Stimmen
    /// können je nach Netz ausfallen. Aufruf: `cargo test -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn listed_voices_can_speak() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let tts = Tts::new().expect("SAPI nicht verfügbar");
        assert!(!tts.voices().is_empty(), "keine Stimmen gefunden");
        unsafe { tts.voice.SetVolume(0).unwrap() };
        let mut usable = 0;
        for v in tts.voices() {
            let ok = tts.set_voice(&v.id)
                && unsafe { tts.voice.Speak(&HSTRING::from("Test"), SPF_IS_NOT_XML.0 as u32, None).is_ok() };
            println!("{}  {} | {}  [{}]", if ok { "OK    " } else { "FEHLER" }, v.language, v.name, v.id);
            usable += ok as usize;
        }
        assert!(usable > 0, "keine Stimme nutzbar");
    }

    /// Jede Stimme mit bekannter Sprachkennung bekommt einen lesbaren Gruppennamen fürs Menü.
    #[test]
    fn voices_have_language_names() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let tts = Tts::new().expect("SAPI nicht verfügbar");
        let mut groups = std::collections::BTreeMap::new();
        for v in tts.voices() {
            assert!(v.lang_id == 0 || v.language != "Andere", "kein Sprachname für {}", v.name);
            *groups.entry(v.language.as_str()).or_insert(0) += 1;
        }
        println!("{groups:?}");
    }

    #[test]
    fn saves_a_wav_file() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let tts = Tts::new().expect("SAPI nicht verfügbar");
        let local = tts.voices().first().map(|v| v.id.as_str());
        let path = std::env::temp_dir().join("vorleser_test.wav");
        // Wie im Programm auf einem eigenen Thread.
        let id = local.map(str::to_string);
        let target = path.clone();
        std::thread::spawn(move || {
            unsafe {
                let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
            }
            save_wav(id.as_deref(), 2, "Dies ist ein Test.", target.to_str().unwrap()).unwrap();
        })
        .join()
        .unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(&bytes[..4], b"RIFF");
        assert!(bytes.len() > 10_000, "Datei enthält kaum Audio: {} Bytes", bytes.len());
    }

    /// Das Ende der Ausgabe muss als Ereignis ankommen, sonst bleibt das Programm auf "liest vor".
    #[test]
    fn end_of_speech_is_reported() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let mut tts = Tts::new().expect("SAPI nicht verfügbar");
        unsafe {
            tts.voice.SetVolume(0).unwrap();
            tts.voice.SetInterest(INTEREST, INTEREST).unwrap();
        }
        // Eine lokale Stimme, damit der Test nicht vom Netz abhängt.
        let local = tts.voices().first().map(|v| v.id.clone());
        if let Some(id) = local {
            assert!(tts.set_voice(&id));
        }
        let text = "Erster Satz. Zweiter Satz.";
        tts.speak(text).unwrap();
        // Die Satzgrenze mitten im Text darf nicht als Ende gelten.
        std::thread::sleep(std::time::Duration::from_millis(300));
        let early = tts.poll();
        assert!(early.finished.is_none(), "Ende zu früh gemeldet");
        // Zum Mitlesen: Das erste Wort wird mit Position und Länge gemeldet.
        assert_eq!(early.word.map(|(start, _)| start), Some(0));
        unsafe { tts.voice.WaitUntilDone(20000).unwrap() };
        let end = tts.poll();
        assert!(matches!(end.finished, Some(Ok(()))));
        // Das letzte gemeldete Wort ist "Satz" am Ende des Textes.
        assert_eq!(end.word, Some((21, 4)));
    }

    /// Probe von Hand: Melden auch die Online-Stimmen Wortgrenzen? `cargo test -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn online_voices_report_words() {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        }
        let mut tts = Tts::new().expect("SAPI nicht verfügbar");
        unsafe {
            tts.voice.SetVolume(0).unwrap();
            tts.voice.SetInterest(INTEREST, INTEREST).unwrap();
        }
        let ids: Vec<(String, String)> = tts
            .voices()
            .iter()
            .filter(|v| v.name.contains("Katja"))
            .map(|v| (v.id.clone(), v.name.clone()))
            .collect();
        for (id, name) in ids {
            tts.set_voice(&id);
            tts.speak("Das ist ein kurzer Test mit sieben Wörtern.").unwrap();
            let mut words = Vec::new();
            loop {
                std::thread::sleep(std::time::Duration::from_millis(50));
                let progress = tts.poll();
                words.extend(progress.word);
                if progress.finished.is_some() {
                    break;
                }
            }
            println!("{name}: {words:?}");
        }
    }
}
