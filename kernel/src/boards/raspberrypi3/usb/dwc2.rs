//! DWC2 (Synopsys DesignWare Hi-Speed USB 2.0 OTG) host controller driver —
//! the BCM2837's on-chip USB core, wired on every Raspberry Pi 3 to an
//! onboard SMSC LAN9514 chip (a USB hub + 10/100 Ethernet MAC) that fans out
//! to the board's physical USB-A ports.
//!
//! Polling for completion (no CPU interrupt routing - see below), but
//! internal-DMA for the actual data movement: `GHWCFG2`'s Architecture field
//! reads back "Internal DMA" on real BCM2837 silicon, and slave/PIO-mode
//! transfers (manually pushing/popping channel FIFOs) never completed a
//! single control transfer against it - every attempt produced a channel
//! that enabled cleanly, on a port with a confirmed-running frame counter,
//! and then reported zero interrupt-status activity at all until timeout,
//! consistent with a core that doesn't actually implement the slave-mode
//! data path despite the register bit existing. DMA buffers are physical
//! addresses handed straight to `HCDMA`; since the CPU's D-cache is on by
//! now, every buffer needs the same manual clean-before-OUT/
//! invalidate-after-IN dance `mailbox.rs` already does for the same reason.
//!
//! No CPU interrupt routing: completion is still detected by polling
//! `HCINT` directly rather than taking an actual IRQ, avoiding the need to
//! wire this peripheral into the legacy interrupt controller at all. Every
//! wait loop below is bounded by a real millisecond/microsecond timeout
//! derived from the ARM generic timer (`delay_us`), USB has hard timing
//! requirements (reset pulse width, recovery time, ...) a NOP-count spin
//! can't meet reliably across core frequencies.
//!
//! Scope actually implemented: control transfers (needed for the whole
//! enumeration handshake), single-shot interrupt-IN polls (needed for HID
//! reports), and bulk transfers (needed for USB Mass Storage's Bulk-Only
//! Transport, `msc.rs`) - all three with split-transaction support for
//! Low/Full-speed devices behind the LAN9514's High-speed hub
//! (`Endpoint::needs_split`). No isochronous, no OUT interrupt.
//!
//! Unlike the x86_64/UHCI sibling driver - which has to submit one Transfer
//! Descriptor per max-packet-sized chunk and chase the schedule itself for
//! a multi-packet transfer - this core's internal DMA engine walks a
//! multi-packet buffer on its own from a single `HCTSIZ`/`HCDMA` setup, so
//! `bulk_transfer` below (like `control_transfer`'s DATA stage already did)
//! is one `configure_channel`/`run_channel` call regardless of how many
//! packets the transfer actually spans.

use super::protocol::{Endpoint, SetupPacket, Speed};

const USB_BASE: usize = crate::boards::raspberrypi3::PERIPHERAL_BASE + 0x0098_0000;

// ── Core global registers ─────────────────────────────────────────────────
const GOTGCTL:   usize = USB_BASE;
const GAHBCFG:   usize = USB_BASE + 0x008;
const GUSBCFG:   usize = USB_BASE + 0x00C;
const GRSTCTL:   usize = USB_BASE + 0x010;
const GINTSTS:   usize = USB_BASE + 0x014;
const GINTMSK:   usize = USB_BASE + 0x018;
const GRXFSIZ:   usize = USB_BASE + 0x024;
const GNPTXFSIZ: usize = USB_BASE + 0x028;
const HPTXFSIZ:  usize = USB_BASE + 0x100;
const GSNPSID:   usize = USB_BASE + 0x040; // "Synopsys ID" - reads a fixed vendor magic, never 0 or all-1s on a real core
const GHWCFG1:   usize = USB_BASE + 0x044;
const GHWCFG2:   usize = USB_BASE + 0x048;
const GHWCFG3:   usize = USB_BASE + 0x04C;
const GHWCFG4:   usize = USB_BASE + 0x050;

// ── Host mode registers ───────────────────────────────────────────────────
const HCFG:     usize = USB_BASE + 0x400;
const HFNUM:    usize = USB_BASE + 0x408;
const HAINTMSK: usize = USB_BASE + 0x418;
const HPRT:     usize = USB_BASE + 0x440;

fn hcchar(n: usize)   -> usize { USB_BASE + 0x500 + n * 0x20 }
fn hcsplt(n: usize)   -> usize { USB_BASE + 0x504 + n * 0x20 }
fn hcint(n: usize)    -> usize { USB_BASE + 0x508 + n * 0x20 }
fn hcintmsk(n: usize) -> usize { USB_BASE + 0x50C + n * 0x20 }
fn hctsiz(n: usize)   -> usize { USB_BASE + 0x510 + n * 0x20 }
fn hcdma(n: usize)    -> usize { USB_BASE + 0x514 + n * 0x20 }

