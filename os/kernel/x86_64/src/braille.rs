//! Braille rendering proof (roadmap Phase 5 "Braille transport layer").
//!
//! The speech proof shows the screen reader saying a control; this shows the same
//! control rendered to braille, for a deaf-blind user on a refreshable display.
//! It runs the whole pipeline end to end: build a semantic node, ask the
//! announcement engine for its utterance, and translate that utterance into
//! six-dot braille cells with [`aw_braille`]. The cells are checked against the
//! known-correct Grade 1 pattern for the label, then emitted as hex (the bytes a
//! display's transport sends) and as Unicode braille glyphs a developer can read.
//!
//! Device-free, so it runs on the normal boot path as braille delivery evidence
//! next to the spoken evidence.

use aw_accessibility::{NodeId, Rect, Role, SemanticNode, State};
use aw_braille::{to_unicode, translate};
use aw_screen_reader::{announce_focus, FocusContext};

use crate::debug_write;

/// Emit one byte as two lowercase hex digits.
fn emit_hex_byte(byte: u8) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let pair = [HEX[usize::from(byte >> 4)], HEX[usize::from(byte & 0x0f)]];
    debug_write(core::str::from_utf8(&pair).unwrap_or("??"));
}

/// Prove the braille path: take the utterance the screen reader would speak for
/// the Install button, translate it to braille cells, and confirm they match the
/// known Grade 1 pattern - then emit the cells and their glyphs as evidence.
pub fn prove() {
    debug_write("AW_BRAILLE_BEGIN\n");

    let button = SemanticNode {
        id: NodeId(1),
        parent: Some(NodeId(0)),
        role: Role::Button,
        name: "Install",
        description: "",
        value: "",
        state: State::from_bits(State::FOCUSABLE),
        bounds: Rect {
            x: 40,
            y: 200,
            width: 320,
            height: 28,
        },
    };

    // Semantic node -> spoken utterance -> braille cells: the full pipeline.
    let mut speech = [0u8; 128];
    let utterance = announce_focus(&button, FocusContext::NONE, &mut speech);
    let mut cells = [0u8; 64];
    let count = translate(utterance, &mut cells);

    debug_write("AW_BRAILLE_TEXT \"");
    debug_write(utterance);
    debug_write("\"\n");

    debug_write("AW_BRAILLE_CELLS");
    for &cell in &cells[..count] {
        debug_write(" ");
        emit_hex_byte(cell);
    }
    debug_write("\n");

    let mut glyph_buffer = [0u8; 192];
    let glyphs = to_unicode(&cells[..count], &mut glyph_buffer);
    debug_write("AW_BRAILLE_GLYPHS \"");
    debug_write(glyphs);
    debug_write("\"\n");

    // The known-correct Grade 1 braille for "Install, button": a capital sign,
    // the seven letters of install, comma, space, then the six of button.
    const EXPECTED: [u8; 16] = [
        aw_braille::CAPITAL_SIGN,
        0b00_1010, // i
        0b01_1101, // n
        0b00_1110, // s
        0b01_1110, // t
        0b00_0001, // a
        0b00_0111, // l
        0b00_0111, // l
        0b00_0010, // ,
        aw_braille::SPACE,
        0b00_0011, // b
        0b10_0101, // u
        0b01_1110, // t
        0b01_1110, // t
        0b01_0101, // o
        0b01_1101, // n
    ];

    if count == EXPECTED.len() && cells[..count] == EXPECTED {
        debug_write("AW_BRAILLE_PROOF_OK\n");
    } else {
        debug_write("AW_BRAILLE_FAIL reason=mismatch\n");
    }
}
