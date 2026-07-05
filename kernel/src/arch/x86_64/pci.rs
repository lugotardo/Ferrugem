//! PCI configuration space access (the legacy I/O-port mechanism, PCI 2.x
//! "Configuration Mechanism #1" - every real chipset and QEMU's q35/i440fx
//! still support it even though MMCONFIG/ECAM exists for newer devices).
//! Just enough to find a device by class code and read its BARs/command
//! register: this project's only PCI consumer so far is `drivers::usb::uhci`
//! looking for a UHCI host controller.

use super::port;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

const CONFIG_ENABLE: u32 = 1 << 31;

/// A PCI function's location in the (bus, device, function) address space.
/// Cheap to copy around; every actual register access re-derives the
/// CONFIG_ADDRESS value from these three fields.
#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
}

fn config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    CONFIG_ENABLE
        | ((bus as u32) << 16)
        | ((device as u32) << 11)
        | ((function as u32) << 8)
        | (offset as u32 & 0xFC) // register offset must be 4-byte aligned; low 2 bits are reserved
}

fn read32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    unsafe {
        port::outl(CONFIG_ADDRESS, config_address(bus, device, function, offset));
        port::inl(CONFIG_DATA)
    }
}

fn write32(bus: u8, device: u8, function: u8, offset: u8, val: u32) {
    unsafe {
        port::outl(CONFIG_ADDRESS, config_address(bus, device, function, offset));
        port::outl(CONFIG_DATA, val);
    }
}

impl PciDevice {
    /// Raw doubleword read at a 4-byte-aligned config space offset.
    pub fn read32(&self, offset: u8) -> u32 {
        read32(self.bus, self.device, self.function, offset)
    }

    pub fn write32(&self, offset: u8, val: u32) {
        write32(self.bus, self.device, self.function, offset, val);
    }

    /// One of the 6 Base Address Registers (offsets 0x10, 0x14, ..., 0x24).
    /// Returns the raw register value; callers care whether bit 0 is set
    /// (I/O-space BAR, the low 2 bits are then reserved-as-01 rather than
    /// part of the address) vs memory-space, see `io_bar_port`.
    pub fn bar(&self, index: u8) -> u32 {
        self.read32(0x10 + index * 4)
    }

    /// Decode BAR `index` as an I/O-space BAR and return its port number,
    /// or `None` if it's actually a memory-space BAR (bit 0 clear) - the
    /// two encodings aren't compatible, and this driver (UHCI) only ever
    /// uses port I/O, never MMIO.
    pub fn io_bar_port(&self, index: u8) -> Option<u16> {
        let bar = self.bar(index);
        if bar & 1 == 0 {
            return None;
        }
        Some((bar & 0xFFFC) as u16)
    }

    /// PCI command register (offset 0x04): enable I/O space decoding and
    /// bus mastering, both required before a device will respond to port
    /// I/O or generate DMA - real BIOSes/UEFI usually already do this, but
    /// nothing guarantees it, and QEMU's firmware skipping straight to
    /// `-kernel` load doesn't run any BIOS PCI init pass at all.
    pub fn enable_io_and_bus_master(&self) {
        const IO_SPACE: u32 = 1 << 0;
        const BUS_MASTER: u32 = 1 << 2;
        let cmd = self.read32(0x04) & 0xFFFF;
        self.write32(0x04, cmd | IO_SPACE | BUS_MASTER);
    }

    fn vendor_device(&self) -> (u16, u16) {
        let v = self.read32(0x00);
        ((v & 0xFFFF) as u16, (v >> 16) as u16)
    }

    /// (class, subclass, prog_if) from offset 0x08, the same triple used to
    /// identify a device's function without needing to know its exact
    /// vendor/device ID (e.g. "any UHCI controller": class 0x0C, subclass
    /// 0x03, prog_if 0x00, regardless of which chipset made it).
    fn class_triple(&self) -> (u8, u8, u8) {
        let v = self.read32(0x08);
        (((v >> 24) & 0xFF) as u8, ((v >> 16) & 0xFF) as u8, ((v >> 8) & 0xFF) as u8)
    }

    fn header_type(&self) -> u8 {
        ((self.read32(0x0C) >> 16) & 0xFF) as u8
    }
}

/// Brute-force scan of every (bus, device, function) - simple and slow
/// (up to 256*32*8 = 65536 config reads) compared to walking bridges'
/// secondary-bus numbers, but this only runs once at boot looking for one
/// device, and even the full scan finishes in well under a millisecond of
/// wall-clock port I/O.
pub fn find_device(class: u8, subclass: u8, prog_if: u8) -> Option<PciDevice> {
    for bus in 0..=255u16 {
        let bus = bus as u8;
        for device in 0..32u8 {
            let f0 = PciDevice { bus, device, function: 0 };
            if f0.vendor_device().0 == 0xFFFF {
                continue; // no device at this slot
            }
            let multi_function = f0.header_type() & 0x80 != 0;
            let max_function = if multi_function { 8 } else { 1 };
            for function in 0..max_function {
                let dev = PciDevice { bus, device, function };
                if dev.vendor_device().0 == 0xFFFF {
                    continue;
                }
                if dev.class_triple() == (class, subclass, prog_if) {
                    return Some(dev);
                }
            }
        }
    }
    None
}
