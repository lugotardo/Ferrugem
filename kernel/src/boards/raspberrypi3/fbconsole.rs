//! Framebuffer text console, draws `font5x7` glyphs into the
//! VideoCore-allocated framebuffer (`hdmi.rs`) as a visual mirror of
//! the UART console. Never authoritative: every function here is a no-op
//! until `init` succeeds, and stays a no-op forever if it doesn't (no
//! display attached, mailbox unavailable, ...), see `hdmi.rs`.
//!
//! `arch::aarch64::console::print_byte` is the single choke point for every
//! byte of aarch64 console output (shell, panics, exception dumps); it taps
//! `put_byte` below unconditionally when `board-raspberrypi3` is enabled.
//!
//! Also keeps a scrollback history (`BUF`) behind the visible viewport, so
//! Shift+PageUp/PageDown (see `drivers::keyboard::arch::aarch64::read_byte`)
//! can bring rows that scrolled off back into view, same feature and same
//! design as `boards::qemu_pc::console`'s VGA text mirror: the live path
//! (`put_byte`/`scroll`) is untouched pixel-drawing exactly as before, only
//! now every glyph is also recorded into `BUF` so `scroll_view` has
//! something to redraw from when the reader scrolls back.

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

/// Scrollback depth: `BUF` holds every row ever written (up to this many,
/// oldest evicted first) as plain bytes (this console is monochrome, no
/// per-cell colour to remember), of which only `state.rows` are ever
/// visible at once. `MAX_COLS` just needs to cover the widest framebuffer
/// `init` will ever see (1024px / 18px-per-cell = 56 today); `state.cols`
/// is clamped to it defensively in case firmware ever grants something wider.
const MAX_COLS: usize = 200;
const HISTORY: usize = 500;

static mut BUF: [[u8; MAX_COLS]; HISTORY] = [[b' '; MAX_COLS]; HISTORY];

struct State {
    base: usize,
    width: usize,
    height: usize,
    pitch: usize,
    cols: usize,
    rows: usize,
    col: usize,
    /// Live viewport row (0..rows), exactly as before scrollback existed:
    /// drives the incremental pixel-drawing path and `scroll`.
    row: usize,
    /// Absolute row count, monotonically increasing, indexes `BUF` and
    /// drives `scroll_view`'s window math the same way
    /// `boards::qemu_pc::console::ROW` does for the VGA text console.
    abs_row: usize,
    /// Rows back from the live tail currently shown; 0 means "following"
    /// (drawn straight to the framebuffer as `put_byte` writes). Nonzero
    /// freezes the viewport - `put_byte` keeps recording into `BUF` either
    /// way, it just stops touching the framebuffer until the reader scrolls
    /// back down.
    view_offset: usize,
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
    let cols = (width / CELL_W).min(MAX_COLS);
    let rows = height / CELL_H;
    if cols == 0 || rows == 0 {
        return;
    }
    unsafe {
        for row in BUF.iter_mut() {
            *row = [b' '; MAX_COLS];
        }
        STATE = Some(State {
            base, width, height, pitch, cols, rows,
            col: 0, row: 0, abs_row: 0, view_offset: 0, esc: EscState::Normal,
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

fn draw_char_at(state: &State, row: usize, col: usize, c: u8) {
    let glyph = font5x7::glyph(c);
    let ox = col * CELL_W;
    let oy = row * CELL_H;
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

fn draw_char(state: &State, c: u8) {
    draw_char_at(state, state.row, state.col, c);
}

/// Redraw every visible row from `BUF` (the window `state.view_offset`
/// selects), used by `scroll_view` and when returning to the live tail.
/// Expensive (one glyph draw per visible cell) compared to the incremental
/// live path, but only runs on an explicit scroll action, not per byte.
fn full_render(state: &State) {
    unsafe {
        let total = state.abs_row + 1;
        let start = total
            .saturating_sub(state.rows + state.view_offset)
            .max(total.saturating_sub(HISTORY));
        for r in 0..state.rows {
            let abs = start + r;
            for c in 0..state.cols {
                let ch = if abs < total { BUF[abs % HISTORY][c] } else { b' ' };
                draw_char_at(state, r, c, ch);
            }
        }
    }
}

/// Scroll the viewport by one page: `dir < 0` goes back into history
/// (Shift+PageUp), `dir > 0` goes forward toward the tail again
/// (Shift+PageDown). Called from `drivers::keyboard::arch::aarch64::
/// read_byte`. No-op if `init` never succeeded.
pub fn scroll_view(dir: i8) {
    unsafe {
        let Some(state) = STATE.as_mut() else { return };
        let total = state.abs_row + 1;
        let max_offset = total.saturating_sub(state.rows).min(HISTORY.saturating_sub(state.rows));
        state.view_offset = if dir < 0 {
            (state.view_offset + state.rows).min(max_offset)
        } else {
            state.view_offset.saturating_sub(state.rows)
        };
        full_render(state);
        if state.view_offset == 0 {
            // Resync the live incremental cursor to the tail (matches the
            // invariant `put_byte`/`scroll` maintain on their own: `row` is
            // always `min(abs_row, rows - 1)`) so writes right after
            // scrolling back down keep landing in the right physical row.
            state.row = state.abs_row.min(state.rows - 1);
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

        let live = state.view_offset == 0;

        match b {
            b'\n' => {
                state.col = 0;
                advance_row(state, live);
            }
            b'\r' => {
                state.col = 0;
            }
            0x08 => {
                if state.col > 0 {
                    state.col -= 1;
                    BUF[state.abs_row % HISTORY][state.col] = b' ';
                    if live { draw_char(state, b' '); }
                }
            }
            _ => {
                BUF[state.abs_row % HISTORY][state.col] = b;
                if live { draw_char(state, b); }
                state.col += 1;
                if state.col >= state.cols {
                    state.col = 0;
                    advance_row(state, live);
                }
            }
        }

        if live && state.row >= state.rows {
            scroll(state);
            state.row = state.rows - 1;
        }
    }
}

/// Move to a new absolute row, always recording it into `BUF`; only steps
/// the live viewport row (which drives the framebuffer-scroll check right
/// after this returns) while not scrolled back into history.
unsafe fn advance_row(state: &mut State, live: bool) {
    state.abs_row += 1;
    BUF[state.abs_row % HISTORY] = [b' '; MAX_COLS];
    if live {
        state.row += 1;
    }
}
