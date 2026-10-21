//! KI-Anbindung des Beseco Officeassistenten über die OpenAI-kompatible Schnittstelle
//! (`/v1/chat/completions`). Das sprechen Ollama, IONOS AI Model Hub, Mistral, OpenAI, LM Studio
//! und viele andere; ein eigener Code je Anbieter ist dadurch nicht nötig.
//!
//! Aufrufe blockieren und sind für einen Hintergrund-Thread gedacht.

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
}

impl Summary {
    pub const ALL: [Summary; 3] = [Summary::Short, Summary::Bullets, Summary::Detailed];

    pub fn code(self) -> &'static str {
        match self {
            Summary::Short => "short",
            Summary::Bullets => "bullets",
            Summary::Detailed => "detailed",
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

    /// Eine Anfrage an das Modell: Anweisung (`system`) und Inhalt (`user`).
    pub fn chat(&self, system: &str, user: &str) -> Result<String, String> {
        let body = format!(
            "{{\"model\":{},\"temperature\":0.3,\"stream\":false,\"messages\":[{{\"role\":\"system\",\"content\":{}}},{{\"role\":\"user\",\"content\":{}}}]}}",
            json::string(self.model.trim()),
            json::string(system),
            json::string(user)
        );
        let url = format!("{}/chat/completions", self.base());
        let request = Request {
            method: "POST",
            url: &url,
            headers: self.headers(),
            body: Some(body.as_bytes()),
            // Lokale Modelle ohne Grafikkarte brauchen für lange Texte mehrere Minuten.
            timeout: 600_000,
        };
        let (status, answer) = http::fetch(&request)?;
        if status != 200 {
            return Err(Self::api_error(status, &answer));
        }
        let value = json::parse(&answer).ok_or_else(|| t("ai.err_answer").to_string())?;
        let content = value
            .path(&["choices", "0", "message", "content"])
            .and_then(Value::as_str)
            .ok_or_else(|| t("ai.err_answer").to_string())?;
        Ok(clean_answer(content))
    }

    /// Kurzer Probe-Auftrag: Dauer in Millisekunden und Antwort.
    pub fn test(&self) -> Result<(u128, String), String> {
        let start = Instant::now();
        let answer = self.chat("Reply with exactly one word: OK", "Test")?;
        Ok((start.elapsed().as_millis(), answer))
    }

    /// Überarbeitet einen diktierten Text in der Stufe `mode`.
    pub fn correct(&self, text: &str, mode: Correction, custom: &str) -> Result<String, String> {
        let system = format!(
            "You edit text that was dictated and converted by speech recognition. {} {} Output only the edited text.",
            mode.instruction(custom),
            BASE_RULES
        );
        self.chat(&system, text)
    }

    /// Fasst `text` zusammen. Lange Texte werden zuerst teilweise verdichtet; `progress` erhält
    /// den gerade bearbeiteten Teil und die Zahl der Schritte.
    pub fn summarize(&self, text: &str, mode: Summary, mut progress: impl FnMut(usize, usize)) -> Result<String, String> {
        let parts = chunks(text, CHUNK);
        let system = format!("You summarize texts. {} {}", mode.instruction(), BASE_RULES);
        if parts.len() <= 1 {
            progress(1, 1);
            return self.chat(&system, text);
        }
        let steps = parts.len() + 1;
        let mut notes = Vec::new();
        for (i, part) in parts.iter().enumerate() {
            progress(i + 1, steps);
            let note = self.chat(
                &format!("You extract the key content of one part of a longer text: all important facts, names, numbers and statements, as compact notes. {BASE_RULES}"),
                part,
            )?;
            notes.push(note);
        }
        progress(steps, steps);
        self.chat(&format!("{system} The text consists of notes on consecutive parts of one document."), &notes.join("\n\n"))
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
        println!("Zusammenfassung: {:?}", provider.summarize("Der Vorleser ist ein kleines Programm für Windows. Es liest markierten Text vor. Neu ist der Officeassistent, der Texte mit KI zusammenfasst.", Summary::Short, |_, _| {}));
    }
}
