//! The QUAL-02 regression gate: `pg-bench --baseline base.json` compares this run's p50 with a
//! report an earlier build wrote with `--json` on the same machine, and fails when the whole
//! cycle got more than `--max-regression` percent slower.
//!
//! Both runs must be of the same setup (fixture image, pair, OCR, translator, cache); anything
//! else is an error rather than a comparison. A slowdown smaller than `--min-regression-ms` never
//! fails the gate: with mock stages a cycle takes well under a millisecond and 20% of that is
//! timer noise.

use serde::Deserialize;

/// The stage the gate decides on: the whole cycle.
pub const GATED_STAGE: &str = "total";

/// The parts of a `--json` report the gate reads.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Baseline {
    pub image: String,
    pub pair: String,
    pub ocr: String,
    pub translator: Translator,
    pub cache: String,
    pub stages: Vec<Stage>,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Translator {
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Stage {
    pub stage: String,
    pub p50_ms: f64,
}

/// How far a regression may go before the gate fails.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// Percent of the baseline's p50 (QUAL-02: 20).
    pub max_percent: f64,
    /// A slowdown below this many milliseconds passes whatever its percentage.
    pub min_ms: f64,
}

/// One stage's p50 in both runs.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub stage: String,
    pub base_ms: f64,
    pub head_ms: f64,
    /// Whether this row fails the gate (only [`GATED_STAGE`] can).
    pub regressed: bool,
}

impl Row {
    /// Change against the baseline in percent; 0 when the baseline took no measurable time.
    pub fn percent(&self) -> f64 {
        if self.base_ms > 0.0 {
            (self.head_ms - self.base_ms) / self.base_ms * 100.0
        } else {
            0.0
        }
    }
}

impl Baseline {
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read the baseline {}: {e}", path.display()))?;
        // PowerShell's `>` writes a byte-order mark.
        serde_json::from_str(text.trim_start_matches('\u{feff}'))
            .map_err(|e| format!("{} isn't a pg-bench --json report: {e}", path.display()))
    }

    fn setup(&self) -> [(&'static str, &str); 5] {
        [
            ("image", &self.image),
            ("pair", &self.pair),
            ("ocr", &self.ocr),
            ("translator", &self.translator.id),
            ("cache", &self.cache),
        ]
    }
}

/// Compare `head` with `base`, stage by stage in `head`'s order. An error when the two runs
/// aren't of the same setup or the baseline lacks a stage.
pub fn compare(base: &Baseline, head: &Baseline, limits: Limits) -> Result<Vec<Row>, String> {
    for ((what, b), (_, h)) in base.setup().into_iter().zip(head.setup()) {
        if b != h {
            return Err(format!(
                "the baseline is of another setup ({what}: {b}, this run: {h})"
            ));
        }
    }
    head.stages
        .iter()
        .map(|h| {
            let b = base
                .stages
                .iter()
                .find(|b| b.stage == h.stage)
                .ok_or_else(|| format!("the baseline has no {:?} stage", h.stage))?;
            let slower = h.p50_ms - b.p50_ms;
            Ok(Row {
                stage: h.stage.clone(),
                base_ms: b.p50_ms,
                head_ms: h.p50_ms,
                regressed: h.stage == GATED_STAGE
                    && slower > limits.min_ms
                    && slower > b.p50_ms * limits.max_percent / 100.0,
            })
        })
        .collect()
}

