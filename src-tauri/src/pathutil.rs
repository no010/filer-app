//! Filename / path helpers: template expansion, sanitization, conflict
//! resolution. Pure functions, unit-tested.

use std::path::{Path, PathBuf};

/// Replace `${key}` placeholders in `tpl` with values from `vars`. Unknown
/// placeholders are left as-is (so they're visible in the UI rather than
/// silently dropped).
pub fn expand_template(tpl: &str, vars: &[(&str, String)]) -> String {
    let mut out = tpl.to_string();
    for (key, val) in vars {
        out = out.replace(key, val);
    }
    out
}

/// Sanitize a filename for the local FS (Windows-first, also safe on Unix):
/// replace `<>:"/\|?*` and control chars with `_`, collapse runs, and trim
/// trailing dots/spaces (Windows forbids them). Empty result → "untitled".
pub fn sanitize_filename(name: &str) -> String {
    let mut s = String::with_capacity(name.len());
    let mut prev_under = false;
    for ch in name.chars() {
        let bad = matches!(ch, '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*')
            || (ch as u32) < 0x20;
        if bad {
            if !prev_under && !s.is_empty() {
                s.push('_');
                prev_under = true;
            }
        } else {
            s.push(ch);
            prev_under = false;
        }
    }
    // Trim trailing dots/spaces/underscores (Windows: no trailing dot/space).
    let trimmed = s.trim_end_matches(|c: char| c == '.' || c == ' ' || c == '_');
    let trimmed = trimmed.trim_start_matches(|c: char| c == '.' || c == ' ');
    if trimmed.is_empty() {
        "untitled".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Join a directory and filename into a full path.
pub fn join(dir: &str, filename: &str) -> PathBuf {
    Path::new(dir).join(filename)
}

/// Resolve a target path under `dir` for `desired_filename` according to the
/// conflict strategy:
/// - `rename`: if exists, append ` (1)`, ` (2)`… before the extension.
/// - `skip`: if exists, return None (caller treats as duplicate/skip).
/// - `overwrite` (or anything else): return the path as-is.
pub fn resolve_conflict(dir: &str, desired_filename: &str, strategy: &str) -> Option<PathBuf> {
    let target = join(dir, desired_filename);
    if !target.exists() {
        return Some(target);
    }
    match strategy {
        "skip" => None,
        "overwrite" => Some(target),
        _ => {
            // rename
            let stem = Path::new(desired_filename).file_stem().and_then(|s| s.to_str()).unwrap_or(desired_filename);
            let ext = Path::new(desired_filename).extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
            for i in 1..10000 {
                let cand_name = format!("{stem} ({i}){ext}");
                let cand = join(dir, &cand_name);
                if !cand.exists() {
                    return Some(cand);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_replaces_known_keys() {
        let vars = vec![("${a}", "X".into()), ("${b}", "Y".into())];
        assert_eq!(expand_template("${a}/${b}/${c}", &vars), "X/Y/${c}");
    }

    #[test]
    fn sanitize_strips_illegal() {
        assert_eq!(sanitize_filename("a<b>c:d\"e/f\\g|h?i*j"), "a_b_c_d_e_f_g_h_i_j");
        assert_eq!(sanitize_filename("name..  "), "name");
        assert_eq!(sanitize_filename("..name."), "name");
        assert_eq!(sanitize_filename("   "), "untitled");
        assert_eq!(sanitize_filename(""), "untitled");
    }

    #[test]
    fn sanitize_keeps_unicode() {
        assert_eq!(sanitize_filename("发票_2026.pdf"), "发票_2026.pdf");
        assert_eq!(sanitize_filename("数据 表"), "数据 表");
    }

    #[test]
    fn resolve_conflict_rename_appends_suffix() {
        let dir = std::env::temp_dir();
        let base = format!("filer_test_{}", std::process::id());
        let _ = std::fs::create_dir_all(dir.join(&base));
        let d = dir.join(&base);
        let f1 = d.join("x.txt");
        std::fs::write(&f1, b"a").unwrap();
        // x.txt exists → rename → x (1).txt
        let r = resolve_conflict(d.to_str().unwrap(), "x.txt", "rename").unwrap();
        assert_eq!(r.file_name().unwrap(), "x (1).txt");
        std::fs::write(&r, b"b").unwrap();
        // now x (1).txt exists too → x (2).txt
        let r2 = resolve_conflict(d.to_str().unwrap(), "x.txt", "rename").unwrap();
        assert_eq!(r2.file_name().unwrap(), "x (2).txt");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn resolve_conflict_skip_returns_none_when_exists() {
        let dir = std::env::temp_dir();
        let base = format!("filer_test_skip_{}", std::process::id());
        let d = dir.join(&base);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("x.txt"), b"a").unwrap();
        assert!(resolve_conflict(d.to_str().unwrap(), "x.txt", "skip").is_none());
        // non-existent → Some
        assert!(resolve_conflict(d.to_str().unwrap(), "y.txt", "skip").is_some());
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn resolve_conflict_overwrite_returns_same() {
        let dir = std::env::temp_dir();
        let base = format!("filer_test_ow_{}", std::process::id());
        let d = dir.join(&base);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("x.txt"), b"a").unwrap();
        let r = resolve_conflict(d.to_str().unwrap(), "x.txt", "overwrite").unwrap();
        assert_eq!(r.file_name().unwrap(), "x.txt");
        std::fs::remove_dir_all(&d).ok();
    }
}
