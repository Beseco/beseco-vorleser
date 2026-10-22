//! Liest die Textebene von PDF-Dateien: schnell und zeichengenau, mit der Lage jeder Zeile auf
//! der Seite (für das Erkennen von Kopf- und Fußzeilen). Unterstützt komprimierte Inhalte,
//! Objekt-Streams, Schriften mit `ToUnicode`-Tabelle, die üblichen einfachen Kodierungen und
//! Text in Formular-Objekten. Seiten ohne lesbare Textebene (Scans, unbekannte Schriften)
//! meldet [`Page::readable`] als `false`; die gehen dann an die Texterkennung.

use std::collections::HashMap;

/// Eine Zeile mit ihrer Höhe auf der Seite: 0 = oben, 1 = unten.
#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub top: f32,
}

#[derive(Clone, Debug, Default)]
pub struct Page {
    /// Zeilen in Lesereihenfolge; eine leere Zeile trennt Absätze.
    pub lines: Vec<Line>,
    pub readable: bool,
}

#[derive(Clone, Debug, PartialEq)]
enum Obj {
    Null,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Name(String),
    Array(Vec<Obj>),
    Dict(Vec<(String, Obj)>),
    Ref(u32),
    Stream(Vec<(String, Obj)>, Vec<u8>),
    /// Befehl in einem Inhaltsstrom (z. B. `Tj`).
    Op(String),
}

impl Obj {
    fn get(&self, key: &str) -> Option<&Obj> {
        match self {
            Obj::Dict(entries) | Obj::Stream(entries, _) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    fn num(&self) -> Option<f64> {
        match self {
            Obj::Num(n) => Some(*n),
            _ => None,
        }
    }

    fn name(&self) -> Option<&str> {
        match self {
            Obj::Name(n) => Some(n),
            _ => None,
        }
    }
}

fn is_white(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0C | 0)
}

fn is_delim(b: u8) -> bool {
    matches!(b, b'(' | b')' | b'<' | b'>' | b'[' | b']' | b'{' | b'}' | b'/' | b'%')
}

struct Parser<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(data: &'a [u8], pos: usize) -> Self {
        Parser { data, pos }
    }

    fn peek(&self) -> Option<u8> {
        self.data.get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while let Some(b) = self.peek() {
            if is_white(b) {
                self.pos += 1;
            } else if b == b'%' {
                while self.peek().is_some_and(|b| b != b'\n' && b != b'\r') {
                    self.pos += 1;
                }
            } else {
                break;
            }
        }
    }

