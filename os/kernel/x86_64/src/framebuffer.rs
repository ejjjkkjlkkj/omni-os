//! Kernel framebuffer text console (roadmap Phase 3 "Basic framebuffer console").
//!
//! The UEFI stage's visible text goes to `ConOut`, which the firmware tears down
//! at `ExitBootServices`. From that instant the kernel's only human-visible
//! channel on a real machine is the linear framebuffer the loader handed off: the
//! `0xE9` debug port and COM1 that carry every marker are QEMU/dev-board fixtures,
//! absent on a laptop. This renders readable text straight into that framebuffer
//! with the public-domain 8x8 [`crate::font`], so a physical machine shows that
//! the kernel is alive and what it is doing - the first post-firmware output that
//! survives on hardware, and the surface the on-screen screen reader will draw on.
//!
//! Every write is bounds-checked against the handed-off geometry, and the whole
//! console is a no-op until [`init`] has validated a usable framebuffer, so a
//! machine that exposes none (or a bitmask/blt-only mode) simply stays dark
//! instead of faulting. Writes come only from the bootstrap processor during
//! single-core bring-up, so the single shared [`Console`] needs no lock.

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicBool, Ordering};

use aw_kernel_core::{HANDOFF_FLAG_FRAMEBUFFER_PRESENT, HandoffPixelFormat, KernelHandoff};

use crate::debug_write;
use crate::font::{FONT8X8_BASIC, GLYPH_HEIGHT, GLYPH_WIDTH};

/// Each font pixel is drawn as a `SCALE`x`SCALE` block, so the 8x8 cell becomes
/// 16x16 - legible on a 1280x800 panel and up without a second font size.
const SCALE: u32 = 2;
/// Rendered glyph cell size in pixels, including the one-pixel inter-line gap the
/// 8x8 font bakes into its last row.
const CELL_W: u32 = GLYPH_WIDTH as u32 * SCALE;
const CELL_H: u32 = GLYPH_HEIGHT as u32 * SCALE;
/// Left margin, in pixels, so text does not start against the bezel.
const MARGIN_X: u32 = 8;
/// Pixels reserved at the top of the panel. Text starts below it, so the boot
/// marker painted over the top rows at the end of bring-up never lands on a line.
const TEXT_TOP: u32 = 40;

/// Foreground (text) and background colours as `(r, g, b)`. A calm dark blue with
/// near-white text, chosen for contrast rather than decoration.
const FG: Rgb = Rgb(0xd0, 0xe0, 0xff);
const BG: Rgb = Rgb(0x00, 0x00, 0x20);

/// A 24-bit colour, encoded into the framebuffer's own pixel order on write.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Rgb(u8, u8, u8);

/// The framebuffer text console. One instance lives in [`CONSOLE`]; it is only
/// ever touched from the bootstrap processor while other cores are parked, so it
/// carries no lock of its own.
struct Console {
    base: *mut u8,
    width: u32,
    height: u32,
    stride_pixels: u32,
    byte_len: u64,
    format: HandoffPixelFormat,
    cursor_x: u32,
    cursor_y: u32,
}

impl Console {
    const EMPTY: Self = Self {
        base: core::ptr::null_mut(),
        width: 0,
        height: 0,
        stride_pixels: 0,
        byte_len: 0,
        format: HandoffPixelFormat::Unknown,
        cursor_x: MARGIN_X,
        cursor_y: TEXT_TOP,
    };

    /// Byte offset of pixel `(x, y)`, or `None` if it (with its 4-byte pixel) does
    /// not fit inside the handed-off framebuffer.
    fn pixel_offset(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let index = u64::from(y)
            .checked_mul(u64::from(self.stride_pixels))?
            .checked_add(u64::from(x))?;
        let offset = index.checked_mul(4)?;
        if offset.checked_add(4)? > self.byte_len {
            return None;
        }
        usize::try_from(offset).ok()
    }

