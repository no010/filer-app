//! File type detection + metadata extraction.
//!
//! Magic-byte sniffing (with extension fallback) decides the `kind`; per-kind
//! extractors pull what's cheap to get: PDF page count/title/vendor (lopdf),
//! image dimensions (image crate). sha256 is computed for dedup. Everything
//! else is best-effort — analyze never panishes, only returns defaults.
//!
//! Two entry points:
//! - `analyze(filename, bytes)` — bytes already in memory (tests, small files).
//! - `analyze_file(path, filename)` — streams the file: reads only the first
//!   512 B for magic, hashes in 256 KB chunks (never loads the whole file),
//!   and skips PDF/image extraction above `ANALYZE_FULL_CAP` so a multi-GB
//!   download can't blow up memory or hang the scan.

use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::pdfinfo;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubMeta {
    /// pdf | image | archive | office | document | other
    pub kind: String,
    pub mime: String,
    pub ext: String,
    #[serde(default)]
    pub n_pages: u32,
    /// Vendor suggested from PDF title/filename (datasheets).
    #[serde(default)]
    pub vendor: String,
    /// PDF Info-dict title (or "" if none).
    #[serde(default)]
    pub title: String,
    #[serde(default)]
    pub width: u32,
    #[serde(default)]
    pub height: u32,
    /// EXIF DateTimeOriginal (photos).
    #[serde(default)]
    pub exif_taken_at: String,
    /// EXIF Make+Model (camera).
    #[serde(default)]
    pub exif_camera: String,
    /// EXIF GPS as "lat,lon" (or "").
    #[serde(default)]
    pub exif_gps: String,
    /// office docProps/core.xml dc:title.
    #[serde(default)]
    pub office_title: String,
    /// office docProps/core.xml dc:creator.
    #[serde(default)]
    pub office_author: String,
    /// Top-level entry names for archive (zip) files, capped at ~20.
    #[serde(default)]
    pub archive_entries: Vec<String>,
}

const OFFICE_EXTS: &[&str] = &["docx", "docm", "xlsx", "xlsm", "pptx", "pptm"];
const ARCHIVE_EXTS: &[&str] = &["zip", "7z", "rar", "tar", "gz", "bz2", "xz", "tgz"];
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp", "bmp"];
const DOC_EXTS: &[&str] = &["doc", "xls", "ppt", "txt", "md", "csv", "rtf"];

/// Files larger than this skip full-content extraction (PDF pages/title,
/// vendor). Still get kind (by magic) + sha (streamed) so dedup works; the
/// datasheet rule (which needs a vendor) won't fire → falls to document/misc.
pub const ANALYZE_FULL_CAP: u64 = 50 * 1024 * 1024;

pub fn ext_of(filename: &str) -> String {
    let lower = filename.to_lowercase();
    match lower.rsplit_once('.') {
        Some((_, e)) => e.to_string(),
        None => String::new(),
    }
}

/// Classify by magic bytes (with extension fallback). Sets kind/mime/ext only.
fn classify(head: &[u8], ext: &str) -> SubMeta {
    let mut m = SubMeta { ext: ext.to_string(), ..Default::default() };
    if head.len() >= 5 && &head[..5] == b"%PDF-" {
        m.kind = "pdf".into();
        m.mime = "application/pdf".into();
    } else if is_zip(head) {
        if OFFICE_EXTS.contains(&ext) {
            m.kind = "office".into();
            m.mime = format!("application/vnd.openxmlformats-officedocument.{ext}");
        } else {
            m.kind = "archive".into();
            m.mime = "application/zip".into();
        }
    } else if head.len() >= 8 && &head[..8] == b"\x89PNG\r\n\x1a\n" {
        m.kind = "image".into(); m.mime = "image/png".into();
    } else if head.len() >= 3 && &head[..3] == b"\xFF\xD8\xFF" {
        m.kind = "image".into(); m.mime = "image/jpeg".into();
    } else if head.len() >= 6 && (&head[..6] == b"GIF87a" || &head[..6] == b"GIF89a") {
        m.kind = "image".into(); m.mime = "image/gif".into();
    } else if head.len() >= 12 && &head[..4] == b"RIFF" && &head[8..12] == b"WEBP" {
        m.kind = "image".into(); m.mime = "image/webp".into();
    } else if head.len() >= 2 && &head[..2] == b"BM" {
        m.kind = "image".into(); m.mime = "image/bmp".into();
    } else if ARCHIVE_EXTS.contains(&ext) {
        m.kind = "archive".into(); m.mime = "application/octet-stream".into();
    } else if IMAGE_EXTS.contains(&ext) {
        m.kind = "image".into(); m.mime = format!("image/{ext}");
    } else if OFFICE_EXTS.contains(&ext) || DOC_EXTS.contains(&ext) {
        m.kind = "document".into(); m.mime = "application/octet-stream".into();
    } else {
        m.kind = "other".into(); m.mime = "application/octet-stream".into();
    }
    m
}

