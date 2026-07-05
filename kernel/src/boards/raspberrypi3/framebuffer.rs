//! VideoCore framebuffer bring-up, via the property-tag mailbox interface
//! (`mailbox.rs`). Requests a fixed 1024x768x32 framebuffer, a VESA/CEA
//! resolution essentially every HDMI display and TV accepts, rather than
//! parsing EDID for the display's preferred mode, which needs a separate
//! (and considerably more involved) mailbox tag.
//!
//! Entirely best-effort: no display attached, a firmware that refuses the
//! request, or (as on QEMU's `raspi3b` machine, which doesn't implement
//! this mailbox at all) a timed-out call all degrade to "no framebuffer
//! console" rather than a boot failure, UART0 remains the authoritative
//! console either way, this is only ever a visual mirror of it.

use super::{fbconsole, mailbox};

const WIDTH: u32 = 1024;
const HEIGHT: u32 = 768;
const DEPTH: u32 = 32;

const TAG_SET_PHYS_WH:     u32 = 0x0004_8003;
const TAG_SET_VIRT_WH:     u32 = 0x0004_8004;
const TAG_SET_VIRT_OFFSET: u32 = 0x0004_8009;
const TAG_SET_DEPTH:       u32 = 0x0004_8005;
const TAG_SET_PIXEL_ORDER: u32 = 0x0004_8006;
const TAG_ALLOCATE_BUFFER: u32 = 0x0004_0001;
const TAG_GET_PITCH:       u32 = 0x0004_0008;
const TAG_END:             u32 = 0;

const PIXEL_ORDER_RGB: u32 = 1;
const BUFFER_ALIGN: u32 = 4096;

#[repr(C, align(16))]
struct MboxBuffer([u32; 36]);

/// Request the framebuffer from firmware and, on success, hand it to
/// `fbconsole` as a secondary mirror of the UART console. Called once from
/// `arch::board_late_init`, after paging is up (needed for `map_uncached`).
pub fn init() {
    let mut msg = MboxBuffer([0; 36]);
    let b = &mut msg.0;
    let mut i = 2usize;

    b[i] = TAG_SET_PHYS_WH; b[i + 1] = 8; b[i + 2] = 0; b[i + 3] = WIDTH; b[i + 4] = HEIGHT;
    i += 5;
    b[i] = TAG_SET_VIRT_WH; b[i + 1] = 8; b[i + 2] = 0; b[i + 3] = WIDTH; b[i + 4] = HEIGHT;
    i += 5;
    b[i] = TAG_SET_VIRT_OFFSET; b[i + 1] = 8; b[i + 2] = 0; b[i + 3] = 0; b[i + 4] = 0;
    i += 5;
    b[i] = TAG_SET_DEPTH; b[i + 1] = 4; b[i + 2] = 0; b[i + 3] = DEPTH;
    i += 4;
    b[i] = TAG_SET_PIXEL_ORDER; b[i + 1] = 4; b[i + 2] = 0; b[i + 3] = PIXEL_ORDER_RGB;
    i += 4;
    let alloc = i;
    b[i] = TAG_ALLOCATE_BUFFER; b[i + 1] = 8; b[i + 2] = 0; b[i + 3] = BUFFER_ALIGN; b[i + 4] = 0;
    i += 5;
    let pitch_tag = i;
    b[i] = TAG_GET_PITCH; b[i + 1] = 4; b[i + 2] = 0; b[i + 3] = 0;
    i += 4;
    b[i] = TAG_END;
    i += 1;

    b[0] = (i * 4) as u32;
    b[1] = 0;

    if !mailbox::call(&mut b[..i]) {
        return; // no display / firmware refused / mailbox not implemented (QEMU)
    }

    let bus_addr = b[alloc + 3];
    let fb_size = b[alloc + 4];
    let pitch = b[pitch_tag + 3];
    if bus_addr == 0 || fb_size == 0 || pitch == 0 {
        return;
    }

    let phys = mailbox::bus_to_phys(bus_addr);
    crate::arch::aarch64::paging::map_uncached(phys, fb_size as usize);

    fbconsole::init(phys, WIDTH, HEIGHT, pitch);
}
