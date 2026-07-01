use super::sbi;

pub fn init() {
    // Nothing to initialise OpenSBI manages the UART
}

pub fn print_str(s: &str) {
    for b in s.bytes() {
        sbi::putchar(b);
    }
}

pub fn print_byte(b: u8) {
    sbi::putchar(b);
}
