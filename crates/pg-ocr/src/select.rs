use pg_core::{Error, LangTag, Result};

/// Pick the engine language tag to use for `wanted` from the engine's runtime languages.
///
/// Prefers an exact match, then any tag with the same primary language (`sv` → `sv-SE`).
/// Returns [`Error::OcrLanguageMissing`] so the UI can show an actionable message (OCR-03, SV-02).
pub fn resolve_language(supported: &[LangTag], wanted: &LangTag) -> Result<LangTag> {
    supported
        .iter()
        .find(|t| *t == wanted)
        .or_else(|| supported.iter().find(|t| t.same_language(wanted)))
        .cloned()
        .ok_or_else(|| Error::OcrLanguageMissing(wanted.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> LangTag {
        LangTag::parse(s).unwrap()
    }

    #[test]
    fn resolve_prefers_exact_then_primary() {
        let supported = [tag("en-GB"), tag("en-US"), tag("sv-SE")];
        assert_eq!(
            resolve_language(&supported, &tag("en-US")).unwrap(),
            tag("en-US")
        );
        assert_eq!(
            resolve_language(&supported, &tag("en")).unwrap(),
            tag("en-GB")
        );
        assert_eq!(
            resolve_language(&supported, &tag("sv")).unwrap(),
            tag("sv-SE")
        );
    }

    #[test]
    fn resolve_reports_missing_swedish() {
        // The dev machine's actual Windows OCR languages (no sv-SE): SV-02 path.
        let supported = [tag("en-GB"), tag("en-US")];
        let err = resolve_language(&supported, &LangTag::sv()).unwrap_err();
        assert!(matches!(err, Error::OcrLanguageMissing(t) if t.is_swedish()));
    }
}
