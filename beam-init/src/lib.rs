#![deny(clippy::unwrap_used)]

pub const BOOTSTRAP_NAME: &str = "bootstrap";

pub mod system;

/// Convert the input to UTF-8 and replace Control codes
pub fn sanitize(line: String, allow_whitespace: bool) -> String {
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
    use super::sanitize;

    #[test]
    fn log_sanitizer() {
        let hello = "hello".to_string();
        let p = hello.as_ptr();
        let hello = sanitize(hello, true);
        let q = hello.as_ptr();
        assert_eq!(p, q);

        assert_eq!(sanitize("hello".to_string(), true), "hello");
        assert_eq!(sanitize("he\tllo".to_string(), true), "he\tllo");
        assert_eq!(sanitize("he\nllo".to_string(), true), "he\nllo");
        assert_eq!(sanitize("he\rllo".to_string(), true), "he\u{240D}llo");
        assert_eq!(sanitize("he\n\x7Fllo".to_string(), true), "he\n⍰llo");
        assert_eq!(sanitize("he\t\rllo".to_string(), true), "he\t\u{240D}llo");
        assert_eq!(sanitize("he\x7Fllo".to_string(), true), "he⍰llo");
    }
}
