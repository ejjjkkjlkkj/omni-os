#![no_std]

//! Minimal USB HID Boot Keyboard decoding shared by the kernel USB stack.
//!
//! This crate deliberately contains no MMIO, DMA, PCI or xHCI code. It accepts
//! the architected 8-byte HID boot-keyboard input report and turns report-state
//! transitions into key press/release events. Keeping this logic pure makes the
//! user-input semantics host-testable before the kernel transport is attached.

/// Size of a HID Boot Keyboard input report.
pub const BOOT_REPORT_LEN: usize = 8;
/// Maximum number of events one report transition can generate:
/// six released keys plus six newly pressed keys.
pub const MAX_EVENTS_PER_REPORT: usize = 12;

const USAGE_ERROR_ROLLOVER: u8 = 0x01;
const USAGE_POST_FAIL: u8 = 0x02;
const USAGE_ERROR_UNDEFINED: u8 = 0x03;

/// Modifier bitmap from byte 0 of the boot-keyboard report.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Modifiers(u8);

impl Modifiers {
    pub const LEFT_CTRL: u8 = 1 << 0;
    pub const LEFT_SHIFT: u8 = 1 << 1;
    pub const LEFT_ALT: u8 = 1 << 2;
    pub const LEFT_GUI: u8 = 1 << 3;
    pub const RIGHT_CTRL: u8 = 1 << 4;
    pub const RIGHT_SHIFT: u8 = 1 << 5;
    pub const RIGHT_ALT: u8 = 1 << 6;
    pub const RIGHT_GUI: u8 = 1 << 7;

    #[must_use]
    pub const fn from_bits(bits: u8) -> Self {
        Self(bits)
    }

    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    #[must_use]
    pub const fn shift(self) -> bool {
        self.0 & (Self::LEFT_SHIFT | Self::RIGHT_SHIFT) != 0
    }

    #[must_use]
    pub const fn ctrl(self) -> bool {
        self.0 & (Self::LEFT_CTRL | Self::RIGHT_CTRL) != 0
    }

    #[must_use]
    pub const fn alt(self) -> bool {
        self.0 & (Self::LEFT_ALT | Self::RIGHT_ALT) != 0
    }

    #[must_use]
    pub const fn gui(self) -> bool {
        self.0 & (Self::LEFT_GUI | Self::RIGHT_GUI) != 0
    }
}

/// Logical keys needed by the accessible boot menu, plus common printable keys.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Key {
    Up,
    Down,
    Left,
    Right,
    Enter,
    Escape,
    Tab,
    Backspace,
    Space,
    Letter(u8),
    Digit(u8),
    Other(u8),
}

impl Key {
    /// Convert a printable key to ASCII using the current Shift state.
    #[must_use]
    pub fn ascii(self, modifiers: Modifiers) -> Option<u8> {
        match self {
            Self::Letter(letter) => Some(if modifiers.shift() {
                letter.to_ascii_uppercase()
            } else {
                letter
            }),
            Self::Digit(digit) => {
                if modifiers.shift() {
                    Some(match digit {
                        b'1' => b'!',
                        b'2' => b'@',
                        b'3' => b'#',
                        b'4' => b'$',
                        b'5' => b'%',
                        b'6' => b'^',
                        b'7' => b'&',
                        b'8' => b'*',
                        b'9' => b'(',
                        b'0' => b')',
                        other => other,
                    })
                } else {
                    Some(digit)
                }
            }
            Self::Space => Some(b' '),
            _ => None,
        }
    }
}

/// One transition between two successive boot reports.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct KeyEvent {
    pub key: Key,
    pub pressed: bool,
    pub modifiers: Modifiers,
}

/// Invalid report or caller buffer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DecodeError {
    WrongLength,
    Rollover,
    OutputTooSmall,
}

