//! Liest einzelne Dateien aus ZIP-Archiven (DOCX, ODT und EPUB sind ZIP-Archive mit XML).
//! Unterstützt die beiden Verfahren, die dort vorkommen: unkomprimiert und Deflate.

pub struct Zip<'a> {
    data: &'a [u8],
    entries: Vec<Entry>,
}

struct Entry {
    name: String,
    method: u16,
    compressed: usize,
    size: usize,
    offset: usize,
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

impl<'a> Zip<'a> {
    /// Liest das Inhaltsverzeichnis am Ende des Archivs.
    pub fn open(data: &'a [u8]) -> Option<Zip<'a>> {
        // „End of central directory“ steht in den letzten 64 KB (danach höchstens ein Kommentar).
        let search_from = data.len().saturating_sub(22 + 65_535);
        let end = (search_from..data.len().saturating_sub(21)).rev().find(|&i| u32_at(data, i) == Some(0x0605_4b50))?;
        let count = u16_at(data, end + 10)? as usize;
        let mut at = u32_at(data, end + 16)? as usize;
        let mut entries = Vec::with_capacity(count);
        for _ in 0..count {
            if u32_at(data, at)? != 0x0201_4b50 {
                return None;
            }
            let method = u16_at(data, at + 10)?;
            let compressed = u32_at(data, at + 20)? as usize;
            let size = u32_at(data, at + 24)? as usize;
            let name_len = u16_at(data, at + 28)? as usize;
            let extra_len = u16_at(data, at + 30)? as usize;
            let comment_len = u16_at(data, at + 32)? as usize;
            let offset = u32_at(data, at + 42)? as usize;
            let name = String::from_utf8_lossy(data.get(at + 46..at + 46 + name_len)?).replace('\\', "/");
            entries.push(Entry { name, method, compressed, size, offset });
            at += 46 + name_len + extra_len + comment_len;
        }
        Some(Zip { data, entries })
    }

    #[cfg(test)]
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|e| e.name.as_str())
    }

    /// Inhalt einer Datei; der Name wird ohne Rücksicht auf Groß- und Kleinschreibung gesucht.
    pub fn read(&self, name: &str) -> Option<Vec<u8>> {
        let entry = self.entries.iter().find(|e| e.name.eq_ignore_ascii_case(name))?;
        let header = entry.offset;
        if u32_at(self.data, header)? != 0x0403_4b50 {
            return None;
        }
        let start = header + 30 + u16_at(self.data, header + 26)? as usize + u16_at(self.data, header + 28)? as usize;
        let raw = self.data.get(start..start + entry.compressed)?;
        match entry.method {
            0 => Some(raw.to_vec()),
            8 => miniz_oxide::inflate::decompress_to_vec_with_limit(raw, entry.size.max(1) * 2 + 1024).ok(),
            _ => None,
        }
    }

    pub fn read_text(&self, name: &str) -> Option<String> {
        self.read(name).map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
    }
}

/// Baut ein ZIP-Archiv (für Tests).
#[cfg(test)]
pub fn build(files: &[(&str, &str)]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut directory = Vec::new();
    for (name, content) in files {
        let packed = miniz_oxide::deflate::compress_to_vec(content.as_bytes(), 6);
        let offset = out.len() as u32;
        let header = |signature: u32, central: bool| {
            let mut h = signature.to_le_bytes().to_vec();
            if central {
                h.extend(20u16.to_le_bytes());
            }
            h.extend(20u16.to_le_bytes()); // Version
            h.extend(0u16.to_le_bytes()); // Flags
            h.extend(8u16.to_le_bytes()); // Deflate
            h.extend(0u32.to_le_bytes()); // Zeit und Datum
            h.extend(0u32.to_le_bytes()); // CRC (wird hier nicht geprüft)
            h.extend((packed.len() as u32).to_le_bytes());
            h.extend((content.len() as u32).to_le_bytes());
            h.extend((name.len() as u16).to_le_bytes());
            h.extend(0u16.to_le_bytes());
            if central {
                h.extend([0u8; 6]); // Kommentarlänge, Datenträger, interne Attribute
                h.extend(0u32.to_le_bytes());
                h.extend(offset.to_le_bytes());
            }
            h.extend(name.as_bytes());
            h
        };
        out.extend(header(0x0403_4b50, false));
        out.extend(&packed);
        directory.extend(header(0x0201_4b50, true));
    }
    let start = out.len() as u32;
    out.extend(&directory);
    out.extend(0x0605_4b50u32.to_le_bytes());
    out.extend([0u8; 4]);
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((files.len() as u16).to_le_bytes());
    out.extend((directory.len() as u32).to_le_bytes());
    out.extend(start.to_le_bytes());
    out.extend(0u16.to_le_bytes());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_deflated_entries() {
        let data = build(&[("a.txt", "Hallo Welt"), ("Ordner/b.xml", "<x>Größe</x>")]);
        let zip = Zip::open(&data).unwrap();
        assert_eq!(zip.names().collect::<Vec<_>>(), vec!["a.txt", "Ordner/b.xml"]);
        assert_eq!(zip.read_text("a.txt").unwrap(), "Hallo Welt");
        assert_eq!(zip.read_text("ordner/B.XML").unwrap(), "<x>Größe</x>");
        assert!(zip.read("fehlt").is_none());
        assert!(Zip::open(b"kein Archiv").is_none());
    }
}