const GRSTCTL_CSFTRST: u32 = 1 << 0;
const GRSTCTL_RXFFLSH: u32 = 1 << 4;
const GRSTCTL_TXFFLSH: u32 = 1 << 5;
const GRSTCTL_AHBIDLE: u32 = 1 << 31;

const GUSBCFG_FORCEHOSTMODE: u32 = 1 << 29;
const GAHBCFG_DMAEN: u32 = 1 << 5;
const GAHBCFG_GLBLINTRMSK: u32 = 1 << 0;

const HPRT_CONNSTS:     u32 = 1 << 0;
const HPRT_CONNDET:     u32 = 1 << 1;
const HPRT_ENA:         u32 = 1 << 2;
const HPRT_ENCHNG:      u32 = 1 << 3;
const HPRT_OVERCURRCHNG: u32 = 1 << 5;
const HPRT_RST:         u32 = 1 << 8;
const HPRT_PWR:         u32 = 1 << 12;
const HPRT_SPD_SHIFT:   u32 = 17;
const HPRT_SPD_MASK:    u32 = 0b11 << HPRT_SPD_SHIFT;
/// Write-1-to-clear status bits mixed into HPRT alongside its few genuine
/// read/write control bits (PWR, RST), every read-modify-write of this
/// register must mask these out first, or an unrelated write (e.g. turning
/// port power on) silently also acks a pending connect/enable-change flag
/// the caller hasn't looked at yet.
const HPRT_W1C_MASK: u32 = HPRT_CONNDET | HPRT_ENCHNG | HPRT_OVERCURRCHNG;

const HCCHAR_CHENA:  u32 = 1 << 31;
const HCCHAR_CHDIS:  u32 = 1 << 30;
const HCCHAR_LSPDDEV: u32 = 1 << 17;
const HCCHAR_EPDIR_IN: u32 = 1 << 15;

const HCSPLT_SPLTENA: u32 = 1 << 31;
const HCSPLT_XACTPOS_ALL: u32 = 0b11 << 14;

const HCINT_XFERCOMPL: u32 = 1 << 0;
const HCINT_CHHLTD:    u32 = 1 << 1;
const HCINT_STALL:     u32 = 1 << 3;
const HCINT_NAK:       u32 = 1 << 4;
const HCINT_ACK:       u32 = 1 << 5;
const HCINT_NYET:      u32 = 1 << 6;
const HCINT_XACTERR:   u32 = 1 << 7;
const HCINT_BBLERR:    u32 = 1 << 8;
const HCINT_FRMOVRUN:  u32 = 1 << 9;
const HCINT_DATATGLERR: u32 = 1 << 10;
const HCINT_ALL: u32 = 0x7FF;

const PID_DATA0: u32 = 0b00;
const PID_DATA1: u32 = 0b10;
const PID_SETUP: u32 = 0b11;

const EPTYPE_CONTROL:   u32 = 0;
const EPTYPE_BULK:      u32 = 2;
const EPTYPE_INTERRUPT: u32 = 3;

fn reg(addr: usize) -> *mut u32 {
    addr as *mut u32
}

fn read(addr: usize) -> u32 {
    unsafe { reg(addr).read_volatile() }
}

fn write(addr: usize, val: u32) {
    unsafe { reg(addr).write_volatile(val) };
}

/// aarch64 D-cache line size in bytes, from `CTR_EL0.DminLine` - same
/// technique as `mailbox.rs` (Cortex-A53 is always 64 B, but this reads the
/// real value instead of assuming it).
fn dcache_line_size() -> usize {
    let ctr: u64;
    unsafe { core::arch::asm!("mrs {v}, ctr_el0", v = out(reg) ctr, options(nostack)) };
    4usize << ((ctr >> 16) & 0xF)
}

/// Flush CPU-cached writes to RAM before handing a buffer's address to
/// `HCDMA` for an OUT/SETUP transfer - the DMA engine reads directly from
/// RAM, bypassing the CPU cache entirely, so a write that's still only in
/// cache is invisible to it.
fn clean_range(start: usize, len: usize) {
    let line = dcache_line_size();
    let end = start + len;
    let mut addr = start & !(line - 1);
    unsafe {
        while addr < end {
            core::arch::asm!("dc cvac, {a}", a = in(reg) addr, options(nostack));
            addr += line;
        }
        core::arch::asm!("dsb sy", options(nostack));
    }
}

/// Discard stale cached copies of a buffer after an IN transfer wrote fresh
/// data to RAM via DMA, so a subsequent CPU read sees what the device
/// actually sent instead of whatever was cached before the transfer.
fn invalidate_range(start: usize, len: usize) {
    let line = dcache_line_size();
    let end = start + len;
    let mut addr = start & !(line - 1);
    unsafe {
        core::arch::asm!("dsb sy", options(nostack));
        while addr < end {
            core::arch::asm!("dc ivac, {a}", a = in(reg) addr, options(nostack));
            addr += line;
        }
        core::arch::asm!("dsb sy", options(nostack));
    }
}

