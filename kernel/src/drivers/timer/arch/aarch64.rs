use crate::drivers::timer::src::generic_timer;

pub fn init()  { generic_timer::init(); }
pub fn rearm() { generic_timer::rearm(); }
