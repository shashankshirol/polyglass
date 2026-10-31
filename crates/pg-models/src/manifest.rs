use std::collections::HashSet;

use pg_core::{LangPair, LangTag};
use serde::Deserialize;

use crate::{ModelError, Result};

pub const SCHEMA_VERSION: u32 = 1;

/// The model manifest shipped with the app (`models/manifest.toml`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub schema_version: u32,
    #[serde(default, rename = "model")]
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelEntry {
    /// Stable ID, also the directory name in the model store (`opus-mt-sv-en`).
    pub id: String,
    /// Translation engine that loads it (`ctranslate2`).
    pub engine: String,
    pub source: LangTag,
    pub target: LangTag,
    pub version: String,
    /// SPDX license expression, shown in the model manager (MDL-01).
    pub license: String,
    /// Credit line shown in the model manager. Required for `CC-BY-*` licenses.
    #[serde(default)]
    pub attribution: Option<String>,
    /// Where the model comes from (project page), shown in the model manager.
    pub source_url: String,
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelFile {
    /// Plain file name inside the model directory.
    pub name: String,
    /// https download URL. A model whose files have no URL is import-only (MDL-04): it isn't
    /// hosted yet, so it can only be installed with [`ModelStore::import_from_dir`].
    ///
    /// [`ModelStore::import_from_dir`]: crate::ModelStore::import_from_dir
    #[serde(default)]
    pub url: Option<String>,
    pub size: u64,
    /// Lowercase hex SHA-256.
    pub sha256: String,
}

/// One row of the model manager (MDL-01).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub id: String,
    pub pair: LangPair,
    pub download_size: u64,
    /// `false` for import-only models (no download URLs yet).
    pub downloadable: bool,
    pub license: String,
    pub attribution: Option<String>,
    pub source_url: String,
}

impl ModelEntry {
    pub fn pair(&self) -> LangPair {
        LangPair::new(self.source.clone(), self.target.clone())
    }

    pub fn download_size(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }

    /// Whether the files have download URLs. If not, the model is import-only (MDL-04).
    pub fn is_downloadable(&self) -> bool {
        self.files.iter().all(|f| f.url.is_some())
    }
}

impl Manifest {
    /// Parse and validate.
    pub fn parse(toml_text: &str) -> Result<Self> {
        let m: Manifest =
            toml::from_str(toml_text).map_err(|e| ModelError::InvalidManifest(e.to_string()))?;
        m.validate()?;
        Ok(m)
    }

    /// The manifest compiled into the binary.
    pub fn bundled() -> Result<Self> {
        Self::parse(include_str!("../../../models/manifest.toml"))
    }

