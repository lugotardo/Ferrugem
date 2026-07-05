//! Canonical logical keycode, shared by every keyboard transport (PS/2 and
//! USB HID today, anything else later): each transport translates its own
//! wire format into `KeyCode` once, then the rest of the stack (modifier
//! tracking, layout, LED sync) is transport-agnostic, the same split Linux's
//! input layer makes between a device driver and `drivers/input/keyboard`.
//!
//! Variant order/values intentionally match the USB HID "Keyboard/Keypad"
//! usage page (HID Usage Tables 1.12, page 0x07): `KeyCode as u8` is exactly
//! that usage ID, so `hid_usage_to_keycode` is a single range check and the
//! PS/2 tables below are the only place a real translation table is needed.

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum KeyCode {
    A = 0x04, B = 0x05, C = 0x06, D = 0x07, E = 0x08, F = 0x09, G = 0x0A, H = 0x0B,
    I = 0x0C, J = 0x0D, K = 0x0E, L = 0x0F, M = 0x10, N = 0x11, O = 0x12, P = 0x13,
    Q = 0x14, R = 0x15, S = 0x16, T = 0x17, U = 0x18, V = 0x19, W = 0x1A, X = 0x1B,
    Y = 0x1C, Z = 0x1D,
    Num1 = 0x1E, Num2 = 0x1F, Num3 = 0x20, Num4 = 0x21, Num5 = 0x22,
    Num6 = 0x23, Num7 = 0x24, Num8 = 0x25, Num9 = 0x26, Num0 = 0x27,
    Enter = 0x28, Escape = 0x29, Backspace = 0x2A, Tab = 0x2B, Space = 0x2C,
    Minus = 0x2D, Equal = 0x2E, LBracket = 0x2F, RBracket = 0x30, Backslash = 0x31,
    Semicolon = 0x33, Quote = 0x34, Grave = 0x35, Comma = 0x36, Period = 0x37, Slash = 0x38,
    CapsLock = 0x39,
    F1 = 0x3A, F2 = 0x3B, F3 = 0x3C, F4 = 0x3D, F5 = 0x3E, F6 = 0x3F,
    F7 = 0x40, F8 = 0x41, F9 = 0x42, F10 = 0x43, F11 = 0x44, F12 = 0x45,
    PrintScreen = 0x46, ScrollLock = 0x47, Pause = 0x48,
    Insert = 0x49, Home = 0x4A, PageUp = 0x4B, Delete = 0x4C, End = 0x4D, PageDown = 0x4E,
    Right = 0x4F, Left = 0x50, Down = 0x51, Up = 0x52,
    NumLock = 0x53,
    KpDivide = 0x54, KpMultiply = 0x55, KpMinus = 0x56, KpPlus = 0x57, KpEnter = 0x58,
    Kp1 = 0x59, Kp2 = 0x5A, Kp3 = 0x5B, Kp4 = 0x5C, Kp5 = 0x5D,
    Kp6 = 0x5E, Kp7 = 0x5F, Kp8 = 0x60, Kp9 = 0x61, Kp0 = 0x62, KpDot = 0x63,
    LCtrl = 0xE0, LShift = 0xE1, LAlt = 0xE2, LMeta = 0xE3,
    RCtrl = 0xE4, RShift = 0xE5, RAlt = 0xE6, RMeta = 0xE7,
}

impl KeyCode {
    pub fn is_modifier(self) -> bool {
        matches!(self as u8, 0xE0..=0xE7)
    }
}