/// Stateful decoder. HID boot reports describe the complete set of currently
/// held keys; events are therefore produced by diffing each report against the
/// previous one.
#[derive(Clone, Copy, Debug, Default)]
pub struct BootKeyboardDecoder {
    previous: [u8; 6],
    previous_modifiers: Modifiers,
}

impl BootKeyboardDecoder {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            previous: [0; 6],
            previous_modifiers: Modifiers::from_bits(0),
        }
    }

    /// Decode one 8-byte boot-keyboard report into transition events.
    ///
    /// Rollover/error reports are rejected without changing decoder state.
    pub fn decode(&mut self, report: &[u8], out: &mut [KeyEvent]) -> Result<usize, DecodeError> {
        if report.len() != BOOT_REPORT_LEN {
            return Err(DecodeError::WrongLength);
        }

        let current = [
            report[2], report[3], report[4], report[5], report[6], report[7],
        ];
        if current.iter().copied().any(is_error_usage) {
            return Err(DecodeError::Rollover);
        }

        let current_modifiers = Modifiers::from_bits(report[0]);
        let released = self
            .previous
            .iter()
            .copied()
            .filter(|usage| *usage != 0 && !contains_usage(&current, *usage))
            .count();
        let pressed = current
            .iter()
            .copied()
            .filter(|usage| *usage != 0 && !contains_usage(&self.previous, *usage))
            .count();
        let required = released + pressed;
        if out.len() < required {
            return Err(DecodeError::OutputTooSmall);
        }

        let mut written = 0;
        for usage in self.previous.iter().copied() {
            if usage != 0 && !contains_usage(&current, usage) {
                out[written] = KeyEvent {
                    key: key_from_usage(usage),
                    pressed: false,
                    modifiers: self.previous_modifiers,
                };
                written += 1;
            }
        }

        for usage in current.iter().copied() {
            if usage != 0 && !contains_usage(&self.previous, usage) {
                out[written] = KeyEvent {
                    key: key_from_usage(usage),
                    pressed: true,
                    modifiers: current_modifiers,
                };
                written += 1;
            }
        }

        self.previous = current;
        self.previous_modifiers = current_modifiers;
        Ok(written)
    }

    /// Clear remembered key state, e.g. after USB device disconnect/reconnect.
    pub fn reset(&mut self) {
        self.previous = [0; 6];
        self.previous_modifiers = Modifiers::from_bits(0);
    }
}

fn contains_usage(usages: &[u8; 6], usage: u8) -> bool {
    usages.contains(&usage)
}

fn is_error_usage(usage: u8) -> bool {
    matches!(
        usage,
        USAGE_ERROR_ROLLOVER | USAGE_POST_FAIL | USAGE_ERROR_UNDEFINED
    )
}