    fn word(&mut self) -> &'a [u8] {
        let start = self.pos;
        while self.peek().is_some_and(|b| !is_white(b) && !is_delim(b)) {
            self.pos += 1;
        }
        &self.data[start..self.pos]
    }

    /// Ein Wert; `refs` erlaubt „12 0 R“ (in Objekten, nicht in Inhaltsströmen).
    fn value(&mut self, refs: bool) -> Option<Obj> {
        self.skip_space();
        let b = self.peek()?;
        match b {
            b'/' => {
                self.pos += 1;
                let raw = self.word();
                let mut name = Vec::new();
                let mut i = 0;
                while i < raw.len() {
                    if raw[i] == b'#' && i + 2 < raw.len() {
                        if let Some(v) = std::str::from_utf8(raw.get(i + 1..i + 3).unwrap_or_default()).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                            name.push(v);
                            i += 3;
                            continue;
                        }
                    }
                    name.push(raw[i]);
                    i += 1;
                }
                Some(Obj::Name(String::from_utf8_lossy(&name).into_owned()))
            }
            b'(' => Some(Obj::Str(self.literal())),
            b'<' if self.data.get(self.pos + 1) == Some(&b'<') => {
                self.pos += 2;
                let mut entries = Vec::new();
                loop {
                    self.skip_space();
                    match self.peek()? {
                        b'>' => {
                            self.pos += 2;
                            break;
                        }
                        b'/' => {
                            let Obj::Name(key) = self.value(refs)? else { return None };
                            let value = self.value(refs)?;
                            entries.push((key, value));
                        }
                        _ => {
                            // Kaputter Eintrag: überspringen.
                            self.value(refs)?;
                        }
                    }
                }
                // Folgt ein Datenstrom, gehört er zum Wörterbuch.
                let save = self.pos;
                self.skip_space();
                if self.data[self.pos..].starts_with(b"stream") {
                    self.pos += 6;
                    if self.peek() == Some(b'\r') {
                        self.pos += 1;
                    }
                    if self.peek() == Some(b'\n') {
                        self.pos += 1;
                    }
                    let start = self.pos;
                    let length = entries.iter().find(|(k, _)| k == "Length").and_then(|(_, v)| v.num()).map(|n| n as usize);
                    let end = match length {
                        Some(len) if start + len <= self.data.len() && self.data[start + len..].starts_with_space_then(b"endstream") => start + len,
                        _ => find(&self.data[start..], b"endstream").map_or(self.data.len(), |i| start + i),
                    };
                    let mut bytes = self.data[start..end].to_vec();
                    while bytes.last().is_some_and(|b| *b == b'\n' || *b == b'\r') && length.is_none() {
                        bytes.pop();
                    }
                    self.pos = end + find(&self.data[end..], b"endstream").map_or(0, |i| i + 9);
                    return Some(Obj::Stream(entries, bytes));
                }
                self.pos = save;
                Some(Obj::Dict(entries))
            }
            b'<' => {
                self.pos += 1;
                let start = self.pos;
                while self.peek().is_some_and(|b| b != b'>') {
                    self.pos += 1;
                }
                let hex: Vec<u8> = self.data[start..self.pos].iter().copied().filter(|b| b.is_ascii_hexdigit()).collect();
                self.pos += 1;
                let mut bytes = Vec::new();
                for pair in hex.chunks(2) {
                    let s = if pair.len() == 2 { format!("{}{}", pair[0] as char, pair[1] as char) } else { format!("{}0", pair[0] as char) };
                    bytes.push(u8::from_str_radix(&s, 16).unwrap_or(0));
                }
                Some(Obj::Str(bytes))
            }
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    self.skip_space();
                    match self.peek()? {
                        b']' => {
                            self.pos += 1;
                            break;
                        }
                        _ => items.push(self.value(refs)?),
                    }
                }
                Some(Obj::Array(items))
            }
            b'>' | b')' | b']' | b'{' | b'}' => {
                self.pos += 1;
                Some(Obj::Null)
            }
            _ => {
                let word = self.word();
                if word.is_empty() {
                    self.pos += 1;
                    return Some(Obj::Null);
                }
                let text = std::str::from_utf8(word).unwrap_or("");
                if let Ok(n) = text.parse::<f64>() {
                    // „12 0 R“ ist ein Verweis.
                    if refs && !text.contains('.') {
                        let save = self.pos;
                        self.skip_space();
                        let generation = self.word();
                        if !generation.is_empty() && generation.iter().all(u8::is_ascii_digit) {
                            self.skip_space();
                            if self.peek() == Some(b'R') && self.data.get(self.pos + 1).is_none_or(|b| is_white(*b) || is_delim(*b)) {
                                self.pos += 1;
                                return Some(Obj::Ref(n as u32));
                            }
                        }
                        self.pos = save;
                    }
                    return Some(Obj::Num(n));
                }
                Some(match text {
                    "true" => Obj::Bool(true),
                    "false" => Obj::Bool(false),
                    "null" => Obj::Null,
                    _ => Obj::Op(text.to_string()),
                })
            }
        }
    }

    fn literal(&mut self) -> Vec<u8> {
        self.pos += 1;
        let mut out = Vec::new();
        let mut depth = 1;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match b {
                b'(' => {
                    depth += 1;
                    out.push(b);
                }
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    out.push(b);
                }
                b'\\' => {
                    let Some(c) = self.peek() else { break };
                    self.pos += 1;
                    match c {
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'\r' => {
                            if self.peek() == Some(b'\n') {
                                self.pos += 1;
                            }
                        }
                        b'\n' => {}
                        b'0'..=b'7' => {
                            let mut v = (c - b'0') as u32;
                            for _ in 0..2 {
                                match self.peek() {
                                    Some(d @ b'0'..=b'7') => {
                                        v = v * 8 + (d - b'0') as u32;
                                        self.pos += 1;
                                    }
                                    _ => break,
                                }
                            }
                            out.push(v as u8);
                        }
                        _ => out.push(c),
                    }
                }
                _ => out.push(b),
            }
        }
        out
    }
}

trait StartsWithSpaceThen {
    fn starts_with_space_then(&self, word: &[u8]) -> bool;
}

