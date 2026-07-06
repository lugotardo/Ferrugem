//! UHCI (Universal Host Controller Interface, USB 1.1) host controller
//! driver: a PCI device found via `arch::x86_64::pci`, bring-up, and
//! control-transfer / one-shot interrupt-IN-poll primitives, the x86
//! equivalent of `boards::raspberrypi3::usb::dwc2` for QEMU q35 and
//! VirtualBox (and any real PC with a UHCI or UHCI-compatible companion
//! controller).
//!
//! UHCI is a much "dumber" controller than DWC2: there's no internal DMA
//! engine walking multi-packet buffers on the host's behalf, software has
//! to submit one Transfer Descriptor (TD) per max-packet-sized chunk and
//! chase the schedule itself. This driver takes the simplest correct
//! approach: exactly one static Queue Head (QH), reachable from every one
//! of the controller's 1024 frame-list slots, executing exactly one TD at
//! a time - there is never more than one transfer in flight, matching
//! `dwc2.rs`'s fully-synchronous, one-request-at-a-time shape. No
//! isochronous, no bulk, no concurrent transfers.
//!
//! Root-hub-only for now: this driver enumerates the controller's own 2
//! built-in ports (`hub.rs`) but doesn't walk an external hub plugged into
//! one of them. Real timing (reset pulse width, etc.) comes from
//! `drivers::timer::src::pit::delay_us`, a PIT channel 2 busy-wait separate
//! from channel 0's scheduler tick.

use super::protocol::{Endpoint, SetupPacket, Speed};
use crate::arch::x86_64::{pci, port};
use crate::drivers::timer::src::pit;

// ── I/O register offsets from the PCI I/O-space BAR (BAR4) ────────────────
const USBCMD:     u16 = 0x00;
const USBSTS:     u16 = 0x02;
const USBINTR:    u16 = 0x04;
const FRNUM:      u16 = 0x06;
const FRBASEADD:  u16 = 0x08;
const SOFMOD:     u16 = 0x0C;
const PORTSC1:    u16 = 0x10;
const PORTSC2:    u16 = 0x12;

const CMD_RS:      u16 = 1 << 0;
const CMD_HCRESET: u16 = 1 << 1;
const CMD_GRESET:  u16 = 1 << 2;
const CMD_CF:      u16 = 1 << 6;
const CMD_MAXP64:  u16 = 1 << 7;

const STS_HALTED: u16 = 1 << 5;

const PORTSC_CCS:      u16 = 1 << 0; // Current Connect Status
const PORTSC_CSC:      u16 = 1 << 1; // Connect Status Change (W1C)
const PORTSC_PE:       u16 = 1 << 2; // Port Enabled
const PORTSC_PEC:      u16 = 1 << 3; // Port Enable Change (W1C)
const PORTSC_LSDA:     u16 = 1 << 8; // Low Speed Device Attached
const PORTSC_PR:       u16 = 1 << 9; // Port Reset
const PORTSC_RESERVED1: u16 = 1 << 7; // always reads 1, must be written 1
const PORTSC_W1C_MASK: u16 = PORTSC_CSC | PORTSC_PEC;

// ── Transfer Descriptor / Queue Head layout (Intel UHCI 1.1 section 3) ────
const LINK_TERMINATE: u32 = 1 << 0;
const LINK_QH_SELECT: u32 = 1 << 1;

// Bits 0-10 Actual Length, 11-16 reserved, then:
const TD_CS_ACTLEN_MASK: u32 = 0x7FF;
const TD_CS_BITSTUFF:    u32 = 1 << 17;
const TD_CS_CRC_TIMEOUT: u32 = 1 << 18;
// bit 19: NAK Received - deliberately not treated as an error (see
// `TD_CS_ERROR_MASK`): the HC auto-retries a NAK every frame on its own
// without clearing Active, exactly the "device has nothing new yet" case
// `submit_and_wait`'s `StillActive` outcome already handles.
const TD_CS_BABBLE:      u32 = 1 << 20;
const TD_CS_DATABUF_ERR: u32 = 1 << 21;
const TD_CS_STALLED:     u32 = 1 << 22;
const TD_CS_ACTIVE:      u32 = 1 << 23;
const TD_CS_LS:          u32 = 1 << 26;
const TD_CS_CERR_SHIFT:  u32 = 27;
const TD_CS_ERROR_MASK: u32 =
    TD_CS_BITSTUFF | TD_CS_CRC_TIMEOUT | TD_CS_BABBLE | TD_CS_DATABUF_ERR | TD_CS_STALLED;

