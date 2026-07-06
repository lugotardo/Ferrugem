//! Transport-agnostic keyboard state machine: turns `KeyCode` press/release
//! events (from any transport - PS/2, USB HID, ...) into an output byte
//! stream (ASCII for printable keys, Ctrl-letter control codes, ANSI escape
//! sequences for arrows/function/navigation keys - the same scheme a Linux
//! text console uses), while tracking modifier/lock-key state and which
//! transport(s) need their LEDs resynced.
//!
//! One `KeyboardState` is shared by every keyboard attached to a given
//! console (see `HidReportDecoder` below for why multiple physical USB
//! keyboards still funnel into a single instance): Caps Lock pressed on one
//! keyboard should affect what any of them types next, exactly as it does
//! on a real machine with two keyboards plugged in.

use super::keycode::{hid_usage_to_keycode, KeyCode, HID_MODIFIER_KEYS};
use super::layout::{Layout, ModState, UsQwerty};
use crate::drivers::ring_buf::RingBuf;

pub struct KeyboardState {
    mods: ModState,
    layout: UsQwerty,
    out: RingBuf<32>,
    led_dirty: bool,
    ps2_extended: bool,
    ps2_skip: u8,
    scroll: Option<i8>,
}

impl KeyboardState {
    pub const fn new() -> Self {
        Self {
            mods: ModState::new(),
            layout: UsQwerty,
            out: RingBuf::new(),
            led_dirty: false,
            ps2_extended: false,
            ps2_skip: 0,
            scroll: None,
        }
    }

    /// Feed one raw PS/2 scancode-set-1 byte from the i8042 data port.
    /// Tracks the `0xE0` extended-key prefix and the `0xE1` Pause/Break
    /// prefix across calls, one byte at a time, since that's how the
    /// hardware delivers them (one interrupt per byte).
    pub fn feed_ps2_byte(&mut self, byte: u8) {
        if self.ps2_skip > 0 {
            self.ps2_skip -= 1;
            return;
        }
        if byte == 0xE0 {
            self.ps2_extended = true;
            return;
        }
        if byte == 0xE1 {
            // Pause/Break sends a fixed 6-byte make-only sequence (E1 1D 45
            // E1 9D C5) with no distinguishable release and nothing else
            // like it in scancode set 1; swallow the remaining 5 bytes
            // rather than misreading them as an unrelated Ctrl press and
            // NumLock toggle.
            self.ps2_skip = 5;
            return;
        }
        let extended = self.ps2_extended;
        self.ps2_extended = false;
        if let Some((key, pressed)) = super::keycode::ps2_set1_to_keycode(byte, extended) {
            self.key_event(key, pressed);
        }
    }

    /// Core state transition: apply a decoded key press/release. Updates
    /// held-modifier state, flips lock-key toggles (and flags their LEDs
    /// dirty) on press, and queues whatever byte(s) the key produces.
    pub fn key_event(&mut self, key: KeyCode, pressed: bool) {
        self.mods.apply(key, pressed);

        if !pressed {
            return;
        }

        match key {
            KeyCode::CapsLock => { self.mods.caps_lock = !self.mods.caps_lock; self.led_dirty = true; }
            KeyCode::NumLock => { self.mods.num_lock = !self.mods.num_lock; self.led_dirty = true; }
            KeyCode::ScrollLock => { self.mods.scroll_lock = !self.mods.scroll_lock; self.led_dirty = true; }
            // Shift+PageUp/PageDown is the conventional Linux-console/xterm
            // scrollback hotkey; steal it here (board-agnostic, transport-
            // agnostic) instead of emitting the usual escape sequence, so
            // whichever board console is listening (see `take_scroll`) can
            // scroll its own history without the shell ever seeing these
            // two key combos. Unshifted PageUp/PageDown are unaffected.
            KeyCode::PageUp if self.mods.shift => self.scroll = Some(-1),
            KeyCode::PageDown if self.mods.shift => self.scroll = Some(1),
            _ if !key.is_modifier() => self.emit(key),
            _ => {}
        }
    }

    fn emit(&mut self, key: KeyCode) {
        if let Some(seq) = escape_sequence(key) {
            for &b in seq {
                self.out.push(b);
            }
            return;
        }
        if let Some(c) = self.layout.to_char(key, &self.mods) {
            self.out.push(self.apply_ctrl(c));
        }
    }

    /// Linux tty convention: Ctrl held with a letter produces that letter's
    /// position in the alphabet as a control code (Ctrl+A=0x01 .. Ctrl+Z=0x1A).
    fn apply_ctrl(&self, c: char) -> u8 {
        if self.mods.ctrl && c.is_ascii_alphabetic() {
            (c.to_ascii_uppercase() as u8) & 0x1F
        } else {
            c as u8 // `Layout` implementations only ever produce ASCII.
        }
    }

    pub fn pop_byte(&mut self) -> Option<u8> {
        self.out.pop()
    }

