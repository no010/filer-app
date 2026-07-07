//! Local PDF metadata extraction (pure Rust, no C dep, no cloud).
//!
//! Ported from apps/shelf — same logic: page count, Info-dict Title/Author,
//! bookmark outline, and a keyword-based vendor suggestion from title +
//! filename. Used to auto-fill a datasheet suggestion on download.

use lopdf::{Document, Object};

pub struct PdfInfo {
    pub n_pages: u32,
    pub title: String,
    pub author: String,
    pub bookmarks: Vec<String>,
}

impl Default for PdfInfo {
    fn default() -> Self {
        Self { n_pages: 0, title: String::new(), author: String::new(), bookmarks: vec![] }
    }
}

pub fn extract(bytes: &[u8]) -> PdfInfo {
    let doc = match Document::load_mem(bytes) {
        Ok(d) => d,
        Err(_) => return PdfInfo::default(),
    };
    let n_pages = doc.get_pages().len() as u32;
    let (title, author) = extract_info(&doc);
    let bookmarks = extract_bookmarks(&doc);
    PdfInfo { n_pages, title, author, bookmarks }
}

fn extract_info(doc: &Document) -> (String, String) {
    let info_ref = match doc.trailer.get(b"Info") {
        Ok(Object::Reference(r)) => *r,
        _ => return (String::new(), String::new()),
    };
    let obj = match doc.get_object(info_ref) {
        Ok(o) => o,
        Err(_) => return (String::new(), String::new()),
    };
    let dict = match obj.as_dict() {
        Ok(d) => d,
        Err(_) => return (String::new(), String::new()),
    };
    (get_pdf_string(dict, b"Title"), get_pdf_string(dict, b"Author"))
}

fn extract_bookmarks(doc: &Document) -> Vec<String> {
    let mut out = Vec::new();
    let catalog = match doc.catalog() {
        Ok(c) => c,
        Err(_) => return out,
    };
    let outlines_ref = match catalog.get(b"Outlines") {
        Ok(Object::Reference(r)) => *r,
        _ => return out,
    };
    let outlines = match doc.get_object(outlines_ref) {
        Ok(o) => o,
        Err(_) => return out,
    };
    let outlines_dict = match outlines.as_dict() {
        Ok(d) => d,
        Err(_) => return out,
    };
    if let Ok(first) = outlines_dict.get(b"First") {
        walk_outline(doc, first, &mut out, 0);
    }
    out
}

fn walk_outline(doc: &Document, obj: &Object, out: &mut Vec<String>, depth: u32) {
    if depth > 6 { return; }
    let mut current = obj;
    let mut guard = 0;
    loop {
        guard += 1;
        if guard > 500 { break; }
        let dict = match current.as_reference().ok().and_then(|r| doc.get_object(r).ok()) {
            Some(o) => match o.as_dict() {
                Ok(d) => d,
                Err(_) => break,
            },
            None => break,
        };
        if let Ok(t) = dict.get(b"Title") {
            let title = get_obj_pdf_string(t);
            if !title.is_empty() {
                let indent = "  ".repeat(depth as usize);
                out.push(format!("{indent}{title}"));
            }
        }
        if let Ok(child) = dict.get(b"First") {
            walk_outline(doc, child, out, depth + 1);
        }
        match dict.get(b"Next") {
            Ok(next) => current = next,
            Err(_) => break,
        }
    }
}

fn get_pdf_string(dict: &lopdf::Dictionary, key: &[u8]) -> String {
    match dict.get(key) {
        Ok(obj) => get_obj_pdf_string(obj),
        Err(_) => String::new(),
    }
}

fn get_obj_pdf_string(obj: &Object) -> String {
    match obj {
        Object::String(bytes, _) => pdf_string_to_rust(bytes),
        Object::Reference(r) => {
            let _ = r;
            String::new()
        }
        _ => String::new(),
    }
}

fn pdf_string_to_rust(bytes: &[u8]) -> String {
    if bytes.len() >= 2 && bytes[0] == 0xFE && bytes[1] == 0xFF {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16_lossy(&u16s)
    } else {
        String::from_utf8_lossy(bytes).to_string()
    }
}

/// Suggest a vendor from the title + filename via keyword matching.
/// Returns "" if no match.
pub fn suggest_vendor(title: &str, filename: &str) -> &'static str {
    let hay = format!("{title} {filename}").to_lowercase();
    let rules: &[(&str, &str)] = &[
        ("stm32", "ST"), ("stm8", "ST"), ("stmicro", "ST"),
        ("lpc", "NXP"), ("i.mx", "NXP"), ("imx", "NXP"), ("kinetis", "NXP"), ("nxp", "NXP"),
        ("msp430", "TI"), ("tms320", "TI"), ("drv", "TI"), ("tm4c", "TI"), ("ti ", "TI"),
        ("esp32", "Espressif"), ("esp8266", "Espressif"), ("espressif", "Espressif"),
        ("atmega", "Microchip"), ("atsam", "Microchip"), ("pic32", "Microchip"), ("avr", "Microchip"),
        ("ra6", "Renesas"), ("ra4", "Renesas"), ("rx65", "Renesas"), ("rz", "Renesas"), ("renesas", "Renesas"),
        ("gd32", "GigaDevice"), ("gigadevice", "GigaDevice"),
        ("ch32", "WCH"), ("ch55", "WCH"), ("ch57", "WCH"),
        ("nrf52", "Nordic"), ("nrf53", "Nordic"), ("nrf54", "Nordic"), ("nordic", "Nordic"),
        ("rtl87", "Realtek"), ("rtl8720", "Realtek"), ("ameba", "Realtek"),
        ("efr32", "Silicon Labs"), ("gecko", "Silicon Labs"), ("silabs", "Silicon Labs"),
        ("xmc", "Infineon"), ("aurix", "Infineon"), ("infineon", "Infineon"),
    ];
    for (kw, vendor) in rules {
        if hay.contains(kw) { return vendor; }
    }
    ""
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggest_vendor_matches() {
        assert_eq!(suggest_vendor("STM32F103C8 Datasheet", "stm.pdf"), "ST");
        assert_eq!(suggest_vendor("", "ESP32-WROOM-32.pdf"), "Espressif");
        assert_eq!(suggest_vendor("MSP430G2553", "ti.pdf"), "TI");
        assert_eq!(suggest_vendor("Random doc", "unknown.pdf"), "");
    }

    #[test]
    fn pdf_string_utf16be() {
        let bytes = [0xFE, 0xFF, 0x00, b'H', 0x00, b'i'];
        assert_eq!(pdf_string_to_rust(&bytes), "Hi");
    }

    #[test]
    fn pdf_string_ascii() {
        assert_eq!(pdf_string_to_rust(b"STM32F103"), "STM32F103");
    }

    #[test]
    fn extract_from_invalid_bytes_returns_default() {
        let info = extract(b"not a pdf");
        assert_eq!(info.n_pages, 0);
        assert!(info.bookmarks.is_empty());
    }
}
