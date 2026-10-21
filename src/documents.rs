//! Liest den Text aus Dateien: TXT, Word (DOCX), OpenDocument (ODT), RTF, EPUB, HTML,
//! PDF und Bilder. PDF und Bilder laufen über die Texterkennung von Windows ([`crate::ocr`]),
//! dadurch gehen auch gescannte Dokumente.

use std::path::Path;

use crate::i18n::t;
use crate::zip::Zip;

/// Dateiendungen, die Vorleser öffnen kann.
pub const EXTENSIONS: [&str; 17] = [
    "txt", "md", "csv", "docx", "odt", "rtf", "epub", "html", "htm", "xhtml", "pdf", "png", "jpg", "jpeg", "bmp",
    "tif", "tiff",
];
const IMAGES: [&str; 7] = ["png", "jpg", "jpeg", "bmp", "tif", "tiff", "gif"];

pub fn is_supported(path: &str) -> bool {
    let ext = extension(path);
    EXTENSIONS.contains(&ext.as_str()) || IMAGES.contains(&ext.as_str())
}

fn extension(path: &str) -> String {
    Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// Liest den Text einer Datei. `progress` erhält bei PDFs Seite und Seitenzahl und bricht mit
/// `false` ab. Blockiert; für einen Hintergrund-Thread mit [`crate::ocr::init_thread`].
pub fn extract(path: &str, progress: impl FnMut(u32, u32) -> bool) -> Result<String, String> {
    let ext = extension(path);
    let read = || std::fs::read(path).map_err(|_| t("app.file_unreadable").to_string());
    let text = match ext.as_str() {
        "pdf" => tidy(&crate::ocr::pdf_file(path, progress)?),
        e if IMAGES.contains(&e) => tidy(&crate::ocr::image_file(path)?),
        "docx" => docx(&read()?)?,
        "odt" => odt(&read()?)?,
        "epub" => epub(&read()?)?,
        "rtf" => rtf(&decode(&read()?)),
        "html" | "htm" | "xhtml" => html_to_text(&decode(&read()?)),
        "doc" => return Err(t("err.old_word").to_string()),
        _ => decode(&read()?),
    };
    if text.trim().is_empty() {
        return Err(t("err.no_text_in_file").to_string());
    }
    Ok(text)
}

/// Text aus Bytes: UTF-8 oder UTF-16 mit Kennung, sonst UTF-8, sonst Windows-1252.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(rest).into_owned();
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let units: Vec<u16> = rest.chunks_exact(2).map(|p| u16::from_le_bytes([p[0], p[1]])).collect();
        return String::from_utf16_lossy(&units);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_string(),
        Err(_) => bytes.iter().map(|&b| windows_1252(b)).collect(),
    }
}

fn windows_1252(byte: u8) -> char {
    // 0x80–0x9F weichen von Latin-1 ab (u. a. €, Anführungszeichen, Gedankenstrich).
    const HIGH: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}', '\u{90}', '‘', '’',
        '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
    ];
    match byte {
        0x80..=0x9F => HIGH[(byte - 0x80) as usize],
        _ => byte as char,
    }
}