    pub fn has_output(&self) -> bool {
        !self.out.is_empty()
    }

    /// Consumes a pending Shift+PageUp(-1)/PageDown(+1) scrollback request,
    /// if `key_event` recorded one since the last call. `None` the rest of
    /// the time, i.e. on every normal keystroke.
    pub fn take_scroll(&mut self) -> Option<i8> {
        self.scroll.take()
    }

    /// `true` at most once per lock-key toggle: callers (the PS/2 and HID
    /// transports) each check this after every event and, if set, send
    /// their own LED command using `ps2_led_mask`/`hid_led_mask`.
    pub fn take_led_dirty(&mut self) -> bool {
        core::mem::take(&mut self.led_dirty)
    }

    /// PS/2 "Set/Reset Status Indicators" (command 0xED) LED byte: bit 0
    /// Scroll Lock, bit 1 Num Lock, bit 2 Caps Lock.
    pub fn ps2_led_mask(&self) -> u8 {
        (self.mods.scroll_lock as u8) | ((self.mods.num_lock as u8) << 1) | ((self.mods.caps_lock as u8) << 2)
    }

    /// USB HID keyboard Output report LED byte (HID Usage Tables 1.12,
    /// page 0x08): bit 0 Num Lock, bit 1 Caps Lock, bit 2 Scroll Lock - a
    /// different bit order than PS/2's, not a typo.
    pub fn hid_led_mask(&self) -> u8 {
        (self.mods.num_lock as u8) | ((self.mods.caps_lock as u8) << 1) | ((self.mods.scroll_lock as u8) << 2)
    }
}

/// ANSI/VT100 escape sequences (the same ones a Linux text console and
/// xterm emit) for keys that don't produce a plain character. Checked
/// before `Layout::to_char` so a layout implementation never needs to know
/// about them.
fn escape_sequence(key: KeyCode) -> Option<&'static [u8]> {
    use KeyCode::*;
    Some(match key {
        Up => b"\x1b[A",
        Down => b"\x1b[B",
        Right => b"\x1b[C",
        Left => b"\x1b[D",
        Home => b"\x1b[H",
        End => b"\x1b[F",
        Insert => b"\x1b[2~",
        Delete => b"\x1b[3~",
        PageUp => b"\x1b[5~",
        PageDown => b"\x1b[6~",
        F1 => b"\x1bOP",
        F2 => b"\x1bOQ",
        F3 => b"\x1bOR",
        F4 => b"\x1bOS",
        F5 => b"\x1b[15~",
        F6 => b"\x1b[17~",
        F7 => b"\x1b[18~",
        F8 => b"\x1b[19~",
        F9 => b"\x1b[20~",
        F10 => b"\x1b[21~",
        F11 => b"\x1b[23~",
        F12 => b"\x1b[24~",
        _ => return None,
    })
}

/// Per-device diff state for one USB HID boot-protocol keyboard endpoint.
/// Several physical keyboards can be attached at once (see
/// `boards::raspberrypi3::usb::hid`); each needs its own previous-report
/// snapshot to detect make/break transitions, even though they all feed the
/// same shared `KeyboardState` - Caps Lock pressed on one should affect what
/// either of them types next, same as two keyboards plugged into one real PC.
pub struct HidReportDecoder {
    last_mods: u8,
    last_keys: [u8; 6],
}

impl HidReportDecoder {
    pub const fn new() -> Self {
        Self { last_mods: 0, last_keys: [0; 6] }
    }

    /// Decode one 8-byte boot-protocol report (byte 0 = modifier bitmap,
    /// byte 1 = reserved, bytes 2-7 = up to 6 simultaneously pressed
    /// non-modifier usage codes) into press/release events against `state`.
    pub fn feed(&mut self, report: &[u8; 8], state: &mut KeyboardState) {
        let modifier = report[0];
        let keys: [u8; 6] = [report[2], report[3], report[4], report[5], report[6], report[7]];

        // 0x01 in any slot is the HID "phantom state" / rollover-error
        // marker (too many keys held at once for this device to report
        // individually) - the whole report is meaningless, not just that slot.
        if keys.contains(&1) {
            return;
        }

        for (i, &mkey) in HID_MODIFIER_KEYS.iter().enumerate() {
            let bit = 1u8 << i;
            let now = modifier & bit != 0;
            if now != (self.last_mods & bit != 0) {
                state.key_event(mkey, now);
            }
        }
        self.last_mods = modifier;

        for &code in &self.last_keys {
            if code != 0 && !keys.contains(&code) {
                if let Some(key) = hid_usage_to_keycode(code) {
                    state.key_event(key, false);
                }
            }
        }
        for &code in &keys {
            if code != 0 && !self.last_keys.contains(&code) {
                if let Some(key) = hid_usage_to_keycode(code) {
                    state.key_event(key, true);
                }
            }
        }
        self.last_keys = keys;
    }
}
