//! Plaintext TOML config at the app's config dir (`app_config_dir/config.toml`).
//!
//! filer is a local, open-source tool — no credentials live here. The only
//! secrets-adjacent thing is the watch dir path, which is personal. First run
//! writes the seeded defaults + the user's chosen watch_dir / destinations.

use std::path::PathBuf;
use serde::{Deserialize, Serialize};

/// Embedded neutral defaults (no internal addresses, no keys). Parsed at
/// runtime as the base config when the user's config.toml is absent (first
/// run) — so users start with the rule set and only configure paths.
const DEFAULTS_TOML: &str = include_str!("defaults.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Optional署名, used for attribution in history (not critical).
    pub member: String,
    /// Auto-start on boot (default true).
    pub autostart: bool,
    /// Minimize to system tray instead of quitting when the window is
    /// closed (default false). When true, the close button hides the
    /// window; the app keeps running and can be restored / quit from the
    /// tray icon.
    pub minimize_to_tray: bool,
    /// Whether the first-close "minimize to tray?" prompt has been shown
    /// and the user's choice recorded (default false). Until this is true
    /// AND minimize_to_tray is still false, the close button pops a native
    /// Yes/No dialog once; the answer is persisted here + minimize_to_tray
    /// and never asked again.
    pub tray_prompted: bool,
    /// IANA timezone, e.g. "Asia/Shanghai". Empty = system local.
    pub timezone: String,
    /// Directory to watch for new downloads. Empty on first run.
    pub watch_dir: String,
    /// Single root directory under which all filed files land. Rule
    /// `dest_template`s are relative sub-paths joined under this root.
    /// Empty on first run.
    pub dest_root: String,
    /// Conflict strategy when the target filename already exists:
    /// `rename` | `skip` | `overwrite`.
    pub conflict_strategy: String,
    /// Default file action: `move` | `copy`. Per-rule `action` overrides.
    pub default_action: String,
    /// If true, a download-complete file pops a confirm modal pre-filled with
    /// the rule suggestion (user adds searchable metadata, then files). If
    /// false, new files just land in the inbox (current behavior).
    pub auto_file: bool,
    pub rules: Vec<Rule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Rule {
    pub id: String,
    pub category: String,
    /// Match if the file extension is in this list (case-insensitive, no dot).
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Match if any keyword is found in the filename (case-insensitive).
    #[serde(default)]
    pub keywords: Vec<String>,
    /// Content-based match: `pdf_vendor` = only if pdfinfo suggested a vendor.
    #[serde(default)]
    pub content_match: String,
    /// Destination dir sub-path template, relative to `dest_root`,
    /// e.g. `Datasheets\${vendor}`.
    pub dest_template: String,
    /// Filename template, e.g. `${title_or_name}.pdf`.
    pub filename_template: String,
    /// `move` | `copy` | `` (= use default_action).
    #[serde(default)]
    pub action: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            member: String::new(),
            autostart: true,
            minimize_to_tray: false,
            tray_prompted: false,
            timezone: String::new(),
            watch_dir: String::new(),
            dest_root: String::new(),
            conflict_strategy: "rename".into(),
            default_action: "move".into(),
            auto_file: false,
            rules: Vec::new(),
        }
    }
}

impl Config {
    /// Seeded neutral defaults (rule set + strategies). Used on first run /
    /// when the user's config.toml is absent or corrupt.
    pub fn seeded() -> Self {
        toml::from_str(DEFAULTS_TOML).unwrap_or_default()
    }

    /// Path to config.toml under the given app config dir.
    pub fn path(config_dir: &PathBuf) -> PathBuf {
        config_dir.join("config.toml")
    }

    /// First-run gate: no watch_dir / dest_root configured yet → force wizard.
    pub fn is_configured(&self) -> bool {
        !self.watch_dir.trim().is_empty() && !self.dest_root.trim().is_empty()
    }

    pub fn load(path: &PathBuf) -> anyhow::Result<Config> {
        if !path.exists() {
            return Ok(Config::seeded());
        }
        let text = std::fs::read_to_string(path)?;
        if text.trim().is_empty() {
            return Ok(Config::seeded());
        }
        match toml::from_str::<Config>(&text) {
            Ok(cfg) => Ok(cfg),
            Err(e) => {
                eprintln!("[warn] config.toml 解析失败 ({e})，回退到默认配置");
                Ok(Config::seeded())
            }
        }
    }

    pub fn save(&self, path: &PathBuf) -> anyhow::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(path, text)?;
        Ok(())
    }

    /// Resolve the configured timezone (None = system local).
    pub fn tz(&self) -> Option<chrono_tz::Tz> {
        crate::timeutil::parse_tz(&self.timezone)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seeded_has_rules_and_strategies() {
        let c = Config::seeded();
        assert!(c.autostart);
        assert_eq!(c.conflict_strategy, "rename");
        assert_eq!(c.default_action, "move");
        assert!(c.timezone.is_empty());
        assert!(c.watch_dir.is_empty());
        assert!(c.dest_root.is_empty());
        assert!(!c.rules.is_empty());
        // misc is the catch-all and must be last
        let last = c.rules.last().unwrap();
        assert_eq!(last.id, "misc");
    }

    #[test]
    fn seeded_datasheet_requires_vendor_content_match() {
        let c = Config::seeded();
        let ds = c.rules.iter().find(|r| r.id == "datasheet").unwrap();
        assert_eq!(ds.content_match, "pdf_vendor");
        assert!(ds.extensions.contains(&"pdf".to_string()));
        // dest_template is now a relative sub-path (no ${library})
        assert!(!ds.dest_template.contains("${library}"));
    }

    #[test]
    fn default_is_unprefilled_no_recurse() {
        let c = Config::default();
        assert!(c.watch_dir.is_empty());
        assert!(c.dest_root.is_empty());
        assert!(c.rules.is_empty());
        assert!(!c.is_configured());
    }

    #[test]
    fn roundtrip() {
        let mut cfg = Config::default();
        cfg.watch_dir = "C:\\Users\\Me\\Downloads".into();
        cfg.dest_root = "D:\\Filer".into();
        cfg.timezone = "Asia/Shanghai".into();
        cfg.rules = vec![Rule {
            id: "x".into(),
            category: "X".into(),
            extensions: vec!["pdf".into()],
            dest_template: "Sub\\${vendor}".into(),
            filename_template: "${original_name}".into(),
            ..Default::default()
        }];
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(back.watch_dir, "C:\\Users\\Me\\Downloads");
        assert_eq!(back.dest_root, "D:\\Filer");
        assert_eq!(back.timezone, "Asia/Shanghai");
        assert_eq!(back.rules.len(), 1);
        assert_eq!(back.rules[0].id, "x");
        assert!(back.is_configured());
    }

    #[test]
    fn is_configured_needs_both_watch_and_root() {
        let mut c = Config::default();
        assert!(!c.is_configured());
        c.watch_dir = "C:\\DL".into();
        assert!(!c.is_configured()); // still no dest_root
        c.dest_root = "D:\\Filer".into();
        assert!(c.is_configured());
    }

    #[test]
    fn tz_resolves() {
        let mut c = Config::default();
        assert!(c.tz().is_none()); // empty → system local
        c.timezone = "Asia/Shanghai".into();
        assert!(c.tz().is_some());
        c.timezone = "Mars/Olympus".into();
        assert!(c.tz().is_none()); // invalid → fall back
    }
}
