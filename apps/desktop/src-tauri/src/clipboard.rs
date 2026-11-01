//! Copy the lens's text to the system clipboard (REN-05).

/// Put `text` on the clipboard as Unicode text. Errors carry technical context only, never the
/// text (PRIV-07).
pub fn copy(text: &str) -> Result<(), String> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(|e| format!("clipboard: {e}"))
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// REN-05: å ä ö (and other non-ASCII text) survive the round trip through the clipboard.
    /// Uses the real clipboard and puts back the text that was on it.
    #[test]
    fn ren05_clipboard_round_trips_swedish_text() {
        let mut clipboard = arboard::Clipboard::new().unwrap();
        let before = clipboard.get_text().ok();
        let text = "Spara ändringar?\nGöteborg, Malmö, Västerås – ÅÄÖ åäö é ü €";
        copy(text).unwrap();
        let got = clipboard.get_text();
        if let Some(b) = before {
            let _ = clipboard.set_text(b);
        }
        assert_eq!(got.unwrap(), text);
    }
}
