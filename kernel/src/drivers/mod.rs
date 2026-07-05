pub mod keyboard;
pub mod ring_buf;
pub mod serial;
pub mod timer;
#[cfg(target_arch = "x86_64")]
pub mod usb;

pub fn init() {
    timer::init();
    #[cfg(target_arch = "x86_64")]
    usb::init();
    keyboard::init();
    serial::init();
}