/// USB HID Keyboard/Keypad usage ID -> `KeyCode`. Covers the boot-protocol
/// report range (`hid.rs` decodes 6-key arrays plus the separate modifier
/// byte) and the modifier usages themselves for when a caller re-derives
/// modifier press/release from that byte (see `state::KeyboardState`).
/// `KeyCode`'s declared values are exactly the HID usage IDs (see the module
/// doc comment), so this is a straight enumeration rather than a lookup
/// table, kept as an explicit match (no unsafe integer-to-enum transmute)
/// so an out-of-range or reserved usage (e.g. 0x32, "Non-US # and ~", which
/// this keymap doesn't model) safely falls through to `None`.
pub fn hid_usage_to_keycode(usage: u8) -> Option<KeyCode> {
    use KeyCode::*;
    Some(match usage {
        0x04 => A, 0x05 => B, 0x06 => C, 0x07 => D, 0x08 => E, 0x09 => F,
        0x0A => G, 0x0B => H, 0x0C => I, 0x0D => J, 0x0E => K, 0x0F => L,
        0x10 => M, 0x11 => N, 0x12 => O, 0x13 => P, 0x14 => Q, 0x15 => R,
        0x16 => S, 0x17 => T, 0x18 => U, 0x19 => V, 0x1A => W, 0x1B => X,
        0x1C => Y, 0x1D => Z,
        0x1E => Num1, 0x1F => Num2, 0x20 => Num3, 0x21 => Num4, 0x22 => Num5,
        0x23 => Num6, 0x24 => Num7, 0x25 => Num8, 0x26 => Num9, 0x27 => Num0,
        0x28 => Enter, 0x29 => Escape, 0x2A => Backspace, 0x2B => Tab, 0x2C => Space,
        0x2D => Minus, 0x2E => Equal, 0x2F => LBracket, 0x30 => RBracket, 0x31 => Backslash,
        0x33 => Semicolon, 0x34 => Quote, 0x35 => Grave, 0x36 => Comma, 0x37 => Period, 0x38 => Slash,
        0x39 => CapsLock,
        0x3A => F1, 0x3B => F2, 0x3C => F3, 0x3D => F4, 0x3E => F5, 0x3F => F6,
        0x40 => F7, 0x41 => F8, 0x42 => F9, 0x43 => F10, 0x44 => F11, 0x45 => F12,
        0x46 => PrintScreen, 0x47 => ScrollLock, 0x48 => Pause,
        0x49 => Insert, 0x4A => Home, 0x4B => PageUp, 0x4C => Delete, 0x4D => End, 0x4E => PageDown,
        0x4F => Right, 0x50 => Left, 0x51 => Down, 0x52 => Up,
        0x53 => NumLock,
        0x54 => KpDivide, 0x55 => KpMultiply, 0x56 => KpMinus, 0x57 => KpPlus, 0x58 => KpEnter,
        0x59 => Kp1, 0x5A => Kp2, 0x5B => Kp3, 0x5C => Kp4, 0x5D => Kp5,
        0x5E => Kp6, 0x5F => Kp7, 0x60 => Kp8, 0x61 => Kp9, 0x62 => Kp0, 0x63 => KpDot,
        0xE0 => LCtrl, 0xE1 => LShift, 0xE2 => LAlt, 0xE3 => LMeta,
        0xE4 => RCtrl, 0xE5 => RShift, 0xE6 => RAlt, 0xE7 => RMeta,
        _ => return None,
    })
}

/// The modifier byte's bit order in a HID boot-protocol report (and, not
/// coincidentally, `KeyCode::LCtrl..=RMeta`'s declaration order): bit N
/// corresponds to `KeyCode` value `0xE0 + N`.
pub const HID_MODIFIER_KEYS: [KeyCode; 8] = [
    KeyCode::LCtrl, KeyCode::LShift, KeyCode::LAlt, KeyCode::LMeta,
    KeyCode::RCtrl, KeyCode::RShift, KeyCode::RAlt, KeyCode::RMeta,
];

