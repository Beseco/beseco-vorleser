//! KI-Anbindung des Beseco Officeassistenten über die OpenAI-kompatible Schnittstelle
//! (`/v1/chat/completions`). Das sprechen Ollama, IONOS AI Model Hub, Mistral, OpenAI, LM Studio
//! und viele andere; ein eigener Code je Anbieter ist dadurch nicht nötig.
//!
//! Aufrufe blockieren und sind für einen Hintergrund-Thread gedacht.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use windows::Win32::Foundation::{LocalFree, HLOCAL};
use windows::Win32::Security::Cryptography::{CryptProtectData, CryptUnprotectData, CRYPT_INTEGER_BLOB};

use crate::http::{self, Request};
use crate::i18n::t;
use crate::json::{self, Value};

/// Art des Anbieters, wie sie in den Einstellungen steht.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    None,
    /// Ollama, auf diesem PC oder auf einem Server im eigenen Netz.
    Ollama,
    /// Jeder Dienst mit OpenAI-kompatibler Schnittstelle (IONOS, Mistral, OpenAI …).
    OpenAi,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::None, Kind::Ollama, Kind::OpenAi];

    pub fn code(self) -> &'static str {
        match self {
            Kind::None => "none",
            Kind::Ollama => "ollama",
            Kind::OpenAi => "openai",
        }
    }

    pub fn from_code(code: &str) -> Kind {
        Kind::ALL.into_iter().find(|k| k.code() == code).unwrap_or(Kind::None)
    }

    /// Vorschlag für die Adresse, wenn das Feld noch leer ist.
    pub fn default_url(self) -> &'static str {
        match self {
            Kind::None => "",
            Kind::Ollama => "http://localhost:11434",
            Kind::OpenAi => "https://openai.inference.de-txl.ionos.com/v1",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Provider {
    pub kind: Kind,
    pub url: String,
    pub key: String,
    pub model: String,
}

/// Wie die Zusammenfassung aussehen soll.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Summary {
    Short,
    Bullets,
    Detailed,
    /// Protokoll einer Besprechung (aus einer Abschrift).
    Minutes,
}

impl Summary {
    pub const ALL: [Summary; 4] = [Summary::Short, Summary::Bullets, Summary::Detailed, Summary::Minutes];

    pub fn code(self) -> &'static str {
        match self {
            Summary::Short => "short",
            Summary::Bullets => "bullets",
            Summary::Detailed => "detailed",
            Summary::Minutes => "minutes",
        }
    }

    pub fn from_code(code: &str) -> Summary {
        Summary::ALL.into_iter().find(|s| s.code() == code).unwrap_or(Summary::Short)
    }

    fn instruction(self) -> &'static str {
        match self {
            Summary::Short => "Summarize the text in 3 to 5 sentences.",
            Summary::Bullets => "Summarize the text as a list of the most important points, one point per line, each line starting with \"• \".",
            Summary::Detailed => "Write a detailed summary of the text in several paragraphs that keeps all important facts, names, numbers and conclusions.",
            Summary::Minutes => "The text is the transcript of a recorded meeting or conversation; times in square brackets are positions in the recording. Write the minutes: first a short title line, then the sections Participants (only if names are mentioned), Topics, Decisions, Action items and Open questions. Put each section heading on its own line followed by a colon, and below it one entry per line starting with \"• \". For action items write who, what and by when, as far as mentioned. In German, name the sections Teilnehmer, Themen, Beschlüsse, Aufgaben and Offene Punkte. Leave out sections without content. Use only what the transcript says; never invent names, dates or decisions. Speech recognition may have misheard words; correct obvious mistakes from the context.",
        }
    }
}

/// Wie stark ein diktierter Text überarbeitet wird.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Correction {
    /// Nur Rechtschreibung, Grammatik und Zeichensetzung.
    Spelling,
    /// Zusätzlich Satzbau glätten.
    Style,
    /// Als gut lesbaren Text neu formulieren.
    Rewrite,
    /// Nach eigener Vorgabe.
    Custom,
}

impl Correction {
    pub const ALL: [Correction; 4] = [Correction::Spelling, Correction::Style, Correction::Rewrite, Correction::Custom];

