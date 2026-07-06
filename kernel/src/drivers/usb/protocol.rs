//! Minimal USB 2.0 chapter 9 / HID class constants and types for the UHCI
//! driver - a smaller, independent copy of what
//! `boards::raspberrypi3::usb::protocol` defines for DWC2 (that one is
//! private to that board and carries split-transaction fields UHCI has no
//! equivalent of, since a pure UHCI topology never has a High-speed hub in
//! it, so it wasn't worth sharing outright).

/// UHCI is USB 1.1 only: every device it can ever talk to, directly or
/// through an external hub, is Full or Low speed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Speed {
    Low,
    Full,
}

#[derive(Clone, Copy)]
pub struct Endpoint {
    pub dev_addr: u8,
    pub ep_num: u8,
    pub max_packet: u16,
    pub speed: Speed,
}

impl Endpoint {
    pub fn control(dev_addr: u8, max_packet: u16, speed: Speed) -> Self {
        Self { dev_addr, ep_num: 0, max_packet, speed }
    }
}

// ── Standard request codes (USB 2.0 table 9-4) ────────────────────────────
pub const REQ_GET_STATUS:        u8 = 0;
pub const REQ_SET_ADDRESS:       u8 = 5;
pub const REQ_GET_DESCRIPTOR:    u8 = 6;
pub const REQ_SET_CONFIGURATION: u8 = 9;

// ── bmRequestType direction/type/recipient bits ───────────────────────────
pub const DIR_IN:  u8 = 0x80;
pub const DIR_OUT: u8 = 0x00;
pub const TYPE_STANDARD: u8 = 0x00;
pub const TYPE_CLASS:    u8 = 0x20;
pub const RECIP_DEVICE:    u8 = 0x00;
pub const RECIP_INTERFACE: u8 = 0x01;
/// A hub port request's recipient (USB 2.0 §11.24.1): "other" means the
/// numbered downstream port, not the hub device itself.
pub const RECIP_OTHER:     u8 = 0x03;

// ── Descriptor types (USB 2.0 table 9-5) ──────────────────────────────────
pub const DESC_DEVICE:        u8 = 1;
pub const DESC_CONFIGURATION: u8 = 2;
pub const DESC_HUB:           u8 = 0x29;

pub const CLASS_HID: u8 = 0x03;
pub const HID_SUBCLASS_BOOT: u8 = 0x01;
pub const HID_PROTOCOL_KEYBOARD: u8 = 0x01;

// ── Hub class (USB 2.0 chapter 11) ─────────────────────────────────────────
pub const CLASS_HUB: u8 = 0x09;
pub const HUB_REQ_GET_DESCRIPTOR:     u8 = 6;
pub const HUB_REQ_SET_PORT_FEATURE:   u8 = 3;
pub const HUB_REQ_CLEAR_PORT_FEATURE: u8 = 1;
pub const FEATURE_PORT_RESET:        u16 = 4;
pub const FEATURE_PORT_POWER:        u16 = 8;
pub const FEATURE_C_PORT_RESET:      u16 = 20;

// ── Mass Storage Class (USB MSC 1.0 / Bulk-Only Transport) ────────────────
pub const CLASS_MSC: u8 = 0x08;
/// SCSI transparent command set - what every USB flash drive and QEMU's
/// `usb-storage` device report; the handful of other MSC subclasses (RBC,
/// UFI, ATAPI, ...) aren't handled.
pub const MSC_SUBCLASS_SCSI: u8 = 0x06;
/// Bulk-Only Transport - the interface protocol `msc.rs` implements. CBI
/// (Control/Bulk/Interrupt) devices aren't handled.
pub const MSC_PROTOCOL_BULK_ONLY: u8 = 0x50;

// ── HID class-specific requests (HID 1.11 section 7.2) ────────────────────
pub const HID_REQ_SET_REPORT:   u8 = 0x09;
pub const HID_REQ_SET_IDLE:     u8 = 0x0A;
pub const HID_REQ_SET_PROTOCOL: u8 = 0x0B;
pub const HID_REPORT_TYPE_OUTPUT: u16 = 0x02;
pub const HID_BOOT_PROTOCOL: u16 = 0;

/// USB 2.0 table 9-2 setup packet layout, sent as the SETUP stage of every
/// control transfer, `repr(C, packed)` so its in-memory layout matches the
/// wire format exactly.
#[repr(C, packed)]
pub struct SetupPacket {
    pub request_type: u8,
    pub request: u8,
    pub value: u16,
    pub index: u16,
    pub length: u16,
}

impl SetupPacket {
    pub fn as_bytes(&self) -> [u8; 8] {
        unsafe { core::mem::transmute_copy(self) }
    }
}
