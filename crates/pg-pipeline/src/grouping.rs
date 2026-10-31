//! Group OCR lines into blocks (OCR-02) and order blocks for reading (MODE-02, REN-05).

use pg_core::{Block, Line};

/// Tuning knobs, all relative to line height so they work at every scale (CAP-04).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GroupingConfig {
    /// Max vertical gap between consecutive lines of a block, in line heights.
    pub max_gap: f32,
    /// Max ratio between the heights of consecutive lines.
    pub max_height_ratio: f32,
    /// Max offset between left edges (or centers, for centered text), in line heights.
    pub max_align_offset: f32,
}

impl Default for GroupingConfig {
    fn default() -> Self {
        Self {
            max_gap: 0.9,
            max_height_ratio: 1.45,
            max_align_offset: 1.5,
        }
    }
}

fn continues(prev: &Line, next: &Line, cfg: &GroupingConfig) -> bool {
    let (a, b) = (prev.bbox, next.bbox);
    let h = (a.h + b.h) / 2.0;
    if h <= 0.0 {
        return false;
    }
    let ratio = a.h.max(b.h) / a.h.min(b.h).max(f32::EPSILON);
    if ratio > cfg.max_height_ratio {
        return false;
    }
    let gap = b.y - a.bottom();
    if gap > cfg.max_gap * h || gap < -0.5 * h {
        return false;
    }
    let left_aligned = (a.x - b.x).abs() <= cfg.max_align_offset * h;
    let centered = ((a.x + a.w / 2.0) - (b.x + b.w / 2.0)).abs() <= cfg.max_align_offset * h;
    let overlap = a.horizontal_overlap(&b) >= 0.5 * a.w.min(b.w);
    (left_aligned || centered) && overlap
}

/// Group lines into blocks by proximity and alignment, returned in reading order.
pub fn group_lines(lines: Vec<Line>, cfg: &GroupingConfig) -> Vec<Block> {
    let mut lines: Vec<Line> = lines
        .into_iter()
        .filter(|l| !l.text.trim().is_empty() && l.bbox.w > 0.0 && l.bbox.h > 0.0)
        .collect();
    lines.sort_by(|a, b| {
        a.bbox
            .y
            .total_cmp(&b.bbox.y)
            .then(a.bbox.x.total_cmp(&b.bbox.x))
    });

    let mut groups: Vec<Vec<Line>> = Vec::new();
    for line in lines {
        // Attach to the open group whose last line is the closest valid predecessor.
        let best = groups
            .iter()
            .enumerate()
            .filter_map(|(i, g)| {
                let last = g.last().expect("groups are non-empty");
                continues(last, &line, cfg).then(|| (i, line.bbox.y - last.bbox.bottom()))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1));
        match best {
            Some((i, _)) => groups[i].push(line),
            None => groups.push(vec![line]),
        }
    }

    let blocks = groups.into_iter().map(Block::from_lines).collect();
    reading_order(blocks)
}

