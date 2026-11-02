//! Clock times in 12- and 24-hour form (SV-05, owner decision D-10).
//!
//! A translation may write a clock time in the other form (`kl. 15.00` → `3 p.m.`,
//! `7:45 a.m.` → `07.45`) as long as it names the same time of day. [`spans`] finds the times
//! in a text; `tokens::check_preserved` pairs each 12-hour time with a time of the same value
//! on the other side.

/// A clock time found in a text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClockSpan {
    /// Char indices: the first digit, and one past the last char (incl. `a.m.`/`p.m.`).
    pub(crate) start: usize,
    pub(crate) end: usize,
    /// 24-hour value.
    pub(crate) hour: u8,
    pub(crate) minute: u8,
    /// Written with a.m./p.m.
    pub(crate) twelve_hour: bool,
}

/// Words before a bare hour that make it a time (`kl. 8`, `klockan 17`).
const CLOCK_WORDS: [&str; 3] = ["kl", "kl.", "klockan"];

/// What may join two times into a range (`kl. 8 och 17`, `kl. 8–17`).
const CONNECTORS: [&str; 10] = [
    "och", "eller", "till", "and", "or", "to", "until", "-", "–", "—",
];

/// Find the clock times in `chars`:
/// - 12-hour: `H`, `H:MM` or `H.MM` with H 1–12, followed by `a.m.`/`p.m.` (`am`, `PM`, ...);
/// - 24-hour: `H:MM` or `H.MM` with H 0–23 and two-digit minutes, or a bare hour 0–23 after
///   `kl.`/`klockan` or joined to such a time by `och`, `–`, ...
///
/// A number that is part of a longer one (`2026-10-30`, `15:00:00`, `1.5.2`) is never a time.
pub(crate) fn spans(chars: &[char]) -> Vec<ClockSpan> {
    let digit_at = |i: usize| chars.get(i).is_some_and(char::is_ascii_digit);
    let mut out: Vec<ClockSpan> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if !digit_at(i) {
            i += 1;
            continue;
        }
        let start = i;
        while digit_at(i) {
            i += 1;
        }
        // Inside a longer number (`4.15`, `08-123`): the digits before it decide.
        let continues_number = start >= 2
            && matches!(chars[start - 1], '.' | ',' | ':')
            && chars[start - 2].is_ascii_digit();
        if continues_number || i - start > 2 {
            continue;
        }
        let hour = number(&chars[start..i]);
        let mut end = i;
        let mut minute = None;
        if matches!(chars.get(end), Some(':' | '.')) && digit_at(end + 1) && digit_at(end + 2) {
            minute = Some(number(&chars[end + 1..end + 3]));
            end += 3;
        }
        // A third digit, or another separator and digit (`15:00:00`, `1.5.2`, `14,30`).
        let longer =
            digit_at(end) || (matches!(chars.get(end), Some(':' | '.' | ',')) && digit_at(end + 1));
        if longer {
            i = end;
            continue;
        }
        let minute_ok = minute.is_none_or(|m| m < 60);
        if let Some((marker_end, pm)) = meridiem(chars, end)
            && (1..=12).contains(&hour)
            && minute_ok
        {
            out.push(ClockSpan {
                start,
                end: marker_end,
                hour: hour % 12 + if pm { 12 } else { 0 },
                minute: minute.unwrap_or(0),
                twelve_hour: true,
            });
            i = marker_end;
            continue;
        }
        let in_context = || {
            after_clock_word(chars, start)
                || out
                    .last()
                    .is_some_and(|prev| !prev.twelve_hour && joined(&chars[prev.end..start]))
        };
        if hour < 24 && minute_ok && (minute.is_some() || in_context()) {
            out.push(ClockSpan {
                start,
                end,
                hour,
                minute: minute.unwrap_or(0),
                twelve_hour: false,
            });
        }
        i = end;
    }
    out
}

fn number(digits: &[char]) -> u8 {
    digits
        .iter()
        .fold(0, |n, c| n * 10 + c.to_digit(10).unwrap_or(0) as u8)
}

