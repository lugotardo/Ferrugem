//! Modifier state and pluggable keymaps: turns a `KeyCode` plus the
//! modifiers currently held/toggled into the character it produces, the
//! same separation Linux draws between raw keycodes and a loadable keymap
//! (`loadkeys`/XKB), just with exactly one built-in layout for now.

use super::keycode::KeyCode;

/// Live modifier state: which modifier keys are currently held down, plus
/// the three lock keys' toggle latches (flipped on press, independent of
/// whether the key is still held).
#[derive(Clone, Copy, Default)]
pub struct ModState {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub alt_gr: bool,
    pub meta: bool,
    pub caps_lock: bool,
    pub num_lock: bool,
    pub scroll_lock: bool,
}

impl ModState {
    pub const fn new() -> Self {
        Self {
            ctrl: false, shift: false, alt: false, alt_gr: false, meta: false,
            caps_lock: false, num_lock: false, scroll_lock: false,
        }
    }

    /// Update held-modifier state for a single key press/release. Lock-key
    /// toggling is handled separately (see `KeyboardState::key_event`) since
    /// it fires once per press, not on every held/released transition.
    pub fn apply(&mut self, key: KeyCode, pressed: bool) {
        match key {
            KeyCode::LCtrl | KeyCode::RCtrl => self.ctrl = pressed,
            KeyCode::LShift | KeyCode::RShift => self.shift = pressed,
            KeyCode::LAlt => self.alt = pressed,
            KeyCode::RAlt => self.alt_gr = pressed,
            KeyCode::LMeta | KeyCode::RMeta => self.meta = pressed,
            _ => {}
        }
    }
}

/// A keyboard layout: decides what character (if any) a key produces given
/// the current modifier state. Implementations only need to handle keys
/// that produce a character; navigation/function keys are encoded as ANSI
/// escape sequences by `state::KeyboardState` itself, independent of layout.
pub trait Layout {
    fn to_char(&self, key: KeyCode, mods: &ModState) -> Option<char>;
}

/// The only layout wired up today; see the module doc comment on why that's
/// fine for now (this is the plug point a second layout would implement).
pub struct UsQwerty;

impl Layout for UsQwerty {
    fn to_char(&self, key: KeyCode, mods: &ModState) -> Option<char> {
        use KeyCode::*;

        if let Some(c) = letter(key) {
            // Shift and Caps Lock both flip case, but pressing both cancels
            // out (matches every real keyboard, not just Linux's console).
            let upper = mods.shift ^ mods.caps_lock;
            return Some((if upper { c.to_ascii_uppercase() } else { c }) as char);
        }

        if mods.num_lock {
            if let Some(c) = keypad_digit(key) {
                return Some(c);
            }
        }

        Some(match key {
            Num1 => shifted_digit(mods.shift, '1', '!'),
            Num2 => shifted_digit(mods.shift, '2', '@'),
            Num3 => shifted_digit(mods.shift, '3', '#'),
            Num4 => shifted_digit(mods.shift, '4', '$'),
            Num5 => shifted_digit(mods.shift, '5', '%'),
            Num6 => shifted_digit(mods.shift, '6', '^'),
            Num7 => shifted_digit(mods.shift, '7', '&'),
            Num8 => shifted_digit(mods.shift, '8', '*'),
            Num9 => shifted_digit(mods.shift, '9', '('),
            Num0 => shifted_digit(mods.shift, '0', ')'),
            Minus => shifted_digit(mods.shift, '-', '_'),
            Equal => shifted_digit(mods.shift, '=', '+'),
            LBracket => shifted_digit(mods.shift, '[', '{'),
            RBracket => shifted_digit(mods.shift, ']', '}'),
            Backslash => shifted_digit(mods.shift, '\\', '|'),
            Semicolon => shifted_digit(mods.shift, ';', ':'),
            Quote => shifted_digit(mods.shift, '\'', '"'),
            Grave => shifted_digit(mods.shift, '`', '~'),
            Comma => shifted_digit(mods.shift, ',', '<'),
            Period => shifted_digit(mods.shift, '.', '>'),
            Slash => shifted_digit(mods.shift, '/', '?'),
            Enter | KpEnter => '\n',
            Backspace => '\x08',
            Tab => '\t',
            Space => ' ',
            Escape => '\x1B',
            KpDivide => '/',
            KpMultiply => '*',
            KpMinus => '-',
            KpPlus => '+',
            KpDot if mods.num_lock => '.',
            _ => return None,
        })
    }
}

fn letter(key: KeyCode) -> Option<u8> {
    let v = key as u8;
    let base = KeyCode::A as u8;
    if (base..=KeyCode::Z as u8).contains(&v) {
        Some(b'a' + (v - base))
    } else {
        None
    }
}

fn keypad_digit(key: KeyCode) -> Option<char> {
    use KeyCode::*;
    Some(match key {
        Kp0 => '0', Kp1 => '1', Kp2 => '2', Kp3 => '3', Kp4 => '4',
        Kp5 => '5', Kp6 => '6', Kp7 => '7', Kp8 => '8', Kp9 => '9',
        _ => return None,
    })
}

fn shifted_digit(shift: bool, plain: char, shifted: char) -> char {
    if shift { shifted } else { plain }
}