impl StartsWithSpaceThen for [u8] {
    fn starts_with_space_then(&self, word: &[u8]) -> bool {
        let skip = self.iter().take_while(|b| is_white(**b)).count();
        self[skip..].starts_with(word)
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Alle Objekte der Datei (spätere Fassungen ersetzen frühere).
struct Document {
    objects: HashMap<u32, Obj>,
}

impl Document {
    fn parse(data: &[u8]) -> Document {
        let mut objects = HashMap::new();
        let mut i = 0;
        while let Some(found) = find(&data[i..], b"obj") {
            let at = i + found;
            i = at + 3;
            // Vor „obj“ stehen Nummer und Generation: „12 0 obj“ (nicht „endobj“).
            let before = &data[..at];
            let mut j = before.len();
            let back = |j: &mut usize, digits: bool| {
                let end = *j;
                while *j > 0 && (if digits { before[*j - 1].is_ascii_digit() } else { is_white(before[*j - 1]) }) {
                    *j -= 1;
                }
                end - *j
            };
            if back(&mut j, false) == 0 || back(&mut j, true) == 0 || back(&mut j, false) == 0 {
                continue;
            }
            let num_end = j;
            if back(&mut j, true) == 0 {
                continue;
            }
            let Some(number) = std::str::from_utf8(&before[j..num_end]).ok().and_then(|s| s.parse::<u32>().ok()) else { continue };
            if !data.get(at + 3).is_none_or(|b| is_white(*b) || is_delim(*b)) {
                continue;
            }
            let mut parser = Parser::new(data, at + 3);
            if let Some(value) = parser.value(true) {
                objects.insert(number, value);
                i = parser.pos.max(i);
            }
        }
        let mut doc = Document { objects };
        doc.unpack_object_streams();
        doc
    }

    /// Objekte aus Objekt-Strömen (PDF 1.5) dazunehmen.
    fn unpack_object_streams(&mut self) {
        let streams: Vec<Obj> = self.objects.values().filter(|o| o.get("Type").and_then(Obj::name) == Some("ObjStm")).cloned().collect();
        for stream in streams {
            let Some(data) = self.decode(&stream) else { continue };
            let count = stream.get("N").and_then(Obj::num).unwrap_or(0.0) as usize;
            let first = stream.get("First").and_then(Obj::num).unwrap_or(0.0) as usize;
            let mut header = Parser::new(&data, 0);
            let mut entries = Vec::new();
            for _ in 0..count {
                let (Some(Obj::Num(n)), Some(Obj::Num(off))) = (header.value(false), header.value(false)) else { break };
                entries.push((n as u32, off as usize));
            }
            for (number, offset) in entries {
                if self.objects.contains_key(&number) {
                    continue;
                }
                let mut parser = Parser::new(&data, first + offset);
                if let Some(value) = parser.value(true) {
                    self.objects.insert(number, value);
                }
            }
        }
    }

    fn resolve<'a>(&'a self, obj: &'a Obj) -> &'a Obj {
        let mut current = obj;
        for _ in 0..8 {
            match current {
                Obj::Ref(n) => current = self.objects.get(n).unwrap_or(&Obj::Null),
                _ => return current,
            }
        }
        current
    }

    fn get<'a>(&'a self, obj: &'a Obj, key: &str) -> Option<&'a Obj> {
        obj.get(key).map(|v| self.resolve(v))
    }

    /// Entpackt einen Datenstrom; `None` bei nicht unterstütztem Verfahren.
    fn decode(&self, stream: &Obj) -> Option<Vec<u8>> {
        let Obj::Stream(_, raw) = stream else { return None };
        let filters: Vec<String> = match self.get(stream, "Filter") {
            None | Some(Obj::Null) => Vec::new(),
            Some(Obj::Name(n)) => vec![n.clone()],
            Some(Obj::Array(items)) => items.iter().filter_map(|i| self.resolve(i).name().map(str::to_string)).collect(),
            _ => return None,
        };
        let mut data = raw.clone();
        for filter in filters {
            data = match filter.as_str() {
                "FlateDecode" | "Fl" => miniz_oxide::inflate::decompress_to_vec_zlib(&data)
                    .or_else(|_| miniz_oxide::inflate::decompress_to_vec(&data))
                    .ok()?,
                "ASCIIHexDecode" | "AHx" => {
                    let hex: Vec<u8> = data.iter().copied().take_while(|b| *b != b'>').filter(|b| b.is_ascii_hexdigit()).collect();
                    hex.chunks(2).map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap_or("0"), 16).unwrap_or(0)).collect()
                }
                _ => return None,
            };
        }
        Some(data)
    }

    fn pages(&self) -> Vec<(Obj, Obj, [f64; 4])> {
        let catalog = self.objects.values().find(|o| o.get("Type").and_then(Obj::name) == Some("Catalog"));
        let mut result = Vec::new();
        if let Some(root) = catalog.and_then(|c| self.get(c, "Pages")) {
            self.collect_pages(root, None, [0.0, 0.0, 612.0, 792.0], &mut result, 0);
        }
        result
    }

    fn collect_pages(&self, node: &Obj, resources: Option<&Obj>, media: [f64; 4], out: &mut Vec<(Obj, Obj, [f64; 4])>, depth: usize) {
        if depth > 32 {
            return;
        }
        let resources = self.get(node, "Resources").or(resources);
        let media = match self.get(node, "MediaBox") {
            Some(Obj::Array(items)) if items.len() == 4 => {
                let v: Vec<f64> = items.iter().map(|i| self.resolve(i).num().unwrap_or(0.0)).collect();
                [v[0], v[1], v[2], v[3]]
            }
            _ => media,
        };
        match node.get("Type").and_then(Obj::name) {
            Some("Page") => out.push((node.clone(), resources.cloned().unwrap_or(Obj::Null), media)),
            _ => {
                if let Some(Obj::Array(kids)) = self.get(node, "Kids") {
                    for kid in kids {
                        self.collect_pages(self.resolve(kid), resources, media, out, depth + 1);
                    }
                }
            }
        }
    }
}

/// Wie die Zeichen einer Schrift zu Text werden.
struct Font {
    /// Bytes je Zeichencode (1 oder 2).
    width: usize,
    unicode: HashMap<u32, String>,
    /// Einfache Schriften: Kodierung der 256 Codes.
    simple: Option<Vec<Option<char>>>,
    /// Breite je Code in Tausendstel der Schriftgröße.
    widths: HashMap<u32, f64>,
    default_width: f64,
}

