//! Framebuffer text console, draws `font5x7` glyphs into the
//! VideoCore-allocated framebuffer (`hdmi.rs`) as a visual mirror of
//! the UART console. Never authoritative: every function here is a no-op
//! until `init` succeeds, and stays a no-op forever if it doesn't (no
//! display attached, mailbox unavailable, ...), see `hdmi.rs`.
//!
//! `arch::aarch64::console::print_byte` is the single choke point for every
//! byte of aarch64 console output (shell, panics, exception dumps); it taps
//! `put_byte` below unconditionally when `board-raspberrypi3` is enabled.

use super::font5x7;

const SCALE: usize = 3;
// Glyphs are drawn flush at the top-left of each cell; the extra room below
// leaves visible gaps between characters and between lines (glyphs used to
// be packed edge-to-edge with zero gap, which made adjacent characters hard
// to tell apart - exactly the problem when eyeballing a hex value off the
// screen instead of reading it over serial).
const SPACING_X: usize = SCALE;
const SPACING_Y: usize = SCALE;
const CELL_W: usize = font5x7::FONT_WIDTH * SCALE + SPACING_X;
const CELL_H: usize = font5x7::FONT_HEIGHT * SCALE + SPACING_Y;

const COLOR_FG: u32 = 0x00FF_FFFF; // white
const COLOR_BG: u32 = 0x0000_0000; // black, channel order doesn't matter for
                                    // either color, so this stays correct
                                    // regardless of the RGB/BGR pixel order
                                    // firmware actually granted.

/// Minimal ANSI CSI-sequence recognizer, just to swallow escape codes
/// (e.g. `\x1b[2J\x1b[H`, used by `arch::console_clear`) instead of drawing
/// their raw bytes as garbage glyphs. The sequence's effect itself is not
/// applied here, this console is a best-effort mirror, not a full
/// terminal emulator.
enum EscState {
    Normal,
    SawEsc,
    InCsi,
}

struct State {
    base: usize,
    width: usize,
    height: usize,
    pitch: usize,
    cols: usize,
    rows: usize,
    col: usize,
    row: usize,
    esc: EscState,
}

static mut STATE: Option<State> = None;

/// Take ownership of a firmware-allocated framebuffer and start mirroring
/// console output into it. `base` must already be mapped Normal
/// Non-cacheable (see `arch::aarch64::paging::map_uncached`).
pub fn init(base: usize, width: u32, height: u32, pitch: u32) {
    let width = width as usize;
    let height = height as usize;
    let pitch = pitch as usize;
    let cols = width / CELL_W;
    let rows = height / CELL_H;
    if cols == 0 || rows == 0 {
        return;
    }
    unsafe {
        STATE = Some(State {
            base, width, height, pitch, cols, rows,
            col: 0, row: 0, esc: EscState::Normal,
        });
    }
    clear_screen();
}

fn clear_screen() {
    unsafe {
        let Some(state) = STATE.as_ref() else { return };
        core::ptr::write_bytes(state.base as *mut u8, 0, state.height * state.pitch);
    }
}

fn put_pixel(state: &State, x: usize, y: usize, color: u32) {
    if x >= state.width || y >= state.height {
        return;
    }
    let addr = state.base + y * state.pitch + x * 4;
    unsafe { (addr as *mut u32).write_volatile(color) };
}

fn draw_char(state: &State, c: u8) {
    let glyph = font5x7::glyph(c);
    let ox = state.col * CELL_W;
    let oy = state.row * CELL_H;
    for (ry, &bits) in glyph.iter().enumerate() {
        for rx in 0..font5x7::FONT_WIDTH {
            let on = (bits >> (font5x7::FONT_WIDTH - 1 - rx)) & 1 != 0;
            let color = if on { COLOR_FG } else { COLOR_BG };
            for sy in 0..SCALE {
                for sx in 0..SCALE {
                    put_pixel(state, ox + rx * SCALE + sx, oy + ry * SCALE + sy, color);
                }
            }
        }
    }
}

/// Shift the whole framebuffer up by one text row and blank the row that
/// scrolled into view. Done as a raw byte copy/fill (not per-pixel) since
/// `base` is Normal Non-cacheable memory, ordinary `core::ptr` bulk ops
/// are safe and fast there, unlike on Device memory.
fn scroll(state: &State) {
    unsafe {
        let shift = CELL_H * state.pitch;
        let dst = state.base as *mut u8;
        let src = (state.base + shift) as *const u8;
        let len = state.height * state.pitch - shift;
        core::ptr::copy(src, dst, len);
        core::ptr::write_bytes((state.base + len) as *mut u8, 0, shift);
    }
}

/// Mirror one console byte to the framebuffer. No-op if `init` never
/// succeeded (checked once via the `Option`, not per call site).
pub fn put_byte(b: u8) {
    unsafe {
        let Some(state) = STATE.as_mut() else { return };

        match state.esc {
            EscState::Normal => {
                if b == 0x1B {
                    state.esc = EscState::SawEsc;
                    return;
                }
            }
            EscState::SawEsc => {
                state.esc = if b == b'[' { EscState::InCsi } else { EscState::Normal };
                return;
            }
            EscState::InCsi => {
                if (0x40..=0x7E).contains(&b) {
                    state.esc = EscState::Normal;
                }
                return;
            }
        }

        match b {
            b'\n' => {
                state.col = 0;
                state.row += 1;
            }
            b'\r' => {
                state.col = 0;
            }
            0x08 => {
                if state.col > 0 {
                    state.col -= 1;
                    draw_char(state, b' ');
                }
            }
            _ => {
                draw_char(state, b);
                state.col += 1;
                if state.col >= state.cols {
                    state.col = 0;
                    state.row += 1;
                }
            }
        }

        if state.row >= state.rows {
            scroll(state);
            state.row = state.rows - 1;
        }
    }
}