const PID_SETUP: u32 = 0x2D;
const PID_IN:    u32 = 0x69;
const PID_OUT:   u32 = 0xE1;

#[repr(C, align(16))]
#[derive(Clone, Copy)]
struct Td {
    link: u32,
    cs: u32,
    token: u32,
    buffer: u32,
}

#[repr(C, align(16))]
struct Qh {
    horizontal: u32,
    element: u32,
}

#[repr(C, align(4096))]
struct FrameList([u32; 1024]);

static mut IO_BASE: u16 = 0;
static mut FRAME_LIST: FrameList = FrameList([LINK_TERMINATE; 1024]);
static mut QH: Qh = Qh { horizontal: LINK_TERMINATE, element: LINK_TERMINATE };
/// Only one TD is ever "in flight" (see module doc comment), reused by
/// every `submit_and_wait` call.
static mut TD: Td = Td { link: LINK_TERMINATE, cs: 0, token: 0, buffer: 0 };

fn r16(offset: u16) -> u16 { unsafe { port::inw(IO_BASE + offset) } }
fn w16(offset: u16, val: u16) { unsafe { port::outw(IO_BASE + offset, val) } }
fn w32(offset: u16, val: u32) { unsafe { port::outl(IO_BASE + offset, val) } }
fn w8(offset: u16, val: u8) { unsafe { port::outb(IO_BASE + offset, val) } }

// `TD`/`QH`/`FRAME_LIST` are ordinary RAM, not memory-mapped registers, but
// the UHCI controller writes to them behind the compiler's back via DMA -
// exactly like any other producer/consumer shared memory, a plain load
// inside `wait_until`'s retry loop is legal for LLVM to hoist out of the
// loop entirely (nothing in this translation unit *looks* like it can
// modify a `static` that no reachable code ever writes to from the
// optimizer's point of view), which would turn "poll until the hardware
// clears Active" into "read Active once, then spin on a compile-time
// constant forever." `read_volatile`/`write_volatile` are the only correct
// way to access this memory.
fn qh_addr() -> u32 { core::ptr::addr_of!(QH) as u32 }
fn qh_set_element(val: u32) { unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(QH.element), val) } }
fn qh_set_horizontal(val: u32) { unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(QH.horizontal), val) } }

fn td_addr() -> u32 { core::ptr::addr_of!(TD) as u32 }
fn td_write(val: Td) { unsafe { core::ptr::write_volatile(core::ptr::addr_of_mut!(TD), val) } }
fn td_read_cs() -> u32 { unsafe { core::ptr::read_volatile(core::ptr::addr_of!(TD.cs)) } }

fn frame_list_addr() -> u32 { core::ptr::addr_of!(FRAME_LIST) as u32 }
fn frame_list_fill(entry: u32) {
    unsafe {
        for slot in FRAME_LIST.0.iter_mut() {
            core::ptr::write_volatile(slot, entry);
        }
    }
}

fn log(s: &str) {
    crate::arch::x86_64::console::print_str(s);
}

fn log_dec(mut n: u32) {
    let mut buf = [0u8; 10];
    let mut i = 10;
    if n == 0 {
        log("0");
        return;
    }
    while n > 0 && i > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        log(s);
    }
}

/// Busy-wait (bounded, via `pit::delay_us` in small steps) for `cond` to
/// become true. Returns `false` on timeout - the same "give up, don't hang
/// forever" contract `dwc2::wait_for` uses.
fn wait_until(timeout_us: u32, mut cond: impl FnMut() -> bool) -> bool {
    const STEP_US: u32 = 100;
    let mut elapsed = 0u32;
    loop {
        if cond() {
            return true;
        }
        if elapsed >= timeout_us {
            return false;
        }
        pit::delay_us(STEP_US);
        elapsed += STEP_US;
    }
}

