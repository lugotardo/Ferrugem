pub mod arch;
pub(crate) mod src;

pub fn init() {
    arch::init();
}
