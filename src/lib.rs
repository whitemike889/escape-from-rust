#![allow(unused)]
//! Utilities for validating  string and char literals and turning them into
//! values they represent.

use std::str::Chars;
use std::ops::Range;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum EscapeError {
    ZeroChars,
    MoreThanOneChar,

    LoneSlash,
    InvalidEscape,
    BareCarriageReturn,
    EscapeOnlyChar,

    TooShortHexEscape,
    InvalidCharInHexEscape,
    OutOfRangeHexEscape,

    InvalidUnicodeEscape,
    InvalidCharInUnicodeEscape,
    EmptyUnicodeEscape,
    UnclosedUnicodeEscape,
    LeadingUnderscoreUnicodeEscape,
    OverlongUnicodeEscape,
    LoneSurrogateUnicodeEscape,
    OutOfRangeUnicodeEscape,

    UnicodeEscapeInByte,
    NonAsciiCharInByte,
}

/// Takes a contents of a char literal (without quotes), and returns an
/// unescaped char or an error
pub(crate) fn unescape_char(literal_text: &str) -> Result<char, (usize, EscapeError)> {
    let mut chars = literal_text.chars();
    let res = (|| {
        let first_char = chars.next().ok_or(EscapeError::ZeroChars)?;
        let res = scan_escape(first_char, &mut chars, Mode::Char)?;
        if chars.next().is_some() {
            return Err(EscapeError::MoreThanOneChar);
        }
        Ok(res)
    })();
    res.map_err(|err| (literal_text.len() - chars.as_str().len(), err))
}

/// Takes a contents of a string literal (without quotes) and produces a
/// sequence of escaped characters or errors.
pub(crate) fn unescape_str<F>(literal_text: &str, callback: &mut F)
where
    F: FnMut(Range<usize>, Result<char, EscapeError>),
{
    unescape_str_or_byte_str(literal_text, Mode::Str, callback)
}

pub(crate) fn unescape_byte(literal_text: &str) -> Result<u8, (usize, EscapeError)> {
    let mut chars = literal_text.chars();
    let res = (|| {
        let first_char = chars.next().ok_or(EscapeError::ZeroChars)?;
        let res = scan_escape(first_char, &mut chars, Mode::Byte)?;
        if chars.next().is_some() {
            return Err(EscapeError::MoreThanOneChar);
        }
        Ok(byte_from_char(res))
    })();
    res.map_err(|err| (literal_text.len() - chars.as_str().len(), err))
}

/// Takes a contents of a string literal (without quotes) and produces a
/// sequence of escaped characters or errors.
pub(crate) fn unescape_byte_str<F>(literal_text: &str, callback: &mut F)
where
    F: FnMut(Range<usize>, Result<u8, EscapeError>),
{
    unescape_str_or_byte_str(literal_text, Mode::Str, &mut |range, char| {
        callback(range, char.map(byte_from_char))
    })
}

fn byte_from_char(c: char) -> u8 {
    let res = c as u32;
    assert!(res <= u8::max_value() as u32, "guaranteed because of Mode::Byte");
    res as u8
}

#[derive(Clone, Copy)]
enum Mode {
    Char,
    Str,
    Byte,
    #[allow(unused)]
    ByteStr,
}

impl Mode {
    fn in_single_quotes(self) -> bool {
        match self {
            Mode::Char | Mode::Byte => true,
            Mode::Str | Mode::ByteStr => false,
        }
    }

    fn in_double_quotes(self) -> bool {
        !self.in_single_quotes()
    }

    fn allow_unicode(self) -> bool {
        match self {
            Mode::Byte | Mode::ByteStr => false,
            Mode::Char | Mode::Str => true,
        }
    }
}