impl Font {
    fn load(doc: &Document, font: &Obj) -> Font {
        let subtype = font.get("Subtype").and_then(Obj::name).unwrap_or("");
        let composite = subtype == "Type0";
        let mut unicode = HashMap::new();
        let mut width = if composite { 2 } else { 1 };
        if let Some(cmap) = doc.get(font, "ToUnicode").and_then(|s| doc.decode(s)) {
            let (w, map) = parse_cmap(&cmap);
            if let Some(w) = w {
                width = w;
            }
            unicode = map;
        }
        let simple = (!composite).then(|| encoding(doc, font));
        let mut widths = HashMap::new();
        let mut default_width = 500.0;
        if composite {
            // Breiten stehen in der Nachfahren-Schrift: /W [c [w1 w2 …] c1 c2 w …], /DW.
            if let Some(Obj::Array(kids)) = doc.get(font, "DescendantFonts") {
                if let Some(cid) = kids.first().map(|k| doc.resolve(k)) {
                    default_width = doc.get(cid, "DW").and_then(Obj::num).unwrap_or(1000.0);
                    if let Some(Obj::Array(w)) = doc.get(cid, "W") {
                        let items: Vec<&Obj> = w.iter().map(|i| doc.resolve(i)).collect();
                        let mut i = 0;
                        while i < items.len() {
                            match (items[i].num(), items.get(i + 1)) {
                                (Some(first), Some(Obj::Array(list))) => {
                                    for (k, value) in list.iter().enumerate() {
                                        if let Some(v) = doc.resolve(value).num() {
                                            widths.insert(first as u32 + k as u32, v);
                                        }
                                    }
                                    i += 2;
                                }
                                (Some(first), Some(Obj::Num(last))) => {
                                    if let Some(v) = items.get(i + 2).and_then(|o| o.num()) {
                                        for c in first as u32..=(*last as u32).min(first as u32 + 0xFFFF) {
                                            widths.insert(c, v);
                                        }
                                    }
                                    i += 3;
                                }
                                _ => i += 1,
                            }
                        }
                    }
                }
            }
        } else if let Some(Obj::Array(w)) = doc.get(font, "Widths") {
            let first = doc.get(font, "FirstChar").and_then(Obj::num).unwrap_or(0.0) as u32;
            for (k, value) in w.iter().enumerate() {
                if let Some(v) = doc.resolve(value).num() {
                    widths.insert(first + k as u32, v);
                }
            }
        }
        Font { width, unicode, simple, widths, default_width }
    }

    /// Text, Breite (Tausendstel der Schriftgröße), Zahl der Zeichen und der Leerzeichen-Codes.
    fn text(&self, bytes: &[u8]) -> (String, f64, usize, usize) {
        let mut out = String::new();
        let (mut advance, mut count, mut spaces) = (0.0, 0, 0);
        for code in bytes.chunks(self.width) {
            let value = code.iter().fold(0u32, |v, b| v << 8 | *b as u32);
            advance += self.widths.get(&value).copied().unwrap_or(self.default_width);
            count += 1;
            if self.width == 1 && value == 32 {
                spaces += 1;
            }
            if let Some(text) = self.unicode.get(&value) {
                out.push_str(text);
            } else if let Some(ch) = self.simple.as_ref().and_then(|table| table.get(value as usize).copied().flatten()) {
                out.push(ch);
            } else {
                out.push('\u{FFFD}');
            }
        }
        (out, advance, count, spaces)
    }
}