/// Analyze bytes (read from disk by the caller). Returns (sha256_hex, SubMeta).
pub fn analyze(filename: &str, bytes: &[u8]) -> (String, SubMeta) {
    let mut h = Sha256::new();
    h.update(bytes);
    let sha: String = h.finalize().iter().map(|b| format!("{:02x}", b)).collect();

    let ext = ext_of(filename);
    let mut m = classify(bytes, &ext);
    extract(&mut m, filename, bytes, bytes.len() as u64);
    (sha, m)
}

/// Stream-analyze a file on disk: 512 B head for magic, chunked sha256, and
/// extraction only if the file is under `ANALYZE_FULL_CAP`. Never loads the
/// whole file into memory for hashing — so a 10 GB ISO won't hang or OOM.
pub fn analyze_file(path: &Path, filename: &str) -> std::io::Result<(String, SubMeta)> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();

    let mut f = File::open(path)?;
    let mut head = [0u8; 512];
    let n = f.read(&mut head)?;
    let head = &head[..n];

    // Hash the whole file in chunks (re-read from start).
    f.seek(SeekFrom::Start(0))?;
    let sha = stream_sha(&mut f)?;

    let ext = ext_of(filename);
    let mut m = classify(head, &ext);

    // Extraction only for reasonably-sized files; for huge files we keep
    // kind + sha (dedup still works) but skip page/vendor/dimensions.
    match m.kind.as_str() {
        "pdf" if size <= ANALYZE_FULL_CAP => {
            f.seek(SeekFrom::Start(0))?;
            let mut bytes = Vec::with_capacity(size.min(ANALYZE_FULL_CAP) as usize);
            f.read_to_end(&mut bytes)?;
            extract(&mut m, filename, &bytes, size);
        }
        "image" if size <= ANALYZE_FULL_CAP => {
            fill_dims_from_path(path, &mut m);
            read_exif(path, &mut m);
        }
        "office" => {
            read_office_core(path, &mut m);
        }
        "archive" => {
            read_archive_entries(path, &mut m);
        }
        _ => {}
    }
    Ok((sha, m))
}

/// Pull PDF pages/title/vendor from `bytes` (already loaded) into `m`.
fn extract(m: &mut SubMeta, filename: &str, bytes: &[u8], _size: u64) {
    if m.kind == "pdf" {
        let info = pdfinfo::extract(bytes);
        m.n_pages = info.n_pages;
        m.title = info.title.clone();
        if m.title.is_empty() {
            m.title = filename_stem(filename);
        }
        let v = pdfinfo::suggest_vendor(&info.title, filename);
        if !v.is_empty() {
            m.vendor = v.into();
        }
    } else if m.kind == "image" {
        // bytes form (used by `analyze`) — decode dims from memory.
        if let Ok(r) = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format() {
            if let Ok(dims) = r.into_dimensions() {
                m.width = dims.0;
                m.height = dims.1;
            }
        }
    }
}

/// Decode image dimensions straight from the file path (no full read).
fn fill_dims_from_path(path: &Path, m: &mut SubMeta) {
    if let Ok(ir) = image::ImageReader::open(path) {
        if let Ok(ir) = ir.with_guessed_format() {
            if let Ok(dims) = ir.into_dimensions() {
                m.width = dims.0;
                m.height = dims.1;
            }
        }
    }
}