    /// Write one pixel, encoding `colour` into the framebuffer's channel order.
    fn put_pixel(&self, x: u32, y: u32, colour: Rgb) {
        let Some(offset) = self.pixel_offset(x, y) else {
            return;
        };
        let Rgb(r, g, b) = colour;
        let bytes = match self.format {
            HandoffPixelFormat::Rgb => [r, g, b, 0x00],
            // Bgr is the common UEFI GOP order (and what OVMF reports).
            _ => [b, g, r, 0x00],
        };
        // SAFETY: `pixel_offset` proved `offset..offset+4` lies inside the
        // framebuffer the loader handed off, and the framebuffer is identity
        // mapped both under firmware tables and in the kernel's own map.
        unsafe {
            let pixel = self.base.add(offset) as *mut [u8; 4];
            core::ptr::write_volatile(pixel, bytes);
        }
    }

    /// Read one pixel back, decoding it into `(r, g, b)`. Used only by the proof.
    fn get_pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        let offset = self.pixel_offset(x, y)?;
        // SAFETY: same bounds and mapping guarantee as `put_pixel`.
        let bytes = unsafe { core::ptr::read_volatile(self.base.add(offset) as *const [u8; 4]) };
        Some(match self.format {
            HandoffPixelFormat::Rgb => Rgb(bytes[0], bytes[1], bytes[2]),
            _ => Rgb(bytes[2], bytes[1], bytes[0]),
        })
    }

    /// Fill a rectangle with a solid colour, clipped to the framebuffer.
    fn fill_rect(&self, x: u32, y: u32, w: u32, h: u32, colour: Rgb) {
        for row in y..y.saturating_add(h) {
            for column in x..x.saturating_add(w) {
                self.put_pixel(column, row, colour);
            }
        }
    }

    /// Paint the whole panel the background colour.
    fn clear(&self) {
        self.fill_rect(0, 0, self.width, self.height, BG);
    }

    /// Draw one glyph with its top-left at `(ox, oy)`, scaled by [`SCALE`].
    fn draw_glyph(&self, ox: u32, oy: u32, ch: u8, fg: Rgb, bg: Rgb) {
        // Only the basic-Latin block is encoded; anything else shows as a space.
        let glyph = if (ch as usize) < FONT8X8_BASIC.len() {
            &FONT8X8_BASIC[ch as usize]
        } else {
            &FONT8X8_BASIC[b' ' as usize]
        };
        for (row, bits) in glyph.iter().enumerate() {
            for column in 0..GLYPH_WIDTH {
                // Bit 0 is the leftmost pixel in this font's encoding.
                let lit = (bits >> column) & 1 != 0;
                let colour = if lit { fg } else { bg };
                let px = ox + column as u32 * SCALE;
                let py = oy + row as u32 * SCALE;
                self.fill_rect(px, py, SCALE, SCALE, colour);
            }
        }
    }

    /// Scroll the text region up by one line, clearing the freed bottom line. The
    /// reserved top strip above [`TEXT_TOP`] is left untouched.
    fn scroll_line(&self) {
        let line = CELL_H;
        let top = TEXT_TOP;
        // Move each destination row's pixels up from the row one line below it.
        for y in top..self.height.saturating_sub(line) {
            for x in 0..self.width {
                let colour = self.get_pixel(x, y + line).unwrap_or(BG);
                self.put_pixel(x, y, colour);
            }
        }
        let cleared_top = self.height.saturating_sub(line).max(top);
        self.fill_rect(0, cleared_top, self.width, line, BG);
    }

    /// Advance to the start of the next line, scrolling if the page is full.
    fn newline(&mut self) {
        self.cursor_x = MARGIN_X;
        if self.cursor_y + CELL_H * 2 > self.height {
            self.scroll_line();
        } else {
            self.cursor_y += CELL_H;
        }
    }

    /// Render one character, handling newlines and right-edge wrapping.
    fn putc(&mut self, ch: u8) {
        if ch == b'\n' {
            self.newline();
            return;
        }
        if self.cursor_x + CELL_W > self.width.saturating_sub(MARGIN_X) {
            self.newline();
        }
        self.draw_glyph(self.cursor_x, self.cursor_y, ch, FG, BG);
        self.cursor_x += CELL_W;
    }

    /// Render a string, one byte at a time (non-ASCII bytes fall back to a space).
    fn puts(&mut self, text: &str) {
        for &byte in text.as_bytes() {
            let ch = if byte.is_ascii_graphic() || byte == b' ' || byte == b'\n' {
                byte
            } else {
                b' '
            };
            self.putc(ch);
        }
    }

    /// Draw a string at an absolute pixel position without moving the cursor.
    /// Used by fixed-layout UI (the boot menu), which owns its own coordinates.
    fn draw_text_at(&self, mut x: u32, y: u32, text: &str, fg: Rgb, bg: Rgb) {
        for &byte in text.as_bytes() {
            let ch = if byte.is_ascii_graphic() || byte == b' ' {
                byte
            } else {
                b' '
            };
            if x + CELL_W > self.width.saturating_sub(MARGIN_X) {
                break;
            }
            self.draw_glyph(x, y, ch, fg, bg);
            x += CELL_W;
        }
    }
}

