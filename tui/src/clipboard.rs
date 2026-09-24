//! Copying to the clipboard through the terminal (OSC 52).
//!
//! When the TUI runs over SSH or inside tmux, the system clipboard `arboard`
//! can reach belongs to the wrong machine, if there is one at all. Terminals
//! that support OSC 52 copy the base64 payload of `ESC ] 52 ; c ; <data> BEL`
//! into the clipboard of the machine the user is sitting at. tmux passes it
//! on when its `set-clipboard` option is `on`.

use std::io::Write as _;

/// Write `text` to the terminal's clipboard as an OSC 52 sequence.
///
/// The TUI draws on stderr, so the sequence goes there too, in one write so
/// it cannot interleave with a frame.
///
/// # Errors
///
/// Returns the I/O error when stderr cannot be written.
pub fn write_osc52(text: &str) -> std::io::Result<()> {
    let sequence = osc52_sequence(text);
    let mut stderr = std::io::stderr().lock();
    stderr.write_all(sequence.as_bytes())?;
    stderr.flush()
}

/// The OSC 52 "set clipboard" sequence carrying `text`.
fn osc52_sequence(text: &str) -> String {
    format!("\x1b]52;c;{}\x07", base64(text.as_bytes()))
}

/// Standard base64 (RFC 4648, padded). A dozen lines here rather than a
/// dependency for the one sequence that needs it.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0_u32, |group, (i, &byte)| group | u32::from(byte) << (16 - 8 * i));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[(group >> (18 - 6 * i) & 0x3f) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc_4648() {
        // The RFC 4648 test vectors.
        for (input, expected) in
            [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]
        {
            assert_eq!(base64(input.as_bytes()), expected, "base64({input:?})");
        }
        assert_eq!(base64(&[0xff, 0xfe, 0xfd]), "//79");
    }

    #[test]
    fn osc52_wraps_the_payload() {
        assert_eq!(osc52_sequence("x1"), "\x1b]52;c;eDE=\x07");
    }
}
