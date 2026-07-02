use crate::drivers::timer::src::pit;

pub fn init() { pit::init(); }

/// PIT channel 0 free-runs in square-wave mode; no per-tick re-arm needed.
pub fn rearm() {}