fn scan_escape(first_char: char, chars: &mut Chars<'_>, mode: Mode) -> Result<char, EscapeError> {
    if first_char != '\\' {
        return match first_char {
            '\t' | '\n' => Err(EscapeError::EscapeOnlyChar),
            '\r' => Err(if chars.clone().next() == Some('\n') {
                EscapeError::EscapeOnlyChar
            } else {
                EscapeError::BareCarriageReturn
            }),
            '\'' if mode.in_single_quotes() => Err(EscapeError::EscapeOnlyChar),
            '"' if mode.in_double_quotes() => Err(EscapeError::EscapeOnlyChar),
            _ => {
                if !mode.allow_unicode() && (first_char as u32) > 0x7F {
                    return Err(EscapeError::NonAsciiCharInByte);
                }
                Ok(first_char)
            }
        };
    }

    let second_char = chars.next().ok_or(EscapeError::LoneSlash)?;

    let res = match second_char {
        '"' => '"',
        'n' => '\n',
        'r' => '\r',
        't' => '\t',
        '\\' => '\\',
        '\'' => '\'',
        '0' => '\0',

        'x' => {
            let hi = chars.next().ok_or(EscapeError::TooShortHexEscape)?;
            let hi = hi.to_digit(16).ok_or(EscapeError::InvalidCharInHexEscape)?;

            let lo = chars.next().ok_or(EscapeError::TooShortHexEscape)?;
            let lo = lo.to_digit(16).ok_or(EscapeError::InvalidCharInHexEscape)?;

            let value = hi.checked_mul(16).unwrap().checked_add(lo).unwrap();

            if value > 0x7F {
                return Err(EscapeError::OutOfRangeHexEscape);
            }
            let value = value as u8;

            value as char
        }

        'u' => {
            if chars.next() != Some('{') {
                return Err(EscapeError::InvalidUnicodeEscape);
            }

            let mut n_digits = 1;
            let mut value: u32 = match chars.next().ok_or(EscapeError::UnclosedUnicodeEscape)? {
                '_' => return Err(EscapeError::LeadingUnderscoreUnicodeEscape),
                '}' => return Err(EscapeError::EmptyUnicodeEscape),
                c => c.to_digit(16).ok_or(EscapeError::InvalidCharInUnicodeEscape)?,
            };

            loop {
                match chars.next() {
                    None => return Err(EscapeError::UnclosedUnicodeEscape),
                    Some('_') => continue,
                    Some('}') => {
                        if !mode.allow_unicode() {
                            return Err(EscapeError::UnicodeEscapeInByte);
                        }
                        break std::char::from_u32(value).ok_or_else(|| {
                            if value > 0x10FFFF {
                                EscapeError::OutOfRangeUnicodeEscape
                            } else {
                                EscapeError::LoneSurrogateUnicodeEscape
                            }
                        })?;
                    }
                    Some(c) => {
                        let digit = c.to_digit(16).ok_or(EscapeError::InvalidCharInUnicodeEscape)?;
                        n_digits += 1;
                        if n_digits > 6 {
                            return Err(EscapeError::OverlongUnicodeEscape);
                        }

                        let digit = digit as u32;
                        value = value.checked_mul(16).unwrap().checked_add(digit).unwrap();
                    }
                };
            }
        }
        _ => return Err(EscapeError::InvalidEscape),
    };
    Ok(res)
}

