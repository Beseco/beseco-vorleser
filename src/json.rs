//! Kleines JSON: genug für die KI-Schnittstellen (Anfragen bauen, Antworten lesen), ohne eine
//! größere Bibliothek mitzuschleppen.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(map) => map.get(key),
            _ => None,
        }
    }

    pub fn index(&self, i: usize) -> Option<&Value> {
        match self {
            Value::Array(items) => items.get(i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Value]> {
        match self {
            Value::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Folgt einem Pfad wie `["choices", "0", "message", "content"]`.
    pub fn path(&self, parts: &[&str]) -> Option<&Value> {
        parts.iter().try_fold(self, |value, part| match part.parse::<usize>() {
            Ok(i) if matches!(value, Value::Array(_)) => value.index(i),
            _ => value.get(part),
        })
    }
}

/// Schreibt einen Text als JSON-Zeichenkette (mit Anführungszeichen).
pub fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

pub fn parse(text: &str) -> Option<Value> {
    let mut parser = Parser { chars: text.chars().collect(), pos: 0 };
    let value = parser.value()?;
    parser.space();
    (parser.pos == parser.chars.len()).then_some(value)
}

struct Parser {
    chars: Vec<char>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.pos).copied()
    }

    fn space(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
            self.pos += 1;
        }
    }

    fn literal(&mut self, word: &str, value: Value) -> Option<Value> {
        let end = self.pos + word.chars().count();
        if self.chars.get(self.pos..end)?.iter().copied().eq(word.chars()) {
            self.pos = end;
            Some(value)
        } else {
            None
        }
    }

    fn value(&mut self) -> Option<Value> {
        self.space();
        match self.peek()? {
            '{' => self.object(),
            '[' => self.array(),
            '"' => self.string().map(Value::String),
            't' => self.literal("true", Value::Bool(true)),
            'f' => self.literal("false", Value::Bool(false)),
            'n' => self.literal("null", Value::Null),
            _ => self.number(),
        }
    }

    fn object(&mut self) -> Option<Value> {
        self.pos += 1;
        let mut map = BTreeMap::new();
        self.space();
        if self.peek() == Some('}') {
            self.pos += 1;
            return Some(Value::Object(map));
        }
        loop {
            self.space();
            let key = self.string()?;
            self.space();
            if self.peek()? != ':' {
                return None;
            }
            self.pos += 1;
            map.insert(key, self.value()?);
            self.space();
            match self.peek()? {
                ',' => self.pos += 1,
                '}' => {
                    self.pos += 1;
                    return Some(Value::Object(map));
                }
                _ => return None,
            }
        }
    }

    fn array(&mut self) -> Option<Value> {
        self.pos += 1;
        let mut items = Vec::new();
        self.space();
        if self.peek() == Some(']') {
            self.pos += 1;
            return Some(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            self.space();
            match self.peek()? {
                ',' => self.pos += 1,
                ']' => {
                    self.pos += 1;
                    return Some(Value::Array(items));
                }
                _ => return None,
            }
        }
    }

    fn hex4(&mut self) -> Option<u32> {
        let digits: String = self.chars.get(self.pos..self.pos + 4)?.iter().collect();
        self.pos += 4;
        u32::from_str_radix(&digits, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        if self.peek()? != '"' {
            return None;
        }
        self.pos += 1;
        let mut out = String::new();
        loop {
            let c = self.peek()?;
            self.pos += 1;
            match c {
                '"' => return Some(out),
                '\\' => {
                    let escaped = self.peek()?;
                    self.pos += 1;
                    match escaped {
                        'n' => out.push('\n'),
                        't' => out.push('\t'),
                        'r' => out.push('\r'),
                        'b' => out.push('\u{8}'),
                        'f' => out.push('\u{c}'),
                        'u' => {
                            let first = self.hex4()?;
                            // Zeichen außerhalb der BMP kommen als Paar von Ersatzzeichen.
                            let code = if (0xD800..0xDC00).contains(&first) && self.chars.get(self.pos..self.pos + 2) == Some(&['\\', 'u']) {
                                self.pos += 2;
                                let second = self.hex4()?;
                                0x10000 + ((first - 0xD800) << 10) + (second.wrapping_sub(0xDC00) & 0x3FF)
                            } else {
                                first
                            };
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        other => out.push(other),
                    }
                }
                c => out.push(c),
            }
        }
    }

    fn number(&mut self) -> Option<Value> {
        let start = self.pos;
        while self.peek().is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '+' | '.' | 'e' | 'E')) {
            self.pos += 1;
        }
        let text: String = self.chars[start..self.pos].iter().collect();
        text.parse().ok().map(Value::Number)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_chat_answer() {
        let answer = r#"{"id":"x","choices":[{"index":0,"message":{"role":"assistant","content":"Grüße \"aus\" Freising\n😀"}}],"usage":{"total_tokens":12}}"#;
        let value = parse(answer).unwrap();
        let content = value.path(&["choices", "0", "message", "content"]).and_then(Value::as_str);
        assert_eq!(content, Some("Grüße \"aus\" Freising\n😀"));
        assert_eq!(value.path(&["usage", "total_tokens"]), Some(&Value::Number(12.0)));
        assert!(parse("{\"a\":").is_none());
        assert!(parse("[1, 2] x").is_none());
    }

    #[test]
    fn round_trips_strings() {
        let text = "Zeile 1\nTab\t\"Zitat\" \\ und \u{1}";
        assert_eq!(parse(&string(text)), Some(Value::String(text.to_string())));
    }
}
