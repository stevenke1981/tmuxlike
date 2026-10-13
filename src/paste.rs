//! Text pasted by a user, separate from deliberate send-keys commands.

pub fn normalize(text: &str) -> String {
    text.replace("\r\n", "\n")
        .replace(['\r', '\u{2028}', '\u{2029}'], "\n")
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .collect()
}

pub fn needs_confirmation(text: &str) -> bool {
    text.contains('\n')
}

pub fn encode(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        format!("\x1b[200~{text}\x1b[201~").into_bytes()
    } else {
        text.replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiline_including_trailing_newline_requires_consent() {
        assert!(!needs_confirmation("echo hello"));
        assert!(needs_confirmation(&normalize("echo one\r\necho two")));
        assert!(needs_confirmation(&normalize("echo one\r\n")));
        assert!(needs_confirmation(&normalize("one\u{2028}two")));
    }

    #[test]
    fn strips_terminal_control_bytes_and_preserves_unicode() {
        assert_eq!(
            normalize("繁體中文 日本語 😀 e\u{301}\t\r\nnext"),
            "繁體中文 日本語 😀 e\u{301}\t\nnext"
        );
        assert_eq!(normalize("a\x00\x03\x1b\x7f\u{009b}b"), "ab");
        assert!(
            !encode(&normalize("\x1b[201~echo x"), true)
                .windows(6)
                .take(10)
                .any(|w| w == b"\x1b[201~")
        );
    }

    #[test]
    fn bracketed_paste_uses_lf_and_plain_paste_uses_cr() {
        let text = normalize("one\r\ntwo\rthree");
        assert_eq!(encode(&text, true), b"\x1b[200~one\ntwo\nthree\x1b[201~");
        assert_eq!(encode(&text, false), b"one\rtwo\rthree");
    }
}