/// Find a UHCI controller on the PCI bus and bring it up: global reset,
/// host controller reset, empty schedule (every frame-list slot points at
/// one static, initially-empty Queue Head), then Run/Stop. Returns `false`
/// (logging why) if no UHCI controller exists at all - normal on a QEMU
/// `q35` machine started without `-usb`/`-device piix3-usb-uhci`, not a
/// fatal condition for the rest of the kernel.
pub fn init() -> bool {
    let Some(dev) = pci::find_device(0x0C, 0x03, 0x00) else {
        log("usb: no UHCI controller on the PCI bus\n");
        return false;
    };
    dev.enable_io_and_bus_master();
    let Some(io_base) = dev.io_bar_port(4) else {
        log("usb: UHCI PCI function has no I/O-space BAR4\n");
        return false;
    };
    unsafe { IO_BASE = io_base };
    log("usb: UHCI controller found, io_base=");
    log_dec(io_base as u32);
    log("\n");

    // Global reset: broadcasts a bus reset on every downstream port. Spec
    // minimum hold time is 10 ms; matching `dwc2.rs`'s "be generous" style.
    w16(USBCMD, CMD_GRESET);
    pit::delay_ms(50);
    w16(USBCMD, 0);
    pit::delay_ms(1);

    // Host controller reset: re-initializes internal HC state (not the bus
    // itself). Self-clears when done.
    w16(USBCMD, CMD_HCRESET);
    if !wait_until(10_000, || r16(USBCMD) & CMD_HCRESET == 0) {
        log("usb: UHCI HCRESET never cleared, giving up\n");
        return false;
    }

    w16(USBINTR, 0); // polling only, no interrupt routing needed
    w16(FRNUM, 0);

    qh_set_horizontal(LINK_TERMINATE); // only one QH, nothing else in the schedule
    qh_set_element(LINK_TERMINATE); // empty queue until a transfer is submitted
    let qh_link = (qh_addr() & !0xF) | LINK_QH_SELECT;
    frame_list_fill(qh_link);
    w32(FRBASEADD, frame_list_addr());

    w8(SOFMOD, 0x40); // default 12000-bit SOF timing value (UHCI spec section 3.10)
    w16(USBCMD, CMD_RS | CMD_CF | CMD_MAXP64);
    pit::delay_ms(1); // let the schedule actually start running

    if r16(USBSTS) & STS_HALTED != 0 {
        log("usb: UHCI controller halted right after start, giving up\n");
        return false;
    }

    log("usb: UHCI controller running\n");
    true
}

/// UHCI's root hub is always exactly 2 ports (Intel UHCI 1.1 section 2.1.2);
/// unlike DWC2's single root port feeding a real external hub chip, these
/// are two independent physical/virtual ports directly on the controller.
pub fn port_count() -> u8 {
    2
}

fn portsc_addr(port_1based: u8) -> u16 {
    if port_1based == 1 { PORTSC1 } else { PORTSC2 }
}

/// Set or clear one of PORTSC's two R/W control bits (PR or PE), leaving
/// every other control bit as-is and never writing the W1C status-change
/// bits (CSC/PEC) as 1 - doing that would silently ack a change the caller
/// hasn't looked at yet, same hazard as `dwc2::HPRT_W1C_MASK`. Explicitly
/// clearing `bit` out of the read-back value (rather than just OR-ing the
/// desired state in) matters for turning something *off*: `cur` still has
/// the bit set at that point, so a plain OR would never actually clear it.
/// `offset` here (and everywhere else in this module) is relative to
/// `IO_BASE`, always going through `r16`/`w16` rather than `port::inw`/
/// `outw` directly - those two take an absolute port number, and passing
/// a bare register offset to them would silently address whatever's
/// actually at that low, fixed port number on the motherboard instead of
/// this controller's own registers.
fn set_portsc_bit(offset: u16, bit: u16, on: bool) {
    let cur = r16(offset);
    let base = (cur & !PORTSC_W1C_MASK & !bit) | PORTSC_RESERVED1;
    w16(offset, if on { base | bit } else { base });
}

/// `(connected, change_since_last_ack)` for one root port (1-based).
pub fn port_connection(port_1based: u8) -> (bool, bool) {
    let status = r16(portsc_addr(port_1based));
    (status & PORTSC_CCS != 0, status & PORTSC_CSC != 0)
}

pub fn ack_port_connection_change(port_1based: u8) {
    let addr = portsc_addr(port_1based);
    let cur = r16(addr);
    // Same hazard `set_portsc_bit` already guards against: every bit not
    // explicitly written here is written as 0, so a naive `CSC|RESERVED1`
    // silently clobbers PE (and PR) to 0 - disabling an already-configured
    // device's port - on every rescan that finds CSC pending. Preserve
    // `cur`'s real control bits and only touch the W1C region.
    w16(addr, (cur & !PORTSC_W1C_MASK) | PORTSC_CSC | PORTSC_RESERVED1);
}