/// Real-time microsecond delay via the ARM generic timer counter, USB
/// timing (reset pulse width, recovery time, hub port power-on settling)
/// has hard millisecond-scale minimums a cycle-count spin can't guarantee
/// across CPU frequency scaling.
fn delay_us(us: u64) {
    let freq: u64;
    unsafe { core::arch::asm!("mrs {v}, cntfrq_el0", v = out(reg) freq, options(nostack)) };
    let target = freq / 1_000_000 * us;
    let start: u64;
    unsafe { core::arch::asm!("mrs {v}, cntpct_el0", v = out(reg) start, options(nostack)) };
    loop {
        let now: u64;
        unsafe { core::arch::asm!("mrs {v}, cntpct_el0", v = out(reg) now, options(nostack)) };
        if now.wrapping_sub(start) >= target {
            break;
        }
    }
}

pub fn delay_ms(ms: u64) {
    delay_us(ms * 1000);
}

fn log(s: &str) {
    crate::arch::aarch64::console::print_str(s);
}

fn log_hex_bare(val: u32) {
    let mut buf = [b'0'; 8];
    for (j, slot) in buf.iter_mut().enumerate() {
        let nibble = ((val >> (28 - j * 4)) & 0xF) as u8;
        *slot = if nibble < 10 { b'0' + nibble } else { b'a' + nibble - 10 };
    }
    if let Ok(s) = core::str::from_utf8(&buf) {
        log(s);
    }
}