/// Ersetzt die Zeichen-Entitäten von XML und HTML.
fn entities(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(end) = rest[..rest.len().min(12)].find(';') else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..end];
        let decoded = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            "shy" => Some('\u{ad}'),
            "ndash" => Some('–'),
            "mdash" => Some('—'),
            "hellip" => Some('…'),
            "auml" => Some('ä'),
            "ouml" => Some('ö'),
            "uuml" => Some('ü'),
            "Auml" => Some('Ä'),
            "Ouml" => Some('Ö'),
            "Uuml" => Some('Ü'),
            "szlig" => Some('ß'),
            "euro" => Some('€'),
            "bdquo" => Some('„'),
            "ldquo" => Some('“'),
            "rdquo" => Some('”'),
            "lsquo" => Some('‘'),
            "rsquo" => Some('’'),
            "laquo" => Some('«'),
            "raquo" => Some('»'),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|hex| u32::from_str_radix(hex, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                .and_then(char::from_u32),
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[end + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Geht die Tags eines XML-Dokuments durch. `on_tag` erhält den Namen (mit `/` vorn bei
/// schließenden Tags) und gibt zurück, was an dieser Stelle in den Text kommt.
fn walk_xml(xml: &str, mut on_tag: impl FnMut(&str) -> Option<&'static str>, text_inside: impl Fn(&[String]) -> bool) -> String {
    let mut out = String::new();
    let mut stack: Vec<String> = Vec::new();
    let mut rest = xml;
    while let Some(open) = rest.find('<') {
        if text_inside(&stack) {
            out.push_str(&entities(&rest[..open]));
        }
        rest = &rest[open..];
        let Some(close) = rest.find('>') else { break };
        let tag = &rest[1..close];
        rest = &rest[close + 1..];
        if tag.starts_with('?') || tag.starts_with('!') {
            continue;
        }
        let self_closing = tag.ends_with('/');
        let name = tag.trim_end_matches('/').split_whitespace().next().unwrap_or("");
        if let Some(name) = name.strip_prefix('/') {
            if let Some(i) = stack.iter().rposition(|n| n == name) {
                stack.truncate(i);
            }
            if let Some(insert) = on_tag(&format!("/{name}")) {
                out.push_str(insert);
            }
        } else {
            if let Some(insert) = on_tag(name) {
                out.push_str(insert);
            }
            if !self_closing {
                stack.push(name.to_string());
            }
        }
    }
    out
}

/// Fasst mehr als eine Leerzeile zusammen und entfernt Leerraum am Zeilenende.
fn tidy(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut blank = 0;
    for line in text.lines() {
        let line = line.trim_end();
        if line.trim().is_empty() {
            blank += 1;
            if blank > 1 {
                continue;
            }
        } else {
            blank = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim().to_string()
}

fn docx(data: &[u8]) -> Result<String, String> {
    let zip = Zip::open(data).ok_or_else(|| t("app.file_unreadable").to_string())?;
    let xml = zip.read_text("word/document.xml").ok_or_else(|| t("app.file_unreadable").to_string())?;
    let text = walk_xml(
        &xml,
        |tag| match tag {
            "/w:p" => Some("\n"),
            "w:tab" => Some("\t"),
            "w:br" | "w:cr" => Some("\n"),
            _ => None,
        },
        |stack| stack.last().is_some_and(|t| t == "w:t"),
    );
    Ok(tidy(&text))
}

fn odt(data: &[u8]) -> Result<String, String> {
    let zip = Zip::open(data).ok_or_else(|| t("app.file_unreadable").to_string())?;
    let xml = zip.read_text("content.xml").ok_or_else(|| t("app.file_unreadable").to_string())?;
    let text = walk_xml(
        &xml,
        |tag| match tag {
            "/text:p" | "/text:h" => Some("\n"),
            "text:s" => Some(" "),
            "text:tab" => Some("\t"),
            "text:line-break" => Some("\n"),
            _ => None,
        },
        |stack| stack.iter().any(|t| t == "text:p" || t == "text:h"),
    );
    Ok(tidy(&text))
}

fn attribute<'x>(tag: &'x str, name: &str) -> Option<&'x str> {
    let at = tag.find(&format!("{name}=\""))? + name.len() + 2;
    Some(&tag[at..at + tag[at..].find('"')?])
}

/// Löst einen Pfad relativ zum Ordner der OPF-Datei auf (ohne `..`-Tricks über das Archiv hinaus).
fn join(base: &str, href: &str) -> String {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for part in href.split('#').next().unwrap_or("").split('/') {
        match part {
            ".." => {
                parts.pop();
            }
            "." | "" => {}
            p => parts.push(p),
        }
    }
    parts.join("/")
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(value) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
                out.push(value);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn epub(data: &[u8]) -> Result<String, String> {
    let unreadable = || t("app.file_unreadable").to_string();
    let zip = Zip::open(data).ok_or_else(unreadable)?;
    let container = zip.read_text("META-INF/container.xml").ok_or_else(unreadable)?;
    let rootfile = container.find("<rootfile ").map(|i| &container[i..]).ok_or_else(unreadable)?;
    let opf_path = attribute(&rootfile[..rootfile.find('>').unwrap_or(rootfile.len())], "full-path").ok_or_else(unreadable)?;
    let opf = zip.read_text(opf_path).ok_or_else(unreadable)?;
    let base = opf_path.rsplit_once('/').map_or("", |(dir, _)| dir);

    // manifest: id → Pfad, spine: Reihenfolge der Kapitel.
    let mut manifest = Vec::new();
    let mut spine = Vec::new();
    for piece in opf.split('<').skip(1) {
        let tag = &piece[..piece.find('>').unwrap_or(piece.len())];
        if tag.starts_with("item ") || tag.starts_with("opf:item ") {
            if let (Some(id), Some(href)) = (attribute(tag, "id"), attribute(tag, "href")) {
                manifest.push((id.to_string(), join(base, &percent_decode(href))));
            }
        } else if tag.starts_with("itemref ") || tag.starts_with("opf:itemref ") {
            if let Some(id) = attribute(tag, "idref") {
                spine.push(id.to_string());
            }
        }
    }
    let mut text = String::new();
    for id in spine {
        let Some((_, path)) = manifest.iter().find(|(i, _)| *i == id) else { continue };
        if let Some(chapter) = zip.read_text(path) {
            text.push_str(&html_to_text(&chapter));
            text.push_str("\n\n");
        }
    }
    Ok(tidy(&text))
}

/// HTML ohne Tags; Blöcke werden zu Absätzen, Skripte und Stile fallen weg.
pub fn html_to_text(html: &str) -> String {
    const BLOCKS: [&str; 22] = [
        "p", "div", "br", "li", "tr", "h1", "h2", "h3", "h4", "h5", "h6", "section", "article", "header", "footer",
        "blockquote", "pre", "ul", "ol", "table", "dd", "dt",
    ];
    let text = walk_xml(
        html,
        |tag| {
            // Zeilenumbruch am Ende eines Blocks (und bei <br>), nicht zusätzlich am Anfang.
            let closing = tag.starts_with('/');
            let name = tag.trim_start_matches('/').to_ascii_lowercase();
            if name == "br" || (closing && BLOCKS.contains(&name.as_str())) {
                Some("\n")
            } else if closing && (name == "td" || name == "th") {
                Some("\t")
            } else {
                None
            }
        },
        |stack| {
            !stack.iter().any(|t| {
                let t = t.to_ascii_lowercase();
                t == "script" || t == "style" || t == "head" || t == "noscript"
            })
        },
    );
    // HTML fasst Leerraum innerhalb einer Zeile zusammen.
    let lines: Vec<String> = text.lines().map(|l| l.split_whitespace().collect::<Vec<_>>().join(" ")).collect();
    tidy(&lines.join("\n"))
}

/// Text aus RTF: Steuerwörter für Absätze, Tabulatoren und Sonderzeichen werden umgesetzt,
/// Kopfbereiche wie Schrift- und Farbtabellen übersprungen.
pub fn rtf(source: &str) -> String {
    const SKIP: [&str; 12] = [
        "fonttbl", "colortbl", "stylesheet", "info", "pict", "object", "header", "footer", "listtable",
        "listoverridetable", "themedata", "datastore",
    ];
    let mut out = String::new();
    // Je Gruppe: wird sie übersprungen, und wie viele Ersatzzeichen folgen nach \uN.
    let mut skip_stack: Vec<bool> = vec![false];
    let mut uc_stack: Vec<usize> = vec![1];
    let mut pending_skip = 0usize;
    let bytes: Vec<char> = source.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i];
        let skipping = *skip_stack.last().unwrap_or(&false);
        match c {
            '{' => {
                skip_stack.push(skipping);
                uc_stack.push(*uc_stack.last().unwrap_or(&1));
                i += 1;
            }
            '}' => {
                if skip_stack.len() > 1 {
                    skip_stack.pop();
                    uc_stack.pop();
                }
                i += 1;
            }
            '\\' => {
                i += 1;
                let Some(&next) = bytes.get(i) else { break };
                if next.is_ascii_alphabetic() {
                    let start = i;
                    while i < bytes.len() && bytes[i].is_ascii_alphabetic() {
                        i += 1;
                    }
                    let word: String = bytes[start..i].iter().collect();
                    let num_start = i;
                    if i < bytes.len() && (bytes[i] == '-' || bytes[i].is_ascii_digit()) {
                        i += 1;
                        while i < bytes.len() && bytes[i].is_ascii_digit() {
                            i += 1;
                        }
                    }
                    let number: Option<i32> = bytes[num_start..i].iter().collect::<String>().parse().ok();
                    if i < bytes.len() && bytes[i] == ' ' {
                        i += 1;
                    }
                    if SKIP.contains(&word.as_str()) {
                        if let Some(top) = skip_stack.last_mut() {
                            *top = true;
                        }
                        continue;
                    }
                    if skipping {
                        continue;
                    }
                    match word.as_str() {
                        "par" | "line" | "sect" | "page" => out.push('\n'),
                        "tab" | "cell" => out.push('\t'),
                        "row" => out.push('\n'),
                        "emdash" => out.push('—'),
                        "endash" => out.push('–'),
                        "lquote" => out.push('‘'),
                        "rquote" => out.push('’'),
                        "ldblquote" => out.push('“'),
                        "rdblquote" => out.push('”'),
                        "bullet" => out.push('•'),
                        "uc" => {
                            if let (Some(top), Some(n)) = (uc_stack.last_mut(), number) {
                                *top = n.max(0) as usize;
                            }
                        }
                        "u" => {
                            if let Some(n) = number {
                                let code = if n < 0 { (n + 65_536) as u32 } else { n as u32 };
                                if let Some(ch) = char::from_u32(code) {
                                    out.push(ch);
                                }
                                pending_skip = *uc_stack.last().unwrap_or(&1);
                            }
                        }
                        _ => {}
                    }
                } else {
                    i += 1;
                    match next {
                        '*' => {
                            // \* kennzeichnet eine Erweiterung, die man überspringen darf.
                            if let Some(top) = skip_stack.last_mut() {
                                *top = true;
                            }
                        }
                        '\'' => {
                            let hex: String = bytes.get(i..i + 2).map(|h| h.iter().collect()).unwrap_or_default();
                            i += 2;
                            if pending_skip > 0 {
                                pending_skip -= 1;
                            } else if !skipping {
                                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                                    out.push(windows_1252(byte));
                                }
                            }
                        }
                        '\\' | '{' | '}' if !skipping => out.push(next),
                        '~' if !skipping => out.push(' '),
                        '-' | '_' => {}
                        '\n' | '\r' if !skipping => out.push('\n'),
                        _ => {}
                    }
                }
            }
            '\r' | '\n' => i += 1,
            _ => {
                if pending_skip > 0 {
                    pending_skip -= 1;
                } else if !skipping {
                    out.push(c);
                }
                i += 1;
            }
        }
    }
    tidy(&out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::zip::build;

    #[test]
    fn reads_docx() {
        let xml = r#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Erster</w:t></w:r><w:r><w:t xml:space="preserve"> Absatz &amp; mehr</w:t></w:r></w:p><w:p><w:r><w:t>Zweiter</w:t><w:tab/><w:t>mit Tab</w:t></w:r></w:p><w:p><w:r><w:instrText>FELD</w:instrText></w:r></w:p></w:body></w:document>"#;
        let data = build(&[("[Content_Types].xml", "<x/>"), ("word/document.xml", xml)]);
        assert_eq!(docx(&data).unwrap(), "Erster Absatz & mehr\nZweiter\tmit Tab");
    }

    #[test]
    fn reads_odt() {
        let xml = r#"<office:document-content><office:body><office:text><text:h>Titel</text:h><text:p>Ein<text:s/>Satz<text:line-break/>neue Zeile</text:p></office:text></office:body></office:document-content>"#;
        let data = build(&[("content.xml", xml)]);
        assert_eq!(odt(&data).unwrap(), "Titel\nEin Satz\nneue Zeile");
    }

    #[test]
    fn reads_epub_in_spine_order() {
        let container = r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#;
        let opf = r#"<package><manifest><item id="b" href="Text/zwei.xhtml" media-type="x"/><item id="a" href="Text/eins.xhtml" media-type="x"/></manifest><spine><itemref idref="a"/><itemref idref="b"/></spine></package>"#;
        let data = build(&[
            ("META-INF/container.xml", container),
            ("OEBPS/content.opf", opf),
            ("OEBPS/Text/eins.xhtml", "<html><head><title>X</title></head><body><h1>Kapitel 1</h1><p>Text eins.</p></body></html>"),
            ("OEBPS/Text/zwei.xhtml", "<html><body><p>Text zwei.</p></body></html>"),
        ]);
        assert_eq!(epub(&data).unwrap(), "Kapitel 1\nText eins.\n\nText zwei.");
    }

    #[test]
    fn converts_html() {
        let html = "<html><head><style>p{}</style><script>var x=1;</script></head><body><p>Hallo   <b>Welt</b> &auml;&#246;&#xFC;</p><ul><li>eins</li><li>zwei</li></ul></body></html>";
        assert_eq!(html_to_text(html), "Hallo Welt äöü\neins\nzwei");
    }

    #[test]
    fn converts_rtf() {
        let source = r"{\rtf1\ansi\deff0{\fonttbl{\f0 Arial;}}{\colortbl;\red0\green0\blue0;}{\*\generator Word;}\f0 Gr\'fc\'dfe aus \b Freising\b0.\par Zweite Zeile mit \u8364? Euro\tab Ende\par}";
        assert_eq!(rtf(source), "Grüße aus Freising.\nZweite Zeile mit € Euro\tEnde");
    }

    #[test]
    fn decodes_text_files() {
        assert_eq!(decode(&[0xEF, 0xBB, 0xBF, b'a']), "a");
        assert_eq!(decode(&[0x47, 0x72, 0xFC, 0xDF, 0x80]), "Grüß€");
        assert_eq!(decode("Grüße".as_bytes()), "Grüße");
    }
}
