//! Translation evaluation sets (M0-FIX-4, SV-07): `tests/eval/<set>/segments.json`.
//!
//! Each set has 200 segments of UI, web and document text written for the project (CC0), one
//! source and one reference translation each, plus the segment's domain. Score a backend on a
//! set with [`crate::metrics::chrf_stats`], added up over the set (corpus chrF).

use std::collections::BTreeMap;
use std::io;
use std::path::{Path, PathBuf};

use pg_core::{LangPair, LangTag};
use serde::{Deserialize, Serialize};

/// Where a segment's text comes from (SPEC SV-07: "UI, web and document text").
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Domain {
    /// Buttons, menus, dialogs, settings, error messages.
    Ui,
    /// Shops, news, bookings, forms, cookie banners.
    Web,
    /// Letters, reports, contracts, manuals, notices, prose.
    Doc,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Segment {
    /// 1-based, in file order.
    pub id: u32,
    pub domain: Domain,
    pub source: String,
    pub reference: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvalSet {
    /// Directory name, e.g. `sv-en`.
    pub set: String,
    pub license: String,
    pub source: LangTag,
    pub target: LangTag,
    pub description: String,
    pub segments: Vec<Segment>,
}

impl EvalSet {
    /// Read `dir/segments.json`.
    pub fn load(dir: &Path) -> io::Result<EvalSet> {
        let path = dir.join("segments.json");
        let raw = std::fs::read_to_string(&path)?;
        serde_json::from_str(&raw).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{}: {e}", path.display()),
            )
        })
    }

    pub fn pair(&self) -> LangPair {
        LangPair::new(self.source.clone(), self.target.clone())
    }

    /// Segments per domain.
    pub fn domains(&self) -> BTreeMap<Domain, usize> {
        let mut n = BTreeMap::new();
        for s in &self.segments {
            *n.entry(s.domain).or_default() += 1;
        }
        n
    }
}

/// `tests/eval` in this workspace.
pub fn eval_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/eval")
}

/// One set by directory name (`sv-en`, `en-sv`).
pub fn load(name: &str) -> io::Result<EvalSet> {
    EvalSet::load(&eval_dir().join(name))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;
    use crate::metrics::normalize;

    #[test]
    fn sv07_every_set_has_200_clean_segments() {
        let de_en = LangPair::new(
            pg_core::LangTag::parse("de").unwrap(),
            pg_core::LangTag::en(),
        );
        for (name, pair) in [
            ("sv-en", LangPair::sv_en()),
            ("en-sv", LangPair::en_sv()),
            // M3-9.
            ("de-en", de_en),
        ] {
            let set = load(name).unwrap();
            assert_eq!(set.set, name);
            assert_eq!(set.license, "CC0-1.0", "{name}");
            assert_eq!(set.pair(), pair, "{name}");
            assert_eq!(set.segments.len(), 200, "{name}");
            let mut sources = HashSet::new();
            for (i, s) in set.segments.iter().enumerate() {
                assert_eq!(s.id as usize, i + 1, "{name}: ids run 1..=200 in order");
                for text in [&s.source, &s.reference] {
                    assert!(!text.is_empty(), "{name} #{}", s.id);
                    // NFC, no stray whitespace: what the scorer sees is what the file says.
                    assert_eq!(&normalize(text), text, "{name} #{}", s.id);
                }
                assert!(sources.insert(&s.source), "{name} #{}: duplicate", s.id);
            }
            // UI, web and document text (SV-07), none of them a token share.
            for (domain, n) in set.domains() {
                assert!(n >= 50, "{name}: {domain:?} has {n}");
            }
        }
    }

    #[test]
    fn the_sets_are_written_separately() {
        // en-sv isn't sv-en run backwards: its sources aren't sv-en's references.
        let sv_en: HashSet<String> = load("sv-en")
            .unwrap()
            .segments
            .into_iter()
            .map(|s| s.reference)
            .collect();
        let shared: Vec<String> = load("en-sv")
            .unwrap()
            .segments
            .into_iter()
            .map(|s| s.source)
            .filter(|s| sv_en.contains(s))
            .collect();
        // Single words such as "Delete" may coincide; whole sentences shouldn't.
        assert!(
            shared.iter().all(|s| !s.contains(' ')),
            "en-sv sources that are sv-en references: {shared:?}"
        );
    }
}