/// Reset and enable a root port that's currently connected, returning its
/// negotiated speed. `None` if it doesn't come up (nothing plugged in after
/// all, or the device failed to enable).
pub fn reset_and_enable_port(port_1based: u8) -> Option<Speed> {
    let addr = portsc_addr(port_1based);
    if r16(addr) & PORTSC_CCS == 0 {
        return None;
    }

    set_portsc_bit(addr, PORTSC_PR, true);
    pit::delay_ms(50); // spec minimum 10 ms reset pulse width; be generous
    set_portsc_bit(addr, PORTSC_PR, false);
    pit::delay_us(500); // reset recovery time

    set_portsc_bit(addr, PORTSC_PE, true);
    // USB 2.0 section 9.2.6.2 "Reset Recovery Time" (TRSTRCY): software must
    // wait at least 10 ms after reset before sending the device's first
    // SETUP packet - the caller sends that immediately after this function
    // returns, so this delay has to cover the full 10 ms on its own.
    pit::delay_ms(50);

    let status = r16(addr);
    if status & PORTSC_PE == 0 {
        log("usb:   port did not enable after reset\n");
        return None;
    }
    Some(if status & PORTSC_LSDA != 0 { Speed::Low } else { Speed::Full })
}

/// UHCI's TD MaxLen field is 11 bits, encoding `length - 1` — the largest
/// length a single TD can carry is therefore 2048 bytes, independent of
/// whatever `ep.max_packet` a (possibly malformed) device descriptor
/// reported. Every chunk handed to `make_td` must be capped to this before
/// `len - 1` is masked down to 11 bits, or the value silently wraps and the
/// TD moves far fewer bytes than the caller believes it requested.
const MAX_TD_LEN: usize = 2048;

fn make_td(pid: u32, ep: &Endpoint, data_toggle: bool, len: u16, buffer: u32) -> Td {
    let mut cs = 3u32 << TD_CS_CERR_SHIFT; // C_ERR=3: hardware retries a NAK'd/erroring transaction up to 3 times on its own before giving up
    cs |= TD_CS_ACTIVE;
    if ep.speed == Speed::Low {
        cs |= TD_CS_LS;
    }

    let max_len_field = if len == 0 { 0x7FF } else { (len as u32 - 1) & 0x7FF };
    let mut token = pid;
    token |= (ep.dev_addr as u32 & 0x7F) << 8;
    token |= (ep.ep_num as u32 & 0xF) << 15;
    if data_toggle {
        token |= 1 << 19;
    }
    token |= max_len_field << 21;

    Td { link: LINK_TERMINATE, cs, token, buffer }
}

enum TdOutcome {
    Done(u32),
    StillActive,
    Error,
}

/// Submit one TD to the (only ever one) Queue Head and wait up to
/// `timeout_us` for it to leave the Active state, then detach it from the
/// schedule either way. `StillActive` covers both "device keeps NAK'ing"
/// (the routine case for an interrupt endpoint with nothing new to report -
/// hardware auto-retries a NAK every frame on its own without clearing
/// Active) and a genuinely unresponsive device; callers decide which one it
/// is by how long they were willing to wait.
fn submit_and_wait(td: Td, timeout_us: u32) -> TdOutcome {
    td_write(td);
    qh_set_element(td_addr()); // 16-byte aligned, so T=0/Q=0 bits are already correct

    let completed = wait_until(timeout_us, || td_read_cs() & TD_CS_ACTIVE == 0);
    let cs = td_read_cs();
    qh_set_element(LINK_TERMINATE);

    // Empirically necessary against QEMU's UHCI model: submitting the next
    // TD immediately back-to-back with essentially no gap (as happens
    // between a control transfer's SETUP/DATA/STATUS stages) intermittently
    // makes the following TD never get serviced at all, even though nothing
    // in the UHCI spec requires a gap here. A short settle time avoids it;
    // negligible next to the timeouts this function is bounded by.
    pit::delay_us(100);

    if !completed {
        return TdOutcome::StillActive;
    }
    if cs & TD_CS_ERROR_MASK != 0 {
        return TdOutcome::Error;
    }
    let actlen_field = cs & TD_CS_ACTLEN_MASK;
    TdOutcome::Done(if actlen_field == TD_CS_ACTLEN_MASK { 0 } else { actlen_field + 1 })
}

