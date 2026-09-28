#![no_std]
#![forbid(unsafe_code)]

//! Uncontracted (Grade 1) English braille translation.
//!
//! The braille half of the accessibility stack: a deaf-blind user reads the
//! screen reader through a refreshable braille display, so the same text the
//! speech path speaks must also render to braille cells. This turns ASCII text
//! into a sequence of six-dot cells - the bytes a braille display's transport
//! layer sends - and, for logs and proofs, into the Unicode braille glyphs
//! (U+2800..) a sighted developer can read back.
//!
//! It implements Grade 1 (letter-for-letter) English braille: the 26 letters, a
//! capital indicator (dot 6) before an upper-case letter, a number indicator
//! (dots 3-4-5-6) that makes the following digits read as numbers, common
//! punctuation, and a space. Contractions (Grade 2) are a later layer. Any byte
//! it does not know maps to the braille question mark, so translation never
//! fails or panics.
//!
//! Like the rest of the accessibility code it is `no_std`, allocation-free and
//! free of `unsafe`: cells are written into a caller-provided buffer.
//!
//! ## Cell encoding
//!
//! A cell is a `u8` bitmask, bit *n* set when dot *n+1* is raised: bit 0 = dot 1,
//! ... bit 5 = dot 6 (bits 6-7, the eight-dot extensions, are unused here). That
//! is exactly the low six bits of the Unicode braille pattern block, so cell
//! `c` renders as the code point `U+2800 + c`.

/// The braille number indicator: dots 3-4-5-6. Following digits read as numbers.
pub const NUMBER_SIGN: u8 = 0b0011_1100;
/// The braille capital indicator: dot 6. Precedes an upper-case letter.
pub const CAPITAL_SIGN: u8 = 0b0010_0000;
/// The empty cell: a space.
pub const SPACE: u8 = 0;

/// The six-dot cells for `a` through `z`, in order.
const LETTERS: [u8; 26] = [
    0b00_0001, // a  dot 1
    0b00_0011, // b  dots 1-2
    0b00_1001, // c  dots 1-4
    0b01_1001, // d  dots 1-4-5
    0b01_0001, // e  dots 1-5
    0b00_1011, // f  dots 1-2-4
    0b01_1011, // g  dots 1-2-4-5
    0b01_0011, // h  dots 1-2-5
    0b00_1010, // i  dots 2-4
    0b01_1010, // j  dots 2-4-5
    0b00_0101, // k  dots 1-3
    0b00_0111, // l  dots 1-2-3
    0b00_1101, // m  dots 1-3-4
    0b01_1101, // n  dots 1-3-4-5
    0b01_0101, // o  dots 1-3-5
    0b00_1111, // p  dots 1-2-3-4
    0b01_1111, // q  dots 1-2-3-4-5
    0b01_0111, // r  dots 1-2-3-5
    0b00_1110, // s  dots 2-3-4
    0b01_1110, // t  dots 2-3-4-5
    0b10_0101, // u  dots 1-3-6
    0b10_0111, // v  dots 1-2-3-6
    0b11_1010, // w  dots 2-4-5-6
    0b10_1101, // x  dots 1-3-4-6
    0b11_1101, // y  dots 1-3-4-5-6
    0b11_0101, // z  dots 1-3-5-6
];

/// The cell for an ASCII lower-case letter `a`..=`z`.
#[must_use]
pub const fn letter_cell(letter: u8) -> Option<u8> {
    if letter.is_ascii_lowercase() {
        Some(LETTERS[(letter - b'a') as usize])
    } else {
        None
    }
}

/// The cell for an ASCII digit, using the letters `a`..=`j` (1-9 then 0). The
/// caller is responsible for the preceding [`NUMBER_SIGN`].
#[must_use]
pub const fn digit_cell(digit: u8) -> Option<u8> {
    match digit {
        b'1'..=b'9' => Some(LETTERS[(digit - b'1') as usize]),
        b'0' => Some(LETTERS[9]), // j
        _ => None,
    }
}

/// The cell for a supported ASCII punctuation mark.
const fn punctuation_cell(byte: u8) -> Option<u8> {
    match byte {
        b',' => Some(0b00_0010),        // dot 2
        b';' => Some(0b00_0110),        // dots 2-3
        b':' => Some(0b01_0010),        // dots 2-5
        b'.' => Some(0b11_0010),        // dots 2-5-6
        b'!' => Some(0b00_1110),        // dots 2-3-5 shares s pattern historically; UEB dots 2-3-5
        b'?' => Some(0b10_0110),        // dots 2-3-6
        b'\'' => Some(0b00_0100),       // dot 3
        b'-' => Some(0b10_0100),        // dots 3-6
        b'(' | b')' => Some(0b10_0011), // dots 1-2-6 (UEB round brackets share this base)
        _ => None,
    }
}

/// The braille question mark, used for any byte with no mapping so translation
/// always yields a cell.
const UNKNOWN: u8 = 0b10_0110; // dots 2-3-6