    fn validate(&self) -> Result<()> {
        let bad = |msg: String| Err(ModelError::InvalidManifest(msg));
        if self.schema_version != SCHEMA_VERSION {
            return bad(format!(
                "unsupported schema_version {}",
                self.schema_version
            ));
        }
        let mut ids = HashSet::new();
        for m in &self.models {
            if !is_safe_name(&m.id) {
                return bad(format!("invalid model id {:?}", m.id));
            }
            if !ids.insert(&m.id) {
                return bad(format!("duplicate model id {}", m.id));
            }
            if m.files.is_empty() {
                return bad(format!("{}: no files", m.id));
            }
            if m.license.trim().is_empty() || !m.source_url.starts_with("https://") {
                return bad(format!(
                    "{}: license and https source_url are required",
                    m.id
                ));
            }
            let attributed = m
                .attribution
                .as_deref()
                .is_some_and(|a| !a.trim().is_empty());
            if m.license.starts_with("CC-BY") && !attributed {
                return bad(format!("{}: {} needs an attribution", m.id, m.license));
            }
            if m.files.iter().any(|f| f.url.is_some()) && !m.is_downloadable() {
                return bad(format!(
                    "{}: either every file has a URL or none (import-only)",
                    m.id
                ));
            }
            let mut names = HashSet::new();
            for f in &m.files {
                if !is_safe_name(&f.name) || !names.insert(&f.name) {
                    return bad(format!(
                        "{}: invalid or duplicate file name {:?}",
                        m.id, f.name
                    ));
                }
                if f.url.as_deref().is_some_and(|u| !u.starts_with("https://")) {
                    return bad(format!("{}/{}: URL must be https", m.id, f.name));
                }
                if f.size == 0 {
                    return bad(format!("{}/{}: size must be > 0", m.id, f.name));
                }
                if f.sha256.len() != 64
                    || !f
                        .sha256
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
                {
                    return bad(format!(
                        "{}/{}: sha256 must be 64 lowercase hex",
                        m.id, f.name
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn get(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id)
    }

    /// Model-manager rows: Swedish pairs first (MDL-01), then by source and target.
    pub fn catalog(&self) -> Vec<CatalogEntry> {
        let mut rows: Vec<CatalogEntry> = self
            .models
            .iter()
            .map(|m| CatalogEntry {
                id: m.id.clone(),
                pair: m.pair(),
                download_size: m.download_size(),
                downloadable: m.is_downloadable(),
                license: m.license.clone(),
                attribution: m.attribution.clone(),
                source_url: m.source_url.clone(),
            })
            .collect();
        rows.sort_by(|a, b| {
            (!a.pair.involves_swedish(), &a.pair.source, &a.pair.target).cmp(&(
                !b.pair.involves_swedish(),
                &b.pair.source,
                &b.pair.target,
            ))
        });
        rows
    }

    /// The first-run wizard pre-selects these (SV-01).
    pub fn swedish_english_models(&self) -> Vec<&ModelEntry> {
        [LangPair::sv_en(), LangPair::en_sv()]
            .iter()
            .filter_map(|p| self.models.iter().find(|m| m.pair() == *p))
            .collect()
    }
}

/// ASCII letters, digits, `.`, `-`, `_`; not starting with `.`; no path separators.
fn is_safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && !s.starts_with('.')
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) const HASH: &str =
        "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";

    pub(crate) fn sample() -> String {
        format!(
            r#"
schema_version = 1

[[model]]
id = "test-de-en"
engine = "ctranslate2"
source = "de"
target = "en"
version = "1"
license = "MIT"
source_url = "https://example.org/models"
files = [{{ name = "model.bin", url = "https://example.org/de-en/model.bin", size = 4, sha256 = "{HASH}" }}]

[[model]]
id = "test-en-sv"
engine = "ctranslate2"
source = "en"
target = "sv"
version = "1"
license = "MIT"
source_url = "https://example.org/models"
files = [
  {{ name = "model.bin", url = "https://example.org/en-sv/model.bin", size = 30, sha256 = "{HASH}" }},
  {{ name = "vocab.spm", url = "https://example.org/en-sv/vocab.spm", size = 12, sha256 = "{HASH}" }},
]

[[model]]
id = "test-sv-en"
engine = "ctranslate2"
source = "sv"
target = "en"
version = "1"
license = "CC-BY-4.0"
attribution = "Example models by Example Org, CC-BY 4.0"
source_url = "https://example.org/models"
files = [{{ name = "model.bin", size = 40, sha256 = "{HASH}" }}]
"#
        )
    }

    #[test]
    fn bundled_manifest_is_valid() {
        Manifest::bundled().unwrap();
    }

    #[test]
    fn mdl01_catalog_lists_swedish_first_with_sizes() {
        let m = Manifest::parse(&sample()).unwrap();
        let cat = m.catalog();
        let ids: Vec<_> = cat.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["test-en-sv", "test-sv-en", "test-de-en"]);
        assert_eq!(cat[0].download_size, 42);
        assert_eq!(cat[0].license, "MIT");
        assert!(cat[0].downloadable);
        assert!(!cat[1].downloadable, "no URLs: import-only");
        assert_eq!(
            cat[1].attribution.as_deref(),
            Some("Example models by Example Org, CC-BY 4.0")
        );
    }

    #[test]
    fn sv01_wizard_models() {
        let m = Manifest::parse(&sample()).unwrap();
        let ids: Vec<_> = m
            .swedish_english_models()
            .iter()
            .map(|e| e.id.as_str())
            .collect();
        assert_eq!(ids, ["test-sv-en", "test-en-sv"]);
    }

    #[test]
    fn mdl02_rejects_unsafe_manifests() {
        let cases = [
            (
                "https://example.org/de-en/model.bin",
                "http://example.org/de-en/model.bin",
            ),
            (r#"name = "model.bin""#, r#"name = "../model.bin""#),
            ("size = 40", "size = 0"),
        ];
        let base = sample();
        for (from, to) in cases {
            let bad = base.replacen(from, to, 1);
            assert_ne!(bad, base, "replacement applied for {to}");
            assert!(Manifest::parse(&bad).is_err(), "accepted {to}");
        }
        let bad = base.replace(HASH, &HASH.to_uppercase());
        assert!(Manifest::parse(&bad).is_err());
        let dup = base.replace("test-de-en", "test-sv-en");
        assert!(Manifest::parse(&dup).is_err());
        let unattributed = base.replace("attribution = ", "# attribution = ");
        assert!(
            Manifest::parse(&unattributed).is_err(),
            "CC-BY without attribution"
        );
        let mixed = base.replacen(
            r#"{ name = "vocab.spm", url = "https://example.org/en-sv/vocab.spm","#,
            r#"{ name = "vocab.spm","#,
            1,
        );
        assert_ne!(mixed, base);
        assert!(
            Manifest::parse(&mixed).is_err(),
            "only some files have URLs"
        );
        let unknown = base.replace("schema_version = 1", "schema_version = 1\nextra = true");
        assert!(Manifest::parse(&unknown).is_err());
    }
}
