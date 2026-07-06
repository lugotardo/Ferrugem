# USB (x86_64 / UHCI)

USB host stack for x86_64 boards (`qemu-pc`, `virtualbox`): a UHCI
(Universal Host Controller Interface, USB 1.1) driver found via PCI, plus
device enumeration and two device classes on top of it.

**Status:** root-hub-only (2 built-in ports); no isochronous or concurrent
transfers, one Transfer Descriptor in flight at a time. Best-effort — a
machine with no UHCI controller (e.g. `q35` started without `-usb`) leaves
PS/2 and serial fully usable either way.

| Module | Role |
|---|---|
| `uhci.rs` | Host controller: PCI bring-up, control transfers, one-shot interrupt-IN polling |
| `hub.rs` | Enumeration and classification of newly connected devices, hot-plug rescan |
| `hid.rs` | HID boot-protocol keyboard(s), feeds `drivers::keyboard::src::state` |
| `msc.rs` | Mass Storage Class, Bulk-Only Transport (BOT), SCSI block commands |
| `protocol.rs` | Shared USB wire-format types |

## Version history

- **nightly.2** — Added Mass Storage Class driver (`msc.rs`): BOT envelope
  over TEST UNIT READY / READ CAPACITY(10) / READ(10) / WRITE(10), single
  LUN assumed, no STALL recovery. Exposed via `disk_count`/
  `disk_read_block`/`disk_write_block` and the shell's `diskinfo`/
  `diskread`/`diskwrite` commands. Keyboard driver also gained
  Shift+PageUp/PageDown scrollback (`take_scroll`), wired through here from
  `hid.rs`. Exercised under QEMU.
- **nightly.1** — Initial UHCI driver: PCI bring-up, root-hub enumeration,
  HID boot-protocol keyboard support.
