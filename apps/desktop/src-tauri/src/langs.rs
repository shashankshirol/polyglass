//! Which language pairs the lens can use, and the one it uses (MDL-07, D-5).
//!
//! Pairs are data: every pair the model catalog (`models/manifest.toml`) lists can be picked in
//! the toolbar, installed or not; picking one that isn't installed shows the model manager's ⬇.
//! Nothing here names a language except the SV-01 default (Swedish → English). The choice is
//! `[translation] source`, `target` and `auto` in `settings.toml`; `auto` means each block is
//! identified as the source or the target language and text already in the target passes
//! through (SV-04, TR-07).

use pg_core::{LangPair, LangTag, SourceLang};
use pg_models::Manifest;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::models::{self, Models};
use crate::settings::{self, TranslationSettings};
use crate::state::Lens;

/// Event carrying a [`LanguagesView`] to the lens after the pair changed.
pub const EVENT: &str = "lens-languages";

/// The pair in use, from the settings (made valid against the catalog).
pub fn current(app: &AppHandle) -> Choice {
    let saved = app.state::<settings::Store>().get().translation;
    app.state::<Models>().languages().choice(&saved)
}

/// What the lens's pickers show for the pair in use.
pub fn view(app: &AppHandle) -> LanguagesView {
    let models = app.state::<Models>();
    models
        .languages()
        .view(&current(app), |p| models.is_installed(p))
}

/// Use `choice` from now on: save it, offer its models, retranslate what's under the lens and
/// tell both windows. Returns what the lens shows.
pub fn apply(app: &AppHandle, choice: &Choice) -> LanguagesView {
    app.state::<settings::Store>()
        .update(|s| s.translation.set_choice(choice));
    let models = app.state::<Models>();
    models.set_wanted(models.languages().wanted(choice));
    app.state::<Lens>()
        .set_languages(choice.source_lang(), choice.pair.target.clone());
    let view = view(app);
    let _ = app.emit_to(crate::lens::LABEL, EVENT, &view);
    models::emit(app);
    view
}

/// Swap the direction if the catalog has the reverse pair (SV-01: instant once both models are
/// installed). `None` when it hasn't.
pub fn swap(app: &AppHandle) -> Option<LanguagesView> {
    let choice = current(app);
    let back = app.state::<Models>().languages().swapped(&choice)?;
    Some(apply(app, &back))
}

/// The catalog's language pairs, in catalog order (Swedish pairs first, MDL-01), each once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Languages {
    pairs: Vec<LangPair>,
}

/// The pair in use: where translations come from and go to, and whether blocks are identified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub pair: LangPair,
    pub auto: bool,
}

impl Choice {
    /// What the pipeline runs with.
    pub fn source_lang(&self) -> SourceLang {
        if self.auto {
            SourceLang::Auto(self.pair.source.clone())
        } else {
            SourceLang::Fixed(self.pair.source.clone())
        }
    }
}

/// What the toolbar's pickers show (`lens_config`, the `lens-languages` event).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LanguagesView {
    pub source: String,
    pub target: String,
    pub auto: bool,
    /// Every catalog pair and whether its model is installed.
    pub pairs: Vec<PairView>,
    /// The reverse pair is in the catalog, so ⇄ and the swap hotkey work.
    pub can_swap: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct PairView {
    pub source: String,
    pub target: String,
    pub installed: bool,
}

impl Languages {
    pub fn from_manifest(manifest: &Manifest) -> Self {
        let mut pairs: Vec<LangPair> = Vec::new();
        for entry in manifest.catalog() {
            if !pairs.contains(&entry.pair) {
                pairs.push(entry.pair);
            }
        }
        Self { pairs }
    }

    pub fn contains(&self, pair: &LangPair) -> bool {
        self.pairs.contains(pair)
    }

    /// The saved choice, made valid: a pair the catalog doesn't list becomes one with the same
    /// target (a file from before M3-8 has only `target`), else the SV-01 default Swedish →
    /// English, else the first pair.
    pub fn choice(&self, saved: &TranslationSettings) -> Choice {
        let auto = saved.auto;
        let (source, target) = (
            LangTag::parse(&saved.source).ok(),
            LangTag::parse(&saved.target).ok(),
        );
        if let (Some(s), Some(t)) = (&source, &target) {
            let pair = LangPair::new(s.clone(), t.clone());
            if self.contains(&pair) {
                return Choice { pair, auto };
            }
        }
        let same_target = target
            .as_ref()
            .and_then(|t| self.pairs.iter().find(|p| p.target == *t));
        let pair = same_target
            .or_else(|| self.pairs.iter().find(|p| **p == LangPair::sv_en()))
            .or_else(|| self.pairs.first())
            .cloned()
            .unwrap_or_else(LangPair::sv_en);
        Choice { pair, auto }
    }

    /// The other direction, if the catalog has it (SV-01's swap).
    pub fn swapped(&self, choice: &Choice) -> Option<Choice> {
        let pair = choice.pair.swapped();
        self.contains(&pair).then_some(Choice {
            pair,
            auto: choice.auto,
        })
    }