/// A `Sync` wrapper so the single console can live in a `static`. Sound because
/// only the bootstrap processor touches it, and only during single-core bring-up.
struct ConsoleCell(UnsafeCell<Console>);

// SAFETY: access is confined to the bootstrap processor during single-core
// bring-up; no other core reads or writes this while it is in use.
unsafe impl Sync for ConsoleCell {}

static CONSOLE: ConsoleCell = ConsoleCell(UnsafeCell::new(Console::EMPTY));

/// Set once [`init`] has validated a usable framebuffer. Every public entry point
/// checks it first, so the console is inert on a machine that exposes none.
static READY: AtomicBool = AtomicBool::new(false);

/// Borrow the console mutably. Callers must be on the bootstrap processor during
/// single-core bring-up (the module invariant), so no aliasing occurs.
#[allow(clippy::mut_from_ref)]
fn console() -> &'static mut Console {
    // SAFETY: single-core bootstrap, bootstrap processor only; see module docs.
    unsafe { &mut *CONSOLE.0.get() }
}

/// Bring the framebuffer text console up from the loader handoff.
///
/// Returns `false` (and leaves the console inert) when no framebuffer was handed
/// off, its pixel format is not a directly writable RGB/BGR mode, or its declared
/// geometry does not fit its byte length - the console must never guess at memory
/// the firmware did not describe.
pub fn init(handoff: &KernelHandoff) -> bool {
    debug_write("AW_FBCON_BEGIN\n");

    if handoff.flags & HANDOFF_FLAG_FRAMEBUFFER_PRESENT == 0 {
        debug_write("AW_FBCON_UNAVAILABLE reason=no_framebuffer\n");
        return false;
    }
    let fb = handoff.framebuffer;
    if !matches!(
        fb.pixel_format,
        HandoffPixelFormat::Rgb | HandoffPixelFormat::Bgr
    ) {
        debug_write("AW_FBCON_UNAVAILABLE reason=unsupported_format\n");
        return false;
    }
    if !fb.dimensions_are_valid() || fb.physical_address > usize::MAX as u64 {
        debug_write("AW_FBCON_UNAVAILABLE reason=bad_geometry\n");
        return false;
    }
    // The last pixel must fit: (height-1)*stride + (width-1), times four bytes.
    let last_index = u64::from(fb.height - 1)
        .checked_mul(u64::from(fb.stride_pixels))
        .and_then(|v| v.checked_add(u64::from(fb.width - 1)));
    let fits = match last_index
        .and_then(|i| i.checked_mul(4))
        .and_then(|o| o.checked_add(4))
    {
        Some(required) => required <= fb.byte_len,
        None => false,
    };
    if !fits {
        debug_write("AW_FBCON_UNAVAILABLE reason=byte_len_too_small\n");
        return false;
    }

    let c = console();
    c.base = fb.physical_address as usize as *mut u8;
    c.width = fb.width;
    c.height = fb.height;
    c.stride_pixels = fb.stride_pixels;
    c.byte_len = fb.byte_len;
    c.format = fb.pixel_format;
    c.cursor_x = MARGIN_X;
    c.cursor_y = TEXT_TOP;
    c.clear();

    READY.store(true, Ordering::Release);
    debug_write("AW_FBCON_READY width=");
    crate::debug_write_u64(u64::from(fb.width));
    debug_write(" height=");
    crate::debug_write_u64(u64::from(fb.height));
    debug_write("\n");
    true
}

