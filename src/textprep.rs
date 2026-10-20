//! Textaufbereitung: Zerlegung in Sätze (für KI-Stimmen und die Navigation) und Bereinigung
//! vor dem Vorlesen. Die Bereinigung fügt umbrochene Zeilen (typisch für PDFs) wieder zusammen,
//! hebt Silbentrennung auf und kürzt Internetadressen auf den Namen der Seite.

/// Ab dieser Länge gilt eine Zeile als umbrochener Fließtext; kürzere Zeilen (Überschriften,
/// Listen) behalten ihren Zeilenumbruch, damit die Stimme dort eine Pause macht.
const WRAPPED_LINE: usize = 45;

/// Nach einem Bindestrich am Zeilenende stehen diese Wörter bei Aufzählungen wie
/// "Hals-, Nasen-\nund Ohrenarzt"; dort ist der Strich keine Silbentrennung.
const KEEP_HYPHEN_BEFORE: [&str; 6] = ["und", "oder", "bzw.", "sowie", "and", "or"];

pub fn clean(text: &str) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = text.split('\n').collect();
    let mut out = String::with_capacity(text.len());
    for (i, line) in lines.iter().enumerate() {
        let line = line.trim_end();
        out.push_str(&shorten_urls(line));
        let Some(next) = lines.get(i + 1).map(|next| next.trim_start()) else { break };
        let wrapped = line.chars().count() >= WRAPPED_LINE && !next.is_empty();
        if wrapped && is_hyphenated(line, next) {
            out.pop();
        } else if wrapped {
            out.push(' ');
        } else {
            out.push('\n');
        }
    }
    out
}

fn is_hyphenated(line: &str, next: &str) -> bool {
    let mut end = line.chars().rev();
    let hyphen_after_letter = end.next() == Some('-') && end.next().is_some_and(char::is_alphabetic);
    let first_word = next.split_whitespace().next().unwrap_or("");
    hyphen_after_letter
        && first_word.chars().next().is_some_and(char::is_lowercase)
        && !KEEP_HYPHEN_BEFORE.contains(&first_word)
}

/// Ersetzt "https://www.beispiel.de/pfad?x=1" durch "beispiel.de".
fn shorten_urls(line: &str) -> String {
    if !line.contains("://") && !line.contains("www.") {
        return line.to_string();
    }
    line.split(' ').map(shorten_url).collect::<Vec<_>>().join(" ")
}

fn shorten_url(word: &str) -> String {
    let start = word.find("https://").or_else(|| word.find("http://")).or_else(|| word.find("www."));
    let Some(start) = start else { return word.to_string() };
    let (before, url) = word.split_at(start);
    // Satzzeichen und Klammern direkt hinter der Adresse gehören zum Satz.
    let trimmed = url.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '"', '»', '«']);
    let after = &url[trimmed.len()..];
    let host = trimmed.split("://").last().unwrap_or(trimmed);
    let host = host.split(['/', '?', '#']).next().unwrap_or(host);
    let host = host.strip_prefix("www.").unwrap_or(host);
    if host.is_empty() {
        return word.to_string();
    }
    format!("{before}{host}{after}")
}

/// Ein Satz mit seiner Position im Originaltext (UTF-16-Einheiten, wie im Textfeld).
#[derive(Debug, PartialEq)]
pub struct Sentence {
    pub text: String,
    pub start: u32,
    pub len: u32,
}

/// Zerlegt Text in Sätze: nach `. ! ? …` vor einem Leerraum, an Zeilenumbrüchen, und sehr lange
/// Sätze zusätzlich an Kommas, damit der erste Ton schnell kommt.
pub fn split_sentences(text: &str) -> Vec<Sentence> {
    const LONG: usize = 300;
    let mut sentences = Vec::new();
    let mut current = String::new();
    let mut start = 0u32;
    let mut pos = 0u32;
    let mut chars = text.chars().peekable();
    let mut push = |current: &mut String, start: u32| {
        let leading = current.chars().take_while(|c| c.is_whitespace()).map(char::len_utf16).sum::<usize>() as u32;
        let trimmed = current.trim();
        if !trimmed.is_empty() {
            let len = trimmed.encode_utf16().count() as u32;
            sentences.push(Sentence { text: trimmed.to_string(), start: start + leading, len });
        }
        current.clear();
    };
    while let Some(c) = chars.next() {
        pos += c.len_utf16() as u32;
        if c == '\r' || c == '\n' {
            push(&mut current, start);
            start = pos;
            continue;
        }
        current.push(c);
        let next_is_space = chars.peek().is_none_or(|n| n.is_whitespace());
        let end_mark = matches!(c, '.' | '!' | '?' | '…') && next_is_space;
        let long_break = current.chars().count() > LONG && matches!(c, ',' | ';' | ':') && next_is_space;
        if end_mark || long_break {
            push(&mut current, start);
            start = pos;
        }
    }
    push(&mut current, start);
    sentences
}

