use serde::{Deserialize, Serialize};

use crate::Rect;

/// One OCR output line, in frame-local physical pixels.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub text: String,
    pub bbox: Rect,
    /// 0.0–1.0. Engines without per-line confidence report 1.0.
    pub confidence: f32,
}

impl Line {
    pub fn new(text: impl Into<String>, bbox: Rect, confidence: f32) -> Self {
        Self {
            text: text.into(),
            bbox,
            confidence,
        }
    }
}

/// A group of lines translated as one unit (paragraph, label).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Block {
    pub lines: Vec<Line>,
    pub bbox: Rect,
}

impl Block {
    pub fn from_lines(lines: Vec<Line>) -> Self {
        let bbox = lines
            .iter()
            .map(|l| l.bbox)
            .reduce(|a, b| a.union(&b))
            .unwrap_or_default();
        Self { lines, bbox }
    }

    /// Lowest line confidence in the block.
    pub fn confidence(&self) -> f32 {
        self.lines
            .iter()
            .map(|l| l.confidence)
            .fold(f32::INFINITY, f32::min)
            .min(1.0)
    }

    /// Mean height of the block's lines, in the same units as the boxes (0 for no lines).
    pub fn line_height(&self) -> f32 {
        if self.lines.is_empty() {
            return 0.0;
        }
        self.lines.iter().map(|l| l.bbox.h).sum::<f32>() / self.lines.len() as f32
    }

    /// The block's text with lines joined for translation.
    ///
    /// A line ending in a hyphen directly after a letter is joined without a space and the
    /// hyphen is dropped when the next line starts with a lowercase letter (soft line-break
    /// hyphenation, common with long Swedish compounds). Before an uppercase letter or digit the
    /// hyphen is kept (`USB-` + `C-kabel`). Otherwise lines are joined with a space.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let t = line.text.trim();
            if t.is_empty() {
                continue;
            }
            if out.is_empty() {
                out.push_str(t);
                continue;
            }
            let next_lower = t.chars().next().is_some_and(char::is_lowercase);
            let mut chars = out.chars().rev();
            let hyphenated =
                chars.next() == Some('-') && chars.next().is_some_and(char::is_alphabetic);
            if hyphenated && next_lower {
                out.pop();
            } else if !hyphenated {
                out.push(' ');
            }
            out.push_str(t);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(t: &str, y: f32) -> Line {
        Line::new(t, Rect::new(0.0, y, 100.0, 10.0), 0.9)
    }

    #[test]
    fn line_height_is_the_mean_of_the_lines() {
        let mut tall = line("Avbryt", 12.0);
        tall.bbox.h = 20.0;
        assert_eq!(
            Block::from_lines(vec![line("Spara", 0.0), tall]).line_height(),
            15.0
        );
        assert_eq!(Block::from_lines(vec![]).line_height(), 0.0);
    }

    #[test]
    fn joins_lines_with_space() {
        let b = Block::from_lines(vec![line("Spara dina", 0.0), line("ändringar", 12.0)]);
        assert_eq!(b.text(), "Spara dina ändringar");
        assert_eq!(b.bbox, Rect::new(0.0, 0.0, 100.0, 22.0));
    }

    #[test]
    fn dehyphenates_soft_breaks() {
        let b = Block::from_lines(vec![line("Säkerhets-", 0.0), line("inställningar", 12.0)]);
        assert_eq!(b.text(), "Säkerhetsinställningar");
    }

    #[test]
    fn keeps_hyphen_before_uppercase_or_digit() {
        let b = Block::from_lines(vec![line("USB-", 0.0), line("C-kabel", 12.0)]);
        assert_eq!(b.text(), "USB-C-kabel");
        let b = Block::from_lines(vec![line("Sida 1 -", 0.0), line("klar", 12.0)]);
        assert_eq!(b.text(), "Sida 1 - klar");
    }

    #[test]
    fn confidence_is_minimum() {
        let mut a = line("a", 0.0);
        a.confidence = 0.4;
        let b = Block::from_lines(vec![a, line("b", 12.0)]);
        assert!((b.confidence() - 0.4).abs() < f32::EPSILON);
    }
}