    pub fn code(self) -> &'static str {
        match self {
            Correction::Spelling => "spelling",
            Correction::Style => "style",
            Correction::Rewrite => "rewrite",
            Correction::Custom => "custom",
        }
    }

    pub fn from_code(code: &str) -> Correction {
        Correction::ALL.into_iter().find(|c| c.code() == code).unwrap_or(Correction::Spelling)
    }

    fn instruction(self, custom: &str) -> String {
        let task = match self {
            Correction::Spelling => "Correct only spelling, grammar and punctuation, and obvious speech recognition errors. Keep the wording, word order and sentence structure exactly as they are.",
            Correction::Style => "Correct spelling, grammar, punctuation and speech recognition errors, and smooth the sentence structure so the text reads naturally. Keep the content, tone and length; do not add or remove information.",
            Correction::Rewrite => "Rewrite the text as a clear, well-structured and polished text with sensible paragraphs. Keep all information and the language; you may reorder and rephrase freely.",
            Correction::Custom => "",
        };
        let custom = custom.trim();
        if self == Correction::Custom && !custom.is_empty() {
            format!("Edit the text according to this instruction: {custom}. Keep all information unless the instruction says otherwise.")
        } else if self == Correction::Custom {
            Correction::Style.instruction("")
        } else {
            task.to_string()
        }
    }
}

/// Grundregeln für alle Antworten: Sprache des Textes, nur Klartext (wird vorgelesen).
const BASE_RULES: &str = "Answer in the same language as the text. Write plain text only, without Markdown (no **, no #, no tables), because the answer is read aloud. Do not add an introduction like \"Here is the summary\".";

/// Ein gescheiterter Versuch: HTTP-Status (falls es einen gab) und Meldung.
struct Failure {
    status: Option<u16>,
    message: String,
}

impl Failure {
    fn other(message: String) -> Failure {
        Failure { status: None, message }
    }
}

/// Wartezeiten bei „zu viele Anfragen“ (429) und kurzzeitigen Serverfehlern; danach gilt der Fehler.
const RETRY_WAITS: [u64; 3] = [2, 5, 10];

/// Versucht `attempt` erneut, solange der Anbieter nur kurz überlastet ist. `wait` wartet die
/// angegebene Zeit und meldet mit `false`, dass abgebrochen wurde.
fn retrying(mut attempt: impl FnMut() -> Result<String, Failure>, mut wait: impl FnMut(std::time::Duration) -> bool) -> Result<String, String> {
    for seconds in RETRY_WAITS {
        match attempt() {
            Err(Failure { status: Some(429 | 500 | 502 | 503 | 504 | 529), .. }) => {
                if !wait(std::time::Duration::from_secs(seconds)) {
                    return Err(t("err.cancelled").to_string());
                }
            }
            other => return other.map_err(|f| f.message),
        }
    }
    attempt().map_err(|f| f.message)
}

/// Sekunden bis zum nächsten Versuch, solange auf einen ausgelasteten Anbieter gewartet wird (sonst 0).
pub static RETRY_WAIT: AtomicU64 = AtomicU64::new(0);

