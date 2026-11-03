#![deny(clippy::unwrap_used)]

pub const BOOTSTRAP_NAME: &str = "bootstrap";

pub mod system;

/// Replace control codes and whitespace with Unicode pictures
pub fn sanitize(line: String) -> String {
    sanitize_impl(line, false)
}

/// Replace control codes only with Unicode pictures
pub fn sanitize_with_whitespace(line: String) -> String {
    sanitize_impl(line, true)
}

fn sanitize_impl(line: String, allow_whitespace: bool) -> String {
    let is_filtered_control = if allow_whitespace {
        |ch: char| ch.is_control() && !"\t\n".contains(ch)
    } else {
        |ch: char| ch.is_control() || ch.is_whitespace()
    };

    if !line.chars().any(is_filtered_control) {
        return line;
    }

    // Replace control characters (with the exception of TAB and newline)
    line.chars()
        .map(|ch| {
            if !is_filtered_control(ch) {
                ch
            } else if ch <= '\u{20}' {
                // This is a CC0 control code, replace it with the corresponding Control Picture,
                // which is U+2400 + the ASCII code; i.e. '\a' (U+0007) => '␇' (U+2407).
                //
                // PANIC: It is not possible for this conversion to fail
                char::from_u32(u32::from(ch) + 0x2400).expect("conversion can't fail")
            } else {
                // For CC1 control codes, pictures don't exist so replace with a more generic
                // replacement.
                '⍰'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #[test]
    fn sanitizer() {
        let sanitize = super::sanitize_with_whitespace;

        let hello = "hello".to_string();
        let p = hello.as_ptr();
        let hello = sanitize(hello);
        let q = hello.as_ptr();
        assert_eq!(p, q);

        assert_eq!(sanitize("hello".to_string()), "hello");
        assert_eq!(sanitize("he\tllo".to_string()), "he\tllo");
        assert_eq!(sanitize("he\nllo".to_string()), "he\nllo");
        assert_eq!(sanitize("he\rllo".to_string()), "he\u{240D}llo");
        assert_eq!(sanitize("he\n\x7Fllo".to_string()), "he\n⍰llo");
        assert_eq!(sanitize("he\x7Fllo".to_string()), "he⍰llo");
        assert_eq!(sanitize("he\t\r\n llo".to_string()), "he\t\u{240D}\n llo");
        assert_eq!(
            super::sanitize("he\t\r\n llo".to_string()),
            "he\u{2409}\u{240D}\u{240A}\u{2420}llo"
        );
    }
}
