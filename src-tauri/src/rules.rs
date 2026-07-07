//! Rule matching + suggestion building.
//!
//! A rule matches a file if ALL of its non-empty criteria match:
//!   - `extensions`: file extension (case-insensitive) is in the list
//!   - `keywords`: any keyword is found in the filename (case-insensitive)
//!   - `content_match == "pdf_vendor"`: the analyzer detected a vendor
//! A rule with none of these set is a catch-all (the `misc` fallback).
//! Rules are tried in order; first match wins.

use std::path::Path;

use crate::analyze::SubMeta;
use crate::config::{Config, Rule};
use crate::pathutil::{expand_template, sanitize_filename};
use crate::timeutil;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub rule_id: String,
    pub category: String,
    pub action: String,    // move | copy
    pub dest_dir: String,  // dest_root joined with the expanded sub-path
    pub filename: String,  // sanitized
}

/// Does this rule match the file? (filename lowercased, ext lowercased, meta)
pub fn matches(rule: &Rule, filename_lower: &str, ext: &str, meta: &SubMeta) -> bool {
    if !rule.extensions.is_empty() {
        let in_list = rule.extensions.iter().any(|e| e.eq_ignore_ascii_case(ext));
        if !in_list { return false; }
    }
    if !rule.keywords.is_empty() {
        let any = rule.keywords.iter().any(|kw| {
            let kw_l = kw.to_lowercase();
            kw_l.is_empty() || filename_lower.contains(&kw_l)
        });
        if !any { return false; }
    }
    if rule.content_match == "pdf_vendor" && meta.vendor.is_empty() {
        return false;
    }
    true
}

/// First matching rule, or None.
pub fn match_first<'a>(rules: &'a [Rule], filename: &str, ext: &str, meta: &SubMeta) -> Option<&'a Rule> {
    let filename_lower = filename.to_lowercase();
    rules.iter().find(|r| matches(r, &filename_lower, ext, meta))
}

/// Build the full suggestion (dest dir + filename + action) for a matched rule.
/// `dest_dir` = `dest_root` joined with the expanded `dest_template`. If
/// `dest_root` is empty, the result is relative (the filer treats that as
/// "not configured" and refuses to file).
pub fn build_suggestion(rule: &Rule, cfg: &Config, meta: &SubMeta, original_filename: &str) -> Suggestion {
    let vars = build_vars(cfg, meta, original_filename, &rule.category);

    let sub_path = expand_with(rule.dest_template.as_str(), &vars);
    let filename_raw = expand_with(rule.filename_template.as_str(), &vars);
    let filename = sanitize_filename(&filename_raw);
    let action = if rule.action.is_empty() {
        cfg.default_action.clone()
    } else {
        rule.action.clone()
    };

    let dest_dir = if cfg.dest_root.is_empty() {
        sub_path // relative → caller detects as unconfigured
    } else {
        Path::new(&cfg.dest_root).join(&sub_path).to_string_lossy().to_string()
    };

    Suggestion {
        rule_id: rule.id.clone(),
        category: rule.category.clone(),
        action,
        dest_dir,
        filename,
    }
}

/// Template variables available to dest/filename templates.
fn build_vars(cfg: &Config, meta: &SubMeta, original_filename: &str, category: &str) -> Vec<(String, String)> {
    let tz = cfg.tz();
    let mut vars: Vec<(String, String)> = Vec::new();

    // Date variables (configured tz).
    for (k, v) in timeutil::date_vars(tz) {
        vars.push((k.to_string(), v));
    }

    // Content-derived.
    let vendor = if meta.vendor.is_empty() { "Unknown".to_string() } else { meta.vendor.clone() };
    vars.push(("${vendor}".into(), vendor));

    let title_or_name = if !meta.title.is_empty() {
        meta.title.clone()
    } else {
        original_filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(original_filename).to_string()
    };
    vars.push(("${title_or_name}".into(), title_or_name));

    vars.push(("${original_name}".into(), original_filename.to_string()));
    vars.push(("${ext}".into(), meta.ext.clone()));
    vars.push(("${category}".into(), category.to_string()));

    vars
}