/// Wartet in kleinen Schritten und fragt dabei, ob abgebrochen wurde.
fn pause(wait: std::time::Duration, on_text: &mut dyn FnMut(&str) -> bool) -> bool {
    let end = Instant::now() + wait;
    let mut go_on = true;
    while go_on && Instant::now() < end {
        RETRY_WAIT.store((end - Instant::now()).as_secs() + 1, Ordering::Relaxed);
        go_on = on_text("");
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
    RETRY_WAIT.store(0, Ordering::Relaxed);
    go_on
}

/// Längere Texte werden in Teilen dieser Größe vorverdichtet (passt auch in kleine Modelle).
const CHUNK: usize = 12_000;

impl Provider {
    pub fn is_ready(&self) -> bool {
        self.kind != Kind::None && !self.url.trim().is_empty() && !self.model.trim().is_empty()
    }

    /// Basisadresse der OpenAI-kompatiblen Schnittstelle.
    fn base(&self) -> String {
        let url = self.url.trim().trim_end_matches('/');
        match self.kind {
            Kind::Ollama if !url.ends_with("/v1") => format!("{url}/v1"),
            _ => url.to_string(),
        }
    }

    fn headers(&self) -> Vec<String> {
        let mut headers = vec!["Content-Type: application/json".to_string(), "Accept: application/json".to_string()];
        if !self.key.trim().is_empty() {
            headers.push(format!("Authorization: Bearer {}", self.key.trim()));
        }
        headers
    }

    /// Fehlermeldung aus einer Antwort mit Fehlerstatus.
    fn api_error(status: u16, body: &str) -> String {
        let detail = json::parse(body)
            .and_then(|v| {
                v.path(&["error", "message"]).or_else(|| v.get("error")).or_else(|| v.get("message")).and_then(Value::as_str).map(str::to_string)
            })
            .unwrap_or_default();
        let hint = match status {
            401 | 403 => t("ai.err_key"),
            404 => t("ai.err_not_found"),
            429 => t("ai.err_limit"),
            _ => t("ai.err_server"),
        };
        if detail.is_empty() {
            format!("{hint} (HTTP {status})")
        } else {
            format!("{hint} (HTTP {status}: {detail})")
        }
    }

    /// Verfügbare Modelle des Anbieters.
    pub fn models(&self) -> Result<Vec<String>, String> {
        let url = format!("{}/models", self.base());
        let mut request = Request::get(&url);
        request.headers = self.headers();
        let (status, body) = http::fetch(&request)?;
        if status != 200 {
            return Err(Self::api_error(status, &body));
        }
        let value = json::parse(&body).ok_or_else(|| t("ai.err_answer").to_string())?;
        let mut models: Vec<String> = value
            .get("data")
            .and_then(Value::as_array)
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m.get("id").and_then(Value::as_str).map(str::to_string))
            .collect();
        models.sort();
        Ok(models)
    }

    fn body(&self, system: &str, user: &str, stream: bool) -> String {
        // Denkende Modelle (qwen3.5 u. a.) schreiben sonst erst seitenweise Überlegungen: auf einem
        // PC ohne Grafikkarte Minuten statt Sekunden. Ollama nimmt den Wert auch bei anderen
        // Modellen an; Cloud-Anbieter bekommen ihn nicht, manche lehnen unbekannte Werte ab.
        let reasoning = if self.kind == Kind::Ollama { ",\"reasoning_effort\":\"none\"" } else { "" };
        format!(
            "{{\"model\":{},\"temperature\":0.3,\"stream\":{stream}{reasoning},\"messages\":[{{\"role\":\"system\",\"content\":{}}},{{\"role\":\"user\",\"content\":{}}}]}}",
            json::string(self.model.trim()),
            json::string(system),
            json::string(user)
        )
    }

    /// Wie [`Provider::chat`], liest die Antwort aber Stück für Stück: `on_text` erhält den bisher
    /// geschriebenen Text und bricht mit `false` ab. Server ohne Streaming liefern alles auf einmal.
    pub fn chat_stream(&self, system: &str, user: &str, on_text: impl FnMut(&str) -> bool) -> Result<String, String> {
        let on_text = RefCell::new(on_text);
        retrying(|| self.stream_once(system, user, &mut *on_text.borrow_mut()), |wait| pause(wait, &mut *on_text.borrow_mut()))
    }

    fn stream_once(&self, system: &str, user: &str, on_text: &mut dyn FnMut(&str) -> bool) -> Result<String, Failure> {
        let body = self.body(system, user, true);
        let url = format!("{}/chat/completions", self.base());
        let request = Request { method: "POST", url: &url, headers: self.headers(), body: Some(body.as_bytes()), timeout: 600_000 };
        let mut text = String::new();
        let mut raw = String::new();
        let mut cancelled = false;
        let status = http::stream(&request, |line| {
            let payload = line.strip_prefix("data:").map(str::trim);
            match payload {
                Some("[DONE]") => return false,
                Some(json_text) => {
                    if let Some(piece) = json::parse(json_text).and_then(|v| v.path(&["choices", "0", "delta", "content"]).and_then(Value::as_str).map(str::to_string)) {
                        text.push_str(&piece);
                        if !on_text(&text) {
                            cancelled = true;
                            return false;
                        }
                    }
                }
                // Keine Server-Sent Events: die ganze Antwort (oder ein Fehler) als JSON.
                None => {
                    raw.push_str(line);
                    raw.push('\n');
                }
            }
            true
        })
        .map_err(Failure::other)?;
        if cancelled {
            return Err(Failure::other(t("err.cancelled").to_string()));
        }
        if status != 200 {
            return Err(Failure { status: Some(status), message: Self::api_error(status, &raw) });
        }
        if text.is_empty() {
            let answer_error = || Failure::other(t("ai.err_answer").to_string());
            let value = json::parse(&raw).ok_or_else(answer_error)?;
            text = value.path(&["choices", "0", "message", "content"]).and_then(Value::as_str).ok_or_else(answer_error)?.to_string();
            on_text(&text);
        }
        Ok(clean_answer(&text))
    }

    /// Eine Anfrage an das Modell: Anweisung (`system`) und Inhalt (`user`).
    pub fn chat(&self, system: &str, user: &str) -> Result<String, String> {
        retrying(|| self.chat_once(system, user), |wait| {
            std::thread::sleep(wait);
            true
        })
    }

    fn chat_once(&self, system: &str, user: &str) -> Result<String, Failure> {
        let body = self.body(system, user, false);
        let url = format!("{}/chat/completions", self.base());
        let request = Request {
            method: "POST",
            url: &url,
            headers: self.headers(),
            body: Some(body.as_bytes()),
            // Lokale Modelle ohne Grafikkarte brauchen für lange Texte mehrere Minuten.
            timeout: 600_000,
        };
        let (status, answer) = http::fetch(&request).map_err(Failure::other)?;
        if status != 200 {
            return Err(Failure { status: Some(status), message: Self::api_error(status, &answer) });
        }
        let answer_error = || Failure::other(t("ai.err_answer").to_string());
        let value = json::parse(&answer).ok_or_else(answer_error)?;
        let content = value.path(&["choices", "0", "message", "content"]).and_then(Value::as_str).ok_or_else(answer_error)?;
        Ok(clean_answer(content))
    }

    /// Kurzer Probe-Auftrag: Dauer in Millisekunden und Antwort.
    pub fn test(&self) -> Result<(u128, String), String> {
        let start = Instant::now();
        let answer = self.chat("Reply with exactly one word: OK", "Test")?;
        Ok((start.elapsed().as_millis(), answer))
    }

    /// Findet unter den Rändern der Seiten (je erste und letzte Zeilen) Kopf- und Fußzeilen und
    /// Seitenzahlen. Liefert die betroffenen Zeilen wörtlich.
    pub fn page_edges(&self, edges: &[(Vec<String>, Vec<String>)]) -> Result<Vec<String>, String> {
        let mut listing = String::new();
        for (i, (head, tail)) in edges.iter().enumerate().take(40) {
            for line in head {
                listing.push_str(&format!("Page {} top: {line}\n", i + 1));
            }
            for line in tail {
                listing.push_str(&format!("Page {} bottom: {line}\n", i + 1));
            }
        }
        let system = "You find running headers, running footers and page numbers in a document. You get the top and bottom lines of each page. \
            Running headers and footers repeat on most pages, often with a changing page number or date: for example document or company names, dates, file names, paths or web addresses, \
            \"Page 3 of 12\", \"- 3 -\" or \"Seite 3\". Document titles, chapter titles and normal sentences are body text, not headers. \
            Reply with every line that is a running header, running footer or page number, one per line, copied exactly without the \"Page n top/bottom:\" prefix. \
            Reply with nothing else. If there are none, reply with NONE.";
        let answer = self.chat(system, &listing)?;
        Ok(answer.lines().map(|l| l.trim().trim_start_matches("• ").to_string()).filter(|l| !l.is_empty() && l != "NONE").collect())
    }

    /// Überarbeitet einen diktierten Text in der Stufe `mode`; `on_text` wie bei
    /// [`Provider::chat_stream`].
    pub fn correct(&self, text: &str, mode: Correction, custom: &str, on_text: impl FnMut(&str) -> bool) -> Result<String, String> {
        let system = format!(
            "You edit text that was dictated and converted by speech recognition. {} {} Output only the edited text.",
            mode.instruction(custom),
            BASE_RULES
        );
        self.chat_stream(&system, text, on_text)
    }

    /// Fasst `text` zusammen. Lange Texte werden zuerst teilweise verdichtet; `progress` erhält
    /// den gerade bearbeiteten Teil und die Zahl der Schritte.
    /// `on_text` erhält die entstehende Zusammenfassung (nur im letzten Schritt) und bricht mit
    /// `false` ab; auch die Zwischenschritte prüfen es.
    pub fn summarize(&self, text: &str, mode: Summary, mut progress: impl FnMut(usize, usize), mut on_text: impl FnMut(&str) -> bool) -> Result<String, String> {
        let parts = chunks(text, CHUNK);
        let system = format!("You summarize texts. {} {}", mode.instruction(), BASE_RULES);
        if parts.len() <= 1 {
            progress(1, 1);
            return self.chat_stream(&system, text, on_text);
        }
        let steps = parts.len() + 1;
        let mut notes = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            progress(i + 1, steps);
            let note = self.chat_stream(
                &format!("You extract the key content of one part of a longer text: all important facts, names, numbers and statements, as compact notes. {BASE_RULES}"),
                part,
                |_| on_text(""),
            )?;
            notes.push(note);
        }
        progress(steps, steps);
        self.chat_stream(&format!("{system} The text consists of notes on consecutive parts of one document."), &notes.join("\n\n"), on_text)
    }
}

