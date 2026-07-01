pub mod arch;
pub(crate) mod src;

pub use arch::{init, handle_irq, read_scancode, scancode_to_ascii, has_input};