/// PS/2 scancode set 1, unprefixed bytes (make codes 0x01-0x58ish; a release
/// is the same code with bit 7 set, handled by the caller before indexing
/// this table). `None` entries are codes this table doesn't assign meaning
/// to (unused/reserved bytes in the low range).
const SET1_NORMAL: [Option<KeyCode>; 0x59] = {
    let mut t: [Option<KeyCode>; 0x59] = [None; 0x59];
    t[0x01] = Some(KeyCode::Escape);
    t[0x02] = Some(KeyCode::Num1); t[0x03] = Some(KeyCode::Num2); t[0x04] = Some(KeyCode::Num3);
    t[0x05] = Some(KeyCode::Num4); t[0x06] = Some(KeyCode::Num5); t[0x07] = Some(KeyCode::Num6);
    t[0x08] = Some(KeyCode::Num7); t[0x09] = Some(KeyCode::Num8); t[0x0A] = Some(KeyCode::Num9);
    t[0x0B] = Some(KeyCode::Num0);
    t[0x0C] = Some(KeyCode::Minus); t[0x0D] = Some(KeyCode::Equal); t[0x0E] = Some(KeyCode::Backspace);
    t[0x0F] = Some(KeyCode::Tab);
    t[0x10] = Some(KeyCode::Q); t[0x11] = Some(KeyCode::W); t[0x12] = Some(KeyCode::E);
    t[0x13] = Some(KeyCode::R); t[0x14] = Some(KeyCode::T); t[0x15] = Some(KeyCode::Y);
    t[0x16] = Some(KeyCode::U); t[0x17] = Some(KeyCode::I); t[0x18] = Some(KeyCode::O);
    t[0x19] = Some(KeyCode::P);
    t[0x1A] = Some(KeyCode::LBracket); t[0x1B] = Some(KeyCode::RBracket); t[0x1C] = Some(KeyCode::Enter);
    t[0x1D] = Some(KeyCode::LCtrl);
    t[0x1E] = Some(KeyCode::A); t[0x1F] = Some(KeyCode::S); t[0x20] = Some(KeyCode::D);
    t[0x21] = Some(KeyCode::F); t[0x22] = Some(KeyCode::G); t[0x23] = Some(KeyCode::H);
    t[0x24] = Some(KeyCode::J); t[0x25] = Some(KeyCode::K); t[0x26] = Some(KeyCode::L);
    t[0x27] = Some(KeyCode::Semicolon); t[0x28] = Some(KeyCode::Quote); t[0x29] = Some(KeyCode::Grave);
    t[0x2A] = Some(KeyCode::LShift); t[0x2B] = Some(KeyCode::Backslash);
    t[0x2C] = Some(KeyCode::Z); t[0x2D] = Some(KeyCode::X); t[0x2E] = Some(KeyCode::C);
    t[0x2F] = Some(KeyCode::V); t[0x30] = Some(KeyCode::B); t[0x31] = Some(KeyCode::N);
    t[0x32] = Some(KeyCode::M);
    t[0x33] = Some(KeyCode::Comma); t[0x34] = Some(KeyCode::Period); t[0x35] = Some(KeyCode::Slash);
    t[0x36] = Some(KeyCode::RShift);
    t[0x37] = Some(KeyCode::KpMultiply);
    t[0x38] = Some(KeyCode::LAlt);
    t[0x39] = Some(KeyCode::Space);
    t[0x3A] = Some(KeyCode::CapsLock);
    t[0x3B] = Some(KeyCode::F1); t[0x3C] = Some(KeyCode::F2); t[0x3D] = Some(KeyCode::F3);
    t[0x3E] = Some(KeyCode::F4); t[0x3F] = Some(KeyCode::F5); t[0x40] = Some(KeyCode::F6);
    t[0x41] = Some(KeyCode::F7); t[0x42] = Some(KeyCode::F8); t[0x43] = Some(KeyCode::F9);
    t[0x44] = Some(KeyCode::F10);
    t[0x45] = Some(KeyCode::NumLock); t[0x46] = Some(KeyCode::ScrollLock);
    t[0x47] = Some(KeyCode::Kp7); t[0x48] = Some(KeyCode::Kp8); t[0x49] = Some(KeyCode::Kp9);
    t[0x4A] = Some(KeyCode::KpMinus);
    t[0x4B] = Some(KeyCode::Kp4); t[0x4C] = Some(KeyCode::Kp5); t[0x4D] = Some(KeyCode::Kp6);
    t[0x4E] = Some(KeyCode::KpPlus);
    t[0x4F] = Some(KeyCode::Kp1); t[0x50] = Some(KeyCode::Kp2); t[0x51] = Some(KeyCode::Kp3);
    t[0x52] = Some(KeyCode::Kp0); t[0x53] = Some(KeyCode::KpDot);
    t[0x57] = Some(KeyCode::F11); t[0x58] = Some(KeyCode::F12);
    t
};

/// PS/2 scancode set 1, bytes following the `0xE0` prefix byte (extended
/// keys: the duplicated right-hand modifiers, the arrow/navigation cluster,
/// and the numpad's non-ambiguous twins).
fn set1_extended(code: u8) -> Option<KeyCode> {
    match code {
        0x1C => Some(KeyCode::KpEnter),
        0x1D => Some(KeyCode::RCtrl),
        0x35 => Some(KeyCode::KpDivide),
        0x38 => Some(KeyCode::RAlt),
        0x47 => Some(KeyCode::Home),
        0x48 => Some(KeyCode::Up),
        0x49 => Some(KeyCode::PageUp),
        0x4B => Some(KeyCode::Left),
        0x4D => Some(KeyCode::Right),
        0x4F => Some(KeyCode::End),
        0x50 => Some(KeyCode::Down),
        0x51 => Some(KeyCode::PageDown),
        0x52 => Some(KeyCode::Insert),
        0x53 => Some(KeyCode::Delete),
        0x5B => Some(KeyCode::LMeta),
        0x5C => Some(KeyCode::RMeta),
        _ => None,
    }
}

/// One decoded PS/2 scancode set 1 event: which key, and press (`true`) or
/// release (bit 7 of the trailing byte set). `extended` is whether an 0xE0
/// prefix preceded `code` (the caller tracks that prefix byte-to-byte).
pub fn ps2_set1_to_keycode(code: u8, extended: bool) -> Option<(KeyCode, bool)> {
    let pressed = code & 0x80 == 0;
    let make_code = code & 0x7F;
    let key = if extended {
        set1_extended(make_code)?
    } else {
        (*SET1_NORMAL.get(make_code as usize)?)?
    };
    Some((key, pressed))
}