/// Entfernt Markdown-Reste, die beim Vorlesen stören würden, und „Denk“-Abschnitte mancher Modelle.
pub fn clean_answer(text: &str) -> String {
    let mut text = text.to_string();
    // Modelle wie DeepSeek oder Qwen schreiben ihre Überlegungen in <think>…</think>.
    while let (Some(start), Some(end)) = (text.find("<think>"), text.find("</think>")) {
        if end < start {
            break;
        }
        text.replace_range(start..end + 8, "");
    }
    let lines: Vec<String> = text
        .lines()
        .map(|line| {
            let trimmed = line.trim_start();
            let line = trimmed.trim_start_matches('#').trim_start();
            let line = match line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")) {
                Some(rest) => format!("• {rest}"),
                None => line.to_string(),
            };
            line.replace("**", "").replace("__", "")
        })
        .collect();
    lines.join("\n").trim().to_string()
}

/// Teilt Text an Absatzgrenzen in Stücke von höchstens etwa `size` Zeichen.
fn chunks(text: &str, size: usize) -> Vec<String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    for paragraph in text.split("\n\n") {
        if !current.is_empty() && current.chars().count() + paragraph.chars().count() > size {
            parts.push(std::mem::take(&mut current));
        }
        // Ein einzelner sehr langer Absatz wird hart geteilt.
        let mut rest: &str = paragraph;
        while rest.chars().count() > size {
            let cut = rest.char_indices().nth(size).map_or(rest.len(), |(i, _)| i);
            let cut = rest[..cut].rfind(['.', ' ']).map_or(cut, |i| i + 1);
            parts.push(rest[..cut].to_string());
            rest = &rest[cut..];
        }
        if !current.is_empty() {
            current.push_str("\n\n");
        }
        current.push_str(rest);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Verschlüsselt den Schlüssel mit dem Windows-Konto (DPAPI): Nur derselbe Benutzer auf diesem
/// Rechner kann ihn wieder lesen. Ergebnis: „dpapi:“ und Hex-Zeichen.
pub fn protect(secret: &str) -> String {
    if secret.is_empty() {
        return String::new();
    }
    let mut data = secret.as_bytes().to_vec();
    let input = CRYPT_INTEGER_BLOB { cbData: data.len() as u32, pbData: data.as_mut_ptr() };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if CryptProtectData(&input, windows::core::w!("Vorleser"), None, None, None, 0, &mut output).is_err() {
            return String::new();
        }
        let bytes = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        format!("dpapi:{hex}")
    }
}