/// Generous enough for a control transfer to complete across several
/// hardware-internal NAK retries; a device that's still not answered after
/// this is treated as a real failure by every `control_transfer` caller.
const CONTROL_TIMEOUT_US: u32 = 50_000;
/// One `interrupt_in_poll` call is meant to be a single, quick look at
/// whether a HID report is waiting - not worth blocking the caller's whole
/// input-poll loop on, so this is short; a device that hasn't answered
/// within it just means "nothing new yet," see `TdOutcome::StillActive`.
const INTERRUPT_POLL_TIMEOUT_US: u32 = 3_000;

/// Driver-internal cap on a single control-transfer data stage, matching
/// `dwc2::MAX_XFER` for the same reason: comfortably covers every
/// descriptor `hub.rs` ever reads (device=18 B, config up to 256 B).
pub const MAX_XFER: usize = 256;

#[repr(align(4))]
struct DmaBuf([u8; MAX_XFER]);

pub type Result<T> = core::result::Result<T, ()>;

/// One control transfer: SETUP stage, optional multi-packet DATA stage (one
/// TD per `ep.max_packet`-sized chunk - UHCI has no internal-DMA multi-
/// packet engine like DWC2's, software has to chase each packet), STATUS
/// stage. Returns the number of bytes actually moved in the data stage.
pub fn control_transfer(ep: &Endpoint, setup: &SetupPacket, buf: &mut [u8], data_in: bool) -> Result<usize> {
    if buf.len() > MAX_XFER {
        return Err(());
    }

    // ── SETUP stage: always PID SETUP, DATA0 (toggle=false) ──
    let mut setup_dma = DmaBuf([0; MAX_XFER]);
    setup_dma.0[..8].copy_from_slice(&setup.as_bytes());
    let setup_addr = setup_dma.0.as_ptr() as u32;
    match submit_and_wait(make_td(PID_SETUP, ep, false, 8, setup_addr), CONTROL_TIMEOUT_US) {
        TdOutcome::Done(_) => {}
        _ => return Err(()),
    }

    // ── DATA stage (optional): first packet DATA1, alternating after ──
    let mut transferred = 0usize;
    if !buf.is_empty() {
        let mps = (ep.max_packet as usize).max(1);
        let mut data_dma = DmaBuf([0; MAX_XFER]);
        if !data_in {
            data_dma.0[..buf.len()].copy_from_slice(buf);
        }
        let data_addr = data_dma.0.as_ptr() as u32;

        let mut toggle = true; // DATA1
        let mut offset = 0usize;
        while offset < buf.len() {
            let chunk = (buf.len() - offset).min(mps).min(MAX_TD_LEN);
            let pid = if data_in { PID_IN } else { PID_OUT };
            let td = make_td(pid, ep, toggle, chunk as u16, data_addr + offset as u32);
            match submit_and_wait(td, CONTROL_TIMEOUT_US) {
                TdOutcome::Done(actual) => {
                    let actual = actual as usize;
                    // A short OUT means fewer bytes actually reached the
                    // device than we told the caller; don't silently claim
                    // the full chunk was sent (see MAX_TD_LEN's doc comment
                    // for how this used to happen with an oversized chunk).
                    if !data_in && actual != chunk { return Err(()); }
                    transferred += if data_in { actual } else { chunk };
                }
                _ => return Err(()),
            }
            toggle = !toggle;
            offset += chunk;
        }
        if data_in {
            buf.copy_from_slice(&data_dma.0[..buf.len()]);
        }
    }

    // ── STATUS stage: opposite direction of the data stage, always DATA1, zero length ──
    let status_in = buf.is_empty() || !data_in;
    let status_pid = if status_in { PID_IN } else { PID_OUT };
    match submit_and_wait(make_td(status_pid, ep, true, 0, 0), CONTROL_TIMEOUT_US) {
        TdOutcome::Done(_) => Ok(transferred),
        _ => Err(()),
    }
}

/// Cap for a single bulk transfer's DMA buffer. Kept separate from
/// `MAX_XFER` (control transfers / descriptors, 256 B) rather than just
/// bumping that constant: `msc.rs` moves whole storage sectors (512 B) at
/// once, and every existing `DmaBuf` local in this file would otherwise
/// grow along with it for no reason. 4096 B covers one 4K page or eight
/// 512 B sectors in a single transfer.
pub const MAX_BULK_XFER: usize = 4096;

