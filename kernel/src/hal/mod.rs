//! Hardware Abstraction Layer: the contract every board support package
//! (BSP, under `crate::boards`) is expected to satisfy.
//!
//! The rest of the kernel never calls through these traits directly, it
//! calls the free functions re-exported by `boards::current` (matching the
//! rest of the codebase's cfg-dispatch style). These traits exist so that
//! adding a new board is checked by the compiler against one canonical
//! interface instead of being discovered ad hoc by grepping existing boards.
//!
//! Nothing dispatches through these traits today (call sites use each
//! board's free functions directly, as above), `allow(dead_code)` silences
//! the resulting "trait is never used" lint rather than fabricating a caller.
#![allow(dead_code)]

pub trait Console {
    fn init();
    fn write_byte(b: u8);
    fn write_str(s: &str) {
        for b in s.bytes() {
            Self::write_byte(b);
        }
    }
}

pub trait Timer {
    fn init();
    /// Re-arm the next tick. No-op on hardware whose timer free-runs or
    /// re-arms itself directly in the trap handler.
    fn rearm();
}

pub trait InterruptController {
    fn init();
    fn enable(id: u32);
    fn disable(id: u32);
    fn eoi(id: u32);
}
