use crate::arch::x86_64::port;

// COM1 UART mirror all output to serial for QEMU -serial stdio
const COM1: u16 = 0x3F8;

fn serial_write_byte(b: u8) {
    unsafe {
        // Wait for transmit holding register empty (LSR bit 5)
        while port::inb(COM1 + 5) & 0x20 == 0 {}
        port::outb(COM1, b);
    }
}

fn serial_init() {
    unsafe {
        port::outb(COM1 + 1, 0x00); // disable interrupts
        port::outb(COM1 + 3, 0x80); // DLAB on
        port::outb(COM1 + 0, 0x01); // divisor lo: 115200 baud
        port::outb(COM1 + 1, 0x00); // divisor hi
        port::outb(COM1 + 3, 0x03); // 8N1, DLAB off
        port::outb(COM1 + 2, 0x00); // leave FIFO disabled, com1::init() configures it
        port::outb(COM1 + 4, 0x0B); // RTS+DTR
    }
}

// VGA text buffer: 80x25 physical viewport onto `BUF` below, each cell is (char, attr)
const VGA_ADDR: *mut u16 = 0xB8000 as *mut u16;
const WIDTH: usize = 80;
const HEIGHT: usize = 25;

// Colour: light grey on black
const ATTR: u16 = 0x0700;
const BLANK: u16 = ATTR | b' ' as u16;

// CRTC ports for hardware cursor
const CRTC_ADDR: u16 = 0x3D4;
const CRTC_DATA: u16 = 0x3D5;

/// Scrollback depth: `BUF` holds every row ever written (up to this many,
/// oldest evicted first), of which only `HEIGHT` are ever visible on the
/// physical VGA buffer at once. `render` copies whichever `HEIGHT`-row
/// window `VIEW_OFFSET` currently selects into `VGA_ADDR`; `ROW`/`COL` track
/// the live write position (an absolute row index that only ever grows, not
/// wrapped into the viewport the way the old scroll-in-place design worked).
const HISTORY: usize = 1000;
/// Rows scrolled per Shift+PageUp/PageDown press (see `scroll_view`,
/// wired from `drivers::keyboard::arch::x86_64::handle_irq`): a full page,
/// matching a real text console.
const PAGE_STEP: usize = HEIGHT;

static mut BUF: [[u16; WIDTH]; HISTORY] = [[BLANK; WIDTH]; HISTORY];
static mut COL: usize = 0;
static mut ROW: usize = 0;
/// Rows back from the live tail currently shown; 0 means "following" (every
/// `put_byte` re-renders the viewport as it writes). Nonzero freezes the
/// viewport so scrolled-back history isn't yanked out from under the reader
/// by unrelated output, exactly like a real terminal's scrollback.
static mut VIEW_OFFSET: usize = 0;

/// Minimal ANSI CSI-sequence recognizer, just to swallow escape codes (e.g.
/// `\x1b[2J\x1b[H`, sent by the userspace shell to clear its terminal)
/// instead of drawing their raw bytes as garbage glyphs, same approach as
/// `boards::raspberrypi3::fbconsole`. The sequence's effect itself is not
/// applied here, this console is a best-effort mirror, not a full terminal
/// emulator.
enum EscState {
    Normal,
    SawEsc,
    InCsi,
}

static mut ESC: EscState = EscState::Normal;

pub fn serial_try_read() -> Option<u8> {
    unsafe {
        if port::inb(COM1 + 5) & 0x01 != 0 {
            Some(port::inb(COM1))
        } else {
            None
        }
    }
}

pub fn init() {
    serial_init();
    clear();
}

pub fn clear() {
    unsafe {
        for row in BUF.iter_mut() {
            row.fill(BLANK);
        }
        COL = 0;
        ROW = 0;
        VIEW_OFFSET = 0;
    }
    render();
}

pub fn print_str(s: &str) {
    for b in s.bytes() {
        print_byte(b);
    }
}

pub fn print_byte(b: u8) {
    // Mirror to serial
    if b == b'\n' { serial_write_byte(b'\r'); }
    serial_write_byte(b);
    put_byte(b);
}