#[repr(align(4))]
struct BulkDmaBuf([u8; MAX_BULK_XFER]);

/// Bulk data-stage transfer (no SETUP/STATUS stage, unlike
/// `control_transfer`): moves `buf` in `ep.max_packet`-sized chunks, one TD
/// at a time, same as `control_transfer`'s DATA stage loop.
///
/// Unlike a control transfer's data stage - which always starts at DATA1
/// and only lives for the duration of one transfer - a bulk endpoint owns
/// its data-toggle state for its entire lifetime, reset to DATA0 only by
/// `SET_CONFIGURATION` (USB 2.0 §9.4.5, §5.8.5 for the general rule). The
/// caller must thread the same `toggle` through every call for a given
/// endpoint, exactly like `interrupt_in_poll`'s.
///
/// A short IN packet (actual length < requested chunk) ends the transfer
/// early per USB 2.0 §5.8.3 - normal for the last packet of a
/// not-evenly-divisible transfer (e.g. a 13-byte CSW), not an error.
///
/// `StillActive` (device kept NAK'ing until `timeout_us`) is treated as a
/// hard error here, unlike `interrupt_in_poll`: there's no "nothing new to
/// report yet" case for a command the caller is actively waiting on.
pub fn bulk_transfer(
    ep: &Endpoint,
    toggle: &mut bool,
    buf: &mut [u8],
    data_in: bool,
    timeout_us: u32,
) -> Result<usize> {
    if buf.len() > MAX_BULK_XFER {
        return Err(());
    }
    if buf.is_empty() {
        return Ok(0);
    }

    let mps = (ep.max_packet as usize).max(1);
    let mut dma = BulkDmaBuf([0; MAX_BULK_XFER]);
    if !data_in {
        dma.0[..buf.len()].copy_from_slice(buf);
    }
    let base_addr = dma.0.as_ptr() as u32;

    let mut transferred = 0usize;
    let mut offset = 0usize;
    while offset < buf.len() {
        let chunk = (buf.len() - offset).min(mps).min(MAX_TD_LEN);
        let pid = if data_in { PID_IN } else { PID_OUT };
        let td = make_td(pid, ep, *toggle, chunk as u16, base_addr + offset as u32);
        match submit_and_wait(td, timeout_us) {
            TdOutcome::Done(actual) => {
                *toggle = !*toggle;
                let actual = actual as usize;
                // A short OUT means fewer bytes actually reached the device
                // than we told the caller; don't silently advance past data
                // that was never really sent (see MAX_TD_LEN's doc comment).
                if !data_in && actual != chunk { return Err(()); }
                transferred += if data_in { actual } else { chunk };
                offset += chunk;
                if data_in && actual < chunk {
                    break;
                }
            }
            _ => return Err(()),
        }
    }

    if data_in {
        let n = transferred.min(buf.len());
        buf[..n].copy_from_slice(&dma.0[..n]);
    }
    Ok(transferred)
}

/// Single-shot interrupt IN poll: one attempt to read up to `buf.len()`
/// bytes from `ep`'s interrupt endpoint. `toggle` is the caller-owned
/// per-endpoint data toggle (reset to DATA0/`false` right after
/// `SET_CONFIGURATION`, per USB 2.0 §5.8.5) - a hardcoded toggle here (an
/// earlier version of this function always used DATA1) doesn't just risk an
/// occasional redundant report: since the device's own toggle keeps
/// alternating with every report it successfully sends, a host that never
/// matches it drops essentially every report as a phantom retransmission,
/// which is indistinguishable from "device has nothing new" (`StillActive`).
pub fn interrupt_in_poll(ep: &Endpoint, toggle: &mut bool, buf: &mut [u8]) -> Result<usize> {
    if buf.len() > MAX_XFER {
        return Err(());
    }
    let dma = DmaBuf([0; MAX_XFER]);
    let dma_addr = dma.0.as_ptr() as u32;
    match submit_and_wait(make_td(PID_IN, ep, *toggle, buf.len() as u16, dma_addr), INTERRUPT_POLL_TIMEOUT_US) {
        TdOutcome::Done(actual) => {
            *toggle = !*toggle;
            let actual = (actual as usize).min(buf.len());
            buf[..actual].copy_from_slice(&dma.0[..actual]);
            Ok(actual)
        }
        TdOutcome::StillActive => Ok(0),
        TdOutcome::Error => Err(()),
    }
}