/// `ToUnicode`-Tabelle: Breite der Codes und Code → Text.
fn parse_cmap(data: &[u8]) -> (Option<usize>, HashMap<u32, String>) {
    let mut map = HashMap::new();
    let mut width = None;
    let mut parser = Parser::new(data, 0);
    let mut operands: Vec<Obj> = Vec::new();
    let utf16 = |bytes: &[u8]| -> String {
        let units: Vec<u16> = bytes.chunks(2).map(|p| if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) } else { p[0] as u16 }).collect();
        String::from_utf16_lossy(&units)
    };
    let code = |bytes: &[u8]| bytes.iter().fold(0u32, |v, b| v << 8 | *b as u32);
    while let Some(value) = parser.value(false) {
        if parser.pos >= data.len() && value == Obj::Null {
            break;
        }
        match value {
            Obj::Op(op) => {
                match op.as_str() {
                    "endcodespacerange" => {
                        if let Some(Obj::Str(low)) = operands.first() {
                            width = Some(low.len().clamp(1, 4));
                        }
                    }
                    "endbfchar" => {
                        for pair in operands.chunks(2) {
                            if let [Obj::Str(src), Obj::Str(dst)] = pair {
                                map.insert(code(src), utf16(dst));
                            }
                        }
                    }
                    "endbfrange" => {
                        for triple in operands.chunks(3) {
                            if let [Obj::Str(lo), Obj::Str(hi), dst] = triple {
                                let (lo, hi) = (code(lo), code(hi));
                                if hi < lo || hi - lo > 0xFFFF {
                                    continue;
                                }
                                match dst {
                                    Obj::Str(start) => {
                                        let base: Vec<u16> = start.chunks(2).map(|p| if p.len() == 2 { u16::from_be_bytes([p[0], p[1]]) } else { p[0] as u16 }).collect();
                                        for (i, c) in (lo..=hi).enumerate() {
                                            let mut units = base.clone();
                                            if let Some(last) = units.last_mut() {
                                                *last = last.wrapping_add(i as u16);
                                            }
                                            map.insert(c, String::from_utf16_lossy(&units));
                                        }
                                    }
                                    Obj::Array(items) => {
                                        for (c, item) in (lo..=hi).zip(items) {
                                            if let Obj::Str(s) = item {
                                                map.insert(c, utf16(s));
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    _ => {}
                }
                operands.clear();
            }
            other => operands.push(other),
        }
    }
    (width, map)
}

/// Kodierung einer einfachen Schrift: Windows-1252 bzw. MacRoman, ergänzt um `Differences`.
fn encoding(doc: &Document, font: &Obj) -> Vec<Option<char>> {
    let mac = |name: &str| name == "MacRomanEncoding";
    let mut table: Vec<Option<char>> = (0..256u32).map(|b| Some(crate::documents::windows_1252(b as u8))).collect();
    let set_base = |table: &mut Vec<Option<char>>, name: &str| {
        if mac(name) {
            for (i, slot) in table.iter_mut().enumerate().skip(128) {
                *slot = MAC_ROMAN.chars().nth(i - 128);
            }
        }
    };
    match doc.get(font, "Encoding") {
        Some(Obj::Name(name)) => set_base(&mut table, name),
        Some(dict @ Obj::Dict(_)) => {
            if let Some(base) = doc.get(dict, "BaseEncoding").and_then(Obj::name) {
                set_base(&mut table, base);
            }
            if let Some(Obj::Array(diffs)) = doc.get(dict, "Differences") {
                let mut code = 0usize;
                for item in diffs {
                    match doc.resolve(item) {
                        Obj::Num(n) => code = *n as usize,
                        Obj::Name(glyph) => {
                            if code < 256 {
                                if let Some(ch) = glyph_char(glyph) {
                                    table[code] = Some(ch);
                                }
                            }
                            code += 1;
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }
    table
}

const MAC_ROMAN: &str = "ÄÅÇÉÑÖÜáàâäãåçéèêëíìîïñóòôöõúùûü†°¢£§•¶ß®©™´¨≠ÆØ∞±≤≥¥µ∂∑∏π∫ªºΩæø¿¡¬√ƒ≈∆«»… ÀÃÕŒœ–—“”‘’÷◊ÿŸ⁄€‹›ﬁﬂ‡·‚„‰ÂÊÁËÈÍÎÏÌÓÔÒÚÛÙıˆ˜¯˘˙˚¸˝˛ˇ";

/// Zeichen zu einem Glyphennamen (die in deutschen und englischen Texten üblichen).
fn glyph_char(name: &str) -> Option<char> {
    if name.chars().count() == 1 {
        return name.chars().next();
    }
    if let Some(hex) = name.strip_prefix("uni").filter(|h| h.len() == 4) {
        return u32::from_str_radix(hex, 16).ok().and_then(char::from_u32);
    }
    let digits = ["zero", "one", "two", "three", "four", "five", "six", "seven", "eight", "nine"];
    if let Some(i) = digits.iter().position(|d| *d == name) {
        return char::from_digit(i as u32, 10);
    }
    Some(match name {
        "space" | "nbspace" => ' ',
        "exclam" => '!',
        "quotedbl" => '"',
        "numbersign" => '#',
        "dollar" => '$',
        "percent" => '%',
        "ampersand" => '&',
        "quotesingle" | "quoteright" => '’',
        "quoteleft" => '‘',
        "parenleft" => '(',
        "parenright" => ')',
        "asterisk" => '*',
        "plus" => '+',
        "comma" => ',',
        "hyphen" | "minus" => '-',
        "period" => '.',
        "slash" => '/',
        "colon" => ':',
        "semicolon" => ';',
        "less" => '<',
        "equal" => '=',
        "greater" => '>',
        "question" => '?',
        "at" => '@',
        "bracketleft" => '[',
        "bracketright" => ']',
        "underscore" => '_',
        "endash" => '–',
        "emdash" => '—',
        "quotedblleft" => '“',
        "quotedblright" => '”',
        "quotedblbase" => '„',
        "quotesinglbase" => '‚',
        "bullet" => '•',
        "ellipsis" => '…',
        "section" => '§',
        "degree" => '°',
        "Euro" => '€',
        "copyright" => '©',
        "registered" => '®',
        "adieresis" => 'ä',
        "odieresis" => 'ö',
        "udieresis" => 'ü',
        "Adieresis" => 'Ä',
        "Odieresis" => 'Ö',
        "Udieresis" => 'Ü',
        "germandbls" => 'ß',
        "eacute" => 'é',
        "egrave" => 'è',
        "agrave" => 'à',
        "ccedilla" => 'ç',
        _ => return None,
    })
}

/// Ein Stück Text an einer Stelle der Seite (x von Anfang bis Ende).
struct Run {
    text: String,
    x: f64,
    end: f64,
    y: f64,
    size: f64,
}

type Matrix = [f64; 6];

fn multiply(a: &Matrix, b: &Matrix) -> Matrix {
    [
        a[0] * b[0] + a[1] * b[2],
        a[0] * b[1] + a[1] * b[3],
        a[2] * b[0] + a[3] * b[2],
        a[2] * b[1] + a[3] * b[3],
        a[4] * b[0] + a[5] * b[2] + b[4],
        a[4] * b[1] + a[5] * b[3] + b[5],
    ]
}

const IDENTITY: Matrix = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];

struct Interpreter<'a> {
    doc: &'a Document,
    fonts: HashMap<String, Font>,
    runs: Vec<Run>,
}

impl Interpreter<'_> {
    fn font(&mut self, resources: &Obj, name: &str) {
        if self.fonts.contains_key(name) {
            return;
        }
        let font = self.doc.get(resources, "Font").and_then(|fonts| self.doc.get(fonts, name)).map(|f| Font::load(self.doc, f));
        self.fonts.insert(
            name.to_string(),
            font.unwrap_or(Font { width: 1, unicode: HashMap::new(), simple: Some(Vec::new()), widths: HashMap::new(), default_width: 500.0 }),
        );
    }

    fn run(&mut self, content: &[u8], resources: &Obj, base: Matrix, depth: usize) {
        let mut parser = Parser::new(content, 0);
        let mut operands: Vec<Obj> = Vec::new();
        let mut ctm = base;
        let mut stack = Vec::new();
        let (mut tm, mut lm) = (IDENTITY, IDENTITY);
        let (mut font, mut size, mut leading) = (String::new(), 10.0f64, 0.0f64);
        let (mut char_spacing, mut word_spacing) = (0.0f64, 0.0f64);
        // Fonts sind je Formular eigene Namensräume.
        let saved_fonts = std::mem::take(&mut self.fonts);
        let num = |ops: &[Obj], i: usize| ops.get(i).and_then(Obj::num).unwrap_or(0.0);
        while parser.pos < content.len() {
            let Some(value) = parser.value(false) else { break };
            let Obj::Op(op) = value else {
                operands.push(value);
                continue;
            };
            let n = operands.len();
            let arg = |i: usize| num(&operands, n.wrapping_sub(i));
            match op.as_str() {
                "q" => stack.push(ctm),
                "Q" => ctm = stack.pop().unwrap_or(base),
                "cm" if n >= 6 => ctm = multiply(&[arg(6), arg(5), arg(4), arg(3), arg(2), arg(1)], &ctm),
                "BT" => {
                    tm = IDENTITY;
                    lm = IDENTITY;
                }
                "Tc" if n >= 1 => char_spacing = arg(1),
                "Tw" if n >= 1 => word_spacing = arg(1),
                "Tf" if n >= 2 => {
                    if let Some(Obj::Name(name)) = operands.get(n - 2) {
                        font = name.clone();
                        self.font(resources, &font);
                    }
                    size = arg(1);
                }
                "TL" if n >= 1 => leading = arg(1),
                "Td" | "TD" if n >= 2 => {
                    if op == "TD" {
                        leading = -arg(1);
                    }
                    lm = multiply(&[1.0, 0.0, 0.0, 1.0, arg(2), arg(1)], &lm);
                    tm = lm;
                }
                "Tm" if n >= 6 => {
                    lm = [arg(6), arg(5), arg(4), arg(3), arg(2), arg(1)];
                    tm = lm;
                }
                "T*" => {
                    lm = multiply(&[1.0, 0.0, 0.0, 1.0, 0.0, -leading], &lm);
                    tm = lm;
                }
                "Tj" | "'" | "\"" | "TJ" => {
                    if op == "'" || op == "\"" {
                        lm = multiply(&[1.0, 0.0, 0.0, 1.0, 0.0, -leading], &lm);
                        tm = lm;
                    }
                    let mut text = String::new();
                    let Some(decoder) = self.fonts.get(&font) else {
                        operands.clear();
                        continue;
                    };
                    // Vorschub in Texteinheiten, damit der nächste Lauf an der richtigen Stelle beginnt.
                    let mut advance = 0.0;
                    let piece = |bytes: &[u8], text: &mut String, advance: &mut f64| {
                        let (t, width, count, spaces) = decoder.text(bytes);
                        text.push_str(&t);
                        *advance += width / 1000.0 * size + char_spacing * count as f64 + word_spacing * spaces as f64;
                    };
                    match operands.last() {
                        Some(Obj::Str(bytes)) => piece(bytes, &mut text, &mut advance),
                        Some(Obj::Array(items)) => {
                            for item in items {
                                match item {
                                    Obj::Str(bytes) => piece(bytes, &mut text, &mut advance),
                                    Obj::Num(gap) => {
                                        // Großer Abstand nach rechts ist ein Wortzwischenraum.
                                        if *gap < -180.0 && !text.ends_with(' ') && !text.is_empty() {
                                            text.push(' ');
                                        }
                                        advance -= gap / 1000.0 * size;
                                    }
                                    _ => {}
                                }
                            }
                        }
                        _ => {}
                    }
                    let m = multiply(&tm, &ctm);
                    if !text.is_empty() {
                        let scale = (m[2] * m[2] + m[3] * m[3]).sqrt();
                        let end = m[4] + advance * m[0];
                        self.runs.push(Run { text, x: m[4], end, y: m[5], size: (size * scale).abs().max(1.0) });
                    }
                    tm = multiply(&[1.0, 0.0, 0.0, 1.0, advance, 0.0], &tm);
                }
                "Do" if depth < 4 => {
                    if let Some(Obj::Name(name)) = operands.last() {
                        let form = self.doc.get(resources, "XObject").and_then(|x| self.doc.get(x, name)).cloned();
                        if let Some(form) = form.filter(|f| f.get("Subtype").and_then(Obj::name) == Some("Form")) {
                            let matrix = match self.doc.get(&form, "Matrix") {
                                Some(Obj::Array(m)) if m.len() == 6 => {
                                    let v: Vec<f64> = m.iter().map(|i| self.doc.resolve(i).num().unwrap_or(0.0)).collect();
                                    [v[0], v[1], v[2], v[3], v[4], v[5]]
                                }
                                _ => IDENTITY,
                            };
                            if let Some(data) = self.doc.decode(&form) {
                                let inner = self.doc.get(&form, "Resources").cloned().unwrap_or_else(|| resources.clone());
                                self.run(&data, &inner, multiply(&matrix, &ctm), depth + 1);
                            }
                        }
                    }
                }
                _ => {}
            }
            operands.clear();
        }
        self.fonts = saved_fonts;
    }
}

/// Setzt die Textstücke einer Seite zu Zeilen zusammen (in der Reihenfolge des Inhalts, damit
/// Spalten nacheinander gelesen werden).
fn lines(runs: &[Run], media: [f64; 4]) -> Vec<Line> {
    let height = (media[3] - media[1]).abs().max(1.0);
    let top = |y: f64| ((media[3] - y) / height).clamp(0.0, 1.0) as f32;
    let mut lines: Vec<(f64, f64, String)> = Vec::new();
    let mut last_end = f64::MIN;
    for run in runs {
        match lines.last_mut() {
            Some((y, size, text)) if (run.y - *y).abs() < 0.5 * size.max(run.size) => {
                // Ein Leerzeichen nur bei einer echten Lücke (nicht bei Unterschneidungen) oder
                // einem Sprung zurück (Spalte, Tabelle).
                let gap = run.x - last_end;
                if (gap > 0.22 * run.size || gap < -run.size) && !text.ends_with(' ') && !run.text.starts_with(' ') {
                    text.push(' ');
                }
                text.push_str(&run.text);
                *size = size.max(run.size);
            }
            _ => lines.push((run.y, run.size, run.text.clone())),
        }
        last_end = run.end;
    }
    let mut out: Vec<Line> = Vec::new();
    let mut previous: Option<(f64, f64)> = None;
    for (y, size, text) in lines {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        // Größerer Abstand oder ein Sprung nach oben (neue Spalte): neuer Absatz.
        if let Some((py, psize)) = previous {
            if py - y > 1.7 * psize.max(size) || y > py + psize {
                out.push(Line { text: String::new(), top: top(y) });
            }
        }
        out.push(Line { text, top: top(y) });
        previous = Some((y, size));
    }
    out
}

/// Lohnt die Textebene? Fast leer (Scan) oder voller unbekannter Zeichen: nein.
fn readable(lines: &[Line]) -> bool {
    let text: String = lines.iter().map(|l| l.text.as_str()).collect();
    let letters = text.chars().filter(|c| c.is_alphanumeric()).count();
    let unknown = text.chars().filter(|c| *c == '\u{FFFD}' || c.is_control()).count();
    letters >= 10 && unknown * 10 <= text.chars().count()
}

/// Liest alle Seiten; `None`, wenn die Datei verschlüsselt ist oder keine Seiten findet.
pub fn extract(data: &[u8]) -> Option<Vec<Page>> {
    // Verschlüsselte Dateien (auch ohne Kennwort zum Öffnen) liest die Texterkennung.
    if find(data, b"/Encrypt").is_some() {
        return None;
    }
    let doc = Document::parse(data);
    let pages = doc.pages();
    if pages.is_empty() {
        return None;
    }
    Some(
        pages
            .iter()
            .map(|(page, resources, media)| {
                let mut interpreter = Interpreter { doc: &doc, fonts: HashMap::new(), runs: Vec::new() };
                let contents: Vec<u8> = match doc.get(page, "Contents") {
                    Some(stream @ Obj::Stream(..)) => doc.decode(stream).unwrap_or_default(),
                    Some(Obj::Array(items)) => items
                        .iter()
                        .filter_map(|i| doc.decode(doc.resolve(i)))
                        .flat_map(|mut d| {
                            d.push(b'\n');
                            d
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                interpreter.run(&contents, resources, IDENTITY, 0);
                let lines = lines(&interpreter.runs, *media);
                let readable = readable(&lines);
                Page { lines, readable }
            })
            .collect(),
    )
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Baut eine kleine PDF mit unkomprimierten Seiten (Zeilen von oben nach unten).
    pub(crate) fn sample_pdf(pages: &[Vec<&str>]) -> Vec<u8> {
        let mut objects: Vec<String> = Vec::new();
        let page_ids: Vec<usize> = (0..pages.len()).map(|i| 4 + 2 * i).collect();
        objects.push("<< /Type /Catalog /Pages 2 0 R >>".into());
        let kids: Vec<String> = page_ids.iter().map(|id| format!("{id} 0 R")).collect();
        objects.push(format!("<< /Type /Pages /Kids [{}] /Count {} /MediaBox [0 0 595 842] >>", kids.join(" "), pages.len()));
        objects.push("<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into());
        for lines in pages {
            let mut content = String::from("BT /F1 11 Tf 14 TL 56 800 Td ");
            for line in lines {
                let escaped = line.replace('\\', "\\\\").replace('(', "\\(").replace(')', "\\)");
                // Umlaute als Windows-1252-Oktalcodes.
                let encoded: String = escaped
                    .chars()
                    .map(|c| match c {
                        'ä' => "\\344".to_string(),
                        'ö' => "\\366".to_string(),
                        'ü' => "\\374".to_string(),
                        'ß' => "\\337".to_string(),
                        c => c.to_string(),
                    })
                    .collect();
                content.push_str(&format!("({encoded}) Tj T* "));
            }
            content.push_str("ET");
            objects.push(format!("<< /Type /Page /Parent 2 0 R /Resources << /Font << /F1 3 0 R >> >> /Contents {} 0 R >>", objects.len() + 2));
            objects.push(format!("<< /Length {} >>\nstream\n{content}\nendstream", content.len()));
        }
        let mut out = String::from("%PDF-1.4\n");
        for (i, body) in objects.iter().enumerate() {
            out.push_str(&format!("{} 0 obj\n{body}\nendobj\n", i + 1));
        }
        out.push_str("trailer << /Root 1 0 R >>\n%%EOF\n");
        out.into_bytes()
    }

    #[test]
    fn reads_simple_pdf() {
        let pdf = sample_pdf(&[vec!["Grüße aus Freising", "Zweite Zeile (mit Klammer)"], vec!["Seite zwei"]]);
        let pages = extract(&pdf).unwrap();
        assert_eq!(pages.len(), 2);
        let texts: Vec<&str> = pages[0].lines.iter().map(|l| l.text.as_str()).collect();
        assert_eq!(texts, ["Grüße aus Freising", "Zweite Zeile (mit Klammer)"]);
        assert!(pages[0].lines[0].top < 0.1);
        assert_eq!(pages[1].lines[0].text, "Seite zwei");
        assert!(!pages[1].readable || pages[1].lines.len() == 1);
    }

    #[test]
    fn reads_compressed_streams_and_tounicode() {
        let cmap = b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap 1 begincodespacerange <0000> <FFFF> endcodespacerange 2 beginbfchar <0001> <0048> <0002> <00E4> endbfchar 1 beginbfrange <0003> <0004> <0069> endbfrange endcmap";
        let content = b"BT /F1 12 Tf 50 700 Td <000100020003> Tj [<0004>-300<0001>] TJ ET";
        let deflate = |data: &[u8]| miniz_oxide::deflate::compress_to_vec_zlib(data, 6);
        let (cmap_z, content_z) = (deflate(cmap), deflate(content));
        let mut pdf = Vec::new();
        pdf.extend_from_slice(b"%PDF-1.5\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n");
        pdf.extend_from_slice(b"2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 600 800] >> endobj\n");
        pdf.extend_from_slice(b"3 0 obj << /Type /Page /Parent 2 0 R /Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >> endobj\n");
        pdf.extend_from_slice(b"4 0 obj << /Type /Font /Subtype /Type0 /BaseFont /X /Encoding /Identity-H /ToUnicode 5 0 R >> endobj\n");
        pdf.extend_from_slice(format!("5 0 obj << /Length {} /Filter /FlateDecode >>\nstream\n", cmap_z.len()).as_bytes());
        pdf.extend_from_slice(&cmap_z);
        pdf.extend_from_slice(b"\nendstream endobj\n");
        pdf.extend_from_slice(format!("6 0 obj << /Length {} /Filter /FlateDecode >>\nstream\n", content_z.len()).as_bytes());
        pdf.extend_from_slice(&content_z);
        pdf.extend_from_slice(b"\nendstream endobj\ntrailer << /Root 1 0 R >>\n");
        let pages = extract(&pdf).unwrap();
        assert_eq!(pages[0].lines[0].text, "Häij H");
    }

    #[test]
    fn skips_encrypted_files() {
        assert!(extract(b"%PDF-1.6\ntrailer << /Encrypt 5 0 R >>").is_none());
    }

    /// Echte Datei: `VORLESER_PDF=<Datei>`, dann `cargo test reads_real_pdf -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn reads_real_pdf() {
        let data = std::fs::read(std::env::var("VORLESER_PDF").unwrap()).unwrap();
        let start = std::time::Instant::now();
        let pages = extract(&data).expect("keine Textebene");
        println!("{} Seiten in {:?}", pages.len(), start.elapsed());
        for (i, page) in pages.iter().enumerate().take(3) {
            println!("--- Seite {} (lesbar: {}) ---", i + 1, page.readable);
            for line in &page.lines {
                println!("{:.2}  {}", line.top, line.text);
            }
        }
        let mut lines: Vec<Vec<Line>> = pages.into_iter().map(|p| p.lines).collect();
        let removed = crate::documents::strip_edges(&mut lines);
        println!("=== {removed} Kopf-/Fußzeilen entfernt; Anfang des Textes ===");
        let text = crate::documents::join_pages(&lines);
        println!("{}", &text[..text.char_indices().nth(1500).map_or(text.len(), |(i, _)| i)]);
    }
}