    /// A choice from the webview (SEC-01): valid tags naming a catalog pair.
    pub fn validate(&self, source: &str, target: &str, auto: bool) -> Result<Choice, String> {
        let parse = |s: &str| LangTag::parse(s).map_err(|_| format!("invalid language {s:?}"));
        let pair = LangPair::new(parse(source)?, parse(target)?);
        if !self.contains(&pair) {
            return Err(format!("no model for {pair}"));
        }
        Ok(Choice { pair, auto })
    }

    /// The pairs whose models the lens offers for `choice`: the pair and, if the catalog has it,
    /// its reverse, so the swap works offline once both are installed (SV-01).
    pub fn wanted(&self, choice: &Choice) -> Vec<LangPair> {
        let mut pairs = vec![choice.pair.clone()];
        if let Some(back) = self.swapped(choice) {
            pairs.push(back.pair);
        }
        pairs
    }

    pub fn view(&self, choice: &Choice, installed: impl Fn(&LangPair) -> bool) -> LanguagesView {
        LanguagesView {
            source: choice.pair.source.to_string(),
            target: choice.pair.target.to_string(),
            auto: choice.auto,
            pairs: self
                .pairs
                .iter()
                .map(|p| PairView {
                    source: p.source.to_string(),
                    target: p.target.to_string(),
                    installed: installed(p),
                })
                .collect(),
            can_swap: self.swapped(choice).is_some(),
        }
    }
}

impl TranslationSettings {
    pub fn set_choice(&mut self, choice: &Choice) {
        self.source = choice.pair.source.to_string();
        self.target = choice.pair.target.to_string();
        self.auto = choice.auto;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(s: &str) -> LangTag {
        LangTag::parse(s).unwrap()
    }

    fn languages() -> Languages {
        // The bundled catalog: sv↔en and de→en.
        Languages::from_manifest(&Manifest::bundled().unwrap())
    }

    fn saved(source: &str, target: &str, auto: bool) -> TranslationSettings {
        TranslationSettings {
            source: source.into(),
            target: target.into(),
            auto,
        }
    }

    #[test]
    fn mdl07_pairs_come_from_the_catalog() {
        let l = languages();
        let de_en = LangPair::new(tag("de"), LangTag::en());
        assert!(l.contains(&LangPair::sv_en()) && l.contains(&LangPair::en_sv()));
        assert!(l.contains(&de_en));
        assert!(!l.contains(&de_en.swapped()), "no en→de model");
        let c = l.validate("de", "en", false).unwrap();
        assert_eq!(c.source_lang(), SourceLang::Fixed(tag("de")));
        assert_eq!(l.wanted(&c), std::slice::from_ref(&de_en), "no reverse");
        assert!(l.swapped(&c).is_none());
        let view = l.view(&c, |p| *p == de_en);
        assert_eq!((view.source.as_str(), view.target.as_str()), ("de", "en"));
        assert!(!view.can_swap);
        assert_eq!(view.pairs.iter().filter(|p| p.installed).count(), 1);
    }

    #[test]
    fn sec01_choices_from_the_webview_must_name_a_catalog_pair() {
        let l = languages();
        assert!(l.validate("sv", "en", true).is_ok());
        assert!(l.validate("en", "de", true).is_err(), "not in the catalog");
        assert!(l.validate("sv", "sv", true).is_err());
        assert!(l.validate("../x", "en", true).is_err());
        assert!(l.validate(&"s".repeat(10_000), "en", true).is_err());
    }

    #[test]
    fn sv01_default_and_swap() {
        let l = languages();
        let c = l.choice(&TranslationSettings::default());
        assert_eq!(c.pair, LangPair::sv_en());
        assert_eq!(c.source_lang(), SourceLang::Auto(LangTag::sv()));
        assert_eq!(l.wanted(&c), [LangPair::sv_en(), LangPair::en_sv()]);
        let back = l.swapped(&c).unwrap();
        assert_eq!(back.pair, LangPair::en_sv());
        assert!(back.auto);
        assert_eq!(l.swapped(&back).unwrap(), c);
    }

    #[test]
    fn set01_saved_choices_are_made_valid() {
        let l = languages();
        // Before M3-8 only `target` was saved; `source` takes its default `sv`.
        assert_eq!(l.choice(&saved("sv", "sv", true)).pair, LangPair::en_sv());
        assert_eq!(l.choice(&saved("de", "en", false)).pair.source, tag("de"));
        // Unknown target or a broken tag: the default pair.
        assert_eq!(l.choice(&saved("sv", "fi", true)).pair, LangPair::sv_en());
        assert_eq!(l.choice(&saved("??", "!!", true)).pair, LangPair::sv_en());
        // A source the catalog lacks keeps the target.
        assert_eq!(
            l.choice(&saved("fi", "en", true)).pair.target,
            LangTag::en()
        );
        let mut s = saved("sv", "en", true);
        s.set_choice(&l.validate("de", "en", false).unwrap());
        assert_eq!(s, saved("de", "en", false));
    }
}
