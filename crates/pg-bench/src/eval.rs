//! `pg-bench --eval <set>`: translate an evaluation set (M0-FIX-4) and report corpus chrF per
//! domain and overall (SV-07), and the latency of each segment's translation.
//!
//! Each segment goes to the backend on its own, as one block, the way the lens sends a block.
//! The first segment is translated once more beforehand (untimed) so model warm-up isn't counted.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use pg_testkit::eval::{Domain, EvalSet};
use pg_testkit::metrics::{ChrfStats, chrf_stats};
use pg_translate::Translator;
use serde::Serialize;

use crate::{StageStats, stats};

#[derive(Debug, Serialize)]
pub struct DomainScore {
    pub segments: usize,
    pub chrf: f64,
}

#[derive(Debug, Serialize)]
pub struct EvalReport {
    pub set: String,
    pub pair: String,
    pub backend: String,
    pub segments: usize,
    /// Corpus chrF over the whole set (0–100).
    pub chrf: f64,
    pub domains: BTreeMap<Domain, DomainScore>,
    /// Segments whose translation failed (scored as empty output).
    pub failed: usize,
    /// Segments that came back exactly as the source.
    pub unchanged: usize,
    /// SV-05 guard counters, warm-up included (set by the caller).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub guard: Option<crate::backend::GuardCounts>,
    /// One segment's translation.
    pub latency: StageStats,
}

#[derive(Debug, Serialize)]
pub struct Hypothesis<'a> {
    pub id: u32,
    pub domain: Domain,
    pub source: &'a str,
    pub reference: &'a str,
    pub hypothesis: String,
    pub chrf: f64,
}

pub fn run(
    set_dir: &Path,
    translator: &dyn Translator,
    rt: &tokio::runtime::Runtime,
    hypotheses: Option<&Path>,
) -> Result<EvalReport, String> {
    let set = EvalSet::load(set_dir).map_err(|e| e.to_string())?;
    let pair = set.pair();
    let first = set.segments.first().ok_or("the set has no segments")?;
    rt.block_on(translator.translate(std::slice::from_ref(&first.source), &pair))
        .map_err(|e| format!("{pair}: {e}"))?;

    let mut total = ChrfStats::default();
    let mut per_domain: BTreeMap<Domain, (usize, ChrfStats)> = BTreeMap::new();
    let mut latency = Vec::with_capacity(set.segments.len());
    let mut out = Vec::with_capacity(set.segments.len());
    let mut failed = 0;
    let mut unchanged = 0;
    for s in &set.segments {
        let start = Instant::now();
        let got = rt.block_on(translator.translate(std::slice::from_ref(&s.source), &pair));
        latency.push(start.elapsed().as_secs_f64() * 1000.0);
        let hypothesis = match got {
            Ok(mut t) if t.len() == 1 => t.remove(0),
            Ok(_) | Err(_) => {
                failed += 1;
                String::new()
            }
        };
        if hypothesis == s.source {
            unchanged += 1;
        }
        let st = chrf_stats(&hypothesis, &s.reference);
        total += &st;
        let d = per_domain.entry(s.domain).or_default();
        d.0 += 1;
        d.1 += &st;
        out.push(Hypothesis {
            id: s.id,
            domain: s.domain,
            source: &s.source,
            reference: &s.reference,
            hypothesis,
            chrf: st.score(),
        });
    }
    if let Some(path) = hypotheses {
        let json = serde_json::to_string_pretty(&out).map_err(|e| e.to_string())?;
        std::fs::write(path, json + "\n").map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(EvalReport {
        set: set.set.clone(),
        pair: format!("{}→{}", pair.source, pair.target),
        backend: translator.id().to_string(),
        segments: set.segments.len(),
        chrf: total.score(),
        domains: per_domain
            .into_iter()
            .map(|(d, (n, st))| {
                (
                    d,
                    DomainScore {
                        segments: n,
                        chrf: st.score(),
                    },
                )
            })
            .collect(),
        failed,
        unchanged,
        guard: None,
        latency: stats("segment", latency),
    })
}

pub fn print(r: &EvalReport) {
    println!(
        "{}  {}  backend={}  segments={}  failed={}  unchanged={}",
        r.set, r.pair, r.backend, r.segments, r.failed, r.unchanged
    );
    if let Some(g) = &r.guard {
        crate::print_guard(g);
    }
    println!("chrF       {:>6.2}", r.chrf);
    for (d, s) in &r.domains {
        println!(
            "  {:<8} {:>6.2}  ({} segments)",
            format!("{d:?}").to_lowercase(),
            s.chrf,
            s.segments
        );
    }
    println!(
        "{:<10} {:>9} {:>9} {:>9} {:>9}",
        "latency", "p50 ms", "p95 ms", "p99 ms", "max ms"
    );
    let l = &r.latency;
    println!(
        "{:<10} {:>9.3} {:>9.3} {:>9.3} {:>9.3}",
        l.stage, l.p50_ms, l.p95_ms, l.p99_ms, l.max_ms
    );
}