/// Whether the console is up. Public so callers can skip building text they would
/// only throw away on a machine with no usable framebuffer.
pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// Render `text` then a newline. A no-op until [`init`] has succeeded.
pub fn write_line(text: &str) {
    if !is_ready() {
        return;
    }
    let c = console();
    c.puts(text);
    c.putc(b'\n');
}

/// Repaint the whole panel to the background and home the console cursor. Used
/// when a fixed-layout screen (the boot menu) takes over from the scrolling log.
pub fn clear_screen() {
    if !is_ready() {
        return;
    }
    let c = console();
    c.clear();
    c.cursor_x = MARGIN_X;
    c.cursor_y = TEXT_TOP;
}

/// Draw one full-width menu row at row index `row` (0 at the top of the text
/// area). A focused row is drawn as a solid bar with inverted text, so the
/// selection is visible without colour vision; an unfocused row is plain text on
/// the background. A no-op until [`init`] has succeeded.
pub fn draw_menu_row(row: u32, text: &str, focused: bool) {
    if !is_ready() {
        return;
    }
    let c = console();
    let y = TEXT_TOP + row * CELL_H;
    if y + CELL_H > c.height {
        return;
    }
    let (bar, fg, bg) = if focused { (FG, BG, FG) } else { (BG, FG, BG) };
    c.fill_rect(0, y, c.width, CELL_H, bar);
    // A focused row indents its text by one cell so the bar reads as a selection.
    c.draw_text_at(MARGIN_X, y, text, fg, bg);
}

/// Prove the console renders to the real framebuffer: draw a known glyph at a
/// known origin, read every pixel of it back from framebuffer memory, and require
/// each to match the scaled font bitmap in the framebuffer's own colour order.
///
/// This is stronger than "we wrote some pixels": a matching read-back can only
/// happen if the CPU actually laid the glyph's exact lit/unlit pattern into the
/// linear framebuffer the firmware handed off. On a machine with no framebuffer
/// the console is inert and the proof reports it unavailable rather than passing.
pub fn prove() {
    debug_write("AW_FBCON_PROOF_BEGIN\n");
    if !is_ready() {
        debug_write("AW_FBCON_UNAVAILABLE reason=not_ready\n");
        return;
    }

    // A visible line for anyone watching the real panel, then the verifiable glyph
    // on its own line so its origin is known exactly.
    write_line("omni-os kernel: framebuffer console online.");

    let c = console();
    let ox = c.cursor_x;
    let oy = c.cursor_y;
    const TEST_CH: u8 = b'A';
    c.draw_glyph(ox, oy, TEST_CH, FG, BG);

    let glyph = &FONT8X8_BASIC[TEST_CH as usize];
    let mut lit_pixels = 0u32;
    for (row, bits) in glyph.iter().enumerate() {
        for column in 0..GLYPH_WIDTH {
            let lit = (bits >> column) & 1 != 0;
            let expected = if lit { FG } else { BG };
            // Sample the centre of each scaled block, away from any rounding edge.
            let px = ox + column as u32 * SCALE + SCALE / 2;
            let py = oy + row as u32 * SCALE + SCALE / 2;
            match c.get_pixel(px, py) {
                Some(actual) if actual == expected => {
                    if lit {
                        lit_pixels += 1;
                    }
                }
                _ => {
                    debug_write("AW_FBCON_FAIL reason=glyph_readback\n");
                    return;
                }
            }
        }
    }
    // The chosen glyph is not blank, so a pass must have observed lit pixels: this
    // rejects a framebuffer that reads back as a uniform colour (e.g. all zero).
    if lit_pixels == 0 {
        debug_write("AW_FBCON_FAIL reason=no_lit_pixels\n");
        return;
    }
    // Advance past the glyph so it stays on screen as part of the console output.
    c.cursor_x = ox + CELL_W;
    c.putc(b'\n');

    debug_write("AW_FBCON_GLYPH_READBACK_OK char=A lit=");
    crate::debug_write_u64(u64::from(lit_pixels));
    debug_write("\n");
    debug_write("AW_FBCON_PROOF_OK\n");
}
