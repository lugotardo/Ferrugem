pub mod arch;
pub(crate) mod src;

pub use arch::{init, write_byte, print_str, print_bytes, handle_irq, read_byte, read_byte_blocking, has_input};