/// expand_template adapter: takes owned (String, String) pairs.
fn expand_with(tpl: &str, vars: &[(String, String)]) -> String {
    let refs: Vec<(&str, String)> = vars.iter().map(|(k, v)| (k.as_str(), v.clone())).collect();
    expand_template(tpl, &refs)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Config, Rule};

    fn cfg_with_root(dest_root: &str) -> Config {
        let mut c = Config::default();
        c.dest_root = dest_root.into();
        c.default_action = "move".into();
        c
    }

    fn pdf_meta(vendor: &str) -> SubMeta {
        SubMeta { kind: "pdf".into(), ext: "pdf".into(), vendor: vendor.into(), title: "STM32F103".into(), ..Default::default() }
    }

    #[test]
    fn datasheet_matches_pdf_with_vendor() {
        let rule = Rule {
            id: "datasheet".into(), category: "Datasheet".into(),
            extensions: vec!["pdf".into()], content_match: "pdf_vendor".into(),
            dest_template: "Datasheets\\${vendor}".into(),
            filename_template: "${title_or_name}.pdf".into(),
            ..Default::default()
        };
        assert!(matches(&rule, "stm32f103.pdf", "pdf", &pdf_meta("ST")));
        assert!(!matches(&rule, "report.pdf", "pdf", &pdf_meta(""))); // no vendor
        assert!(!matches(&rule, "stm32.zip", "zip", &pdf_meta("ST"))); // not pdf
    }

    #[test]
    fn invoice_matches_by_keyword() {
        let rule = Rule {
            id: "invoice".into(), category: "Receipt".into(),
            keywords: vec!["发票".into(), "invoice".into()],
            dest_template: "Receipts\\${yyyy-mm}".into(),
            filename_template: "${original_name}".into(),
            ..Default::default()
        };
        assert!(matches(&rule, "发票.pdf", "pdf", &pdf_meta("")));
        assert!(matches(&rule, "tax-invoice-2026.pdf", "pdf", &pdf_meta("")));
        assert!(!matches(&rule, "report.pdf", "pdf", &pdf_meta("")));
    }

    #[test]
    fn misc_catch_all_matches_anything() {
        let rule = Rule { id: "misc".into(), category: "Misc".into(), dest_template: "${yyyy-mm}".into(), ..Default::default() };
        assert!(matches(&rule, "anything.bin", "bin", &SubMeta::default()));
    }

    #[test]
    fn first_match_wins() {
        let rules = vec![
            Rule { id: "datasheet".into(), category: "Datasheet".into(), extensions: vec!["pdf".into()], content_match: "pdf_vendor".into(), dest_template: "x".into(), filename_template: "y".into(), ..Default::default() },
            Rule { id: "misc".into(), category: "Misc".into(), dest_template: "z".into(), filename_template: "w".into(), ..Default::default() },
        ];
        let r = match_first(&rules, "stm.pdf", "pdf", &pdf_meta("ST")).unwrap();
        assert_eq!(r.id, "datasheet");
        let r = match_first(&rules, "report.pdf", "pdf", &pdf_meta("")).unwrap();
        assert_eq!(r.id, "misc");
    }

    #[test]
    fn build_suggestion_joins_dest_root() {
        let rule = Rule {
            id: "datasheet".into(), category: "Datasheet".into(),
            extensions: vec!["pdf".into()], content_match: "pdf_vendor".into(),
            dest_template: "Datasheets\\${vendor}".into(),
            filename_template: "${title_or_name}.pdf".into(),
            ..Default::default()
        };
        let cfg = cfg_with_root("D:\\Filer");
        let s = build_suggestion(&rule, &cfg, &pdf_meta("ST"), "STM32F103.pdf");
        assert_eq!(s.dest_dir, "D:\\Filer\\Datasheets\\ST");
        assert_eq!(s.filename, "STM32F103.pdf");
        assert_eq!(s.action, "move");
        assert_eq!(s.category, "Datasheet");
    }

    #[test]
    fn build_suggestion_unknown_vendor_fills_unknown() {
        let rule = Rule {
            id: "datasheet".into(), category: "Datasheet".into(),
            extensions: vec!["pdf".into()], content_match: "pdf_vendor".into(),
            dest_template: "Datasheets\\${vendor}".into(),
            filename_template: "${title_or_name}.pdf".into(),
            ..Default::default()
        };
        let cfg = cfg_with_root("D:\\Filer");
        let m = pdf_meta("");
        let s = build_suggestion(&rule, &cfg, &m, "weird.pdf");
        assert_eq!(s.dest_dir, "D:\\Filer\\Datasheets\\Unknown");
    }

    #[test]
    fn build_suggestion_no_dest_root_yields_relative_path() {
        // No dest_root → dest is the bare sub-path (relative) → caller flags it.
        let rule = Rule {
            id: "datasheet".into(), category: "Datasheet".into(),
            extensions: vec!["pdf".into()], content_match: "pdf_vendor".into(),
            dest_template: "Datasheets\\${vendor}".into(),
            filename_template: "${title_or_name}.pdf".into(),
            ..Default::default()
        };
        let cfg = Config::default(); // dest_root empty
        let s = build_suggestion(&rule, &cfg, &pdf_meta("ST"), "stm.pdf");
        assert!(!std::path::Path::new(&s.dest_dir).is_absolute());
        assert_eq!(s.dest_dir, "Datasheets\\ST");
    }
}
