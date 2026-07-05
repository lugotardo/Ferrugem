//! USB 2.0 standard descriptor/request constants and small shared types.
//! Values here come from the USB 2.0 specification (chapter 9) and the USB
//! HID / Hub class specs, open standards, not vendor-specific.

/// Negotiated link speed, as reported by `HPRT.PRTSPD` (root port) or a hub
/// port's status word. Split transactions are required whenever a
/// Low/Full-speed device hangs off a High-speed hub (see `Endpoint::needs_split`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Speed {
    Low,
    Full,
    High,
}

/// Everything a control or interrupt transfer needs to address one endpoint,
/// including the hub routing info split transactions require. `dev_addr`
/// 0 is the special "not yet addressed" state every device starts in
/// during enumeration.
#[derive(Clone, Copy)]
pub struct Endpoint {
    pub dev_addr: u8,
    pub ep_num: u8,
    pub max_packet: u16,
    pub speed: Speed,
    /// Address of the hub this device hangs off, or 0 if none (only ever 0
    /// for the single device enumerated directly on the root port, on a
    /// Raspberry Pi 3 that's always the internal LAN9514 hub itself).
    pub hub_addr: u8,
    /// 1-based downstream port number on `hub_addr`.
    pub hub_port: u8,
}

impl Endpoint {
    pub fn control(dev_addr: u8, max_packet: u16, speed: Speed, hub_addr: u8, hub_port: u8) -> Self {
        Self { dev_addr, ep_num: 0, max_packet, speed, hub_addr, hub_port }
    }

    /// A Low/Full-speed device behind a High-speed hub needs the host to
    /// speak split transactions (SSPLIT/CSPLIT) to it; a High-speed device,
    /// or a device connected with no hub in between, does not.
    pub fn needs_split(&self) -> bool {
        self.hub_addr != 0 && self.speed != Speed::High
    }
}

// ── Standard request codes (USB 2.0 table 9-4) ────────────────────────────
pub const REQ_GET_STATUS:        u8 = 0;
pub const REQ_CLEAR_FEATURE:     u8 = 1;
pub const REQ_SET_FEATURE:       u8 = 3;
pub const REQ_SET_ADDRESS:       u8 = 5;
pub const REQ_GET_DESCRIPTOR:    u8 = 6;
pub const REQ_SET_CONFIGURATION: u8 = 9;

// ── bmRequestType direction/type/recipient bits ───────────────────────────
pub const DIR_IN:  u8 = 0x80;
pub const DIR_OUT: u8 = 0x00;
pub const TYPE_STANDARD: u8 = 0x00;
pub const TYPE_CLASS:    u8 = 0x20;
pub const RECIP_DEVICE:  u8 = 0x00;
pub const RECIP_INTERFACE: u8 = 0x01;
pub const RECIP_OTHER:   u8 = 0x03; // hub port requests target "other" (the port)

// ── Descriptor types (USB 2.0 table 9-5) ──────────────────────────────────
pub const DESC_DEVICE:        u8 = 1;
pub const DESC_CONFIGURATION: u8 = 2;
pub const DESC_HUB:           u8 = 0x29;

pub const CLASS_HUB: u8 = 0x09;
pub const CLASS_HID: u8 = 0x03;
pub const HID_SUBCLASS_BOOT: u8 = 0x01;
pub const HID_PROTOCOL_KEYBOARD: u8 = 0x01;

// ── HID class-specific requests (HID 1.11 section 7.2) ────────────────────
pub const HID_REQ_SET_IDLE:     u8 = 0x0A;
pub const HID_REQ_SET_PROTOCOL: u8 = 0x0B;
pub const HID_BOOT_PROTOCOL: u16 = 0;

// ── Hub class-specific requests/features (USB 2.0 table 11-16/11-17) ──────
pub const HUB_REQ_GET_DESCRIPTOR:     u8 = 6;
pub const HUB_REQ_SET_PORT_FEATURE:   u8 = 3;
pub const HUB_REQ_CLEAR_PORT_FEATURE: u8 = 1;

pub const FEATURE_PORT_CONNECTION:  u16 = 0;
pub const FEATURE_PORT_RESET:       u16 = 4;
pub const FEATURE_PORT_POWER:       u16 = 8;
pub const FEATURE_C_PORT_CONNECTION: u16 = 16;
pub const FEATURE_C_PORT_RESET:      u16 = 20;

/// USB 2.0 table 9-2 setup packet layout, sent as the SETUP stage of every
/// control transfer, `repr(C, packed)` so its in-memory layout matches the
/// wire format exactly (this is what actually gets DMA'd/PIO'd to the FIFO).
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