/// Takes a contents of a string literal (without quotes) and produces a
/// sequence of escaped characters or errors.
fn unescape_str_or_byte_str<F>(src: &str, mode: Mode, callback: &mut F)
where
    F: FnMut(Range<usize>, Result<char, EscapeError>),
{
    let initial_len = src.len();
    let mut chars = src.chars();
    while let Some(first_char) = chars.next() {
        let start = initial_len - chars.as_str().len() - first_char.len_utf8();

        let escaped_char = match first_char {
            '\\' => {
                let (second_char, third_char) = {
                    let mut chars = chars.clone();
                    (chars.next(), chars.next())
                };
                match (second_char, third_char) {
                    (Some('\n'), _) | (Some('\r'), Some('\n')) => {
                        skip_ascii_whitespace(&mut chars);
                        continue;
                    }
                    _ => scan_escape(first_char, &mut chars, mode),
                }
            }
            '\n' => Ok('\n'),
            '\r' => {
                let second_char = chars.clone().next();
                if second_char == Some('\n') {
                    chars.next();
                    Ok('\n')
                } else {
                    scan_escape(first_char, &mut chars, mode)
                }
            }
            _ => scan_escape(first_char, &mut chars, mode),
        };
        let end = initial_len - chars.as_str().len();
        callback(start..end, escaped_char);
    }

    fn skip_ascii_whitespace(chars: &mut Chars<'_>) {
        let str = chars.as_str();
        let first_non_space = str
            .bytes()
            .position(|b| b != b' ' && b != b'\t' && b != b'\n' && b != b'\r')
            .unwrap_or(str.len());
        *chars = str[first_non_space..].chars()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unescape_char_bad() {
        fn check(literal_text: &str, expected_error: EscapeError) {
            let actual_result = unescape_char(literal_text).map_err(|(_offset, err)| err);
            assert_eq!(actual_result, Err(expected_error));
        }

        check("", EscapeError::ZeroChars);
        check(r"\", EscapeError::LoneSlash);

        check("\n", EscapeError::EscapeOnlyChar);
        check("\r\n", EscapeError::EscapeOnlyChar);
        check("\t", EscapeError::EscapeOnlyChar);
        check("'", EscapeError::EscapeOnlyChar);
        check("\r", EscapeError::BareCarriageReturn);

        check("spam", EscapeError::MoreThanOneChar);
        check(r"\x0ff", EscapeError::MoreThanOneChar);
        check(r#"\"a"#, EscapeError::MoreThanOneChar);
        check(r"\na", EscapeError::MoreThanOneChar);
        check(r"\ra", EscapeError::MoreThanOneChar);
        check(r"\ta", EscapeError::MoreThanOneChar);
        check(r"\\a", EscapeError::MoreThanOneChar);
        check(r"\'a", EscapeError::MoreThanOneChar);
        check(r"\0a", EscapeError::MoreThanOneChar);
        check(r"\u{0}x", EscapeError::MoreThanOneChar);
        check(r"\u{1F63b}}", EscapeError::MoreThanOneChar);

        check(r"\v", EscapeError::InvalidEscape);
        check(r"\💩", EscapeError::InvalidEscape);
        check(r"\●", EscapeError::InvalidEscape);

        check(r"\x", EscapeError::TooShortHexEscape);
        check(r"\x0", EscapeError::TooShortHexEscape);
        check(r"\xf", EscapeError::TooShortHexEscape);
        check(r"\xa", EscapeError::TooShortHexEscape);
        check(r"\xx", EscapeError::InvalidCharInHexEscape);
        check(r"\xы", EscapeError::InvalidCharInHexEscape);
        check(r"\x🦀", EscapeError::InvalidCharInHexEscape);
        check(r"\xtt", EscapeError::InvalidCharInHexEscape);
        check(r"\xff", EscapeError::OutOfRangeHexEscape);
        check(r"\xFF", EscapeError::OutOfRangeHexEscape);
        check(r"\x80", EscapeError::OutOfRangeHexEscape);

        check(r"\u", EscapeError::InvalidUnicodeEscape);
        check(r"\u[0123]", EscapeError::InvalidUnicodeEscape);
        check(r"\u{0x}", EscapeError::InvalidCharInUnicodeEscape);
        check(r"\u{", EscapeError::UnclosedUnicodeEscape);
        check(r"\u{0000", EscapeError::UnclosedUnicodeEscape);
        check(r"\u{}", EscapeError::EmptyUnicodeEscape);
        check(r"\u{_0000}", EscapeError::LeadingUnderscoreUnicodeEscape);
        check(r"\u{0000000}", EscapeError::OverlongUnicodeEscape);
        check(r"\u{FFFFFF}", EscapeError::OutOfRangeUnicodeEscape);
        check(r"\u{ffffff}", EscapeError::OutOfRangeUnicodeEscape);
        check(r"\u{ffffff}", EscapeError::OutOfRangeUnicodeEscape);

        check(r"\u{DC00}", EscapeError::LoneSurrogateUnicodeEscape);
        check(r"\u{DDDD}", EscapeError::LoneSurrogateUnicodeEscape);
        check(r"\u{DFFF}", EscapeError::LoneSurrogateUnicodeEscape);

        check(r"\u{D800}", EscapeError::LoneSurrogateUnicodeEscape);
        check(r"\u{DAAA}", EscapeError::LoneSurrogateUnicodeEscape);
        check(r"\u{DBFF}", EscapeError::LoneSurrogateUnicodeEscape);
    }

    #[test]
    fn test_unescape_char_good() {
        fn check(literal_text: &str, expected_char: char) {
            let actual_result = unescape_char(literal_text);
            assert_eq!(actual_result, Ok(expected_char));
        }

        check("a", 'a');
        check("ы", 'ы');
        check("🦀", '🦀');

        check(r#"\""#, '"');
        check(r"\n", '\n');
        check(r"\r", '\r');
        check(r"\t", '\t');
        check(r"\\", '\\');
        check(r"\'", '\'');
        check(r"\0", '\0');

        check(r"\x00", '\0');
        check(r"\x5a", 'Z');
        check(r"\x5A", 'Z');
        check(r"\x7f", 127 as char);

        check(r"\u{0}", '\0');
        check(r"\u{000000}", '\0');
        check(r"\u{41}", 'A');
        check(r"\u{0041}", 'A');
        check(r"\u{00_41}", 'A');
        check(r"\u{4__1__}", 'A');
        check(r"\u{1F63b}", '😻');
    }

    #[test]
    fn test_unescape_str_good() {
        fn check(literal_text: &str, expected: &str) {
            let mut buf = Ok(String::with_capacity(literal_text.len()));
            unescape_str(literal_text, &mut |range, c| {
                if let Ok(b) = &mut buf {
                    match c {
                        Ok(c) => b.push(c),
                        Err(e) => buf = Err((range, e)),
                    }
                }
            });
            let buf = buf.as_ref().map(|it| it.as_ref());
            assert_eq!(buf, Ok(expected))
        }

        check("foo", "foo");
        check("", "");
        check(" \n\r\n", " \n\n");

        check("hello \\\n     world", "hello world");
        check("hello \\\r\n     world", "hello world");
        check("thread's", "thread's")
    }

    #[test]
    fn test_unescape_byte_bad() {
        fn check(literal_text: &str, expected_error: EscapeError) {
            let actual_result = unescape_byte(literal_text).map_err(|(_offset, err)| err);
            assert_eq!(actual_result, Err(expected_error));
        }

        check("", EscapeError::ZeroChars);
        check(r"\", EscapeError::LoneSlash);

        check("\n", EscapeError::EscapeOnlyChar);
        check("\r\n", EscapeError::EscapeOnlyChar);
        check("\t", EscapeError::EscapeOnlyChar);
        check("'", EscapeError::EscapeOnlyChar);
        check("\r", EscapeError::BareCarriageReturn);

        check("spam", EscapeError::MoreThanOneChar);
        check(r"\x0ff", EscapeError::MoreThanOneChar);
        check(r#"\"a"#, EscapeError::MoreThanOneChar);
        check(r"\na", EscapeError::MoreThanOneChar);
        check(r"\ra", EscapeError::MoreThanOneChar);
        check(r"\ta", EscapeError::MoreThanOneChar);
        check(r"\\a", EscapeError::MoreThanOneChar);
        check(r"\'a", EscapeError::MoreThanOneChar);
        check(r"\0a", EscapeError::MoreThanOneChar);

        check(r"\v", EscapeError::InvalidEscape);
        check(r"\💩", EscapeError::InvalidEscape);
        check(r"\●", EscapeError::InvalidEscape);

        check(r"\x", EscapeError::TooShortHexEscape);
        check(r"\x0", EscapeError::TooShortHexEscape);
        check(r"\xa", EscapeError::TooShortHexEscape);
        check(r"\xf", EscapeError::TooShortHexEscape);
        check(r"\xx", EscapeError::InvalidCharInHexEscape);
        check(r"\xы", EscapeError::InvalidCharInHexEscape);
        check(r"\x🦀", EscapeError::InvalidCharInHexEscape);
        check(r"\xtt", EscapeError::InvalidCharInHexEscape);
        check(r"\xff", EscapeError::OutOfRangeHexEscape);
        check(r"\xFF", EscapeError::OutOfRangeHexEscape);
        check(r"\x80", EscapeError::OutOfRangeHexEscape);

        check(r"\u", EscapeError::InvalidUnicodeEscape);
        check(r"\u[0123]", EscapeError::InvalidUnicodeEscape);
        check(r"\u{0x}", EscapeError::InvalidCharInUnicodeEscape);
        check(r"\u{", EscapeError::UnclosedUnicodeEscape);
        check(r"\u{0000", EscapeError::UnclosedUnicodeEscape);
        check(r"\u{}", EscapeError::EmptyUnicodeEscape);
        check(r"\u{_0000}", EscapeError::LeadingUnderscoreUnicodeEscape);
        check(r"\u{0000000}", EscapeError::OverlongUnicodeEscape);

        check("ы", EscapeError::NonAsciiCharInByte);
        check("🦀", EscapeError::NonAsciiCharInByte);

        check(r"\u{0}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{000000}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{41}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{0041}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{00_41}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{4__1__}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{1F63b}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{0}x", EscapeError::UnicodeEscapeInByte);
        check(r"\u{1F63b}}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{FFFFFF}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{ffffff}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{ffffff}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{DC00}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{DDDD}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{DFFF}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{D800}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{DAAA}", EscapeError::UnicodeEscapeInByte);
        check(r"\u{DBFF}", EscapeError::UnicodeEscapeInByte);
    }

    #[test]
    fn test_unescape_byte_good() {
        fn check(literal_text: &str, expected_byte: u8) {
            let actual_result = unescape_byte(literal_text);
            assert_eq!(actual_result, Ok(expected_byte));
        }

        check("a", b'a');

        check(r#"\""#, b'"');
        check(r"\n", b'\n');
        check(r"\r", b'\r');
        check(r"\t", b'\t');
        check(r"\\", b'\\');
        check(r"\'", b'\'');
        check(r"\0", b'\0');

        check(r"\x00", b'\0');
        check(r"\x5a", b'Z');
        check(r"\x5A", b'Z');
        check(r"\x7f", 127);
    }

    #[test]
    fn test_unescape_byte_str_good() {
        fn check(literal_text: &str, expected: &[u8]) {
            let mut buf = Ok(Vec::with_capacity(literal_text.len()));
            unescape_byte_str(literal_text, &mut |range, c| {
                if let Ok(b) = &mut buf {
                    match c {
                        Ok(c) => b.push(c),
                        Err(e) => buf = Err((range, e)),
                    }
                }
            });
            let buf = buf.as_ref().map(|it| it.as_ref());
            assert_eq!(buf, Ok(expected))
        }

        check("foo", b"foo");
        check("", b"");
        check(" \n\r\n", b" \n\n");

        check("hello \\\n     world", b"hello world");
        check("hello \\\r\n     world", b"hello world");
        check("thread's", b"thread's")
    }
}