/// Pull EXIF DateTimeOriginal / Make+Model / GPS from a photo (best-effort).
fn read_exif(path: &Path, m: &mut SubMeta) {
    let file = match std::fs::File::open(path) { Ok(f) => f, Err(_) => return };
    let mut buf = std::io::BufReader::new(file);
    let exif = match exif::Reader::new().read_from_container(&mut buf) {
        Ok(e) => e,
        Err(_) => return,
    };
    if let Some(f) = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
        let s = f.display_value().to_string();
        m.exif_taken_at = s.lines().next().unwrap_or("").trim().to_string();
    }
    let make = exif.get_field(exif::Tag::Make, exif::In::PRIMARY).map(|f| f.display_value().to_string());
    let model = exif.get_field(exif::Tag::Model, exif::In::PRIMARY).map(|f| f.display_value().to_string());
    let cam = match (make, model) {
        (Some(a), Some(b)) => format!("{a} {b}"),
        (Some(a), None) => a,
        (None, Some(b)) => b,
        _ => String::new(),
    };
    m.exif_camera = cam.trim().to_string();
    if let (Some(lat), Some(lon)) = (
        exif.get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY),
        exif.get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY),
    ) {
        m.exif_gps = format!("{},{}", lat.display_value().to_string(), lon.display_value().to_string());
    }
}

/// Read docProps/core.xml from a zip-based office doc (docx/xlsx/pptx) and
/// pull dc:title / dc:creator. Best-effort, no XML dep — substring extract.
fn read_office_core(path: &Path, m: &mut SubMeta) {
    let file = match std::fs::File::open(path) { Ok(f) => f, Err(_) => return };
    let mut za = match zip::ZipArchive::new(file) { Ok(z) => z, Err(_) => return };
    let xml = match za.by_name("docProps/core.xml") {
        Ok(mut r) => { let mut s = String::new(); let _ = r.read_to_string(&mut s); s }
        Err(_) => return,
    };
    m.office_title = extract_xml_tag(&xml, "dc:title").unwrap_or_default();
    m.office_author = extract_xml_tag(&xml, "dc:creator").unwrap_or_default();
}

/// List the top-level entry names of a zip archive (cap 20) for a hint of
/// what's inside (useful for retrieval).
fn read_archive_entries(path: &Path, m: &mut SubMeta) {
    let file = match std::fs::File::open(path) { Ok(f) => f, Err(_) => return };
    let mut za = match zip::ZipArchive::new(file) { Ok(z) => z, Err(_) => return };
    let mut entries = Vec::new();
    for i in 0..za.len().min(20) {
        if let Ok(f) = za.by_index(i) {
            entries.push(f.name().to_string());
        }
    }
    m.archive_entries = entries;
}

/// Extract the text content of `<tag ...>text</tag>` from a small XML string.
fn extract_xml_tag(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let open_pos = xml.find(&open)?;
    let after_open = &xml[open_pos..];
    let gt = after_open.find('>')?;
    let content_start = open_pos + gt + 1;
    let end = xml[content_start..].find(&close)? + content_start;
    Some(xml[content_start..end].trim().to_string())
}

fn stream_sha(f: &mut File) -> std::io::Result<String> {
    let mut reader = BufReader::new(f);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 256 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 { break; }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().iter().map(|b| format!("{:02x}", b)).collect())
}

fn is_zip(head: &[u8]) -> bool {
    head.len() >= 4 && &head[..4] == b"PK\x03\x04"
}

fn filename_stem(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _)) => stem.to_string(),
        None => filename.to_string(),
    }
}