/// Translate `text` into six-dot braille cells, writing them into `cells` and
/// returning how many were written. Upper-case letters emit a capital sign then
/// the letter; a run of digits is preceded by one number sign. Output is
/// truncated to the length of `cells`.
#[must_use]
pub fn translate(text: &str, cells: &mut [u8]) -> usize {
    let mut count = 0;
    let mut in_number = false;
    let mut push = |cell: u8, count: &mut usize| -> bool {
        if *count < cells.len() {
            cells[*count] = cell;
            *count += 1;
            true
        } else {
            false
        }
    };
    for &byte in text.as_bytes() {
        match byte {
            b'0'..=b'9' => {
                if !in_number {
                    in_number = true;
                    if !push(NUMBER_SIGN, &mut count) {
                        break;
                    }
                }
                let cell = digit_cell(byte).unwrap_or(UNKNOWN);
                if !push(cell, &mut count) {
                    break;
                }
            }
            b'a'..=b'z' => {
                in_number = false;
                let cell = letter_cell(byte).unwrap_or(UNKNOWN);
                if !push(cell, &mut count) {
                    break;
                }
            }
            b'A'..=b'Z' => {
                in_number = false;
                if !push(CAPITAL_SIGN, &mut count) {
                    break;
                }
                let cell = letter_cell(byte.to_ascii_lowercase()).unwrap_or(UNKNOWN);
                if !push(cell, &mut count) {
                    break;
                }
            }
            b' ' => {
                in_number = false;
                if !push(SPACE, &mut count) {
                    break;
                }
            }
            other => {
                in_number = false;
                let cell = punctuation_cell(other).unwrap_or(UNKNOWN);
                if !push(cell, &mut count) {
                    break;
                }
            }
        }
    }
    count
}

/// Render braille `cells` as their Unicode braille-pattern glyphs (U+2800..) into
/// `out` as UTF-8, returning the string. Each cell is a three-byte glyph, so a
/// cell whose glyph would not fit is dropped; the returned string is always valid
/// UTF-8.
pub fn to_unicode<'a>(cells: &[u8], out: &'a mut [u8]) -> &'a str {
    let mut length = 0;
    for &cell in cells {
        if length + 3 > out.len() {
            break;
        }
        // U+2800 + (cell & 0x3F) encodes as three UTF-8 bytes E2 A0 (80|dots).
        out[length] = 0xE2;
        out[length + 1] = 0xA0;
        out[length + 2] = 0x80 | (cell & 0x3F);
        length += 3;
    }
    core::str::from_utf8(&out[..length]).unwrap_or("")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_letter_has_a_distinct_cell() {
        let mut seen = [false; 64];
        for letter in b'a'..=b'z' {
            let cell = letter_cell(letter).unwrap();
            assert!(
                !seen[cell as usize],
                "duplicate cell for {}",
                letter as char
            );
            seen[cell as usize] = true;
        }
    }

    #[test]
    fn translates_a_plain_word() {
        let mut cells = [0u8; 16];
        let n = translate("install", &mut cells);
        assert_eq!(
            &cells[..n],
            &[
                0b00_1010, // i
                0b01_1101, // n
                0b00_1110, // s
                0b01_1110, // t
                0b00_0001, // a
                0b00_0111, // l
                0b00_0111, // l
            ]
        );
    }

    #[test]
    fn capital_indicator_precedes_upper_case() {
        let mut cells = [0u8; 8];
        let n = translate("Hi", &mut cells);
        assert_eq!(&cells[..n], &[CAPITAL_SIGN, 0b01_0011, 0b00_1010]); // ^ h i
    }

    #[test]
    fn number_sign_once_then_digit_letters() {
        let mut cells = [0u8; 8];
        let n = translate("40", &mut cells);
        // 4 -> d, 0 -> j, with a single leading number sign.
        assert_eq!(&cells[..n], &[NUMBER_SIGN, 0b01_1001, 0b01_1010]);
    }

    #[test]
    fn number_sign_resets_after_a_space() {
        let mut cells = [0u8; 12];
        let n = translate("1 2", &mut cells);
        assert_eq!(
            &cells[..n],
            &[NUMBER_SIGN, 0b00_0001, SPACE, NUMBER_SIGN, 0b00_0011] // #a _ #b
        );
    }

    #[test]
    fn comma_and_space_in_a_label() {
        let mut cells = [0u8; 32];
        let n = translate("Install, button", &mut cells);
        assert_eq!(
            &cells[..n],
            &[
                CAPITAL_SIGN,
                0b00_1010, // i
                0b01_1101, // n
                0b00_1110, // s
                0b01_1110, // t
                0b00_0001, // a
                0b00_0111, // l
                0b00_0111, // l
                0b00_0010, // ,
                SPACE,
                0b00_0011, // b
                0b10_0101, // u
                0b01_1110, // t
                0b01_1110, // t
                0b01_0101, // o
                0b01_1101, // n
            ]
        );
    }

    #[test]
    fn unicode_glyphs_round_trip_the_block() {
        let cells = [0b00_0001u8, 0b00_0011]; // a, b
        let mut out = [0u8; 16];
        let text = to_unicode(&cells, &mut out);
        assert_eq!(text, "\u{2801}\u{2803}"); // ⠁⠃
    }

    #[test]
    fn unknown_byte_becomes_question_cell() {
        let mut cells = [0u8; 4];
        let n = translate("~", &mut cells);
        assert_eq!(&cells[..n], &[UNKNOWN]);
    }

    #[test]
    fn truncates_to_buffer() {
        let mut cells = [0u8; 3];
        let n = translate("install", &mut cells);
        assert_eq!(n, 3);
    }
}