/// Sort blocks top-to-bottom; blocks that start within half a line of each other form a row
/// and are ordered left-to-right.
pub fn reading_order(mut blocks: Vec<Block>) -> Vec<Block> {
    blocks.sort_by(|a, b| a.bbox.y.total_cmp(&b.bbox.y));
    let mut out = Vec::with_capacity(blocks.len());
    let mut row: Vec<Block> = Vec::new();
    let mut row_limit = f32::NEG_INFINITY;
    for b in blocks {
        if !row.is_empty() && b.bbox.y > row_limit {
            row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
            out.append(&mut row);
        }
        if row.is_empty() {
            let first_line_h = b.lines.first().map_or(b.bbox.h, |l| l.bbox.h);
            row_limit = b.bbox.y + first_line_h * 0.5;
        }
        row.push(b);
    }
    row.sort_by(|a, b| a.bbox.x.total_cmp(&b.bbox.x));
    out.append(&mut row);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use pg_core::Rect;

    fn l(text: &str, x: f32, y: f32, w: f32, h: f32) -> Line {
        Line::new(text, Rect::new(x, y, w, h), 0.95)
    }

    fn texts(blocks: &[Block]) -> Vec<String> {
        blocks.iter().map(Block::text).collect()
    }

    #[test]
    fn paragraph_lines_form_one_block() {
        let lines = vec![
            l("Det gick inte att spara", 10.0, 10.0, 200.0, 14.0),
            l("dokumentet eftersom disken", 10.0, 28.0, 220.0, 14.0),
            l("är full.", 10.0, 46.0, 60.0, 14.0),
        ];
        let blocks = group_lines(lines, &GroupingConfig::default());
        assert_eq!(
            texts(&blocks),
            ["Det gick inte att spara dokumentet eftersom disken är full."]
        );
    }

    #[test]
    fn heading_and_body_split_by_height_and_gap() {
        let lines = vec![
            l("Inställningar", 10.0, 0.0, 180.0, 24.0),
            l("Välj hur appen ska bete sig.", 10.0, 32.0, 200.0, 14.0),
            l("Ändringar sparas direkt.", 10.0, 50.0, 180.0, 14.0),
            l("Avancerat", 10.0, 100.0, 80.0, 14.0),
        ];
        let blocks = group_lines(lines, &GroupingConfig::default());
        assert_eq!(
            texts(&blocks),
            [
                "Inställningar",
                "Välj hur appen ska bete sig. Ändringar sparas direkt.",
                "Avancerat"
            ]
        );
    }

    #[test]
    fn side_by_side_buttons_are_separate_and_left_to_right() {
        // OCR may report the right button slightly higher.
        let lines = vec![
            l("Avbryt", 300.0, 199.0, 50.0, 14.0),
            l("Spara", 200.0, 200.0, 40.0, 14.0),
        ];
        let blocks = group_lines(lines, &GroupingConfig::default());
        assert_eq!(texts(&blocks), ["Spara", "Avbryt"]);
    }

    #[test]
    fn two_columns_do_not_merge() {
        let lines = vec![
            l("Vänster kolumn rad ett", 0.0, 0.0, 150.0, 14.0),
            l("Höger kolumn rad ett", 300.0, 0.0, 150.0, 14.0),
            l("vänster rad två", 0.0, 18.0, 120.0, 14.0),
            l("höger rad två", 300.0, 18.0, 110.0, 14.0),
        ];
        let blocks = group_lines(lines, &GroupingConfig::default());
        assert_eq!(
            texts(&blocks),
            [
                "Vänster kolumn rad ett vänster rad två",
                "Höger kolumn rad ett höger rad två"
            ]
        );
    }

    #[test]
    fn centered_lines_group() {
        let lines = vec![
            l("Välkommen tillbaka", 100.0, 0.0, 200.0, 20.0),
            l("Logga in för att fortsätta", 80.0, 26.0, 240.0, 20.0),
        ];
        assert_eq!(group_lines(lines, &GroupingConfig::default()).len(), 1);
    }

    #[test]
    fn scale_invariance() {
        let base = [
            l("Rad ett", 10.0, 10.0, 100.0, 14.0),
            l("rad två", 10.0, 28.0, 100.0, 14.0),
            l("Ny rubrik", 10.0, 70.0, 100.0, 14.0),
        ];
        for s in [1.0f32, 1.25, 1.5, 2.0] {
            let scaled = base
                .iter()
                .map(|x| Line::new(x.text.clone(), x.bbox.scaled(s), x.confidence))
                .collect();
            assert_eq!(
                group_lines(scaled, &GroupingConfig::default()).len(),
                2,
                "scale {s}"
            );
        }
    }

    #[test]
    fn empty_lines_are_dropped() {
        let lines = vec![l("  ", 0.0, 0.0, 10.0, 10.0), l("Ok", 0.0, 0.0, 0.0, 10.0)];
        assert!(group_lines(lines, &GroupingConfig::default()).is_empty());
    }
}