/// The table printed (to stderr, so `--json` stays parseable) after a gated run.
pub fn table(rows: &[Row], limits: Limits) -> String {
    let mut out = format!(
        "p50 against the baseline (QUAL-02: fails when {GATED_STAGE} is more than {:.0}% and {} ms slower)\n\
         {:<10} {:>10} {:>10} {:>8}\n",
        limits.max_percent, limits.min_ms, "stage", "base ms", "now ms", "change"
    );
    for r in rows {
        out.push_str(&format!(
            "{:<10} {:>10.3} {:>10.3} {:>+7.1}%{}\n",
            r.stage,
            r.base_ms,
            r.head_ms,
            r.percent(),
            if r.regressed { "  REGRESSION" } else { "" }
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        max_percent: 20.0,
        min_ms: 1.0,
    };

    fn report(ocr_ms: f64, total_ms: f64) -> Baseline {
        Baseline {
            image: "ui-dialog-en@100-light".into(),
            pair: "en→sv".into(),
            ocr: "windows".into(),
            translator: Translator { id: "mock".into() },
            cache: "uncached".into(),
            stages: vec![
                Stage {
                    stage: "ocr".into(),
                    p50_ms: ocr_ms,
                },
                Stage {
                    stage: "total".into(),
                    p50_ms: total_ms,
                },
            ],
        }
    }

    fn regressed(base_total: f64, head_total: f64, limits: Limits) -> bool {
        compare(&report(1.0, base_total), &report(1.0, head_total), limits)
            .unwrap()
            .iter()
            .any(|r| r.regressed)
    }

    #[test]
    fn qual02_fails_only_above_twenty_percent_of_the_baseline_p50() {
        assert!(!regressed(20.0, 20.0, LIMITS));
        assert!(!regressed(20.0, 24.0, LIMITS), "exactly 20% passes");
        assert!(regressed(20.0, 24.1, LIMITS));
        assert!(regressed(20.0, 60.0, LIMITS));
        assert!(!regressed(20.0, 10.0, LIMITS), "faster is fine");
    }

    #[test]
    fn qual02_a_slowdown_below_the_floor_passes() {
        // 0.30 → 0.45 ms is +50%, but 0.15 ms is timer noise for mock stages.
        assert!(!regressed(0.30, 0.45, LIMITS));
        let strict = Limits {
            min_ms: 0.1,
            ..LIMITS
        };
        assert!(regressed(0.30, 0.45, strict));
    }

    #[test]
    fn qual02_only_the_whole_cycle_is_gated_and_every_stage_is_listed() {
        // OCR doubled, but the cycle stayed within the limit.
        let rows = compare(&report(5.0, 20.0), &report(10.0, 23.0), LIMITS).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].stage, "ocr");
        assert!((rows[0].percent() - 100.0).abs() < 1e-9);
        assert!(rows.iter().all(|r| !r.regressed));
        let text = table(&rows, LIMITS);
        assert!(
            text.contains("+100.0%") && !text.contains("REGRESSION"),
            "{text}"
        );
        let rows = compare(&report(5.0, 20.0), &report(10.0, 30.0), LIMITS).unwrap();
        assert!(table(&rows, LIMITS).contains("+50.0%  REGRESSION"));
    }

    #[test]
    fn qual02_another_setup_or_a_missing_stage_is_an_error() {
        let base = report(5.0, 20.0);
        let mut other = base.clone();
        other.translator.id = "ct2-opus-mt".into();
        let e = compare(&base, &other, LIMITS).unwrap_err();
        assert!(e.contains("translator: mock, this run: ct2-opus-mt"), "{e}");
        let mut scaled = base.clone();
        scaled.image = "ui-dialog-en@150-light".into();
        assert!(compare(&base, &scaled, LIMITS).is_err());
        let mut short = base.clone();
        short.stages.pop();
        assert!(
            compare(&short, &base, LIMITS)
                .unwrap_err()
                .contains("total")
        );
    }

    #[test]
    fn qual02_reads_a_json_report_with_or_without_a_byte_order_mark() {
        let json = r#"{"fixture":"tests/fixtures/ui-dialog-sv","image":"ui-dialog-sv@100-light",
            "pair":"sv→en","ocr":"mock","translator":{"id":"mock","load_ms":0.0},
            "cache":"uncached","runs":100,
            "stages":[{"stage":"total","p50_ms":0.25,"p95_ms":0.3,"p99_ms":0.4,"max_ms":1.0}]}"#;
        let dir = std::env::temp_dir().join(format!("pg-bench-gate-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in [
            ("plain.json", json.to_owned()),
            ("bom.json", format!("\u{feff}{json}")),
        ] {
            let path = dir.join(name);
            std::fs::write(&path, text).unwrap();
            let b = Baseline::load(&path).unwrap();
            assert_eq!(b.pair, "sv→en");
            assert_eq!(b.stages[0].p50_ms, 0.25);
        }
        std::fs::write(dir.join("bad.json"), "{}").unwrap();
        assert!(Baseline::load(&dir.join("bad.json")).is_err());
        assert!(Baseline::load(&dir.join("missing.json")).is_err());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