/// Satzanfänge in einem Text aus UTF-16-Einheiten (wie im Textfeld).
pub fn sentence_starts(text: &[u16]) -> Vec<u32> {
    split_sentences(&String::from_utf16_lossy(text)).into_iter().map(|s| s.start).collect()
}

/// Anfang des Satzes, in dem `pos` liegt (bzw. des ersten Satzes davor).
pub fn sentence_at(starts: &[u32], pos: u32) -> Option<u32> {
    starts.iter().rev().find(|&&start| start <= pos).or(starts.first()).copied()
}

/// Ziel eines Sprungs von `pos` aus: der nächste Satz, oder der Satz vor dem aktuellen.
pub fn neighbor(starts: &[u32], pos: u32, forward: bool) -> Option<u32> {
    if forward {
        return starts.iter().find(|&&start| start > pos).copied();
    }
    let current = starts.iter().rposition(|&start| start <= pos)?;
    Some(starts[current.saturating_sub(1)])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(text: &str) -> Vec<(String, u32, u32)> {
        split_sentences(text).into_iter().map(|s| (s.text, s.start, s.len)).collect()
    }

    #[test]
    fn splits_sentences_with_positions() {
        let text = "Erster Satz. Zweiter Satz!\r\nDritte Zeile ohne Punkt";
        assert_eq!(
            texts(text),
            vec![
                ("Erster Satz.".to_string(), 0, 12),
                ("Zweiter Satz!".to_string(), 13, 13),
                ("Dritte Zeile ohne Punkt".to_string(), 28, 23),
            ]
        );
    }

    #[test]
    fn keeps_abbreviation_like_dots_inside_words() {
        assert_eq!(texts("Version 1.5 ist da.").len(), 1);
        assert_eq!(texts("www.beseco.com ist die Seite.").len(), 1);
    }

    #[test]
    fn counts_utf16_like_the_text_field() {
        // "😀" belegt zwei UTF-16-Einheiten.
        let parts = texts("😀 Hallo. Welt.");
        assert_eq!(parts[1], ("Welt.".to_string(), 10, 5));
    }


    #[test]
    fn joins_wrapped_lines_but_keeps_short_ones() {
        let text = "Überschrift\nDies ist ein längerer Absatz, der im PDF hart umbrochen\nwurde und weitergeht.\n\nNeuer Absatz.";
        assert_eq!(
            clean(text),
            "Überschrift\nDies ist ein längerer Absatz, der im PDF hart umbrochen wurde und weitergeht.\n\nNeuer Absatz."
        );
    }

    #[test]
    fn removes_hyphenation_only_where_it_is_one() {
        let hyphenated = "Dieser Satz endet mitten im Wort mit einer Silben-\ntrennung und geht weiter.";
        assert!(clean(hyphenated).contains("Silbentrennung und"));
        let list = "Er arbeitet seit vielen Jahren als Hals-, Nasen-\nund Ohrenarzt in der Stadt.";
        assert!(clean(list).contains("Nasen- und Ohrenarzt"));
        let name = "Die Besprechung leitet heute ausnahmsweise Frau Müller-\nSchmidt aus der Verwaltung.";
        assert!(clean(name).contains("Müller- Schmidt"));
    }

    #[test]
    fn shortens_urls() {
        assert_eq!(clean("Siehe https://www.beispiel.de/pfad/seite.html?x=1."), "Siehe beispiel.de.");
        assert_eq!(clean("(www.beseco.com)"), "(beseco.com)");
        assert_eq!(clean("Kein Link: a://"), "Kein Link: a://");
    }

    #[test]
    fn finds_neighbor_sentences() {
        let text: Vec<u16> = "Eins. Zwei. Drei.".encode_utf16().collect();
        let starts = sentence_starts(&text);
        assert_eq!(starts, vec![0, 6, 12]);
        assert_eq!(sentence_at(&starts, 8), Some(6));
        assert_eq!(neighbor(&starts, 8, true), Some(12));
        assert_eq!(neighbor(&starts, 8, false), Some(0));
        assert_eq!(neighbor(&starts, 12, true), None);
        assert_eq!(neighbor(&starts, 0, false), Some(0));
    }

    #[test]
    fn handles_windows_line_endings() {
        assert_eq!(clean("eins\r\nzwei"), "eins\nzwei");
    }
}