pub fn unprotect(stored: &str) -> String {
    let Some(hex) = stored.strip_prefix("dpapi:") else { return stored.to_string() };
    let mut bytes: Vec<u8> = (0..hex.len() / 2).filter_map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).ok()).collect();
    let input = CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_mut_ptr() };
    let mut output = CRYPT_INTEGER_BLOB::default();
    unsafe {
        if CryptUnprotectData(&input, None, None, None, None, 0, &mut output).is_err() {
            return String::new();
        }
        let secret = String::from_utf8_lossy(std::slice::from_raw_parts(output.pbData, output.cbData as usize)).into_owned();
        let _ = LocalFree(Some(HLOCAL(output.pbData as *mut _)));
        secret
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_busy_providers() {
        let mut calls = 0;
        let result = retrying(
            || {
                calls += 1;
                if calls < 3 { Err(Failure { status: Some(429), message: "busy".into() }) } else { Ok("fertig".into()) }
            },
            |_| true,
        );
        assert_eq!(result.as_deref(), Ok("fertig"));
        assert_eq!(calls, 3);
        // Ein falscher Schlüssel wird nicht wiederholt.
        let mut calls = 0;
        let result = retrying(
            || {
                calls += 1;
                Err(Failure { status: Some(401), message: "key".into() })
            },
            |_| true,
        );
        assert_eq!(result, Err("key".to_string()));
        assert_eq!(calls, 1);
        // Abbrechen während des Wartens.
        let result = retrying(|| Err(Failure { status: Some(503), message: "x".into() }), |_| false);
        assert_eq!(result, Err(t("err.cancelled").to_string()));
    }

    /// Echtes Ollama mit qwen3.5:4b: Antwort in Stücken, Abbrechen nach wenigen Stücken.
    /// `cargo test streams_from_ollama -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn streams_from_ollama() {
        let provider = Provider { kind: Kind::Ollama, url: "http://localhost:11434".into(), key: String::new(), model: "qwen3.5:4b".into() };
        let start = Instant::now();
        let mut updates = Vec::new();
        let text = provider
            .chat_stream("Answer in German.", "Nenne drei Sehenswürdigkeiten in Freising, je ein Satz.", |t| {
                updates.push((start.elapsed().as_millis(), t.chars().count()));
                true
            })
            .unwrap();
        println!("{} Stücke, erstes nach {} ms, fertig nach {} ms", updates.len(), updates[0].0, start.elapsed().as_millis());
        println!("{text}");
        assert!(updates.len() > 5);
        let mut count = 0;
        let cancelled = provider.chat_stream("Answer in German.", "Erzähle eine lange Geschichte über Freising.", |_| {
            count += 1;
            count < 4
        });
        println!("Abbrechen: {cancelled:?} nach {count} Stücken");
        assert!(cancelled.is_err());
    }

    #[test]
    fn builds_correction_instructions() {
        assert!(Correction::Spelling.instruction("").contains("Keep the wording"));
        assert!(Correction::Custom.instruction("als freundliche E-Mail").contains("als freundliche E-Mail"));
        assert_eq!(Correction::Custom.instruction("  "), Correction::Style.instruction(""));
        assert_eq!(Correction::from_code("rewrite"), Correction::Rewrite);
    }

    #[test]
    fn builds_base_urls() {
        let provider = |kind, url: &str| Provider { kind, url: url.into(), key: String::new(), model: "m".into() };
        assert_eq!(provider(Kind::Ollama, "http://server:11434/").base(), "http://server:11434/v1");
        assert_eq!(provider(Kind::Ollama, "http://server:11434/v1").base(), "http://server:11434/v1");
        assert_eq!(provider(Kind::OpenAi, "https://openai.inference.de-txl.ionos.com/v1/").base(), "https://openai.inference.de-txl.ionos.com/v1");
        assert!(!provider(Kind::None, "x").is_ready());
    }

    #[test]
    fn cleans_answers_for_reading_aloud() {
        let answer = "<think>Ich überlege …</think>\n## Ergebnis\n- **Erstens** wichtig\n* Zweitens\nNormaler Satz.";
        assert_eq!(clean_answer(answer), "Ergebnis\n• Erstens wichtig\n• Zweitens\nNormaler Satz.");
    }

    #[test]
    fn splits_long_texts() {
        let text = (0..30).map(|i| format!("Absatz {i} ").repeat(50)).collect::<Vec<_>>().join("\n\n");
        let parts = chunks(&text, 2_000);
        assert!(parts.len() > 5);
        assert!(parts.iter().all(|p| p.chars().count() <= 2_100));
        assert_eq!(parts.concat().replace("\n\n", "").len(), text.replace("\n\n", "").len());
        assert_eq!(chunks("kurz", 2_000), vec!["kurz".to_string()]);
    }

    #[test]
    fn protects_keys_with_the_windows_account() {
        let stored = protect("geheimer-schlüssel-123");
        assert!(stored.starts_with("dpapi:") && !stored.contains("geheim"));
        assert_eq!(unprotect(&stored), "geheimer-schlüssel-123");
        assert_eq!(unprotect("klartext"), "klartext");
    }

    /// Probe von Hand gegen einen echten Anbieter:
    /// `VORLESER_AI=ollama|openai`, `VORLESER_AI_URL`, `VORLESER_AI_KEY`, `VORLESER_AI_MODEL`, dann
    /// `cargo test real_provider -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn real_provider() {
        let env = |name: &str| std::env::var(name).unwrap_or_default();
        let provider = Provider { kind: Kind::from_code(&env("VORLESER_AI")), url: env("VORLESER_AI_URL"), key: env("VORLESER_AI_KEY"), model: env("VORLESER_AI_MODEL") };
        println!("Modelle: {:?}", provider.models());
        println!("Test: {:?}", provider.test());
        println!("Zusammenfassung: {:?}", provider.summarize("Der Vorleser ist ein kleines Programm für Windows. Es liest markierten Text vor. Neu ist der Officeassistent, der Texte mit KI zusammenfasst.", Summary::Short, |_, _| {}, |_| true));
    }
}