/// `a.m.`/`p.m.` (any case, dots optional) after an optional space at `at`. Returns the index
/// after the marker and whether it is p.m.
fn meridiem(chars: &[char], at: usize) -> Option<(usize, bool)> {
    let lower = |i: usize| chars.get(i).map(char::to_ascii_lowercase);
    let mut i = at;
    if matches!(chars.get(i), Some(' ' | '\u{a0}' | '\u{202f}')) {
        i += 1;
    }
    let pm = match lower(i)? {
        'a' => false,
        'p' => true,
        _ => return None,
    };
    i += 1;
    if lower(i) == Some('.') {
        i += 1;
    }
    if lower(i) != Some('m') {
        return None;
    }
    i += 1;
    if lower(i) == Some('.') {
        i += 1;
    }
    // `8 amps`, `3 pmol` aren't times.
    if chars.get(i).is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    Some((i, pm))
}

/// The word before `start` (across whitespace) is `kl.` or `klockan`.
fn after_clock_word(chars: &[char], start: usize) -> bool {
    let mut end = start;
    while end > 0 && chars[end - 1].is_whitespace() {
        end -= 1;
    }
    let mut begin = end;
    while begin > 0 && (chars[begin - 1].is_alphabetic() || chars[begin - 1] == '.') {
        begin -= 1;
    }
    let word = chars[begin..end].iter().collect::<String>().to_lowercase();
    CLOCK_WORDS.contains(&word.as_str())
}

/// The text between two times joins them into a range or list.
fn joined(between: &[char]) -> bool {
    let between: String = between.iter().collect();
    let between = between.trim().to_lowercase();
    CONNECTORS.contains(&between.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// (hour, minute, 12-hour) of every time in `text`.
    fn times(text: &str) -> Vec<(u8, u8, bool)> {
        let chars: Vec<char> = text.chars().collect();
        spans(&chars)
            .into_iter()
            .map(|s| (s.hour, s.minute, s.twelve_hour))
            .collect()
    }

    #[test]
    fn twelve_hour_times() {
        assert_eq!(times("Check-in from 3 p.m."), [(15, 0, true)]);
        assert_eq!(times("at 6.30 p.m."), [(18, 30, true)]);
        assert_eq!(times("leaves at 7:45 a.m."), [(7, 45, true)]);
        assert_eq!(times("3:00 PM"), [(15, 0, true)]);
        assert_eq!(times("10pm"), [(22, 0, true)]);
        assert_eq!(times("12 a.m. and 12 p.m."), [(0, 0, true), (12, 0, true)]);
        assert_eq!(
            times("between 8 a.m. and 5 p.m."),
            [(8, 0, true), (17, 0, true)]
        );
    }

    #[test]
    fn twenty_four_hour_times() {
        assert_eq!(times("från kl. 15.00"), [(15, 0, false)]);
        assert_eq!(times("klockan 18.30"), [(18, 30, false)]);
        assert_eq!(times("Tåget går 07.45"), [(7, 45, false)]);
        assert_eq!(times("at 23:59"), [(23, 59, false)]);
        assert_eq!(times("Köket stänger kl. 22."), [(22, 0, false)]);
        assert_eq!(
            times("mellan kl. 8 och 17"),
            [(8, 0, false), (17, 0, false)]
        );
        assert_eq!(times("Kl 8–17"), [(8, 0, false), (17, 0, false)]);
    }

    #[test]
    fn other_numbers_are_not_times() {
        assert_eq!(times("Du har 2 försök kvar"), []);
        assert_eq!(times("2 attempts, 8 och 17"), []);
        assert_eq!(times("2026-10-30"), []);
        assert_eq!(times("15:00:00"), []);
        assert_eq!(times("version 4.2 och 1.5.2"), []);
        assert_eq!(times("12,50 kr"), []);
        assert_eq!(times("Pris 1 234,50"), []);
        assert_eq!(times("kl. 25"), []);
        assert_eq!(times("13 p.m., 8 amps"), []);
        assert_eq!(times("18.75"), []);
    }
}