/// Bytes that look like a finished download (not a temp file). Temp suffixes
/// come from Chrome (.crdownload), Firefox (.part), Edge/Safari (.download),
/// and generic Office/aria2 temp prefixes (the leading "." or "~").
pub fn is_temp_filename(name: &str) -> bool {
    let lower = name.to_lowercase();
    let temp_suffixes = [".part", ".crdownload", ".download", ".tmp", ".partial", ".~tmp"];
    if temp_suffixes.iter().any(|s| lower.ends_with(s)) {
        return true;
    }
    lower.starts_with("~$") || (lower.starts_with('.') && !lower.ends_with(".pdf"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn detects_pdf() {
        let bytes = b"%PDF-1.5\n...binary...";
        let (sha, m) = analyze("STM32F103.pdf", bytes);
        assert_eq!(m.kind, "pdf");
        assert_eq!(m.mime, "application/pdf");
        assert_eq!(m.ext, "pdf");
        assert_eq!(sha.len(), 64);
    }

    #[test]
    fn detects_png_with_dims() {
        let img = image::RgbaImage::from_raw(1, 1, vec![255, 0, 0, 255]).unwrap();
        let mut buf = Vec::new();
        image::DynamicImage::ImageRgba8(img)
            .write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
            .unwrap();
        let (_sha, m) = analyze("shot.png", &buf);
        assert_eq!(m.kind, "image");
        assert_eq!(m.mime, "image/png");
        assert_eq!(m.width, 1);
        assert_eq!(m.height, 1);
    }

    #[test]
    fn detects_zip_as_archive() {
        let bytes = b"PK\x03\x04\x00\x00...";
        let (_sha, m) = analyze("backup.zip", bytes);
        assert_eq!(m.kind, "archive");
    }

    #[test]
    fn detects_zip_as_office_by_ext() {
        let bytes = b"PK\x03\x04\x00\x00...";
        let (_sha, m) = analyze("report.docx", bytes);
        assert_eq!(m.kind, "office");
    }

    #[test]
    fn detects_other_for_unknown() {
        let (_sha, m) = analyze("data.bin", b"\x00\x01\x02");
        assert_eq!(m.kind, "other");
    }

    #[test]
    fn temp_filenames_filtered() {
        assert!(is_temp_filename("movie.crdownload"));
        assert!(is_temp_filename("movie.part"));
        assert!(is_temp_filename("movie.download"));
        assert!(is_temp_filename("~$report.docx"));
        assert!(!is_temp_filename("invoice.pdf"));
        assert!(!is_temp_filename("datasheet.PDF"));
    }

    #[test]
    fn ext_case_insensitive() {
        assert_eq!(ext_of("X.PDF"), "pdf");
        assert_eq!(ext_of("archive.ZIP"), "zip");
        assert_eq!(ext_of("noext"), "");
    }

    #[test]
    fn analyze_file_streams_large_pdf_without_loading_all() {
        // A "PDF" whose body is 60 MB of zeros — over ANALYZE_FULL_CAP, so
        // extraction is skipped but sha still computed without OOM.
        let dir = std::env::temp_dir();
        let p = dir.join(format!("filer_big_{}.pdf", std::process::id()));
        {
            let mut f = std::fs::File::create(&p).unwrap();
            f.write_all(b"%PDF-1.5\n").unwrap();
            let chunk = vec![0u8; 4 * 1024 * 1024];
            for _ in 0..15 { f.write_all(&chunk).unwrap(); }
        }
        let (sha, m) = analyze_file(&p, "big.pdf").unwrap();
        assert_eq!(m.kind, "pdf");
        assert_eq!(m.mime, "application/pdf");
        assert_eq!(sha.len(), 64);
        assert_eq!(m.n_pages, 0); // extraction skipped (size > cap)
        std::fs::remove_file(&p).ok();
    }

    #[test]
    fn analyze_file_small_pdf_extracts() {
        let dir = std::env::temp_dir();
        let p = dir.join(format!("filer_small_{}.pdf", std::process::id()));
        std::fs::write(&p, b"%PDF-1.5\nsmall body").unwrap();
        let (sha, m) = analyze_file(&p, "STM32F103.pdf").unwrap();
        assert_eq!(m.kind, "pdf");
        assert_eq!(sha.len(), 64);
        // not a real PDF → lopdf returns default (0 pages, empty title);
        // title falls back to filename stem.
        assert_eq!(m.title, "STM32F103");
        std::fs::remove_file(&p).ok();
    }
}
