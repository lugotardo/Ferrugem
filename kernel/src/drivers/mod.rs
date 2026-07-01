pub mod keyboard;
pub mod ring_buf;
pub mod serial;
pub mod timer;

pub fn init() {
    timer::init();
    keyboard::init();
    serial::init();
}
