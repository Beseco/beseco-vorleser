//! Bereinigt Text vor dem Vorlesen: fügt umbrochene Zeilen (typisch für PDFs) wieder zusammen,
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

#[cfg(test)]
mod tests {
    use super::*;

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
    fn handles_windows_line_endings() {
        assert_eq!(clean("eins\r\nzwei"), "eins\nzwei");
    }
}
