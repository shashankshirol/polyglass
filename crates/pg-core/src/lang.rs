use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// A BCP-47-style language tag, normalized to lowercase primary subtag and uppercase region
/// (`sv`, `en`, `en-GB`, `sv-SE`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct LangTag(String);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("invalid language tag")]
pub struct LangTagError;

impl LangTag {
    /// Parse and normalize a tag. Accepts `_` as a separator (`sv_SE`).
    pub fn parse(s: &str) -> Result<Self, LangTagError> {
        let mut parts = s.trim().split(['-', '_']);
        let primary = parts.next().ok_or(LangTagError)?;
        if !(2..=3).contains(&primary.len()) || !primary.chars().all(|c| c.is_ascii_alphabetic()) {
            return Err(LangTagError);
        }
        let mut out = primary.to_ascii_lowercase();
        for sub in parts {
            if sub.is_empty() || sub.len() > 8 || !sub.chars().all(|c| c.is_ascii_alphanumeric()) {
                return Err(LangTagError);
            }
            out.push('-');
            match sub.len() {
                2 => out.push_str(&sub.to_ascii_uppercase()),
                4 => {
                    // Script subtag: title case (Latn, Cyrl).
                    let mut cs = sub.chars();
                    if let Some(c) = cs.next() {
                        out.push(c.to_ascii_uppercase());
                    }
                    out.push_str(&cs.as_str().to_ascii_lowercase());
                }
                _ => out.push_str(&sub.to_ascii_lowercase()),
            }
        }
        Ok(Self(out))
    }

    pub fn sv() -> Self {
        Self("sv".into())
    }

    pub fn en() -> Self {
        Self("en".into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Primary language subtag (`sv` for `sv-SE`).
    pub fn primary(&self) -> &str {
        self.0.split('-').next().unwrap_or(&self.0)
    }

    /// True if both tags share the same primary language (`en-GB` ~ `en`).
    pub fn same_language(&self, other: &LangTag) -> bool {
        self.primary() == other.primary()
    }

    pub fn is_swedish(&self) -> bool {
        self.primary() == "sv"
    }
}

impl fmt::Display for LangTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl FromStr for LangTag {
    type Err = LangTagError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl TryFrom<String> for LangTag {
    type Error = LangTagError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}

impl From<LangTag> for String {
    fn from(t: LangTag) -> String {
        t.0
    }
}

/// A translation direction.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LangPair {
    pub source: LangTag,
    pub target: LangTag,
}

impl LangPair {
    pub fn new(source: LangTag, target: LangTag) -> Self {
        Self { source, target }
    }

    pub fn sv_en() -> Self {
        Self::new(LangTag::sv(), LangTag::en())
    }

    pub fn en_sv() -> Self {
        Self::new(LangTag::en(), LangTag::sv())
    }

    pub fn swapped(&self) -> Self {
        Self::new(self.target.clone(), self.source.clone())
    }

    pub fn involves_swedish(&self) -> bool {
        self.source.is_swedish() || self.target.is_swedish()
    }
}

impl fmt::Display for LangPair {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}→{}", self.source, self.target)
    }
}

/// The user's source-language choice (OCR-03, MDL-07).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "lang")]
pub enum SourceLang {
    /// Every block is in this language.
    Fixed(LangTag),
    /// Each block is identified as this language or the target language; blocks already in
    /// the target pass through (SV-04, TR-07). `Auto(sv)` with target `en` is SPEC's
    /// "Auto (Swedish/English)".
    Auto(LangTag),
}

impl SourceLang {
    /// The language translated from: the fixed one, or the non-target side of `Auto`.
    pub fn lang(&self) -> &LangTag {
        match self {
            SourceLang::Fixed(l) | SourceLang::Auto(l) => l,
        }
    }

    pub fn is_auto(&self) -> bool {
        matches!(self, SourceLang::Auto(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_normalizes() {
        assert_eq!(LangTag::parse("SV").unwrap().as_str(), "sv");
        assert_eq!(LangTag::parse("sv_se").unwrap().as_str(), "sv-SE");
        assert_eq!(LangTag::parse("zh-hant-tw").unwrap().as_str(), "zh-Hant-TW");
        assert!(LangTag::parse("").is_err());
        assert!(LangTag::parse("s").is_err());
        assert!(LangTag::parse("sv-").is_err());
        assert!(LangTag::parse("s3").is_err());
    }

    #[test]
    fn primary_and_same_language() {
        let gb = LangTag::parse("en-GB").unwrap();
        assert_eq!(gb.primary(), "en");
        assert!(gb.same_language(&LangTag::en()));
        assert!(!gb.same_language(&LangTag::sv()));
    }

    #[test]
    fn pair_swap_and_display() {
        let p = LangPair::sv_en();
        assert_eq!(p.swapped(), LangPair::en_sv());
        assert_eq!(p.to_string(), "sv→en");
        assert!(p.involves_swedish());
    }

    #[test]
    fn serde_round_trip() {
        let s = SourceLang::Fixed(LangTag::sv());
        let j = serde_json::to_string(&s).unwrap();
        assert_eq!(j, r#"{"kind":"fixed","lang":"sv"}"#);
        assert_eq!(serde_json::from_str::<SourceLang>(&j).unwrap(), s);
        let auto = SourceLang::Auto(LangTag::parse("de").unwrap());
        assert_eq!(
            serde_json::to_string(&auto).unwrap(),
            r#"{"kind":"auto","lang":"de"}"#
        );
        assert_eq!(auto.lang().as_str(), "de");
        assert!(auto.is_auto() && !s.is_auto());
        assert!(serde_json::from_str::<LangTag>(r#""x""#).is_err());
    }
}