/// Draw one byte into the VGA text buffer only, no serial mirroring. Split
/// out of `print_byte` so `drivers::serial::arch::x86_64` can mirror every
/// byte it writes to COM1 here too, the interactive shell runs as a real
/// userspace process on x86_64 (see `init::kernel_main`) and talks to COM1
/// straight through the `write` syscall, bypassing this module entirely
/// otherwise, unlike the boot-time diagnostics that call `print_byte`
/// directly and unlike aarch64/riscv64, whose kernel-internal shell already
/// goes through their own arch console.
pub fn put_byte(b: u8) {
    unsafe {
        match ESC {
            EscState::Normal => {
                if b == 0x1B {
                    ESC = EscState::SawEsc;
                    return;
                }
            }
            EscState::SawEsc => {
                ESC = if b == b'[' { EscState::InCsi } else { EscState::Normal };
                return;
            }
            EscState::InCsi => {
                if (0x40..=0x7E).contains(&b) {
                    ESC = EscState::Normal;
                }
                return;
            }
        }

        match b {
            b'\n' => newline(),
            b'\r' => COL = 0,
            b'\x08' => backspace(),
            _ => {
                BUF[ROW % HISTORY][COL] = ATTR | b as u16;
                COL += 1;
                if COL >= WIDTH {
                    newline();
                }
            }
        }
    }
    // Follow the tail only while not scrolled back, matching a real
    // terminal: history keeps recording either way (see `put_byte` above),
    // but the viewport itself doesn't move until the reader scrolls back
    // down (`scroll_view`).
    if unsafe { VIEW_OFFSET } == 0 {
        render();
    }
}

unsafe fn newline() {
    COL = 0;
    ROW += 1;
    BUF[ROW % HISTORY].fill(BLANK);
}

unsafe fn backspace() {
    if COL > 0 {
        COL -= 1;
    } else if ROW > 0 {
        // Best-effort: a backspace across a row boundary is treated as
        // undoing a WIDTH-column wrap, not a real newline, same
        // approximation the single-viewport implementation made.
        ROW -= 1;
        COL = WIDTH - 1;
    }
    BUF[ROW % HISTORY][COL] = BLANK;
}

/// Scroll the viewport by one page without disturbing `ROW`/`COL` (the live
/// write position): `dir < 0` goes back into history (Shift+PageUp), `dir >
/// 0` goes forward toward the tail again (Shift+PageDown). Called from
/// `drivers::keyboard::arch::x86_64::handle_irq`.
pub fn scroll_view(dir: i8) {
    unsafe {
        let total = ROW + 1;
        let max_offset = total.saturating_sub(HEIGHT).min(HISTORY.saturating_sub(HEIGHT));
        VIEW_OFFSET = if dir < 0 {
            (VIEW_OFFSET + PAGE_STEP).min(max_offset)
        } else {
            VIEW_OFFSET.saturating_sub(PAGE_STEP)
        };
    }
    render();
}

/// Copy the `HEIGHT`-row window `VIEW_OFFSET` selects from `BUF` into the
/// physical VGA buffer, and move the hardware cursor to match, but only
/// while live (a cursor blinking in the middle of scrolled-back history
/// would be misleading, there's nothing to type there).
fn render() {
    unsafe {
        let total = ROW + 1;
        let start = total
            .saturating_sub(HEIGHT + VIEW_OFFSET)
            .max(total.saturating_sub(HISTORY));

        for r in 0..HEIGHT {
            let abs = start + r;
            let src = if abs < total { BUF[abs % HISTORY] } else { [BLANK; WIDTH] };
            for c in 0..WIDTH {
                VGA_ADDR.add(r * WIDTH + c).write_volatile(src[c]);
            }
        }

        if VIEW_OFFSET == 0 {
            update_cursor((ROW - start) * WIDTH + COL);
        }
    }
}

fn update_cursor(pos: usize) {
    let p = pos as u16;
    unsafe {
        port::outb(CRTC_ADDR, 0x0F);
        port::outb(CRTC_DATA, (p & 0xFF) as u8);
        port::outb(CRTC_ADDR, 0x0E);
        port::outb(CRTC_DATA, ((p >> 8) & 0xFF) as u8);
    }
}