/// Map USB HID Usage Page 0x07 keyboard usages to logical keys.
#[must_use]
pub fn key_from_usage(usage: u8) -> Key {
    match usage {
        0x04..=0x1d => Key::Letter(b'a' + (usage - 0x04)),
        0x1e..=0x26 => Key::Digit(b'1' + (usage - 0x1e)),
        0x27 => Key::Digit(b'0'),
        0x28 => Key::Enter,
        0x29 => Key::Escape,
        0x2a => Key::Backspace,
        0x2b => Key::Tab,
        0x2c => Key::Space,
        0x4f => Key::Right,
        0x50 => Key::Left,
        0x51 => Key::Down,
        0x52 => Key::Up,
        other => Key::Other(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_event() -> KeyEvent {
        KeyEvent {
            key: Key::Other(0),
            pressed: false,
            modifiers: Modifiers::from_bits(0),
        }
    }

    #[test]
    fn press_hold_release_is_edge_triggered() {
        let mut decoder = BootKeyboardDecoder::new();
        let mut out = [empty_event(); MAX_EVENTS_PER_REPORT];

        let press = [0, 0, 0x04, 0, 0, 0, 0, 0];
        assert_eq!(decoder.decode(&press, &mut out), Ok(1));
        assert_eq!(
            out[0],
            KeyEvent {
                key: Key::Letter(b'a'),
                pressed: true,
                modifiers: Modifiers::from_bits(0),
            }
        );

        assert_eq!(decoder.decode(&press, &mut out), Ok(0));

        let release = [0; BOOT_REPORT_LEN];
        assert_eq!(decoder.decode(&release, &mut out), Ok(1));
        assert_eq!(
            out[0],
            KeyEvent {
                key: Key::Letter(b'a'),
                pressed: false,
                modifiers: Modifiers::from_bits(0),
            }
        );
    }

    #[test]
    fn menu_navigation_usages_are_mapped() {
        assert_eq!(key_from_usage(0x52), Key::Up);
        assert_eq!(key_from_usage(0x51), Key::Down);
        assert_eq!(key_from_usage(0x50), Key::Left);
        assert_eq!(key_from_usage(0x4f), Key::Right);
        assert_eq!(key_from_usage(0x28), Key::Enter);
        assert_eq!(key_from_usage(0x29), Key::Escape);
        assert_eq!(key_from_usage(0x2b), Key::Tab);
    }

    #[test]
    fn shift_changes_ascii_but_not_key_identity() {
        let shifted = Modifiers::from_bits(Modifiers::LEFT_SHIFT);
        assert_eq!(Key::Letter(b'a').ascii(shifted), Some(b'A'));
        assert_eq!(Key::Digit(b'1').ascii(shifted), Some(b'!'));
        assert_eq!(Key::Letter(b'a').ascii(Modifiers::from_bits(0)), Some(b'a'));
    }

    #[test]
    fn rollover_does_not_corrupt_previous_state() {
        let mut decoder = BootKeyboardDecoder::new();
        let mut out = [empty_event(); MAX_EVENTS_PER_REPORT];

        let press = [0, 0, 0x04, 0, 0, 0, 0, 0];
        assert_eq!(decoder.decode(&press, &mut out), Ok(1));

        let rollover = [0, 0, USAGE_ERROR_ROLLOVER, 0, 0, 0, 0, 0];
        assert_eq!(
            decoder.decode(&rollover, &mut out),
            Err(DecodeError::Rollover)
        );

        let release = [0; BOOT_REPORT_LEN];
        assert_eq!(decoder.decode(&release, &mut out), Ok(1));
        assert_eq!(out[0].key, Key::Letter(b'a'));
        assert!(!out[0].pressed);
    }

    #[test]
    fn insufficient_output_buffer_is_non_destructive() {
        let mut decoder = BootKeyboardDecoder::new();
        let mut none: [KeyEvent; 0] = [];
        let report = [0, 0, 0x04, 0x05, 0, 0, 0, 0];
        assert_eq!(
            decoder.decode(&report, &mut none),
            Err(DecodeError::OutputTooSmall)
        );

        let mut out = [empty_event(); MAX_EVENTS_PER_REPORT];
        assert_eq!(decoder.decode(&report, &mut out), Ok(2));
        assert_eq!(out[0].key, Key::Letter(b'a'));
        assert_eq!(out[1].key, Key::Letter(b'b'));
    }

    #[test]
    fn multiple_key_transition_orders_releases_before_presses() {
        let mut decoder = BootKeyboardDecoder::new();
        let mut out = [empty_event(); MAX_EVENTS_PER_REPORT];

        assert_eq!(
            decoder.decode(&[0, 0, 0x04, 0x05, 0, 0, 0, 0], &mut out),
            Ok(2)
        );
        assert_eq!(
            decoder.decode(&[0, 0, 0x05, 0x06, 0, 0, 0, 0], &mut out),
            Ok(2)
        );
        assert_eq!(out[0].key, Key::Letter(b'a'));
        assert!(!out[0].pressed);
        assert_eq!(out[1].key, Key::Letter(b'c'));
        assert!(out[1].pressed);
    }
}
