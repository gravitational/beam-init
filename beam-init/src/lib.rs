#![deny(clippy::unwrap_used)]

pub const BOOTSTRAP_NAME: &str = "bootstrap";

pub mod system;

/// Convert the input to UTF-8 and replace Control codes
pub fn sanitize(line: Vec<u8>, allow_whitespace: bool) -> String {
    // Micro-optimization: convert the input without an additional allocation if not absolutely necessary,
    // which it typically won't be.
    // NOTE: If https://doc.rust-lang.org/stable/std/string/struct.String.html#method.from_utf8_lossy_owned gets
    // stabilized, that could replace this construction.
    let line = String::from_utf8(line)
        .unwrap_or_else(|err| String::from_utf8_lossy(&err.into_bytes()).into_owned());

    let is_filtered_control = if allow_whitespace {
        |ch: char| ch.is_control() && !ch.is_whitespace()
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
        let hello = b"hello".to_vec();
        let p = hello.as_ptr();
        let hello = sanitize(hello, true);
        let q = hello.as_ptr();
        assert_eq!(p, q);

        assert_eq!(sanitize(b"hello".to_vec(), true), "hello");
        assert_eq!(sanitize(b"he\tllo".to_vec(), true), "he\tllo");
        assert_eq!(sanitize(b"he\nllo".to_vec(), true), "he\nllo");
        assert_eq!(sanitize(b"he\rllo".to_vec(), true), "he\u{240D}llo");
        assert_eq!(sanitize(b"he\n\x7Fllo".to_vec(), true), "he\n⍰llo");
        assert_eq!(sanitize(b"he\t\rllo".to_vec(), true), "he\t\u{240D}llo");
        assert_eq!(sanitize(b"he\x7Fllo".to_vec(), true), "he⍰llo");
        assert_eq!(sanitize(b"he\xA0llo".to_vec(), true), "he\u{FFFD}llo");
    }
}
