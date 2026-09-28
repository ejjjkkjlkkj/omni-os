//! USB HID Boot Keyboard semantics shared with the future kernel xHCI transport.
//!
//! The transport is intentionally separate: this module proves that standard
//! HID boot reports become exactly the same logical keys already consumed by
//! the accessible menu's PS/2 path. Once xHCI delivers an 8-byte interrupt-IN
//! report, no second navigation stack is required.

use aw_usb_hid::{BootKeyboardDecoder, Key as HidKey, KeyEvent, MAX_EVENTS_PER_REPORT, Modifiers};

use crate::debug_write;
use crate::ps2_keyboard::Key;

/// Convert a pressed HID event into the menu's existing logical key type.
/// Releases are deliberately ignored by the menu; they are still tracked by
/// the decoder so held keys do not auto-repeat as fresh presses.
#[must_use]
pub fn menu_key(event: KeyEvent) -> Option<Key> {
    if !event.pressed {
        return None;
    }

    Some(match event.key {
        HidKey::Up => Key::Up,
        HidKey::Down => Key::Down,
        HidKey::Left => Key::Left,
        HidKey::Right => Key::Right,
        HidKey::Enter => Key::Enter,
        HidKey::Escape => Key::Escape,
        HidKey::Tab => Key::Tab,
        HidKey::Backspace => Key::Backspace,
        HidKey::Space => Key::Space,
        HidKey::Letter(_) | HidKey::Digit(_) => Key::Char(event.key.ascii(event.modifiers)?),
        HidKey::Other(_) => return None,
    })
}

fn blank_event() -> KeyEvent {
    KeyEvent {
        key: HidKey::Other(0),
        pressed: false,
        modifiers: Modifiers::from_bits(0),
    }
}

/// Deterministic in-kernel proof for the semantic half of USB keyboard input.
///
/// This does not claim a USB controller or keyboard was driven. It proves only
/// the transport-independent contract: architected HID boot reports ->
/// edge-triggered HID events -> the exact logical keys consumed by the menu.
pub fn prove_decode_path() {
    debug_write("AW_USB_HID_DECODE_BEGIN\n");

    // Down press/release, Up press/release, Shift+A press/release, Enter press.
    // USB HID Usage Page 0x07: Down=0x51, Up=0x52, A=0x04, Enter=0x28.
    let reports = [
        [0x00, 0x00, 0x51, 0, 0, 0, 0, 0],
        [0x00, 0x00, 0x00, 0, 0, 0, 0, 0],
        [0x00, 0x00, 0x52, 0, 0, 0, 0, 0],
        [0x00, 0x00, 0x00, 0, 0, 0, 0, 0],
        [Modifiers::LEFT_SHIFT, 0x00, 0x04, 0, 0, 0, 0, 0],
        [0x00, 0x00, 0x00, 0, 0, 0, 0, 0],
        [0x00, 0x00, 0x28, 0, 0, 0, 0, 0],
    ];
    let expected = [Key::Down, Key::Up, Key::Char(b'A'), Key::Enter];

    let mut decoder = BootKeyboardDecoder::new();
    let mut events = [blank_event(); MAX_EVENTS_PER_REPORT];
    let mut seen = [Key::Escape; 4];
    let mut seen_count = 0usize;

    for report in reports {
        let Ok(count) = decoder.decode(&report, &mut events) else {
            debug_write("AW_USB_HID_DECODE_FAIL reason=decode\n");
            return;
        };
        for event in events[..count].iter().copied() {
            if let Some(key) = menu_key(event) {
                if seen_count >= seen.len() {
                    debug_write("AW_USB_HID_DECODE_FAIL reason=too_many_keys\n");
                    return;
                }
                seen[seen_count] = key;
                seen_count += 1;
            }
        }
    }

    if seen_count != expected.len() || seen != expected {
        debug_write("AW_USB_HID_DECODE_FAIL reason=sequence\n");
        return;
    }

    debug_write("AW_USB_HID_KEY name=down\n");
    debug_write("AW_USB_HID_KEY name=up\n");
    debug_write("AW_USB_HID_KEY name=shift_a\n");
    debug_write("AW_USB_HID_KEY name=enter\n");
    debug_write("AW_USB_HID_DECODE_PROOF_OK\n");
}