fn log_hex(label: &str, val: u32) {
    log(label);
    log_hex_bare(val);
    log("\n");
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

/// Spin until `cond(read(addr))` is true or `timeout_us` elapses.
/// Returns `false` on timeout, every caller below treats that as "this
/// piece of hardware bring-up failed," not as something to retry forever.
fn wait_for(addr: usize, timeout_us: u64, cond: impl Fn(u32) -> bool) -> bool {
    let freq: u64;
    unsafe { core::arch::asm!("mrs {v}, cntfrq_el0", v = out(reg) freq, options(nostack)) };
    let target = freq / 1_000_000 * timeout_us;
    let start: u64;
    unsafe { core::arch::asm!("mrs {v}, cntpct_el0", v = out(reg) start, options(nostack)) };
    loop {
        if cond(read(addr)) {
            return true;
        }
        let now: u64;
        unsafe { core::arch::asm!("mrs {v}, cntpct_el0", v = out(reg) now, options(nostack)) };
        if now.wrapping_sub(start) >= target {
            return false;
        }
    }
}

/// Root port speed, once negotiated (valid after `init` returns `true`).
pub fn root_speed() -> Speed {
    match (read(HPRT) & HPRT_SPD_MASK) >> HPRT_SPD_SHIFT {
        1 => Speed::Full,
        2 => Speed::Low,
        _ => Speed::High,
    }
}

/// `true` if the root port is still connected and enabled. Some real
/// hardware (and some hub chips) auto-disable a port on certain fault
/// conditions (e.g. a babble) without the host asking for it - if that
/// happens mid-enumeration, every transaction after the fault would fail
/// identically no matter how many times or how patiently a caller retries
/// the *request*, since the electrical link itself dropped. Called by
/// `hub.rs`'s SET_ADDRESS retry loop to tell "device is slow to respond"
/// (worth just waiting longer) apart from "the port itself gave up"
/// (worth re-resetting instead).
pub fn root_port_ok() -> bool {
    let status = read(HPRT);
    status & HPRT_CONNSTS != 0 && status & HPRT_ENA != 0
}

/// Re-run just the reset-and-enable half of `init` (port power is already
/// on and a device is already known to be connected at this point) to
/// recover from the root port having dropped `HPRT_ENA` on its own mid-
/// enumeration. Returns whether the port came back up enabled.
pub fn recover_root_port() -> bool {
    log("usb:   attempting to re-reset root port...\n");
    write(HPRT, (read(HPRT) & !HPRT_W1C_MASK) | HPRT_RST);
    delay_ms(50);
    write(HPRT, read(HPRT) & !HPRT_W1C_MASK & !HPRT_RST);
    delay_ms(50);
    let ok = read(HPRT) & HPRT_ENA != 0;
    log(if ok { "usb:   root port re-enabled\n" } else { "usb:   root port still not enabled\n" });
    ok
}

/// Bring up the DWC2 core in host mode, power and reset the root port, and
/// report whether a device answered. `false` means "no USB device attached
/// or the core didn't respond as expected", callers treat that as
/// "no keyboard," not a fatal error.
pub fn init() -> bool {
    // Sanity check USB_BASE itself before anything else: GSNPSID is a
    // fixed, always-readable vendor ID register - 0x00000000 or
    // 0xFFFFFFFF here would mean we're not actually talking to the DWC2
    // core at all (wrong base address / unmapped memory), which would
    // make every later HPRT/HCCHAR/HCINT read equally meaningless even
    // where it happens to look plausible.
    let id = read(GSNPSID);
    log("usb: GSNPSID=0x");
    log_hex_bare(id);
    log("\n");
    // Full hardware-capability dump (channel count, FIFO depths, PHY type,
    // ...) - every FIFO-size/host-config assumption this driver makes was
    // never actually cross-checked against what this specific silicon
    // reports, unlike GSNPSID (now confirmed correct) this is genuinely
    // unverified territory.
    log("usb: GHWCFG1=0x"); log_hex_bare(read(GHWCFG1)); log("\n");
    log("usb: GHWCFG2=0x"); log_hex_bare(read(GHWCFG2)); log("\n");
    log("usb: GHWCFG3=0x"); log_hex_bare(read(GHWCFG3)); log("\n");
    log("usb: GHWCFG4=0x"); log_hex_bare(read(GHWCFG4)); log("\n");
    if id == 0 || id == 0xFFFF_FFFF {
        log("usb: BADBASE\n");
        return false;
    }

    log("usb: dwc2 core reset...\n");
    if !wait_for(GRSTCTL, 100_000, |v| v & GRSTCTL_AHBIDLE != 0) {
        log("usb: AHB never went idle, giving up\n");
        return false;
    }
    write(GRSTCTL, GRSTCTL_CSFTRST);
    if !wait_for(GRSTCTL, 100_000, |v| v & GRSTCTL_CSFTRST == 0) {
        log("usb: core soft reset never cleared, giving up\n");
        return false;
    }
    if !wait_for(GRSTCTL, 100_000, |v| v & GRSTCTL_AHBIDLE != 0) {
        log("usb: AHB never went idle after reset, giving up\n");
        return false;
    }
    delay_us(1000);

    log("usb: forcing host mode...\n");
    write(GUSBCFG, read(GUSBCFG) | GUSBCFG_FORCEHOSTMODE);
    delay_ms(25); // mode-switch settling time

    // Enable internal DMA (GHWCFG2's Architecture field confirms that's
    // this core's only real mode - see module docs) and unmask every
    // interrupt-status register in the chain (GAHBCFG.GlblIntrMsk, GINTMSK,
    // HAINTMSK, and each channel's HCINTMSK in `configure_channel`) even
    // though the actual IRQ line is never routed anywhere (no GIC/legacy-IC
    // wiring exists for this peripheral) - harmless since nothing can ever
    // consume a resulting interrupt, but cheap insurance against internal
    // event capture being gated on them too, not just external delivery.
    write(GAHBCFG, read(GAHBCFG) | GAHBCFG_DMAEN | GAHBCFG_GLBLINTRMSK);
    write(GINTMSK, 0xFFFF_FFFF);
    write(HAINTMSK, 0xFFFF_FFFF);

    // Conservative fixed FIFO partition (RX 512 words, non-periodic TX 256
    // words, periodic TX 256 words = 1024 of the core's ~4080-word FIFO RAM)
    //, plenty for control/interrupt-only traffic at one device at a time.
    write(GRXFSIZ, 0x200);
    write(GNPTXFSIZ, (0x100 << 16) | 0x200);
    write(HPTXFSIZ, (0x100 << 16) | 0x300);

    write(GRSTCTL, GRSTCTL_RXFFLSH);
    wait_for(GRSTCTL, 10_000, |v| v & GRSTCTL_RXFFLSH == 0);
    write(GRSTCTL, GRSTCTL_TXFFLSH | (0x10 << 6)); // flush all TX FIFOs
    wait_for(GRSTCTL, 10_000, |v| v & GRSTCTL_TXFFLSH == 0);

    // FSLSPCLKSEL=0: PHY clock for the High-speed-capable root port. This
    // core's root port always runs High-speed on a Pi 3 (the LAN9514 is a
    // High-speed hub), only devices *behind* it can be slower, handled by
    // split transactions instead of this field.
    write(HCFG, 0);

    log("usb: powering root port...\n");
    write(HPRT, (read(HPRT) & !HPRT_W1C_MASK) | HPRT_PWR);
    delay_ms(20);

    if !wait_for(HPRT, 500_000, |v| v & HPRT_CONNSTS != 0) {
        log("usb: no device detected on root port (no USB device plugged in?)\n");
        return false;
    }
    log("usb: device detected, resetting port...\n");
    delay_ms(100); // USB2.0 debounce interval before reset

    write(HPRT, (read(HPRT) & !HPRT_W1C_MASK) | HPRT_RST);
    delay_ms(50); // reset pulse width (spec minimum 10 ms; be generous)
    write(HPRT, read(HPRT) & !HPRT_W1C_MASK & !HPRT_RST);
    delay_ms(50); // reset recovery time (spec minimum 10 ms; be generous on real silicon)

    if read(HPRT) & HPRT_ENA == 0 {
        log("usb: port did not enable after reset\n");
        return false;
    }

    let speed = root_speed();
    log(match speed {
        Speed::High => "usb: root port up, speed=High\n",
        Speed::Full => "usb: root port up, speed=Full\n",
        Speed::Low  => "usb: root port up, speed=Low\n",
    });

    // Diagnostic only: 0x3FFF in the frame-number field is this core's
    // "host has not actually started generating SOF on the bus yet"
    // sentinel. If every channel transaction times out with zero HCINT
    // activity (as observed), this confirms or rules out "the host never
    // truly started" as the cause, in one unambiguous word instead of a
    // hex dump.
    delay_ms(5); // give a few frame intervals (125 us - 1 ms each) to elapse
    if read(HFNUM) & 0x3FFF == 0x3FFF {
        log("usb: FRAMESTUCK\n");
    } else {
        log("usb: FRAMERUNNING\n");
    }

    true
}

/// Result of a control transfer: number of bytes actually moved in the data
/// stage (0 for no-data-stage requests).
pub type Result<T> = core::result::Result<T, ()>;

fn configure_channel(
    chan: usize, ep: &Endpoint, eptype: u32, dir_in: bool, pid: u32, xfer_size: u32, dma_addr: usize,
) {
    // `ep.max_packet` before enumeration completes comes straight from a
    // device-reported descriptor byte (bMaxPacketSize0) - a malformed or
    // not-yet-actually-transferred response reading back as 0 must never
    // crash the host via a divide-by-zero here.
    let mps = (ep.max_packet as usize).max(1);
    let pktcnt = if xfer_size == 0 { 1 } else { (xfer_size as usize).div_ceil(mps) as u32 };

    // We poll HCINT directly rather than relying on an actual interrupt, but
    // unmask everything here anyway (see the comment on GAHBCFG/GINTMSK/
    // HAINTMSK in `init`) in case this core needs it unmasked for HCINT
    // itself to ever update, not just to propagate further.
    write(hcintmsk(chan), HCINT_ALL);
    write(hcint(chan), HCINT_ALL); // clear stale flags from any previous use

    let mut hcchar_val = (ep.max_packet as u32 & 0x7FF)
        | ((ep.ep_num as u32 & 0xF) << 11)
        | (eptype << 18)
        | (1 << 20) // EC/MC = 1 (one transaction per microframe; no split-multi)
        | ((ep.dev_addr as u32 & 0x7F) << 22);
    if dir_in {
        hcchar_val |= HCCHAR_EPDIR_IN;
    }
    if ep.speed == Speed::Low {
        hcchar_val |= HCCHAR_LSPDDEV;
    }
    write(hcchar(chan), hcchar_val);

    let mut hcsplt_val = 0u32;
    if ep.needs_split() {
        hcsplt_val = HCSPLT_SPLTENA
            | HCSPLT_XACTPOS_ALL
            | ((ep.hub_addr as u32 & 0x7F) << 7)
            | (ep.hub_port as u32 & 0x7F);
    }
    write(hcsplt(chan), hcsplt_val);

    write(hctsiz(chan), (pid << 29) | (pktcnt << 19) | (xfer_size & 0x7FFFF));
    write(hcdma(chan), dma_addr as u32);
}

/// Set once the first `HCINT_XACTERR` register dump has fired (see
/// `run_channel`) - deliberately global/one-shot, not per-call, so a real
/// device retried several times only prints its full register state once
/// instead of scrolling a small fbconsole screen off with duplicates.
static mut XACTERR_DUMPED: bool = false;

/// Start the channel and wait (bounded) for it to either complete, halt, or
/// report an error. NAK/NYET (device has nothing yet / not ready) are
/// retried up to a bounded count rather than treated as failures, routine
/// on an interrupt endpoint the device only fills occasionally.
///
/// `label` is purely diagnostic: every failure path logs it plus a single,
/// short, visually-unambiguous keyword (never a hex/decimal value - those
/// have proven unreliable to read back off a small HDMI console character
/// by character) so a failure reported by a caller several layers up (e.g.
/// `hub::enumerate_root`'s "GET_DESCRIPTOR(8) failed") can be traced back
/// to exactly which of SETUP/DATA/STATUS failed and why, without needing a
/// second round of instrumentation to find out.
/// Force a channel to stop and wait (bounded) for it to confirm halted,
/// clearing any stale interrupt-status bits along the way. Called on every
/// error/timeout exit from `run_channel` so a wedged channel never lingers
/// enabled - in practice the next `configure_channel` call already
/// overwrites `HCCHAR` wholesale before its own `run_channel` runs, so this
/// is cheap defensive cleanup rather than a fix for an observed hang, but
/// USB error recovery is exactly the kind of thing not worth assuming away.
/// Flush the RX FIFO and every TX FIFO (same sequence `init` runs once at
/// core bring-up). A BABBLE condition in particular - the device sending
/// more than expected - can leave stray/overrun bytes sitting in the RX
/// FIFO; without flushing them out, they'd corrupt the *next* transaction's
/// data instead of just this one's, turning one bad transfer into a
/// cascade of them. Global (not per-channel), but safe here since this
/// driver only ever runs one channel at a time (see module doc comment).
fn flush_fifos() {
    write(GRSTCTL, GRSTCTL_RXFFLSH);
    wait_for(GRSTCTL, 10_000, |v| v & GRSTCTL_RXFFLSH == 0);
    write(GRSTCTL, GRSTCTL_TXFFLSH | (0x10 << 6));
    wait_for(GRSTCTL, 10_000, |v| v & GRSTCTL_TXFFLSH == 0);
}

fn abort_channel(chan: usize) {
    write(hcchar(chan), read(hcchar(chan)) | HCCHAR_CHDIS);
    wait_for(hcint(chan), 5_000, |v| v & HCINT_CHHLTD != 0);
    write(hcint(chan), HCINT_ALL);
    flush_fifos();
}

fn run_channel(chan: usize, max_retries: u32, label: &str) -> Result<u32> {
    for _ in 0..max_retries {
        // CHENA must see a genuine 0->1 edge to kick off a new attempt; a
        // NAK'd/halted channel isn't guaranteed to have actually cleared it
        // in time for us to just OR it back in, so force it low first. This
        // matters most on retry: without it, the previous attempt's halt
        // could still be settling and this write becomes a silent no-op,
        // which is exactly the TIMEOUT case below.
        write(hcchar(chan), read(hcchar(chan)) & !HCCHAR_CHENA);
        write(hcchar(chan), read(hcchar(chan)) | HCCHAR_CHENA);

        // Diagnostic only: confirms the CHENA write actually latched before
        // we spend 50 ms waiting on a channel that may never have started.
        if read(hcchar(chan)) & HCCHAR_CHENA == 0 {
            log("usb: ");
            log(label);
            log(" NOTENABLED\n");
        }

        if !wait_for(hcint(chan), 50_000, |v| v & HCINT_CHHLTD != 0) {
            log("usb: ");
            log(label);
            log(" TIMEOUT\n");
            abort_channel(chan);
            return Err(());
        }
        let status = read(hcint(chan));
        write(hcint(chan), status); // clear what we observed

        if status & HCINT_XFERCOMPL != 0 {
            // CHHLTD is already confirmed (the outer wait_for above only
            // returns once it's set), so the channel is genuinely done -
            // no extra wait needed here before handing back to the caller.
            return Ok(status);
        }
        if status & HCINT_STALL != 0 {
            log("usb: "); log(label); log(" STALL\n");
            abort_channel(chan);
            return Err(());
        }
        if status & HCINT_BBLERR != 0 {
            log("usb: "); log(label); log(" BABBLE\n");
            abort_channel(chan);
            return Err(());
        }
        if status & HCINT_DATATGLERR != 0 {
            log("usb: "); log(label); log(" DATATOGGLE\n");
            abort_channel(chan);
            return Err(());
        }
        if status & HCINT_XACTERR != 0 {
            log("usb: "); log(label); log(" XACTERR\n");
            // Full register dump, but only once - this specific error has
            // resisted every hypothesis testable by reasoning about the
            // driver alone (port drop, FIFO corruption, addressing bit
            // position all checked and ruled out on real hardware), so the
            // raw values are the next thing actually worth looking at.
            // Logged once rather than on every retry so it doesn't scroll
            // the actually-useful line off a small fbconsole screen.
            unsafe {
                if !XACTERR_DUMPED {
                    XACTERR_DUMPED = true;
                    log_hex("usb:   HCCHAR=0x", read(hcchar(chan)));
                    log_hex("usb:   HCSPLT=0x", read(hcsplt(chan)));
                    log_hex("usb:   HCTSIZ=0x", read(hctsiz(chan)));
                    log_hex("usb:   HCDMA=0x", read(hcdma(chan)));
                    log_hex("usb:   HPRT=0x", read(HPRT));
                    log_hex("usb:   GINTSTS=0x", read(GINTSTS));
                }
            }
            abort_channel(chan);
            return Err(());
        }
        // NAK / NYET / ACK-without-XFERCOMPL (split start-split ACK): the
        // device just isn't ready yet, try again.
        delay_us(100);
    }
    log("usb: ");
    log(label);
    log(" GAVEUP\n");
    abort_channel(chan);
    Err(())
}

/// Driver-internal cap on a single control-transfer data stage - comfortably
/// covers every descriptor this driver ever reads (device=18 B, the
/// largest config descriptor `hub::enumerate_as_hid_keyboard` accepts is
/// 256 B) with room to spare, while keeping the DMA scratch buffer below a
/// small, fixed, always-4-byte-aligned stack allocation.
const MAX_XFER: usize = 256;

/// 4-byte-aligned scratch buffer for `HCDMA` - `SetupPacket::as_bytes` and
/// caller-provided `&mut [u8]` slices are not guaranteed any particular
/// alignment (a `repr(packed)` struct's byte view has alignment 1), and
/// this core's DMA engine is given a raw physical address to read/write
/// with no reason to assume it tolerates misalignment.
#[repr(align(4))]
struct DmaBuf([u8; MAX_XFER]);

/// One control transfer: SETUP stage, optional single-packet-buffer DATA
/// stage, STATUS stage. `buf` is filled (IN) or read from (OUT) for the
/// data stage; pass an empty slice for no-data-stage requests.
/// Returns the number of bytes actually transferred in the data stage.
pub fn control_transfer(
    ep: &Endpoint,
    setup: &SetupPacket,
    buf: &mut [u8],
    data_in: bool,
) -> Result<usize> {
    const CHAN: usize = 0;
    if buf.len() > MAX_XFER {
        return Err(());
    }

    // ── SETUP stage ──
    let mut setup_dma = DmaBuf([0; MAX_XFER]);
    setup_dma.0[..8].copy_from_slice(&setup.as_bytes());
    let setup_addr = setup_dma.0.as_ptr() as usize;
    clean_range(setup_addr, 8);
    configure_channel(CHAN, ep, EPTYPE_CONTROL, false, PID_SETUP, 8, setup_addr);
    run_channel(CHAN, 10, "SETUP")?;

    // ── DATA stage (optional) ──
    let mut transferred = 0usize;
    if !buf.is_empty() {
        let mut data_dma = DmaBuf([0; MAX_XFER]);
        let dma_addr = data_dma.0.as_ptr() as usize;
        if data_in {
            // `data_dma`'s zero-init above is a CPU store that can still be
            // sitting dirty in cache at this point; without cleaning it out
            // first, that dirty line can get evicted/written back to RAM
            // *after* the DMA below has already written the device's real
            // response there, silently clobbering it back to zero. Only
            // ever observed on real silicon - QEMU's TCG treats `dc`
            // instructions as no-ops, so this never surfaced there. This is
            // what the old comment here ("read back as though nothing had
            // been delivered at all") was actually seeing: not a length
            // problem, a stale-dirty-cache-line problem.
            clean_range(dma_addr, buf.len());
            configure_channel(CHAN, ep, EPTYPE_CONTROL, true, PID_DATA1, buf.len() as u32, dma_addr);
            run_channel(CHAN, 20, "DATAIN")?;
            // Every descriptor this driver reads is requested at an exact,
            // expected length, so treat XFERCOMPL as "got all of it" rather
            // than trusting HCTSIZ's post-completion remaining-byte count
            // to mean what it means in slave/PIO mode.
            invalidate_range(dma_addr, buf.len());
            buf.copy_from_slice(&data_dma.0[..buf.len()]);
            transferred = buf.len();
        } else {
            data_dma.0[..buf.len()].copy_from_slice(buf);
            clean_range(dma_addr, buf.len());
            configure_channel(CHAN, ep, EPTYPE_CONTROL, false, PID_DATA1, buf.len() as u32, dma_addr);
            run_channel(CHAN, 20, "DATAOUT")?;
            transferred = buf.len();
        }
    }

    // ── STATUS stage (opposite direction of the data stage, always DATA1, zero length) ──
    let status_in = buf.is_empty() || !data_in;
    configure_channel(CHAN, ep, EPTYPE_CONTROL, status_in, PID_DATA1, 0, 0);
    run_channel(CHAN, 10, "STATUS")?;

    Ok(transferred)
}

/// Single-shot, non-blocking-in-spirit interrupt IN poll: one attempt to
/// read up to `buf.len()` bytes from `ep`'s interrupt endpoint. A NAK
/// (device has nothing new) is the expected common case, not an error —
/// returns `Ok(0)` for it, distinct from `Err(())` (something actually
/// went wrong, e.g. the device was unplugged).
///
/// `toggle` is the caller-owned per-endpoint data toggle (reset to
/// DATA0/`false` right after `SET_CONFIGURATION`, per USB 2.0 §5.8.5) and
/// must be flipped on every successfully read report - a hardcoded toggle
/// here would desync from the device's own after the first report it sends
/// and reads the DWC2 core reports it as a `DATATGLERR`, indistinguishable
/// from a real transfer error.
pub fn interrupt_in_poll(ep: &Endpoint, toggle: &mut bool, buf: &mut [u8]) -> Result<usize> {
    const CHAN: usize = 1;
    if buf.len() > MAX_XFER {
        return Err(());
    }
    let mut dma = DmaBuf([0; MAX_XFER]);
    let dma_addr = dma.0.as_ptr() as usize;
    // See the matching comment in `control_transfer`'s DATAIN stage: clean
    // this buffer's freshly-zeroed cache lines out *before* the DMA below,
    // or a dirty-line writeback on real hardware can clobber the device's
    // real report back to zero after the transfer already completed.
    clean_range(dma_addr, buf.len());
    let pid = if *toggle { PID_DATA1 } else { PID_DATA0 };
    configure_channel(CHAN, ep, EPTYPE_INTERRUPT, true, pid, buf.len() as u32, dma_addr);

    // Same forced 0->1 edge as `run_channel` (see its comment) - this
    // channel is reused across every poll call, so a previous call's halt
    // that hadn't fully settled would otherwise silently swallow this one.
    write(hcchar(CHAN), read(hcchar(CHAN)) & !HCCHAR_CHENA);
    write(hcchar(CHAN), read(hcchar(CHAN)) | HCCHAR_CHENA);
    if !wait_for(hcint(CHAN), 5_000, |v| v & HCINT_CHHLTD != 0) {
        return Ok(0); // no response within one poll window: treat as "nothing yet"
    }
    let status = read(hcint(CHAN));
    write(hcint(CHAN), status);

    if status & HCINT_XFERCOMPL != 0 {
        // See the matching comment in `control_transfer`'s DATAIN stage -
        // HID boot reports are always exactly 8 bytes, so XFERCOMPL alone
        // means "got the full report."
        invalidate_range(dma_addr, buf.len());
        buf.copy_from_slice(&dma.0[..buf.len()]);
        *toggle = !*toggle;
        Ok(buf.len())
    } else if status & (HCINT_NAK | HCINT_NYET) != 0 {
        Ok(0)
    } else if status & (HCINT_STALL | HCINT_BBLERR | HCINT_XACTERR | HCINT_DATATGLERR) != 0 {
        abort_channel(CHAN);
        Err(())
    } else {
        Ok(0)
    }
}

/// Cap for a single bulk transfer's DMA buffer - kept separate from
/// `MAX_XFER` (control transfers/descriptors, 256 B) for the same reason
/// the UHCI sibling driver keeps its own `MAX_BULK_XFER`: `msc.rs` moves
/// whole storage sectors (512 B) at once, and `control_transfer`'s existing
/// `DmaBuf` locals shouldn't grow along with a limit they don't need.
/// 4096 B covers one 4K page or eight 512 B sectors in a single transfer.
const MAX_BULK_XFER: usize = 4096;

#[repr(align(4))]
struct BulkDmaBuf([u8; MAX_BULK_XFER]);

/// Bulk data transfer: SETUP/STATUS-free, moves `buf` in a single DMA burst
/// (see this module's doc comment on why one `configure_channel`/
/// `run_channel` call is enough here, unlike the UHCI sibling).
///
/// Bulk endpoints own their data-toggle state for their whole lifetime,
/// reset to DATA0 only by `SET_CONFIGURATION` (USB 2.0 §9.4.5) - same
/// caller-threaded `toggle` contract as `interrupt_in_poll`. Unlike a
/// single-packet interrupt poll, though, one call here can span several
/// packets, and the core auto-alternates the wire PID between them per the
/// standard USB toggle rule - so the endpoint's toggle only needs flipping
/// here if an *odd* number of packets went out, an even count cancels back
/// out to where it started. `StillActive`-style "nothing new yet" isn't a
/// thing for bulk (unlike interrupt polling): a NAK/error mid-command means
/// the transfer genuinely failed, not that a device chose not to answer yet.
pub fn bulk_transfer(ep: &Endpoint, toggle: &mut bool, buf: &mut [u8], data_in: bool) -> Result<usize> {
    const CHAN: usize = 2;
    if buf.len() > MAX_BULK_XFER {
        return Err(());
    }
    if buf.is_empty() {
        return Ok(0);
    }

    let mps = (ep.max_packet as usize).max(1);
    let pktcnt = buf.len().div_ceil(mps);

    let mut dma = BulkDmaBuf([0; MAX_BULK_XFER]);
    let dma_addr = dma.0.as_ptr() as usize;
    let pid = if *toggle { PID_DATA1 } else { PID_DATA0 };

    if data_in {
        clean_range(dma_addr, buf.len());
        configure_channel(CHAN, ep, EPTYPE_BULK, true, pid, buf.len() as u32, dma_addr);
        run_channel(CHAN, 20, "BULKIN")?;
        invalidate_range(dma_addr, buf.len());
        buf.copy_from_slice(&dma.0[..buf.len()]);
    } else {
        dma.0[..buf.len()].copy_from_slice(buf);
        clean_range(dma_addr, buf.len());
        configure_channel(CHAN, ep, EPTYPE_BULK, false, pid, buf.len() as u32, dma_addr);
        run_channel(CHAN, 20, "BULKOUT")?;
    }

    if pktcnt % 2 == 1 {
        *toggle = !*toggle;
    }
    Ok(buf.len())
}
