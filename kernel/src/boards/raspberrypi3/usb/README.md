# USB (Raspberry Pi 3 / DWC2)

USB host stack for the Raspberry Pi 3's on-chip DWC2 controller: brings up
the root port (on real hardware, always the onboard LAN9514 hub feeding the
4 physical USB-A ports), enumerates it, and layers the same two device
classes as the x86_64 stack (see
[`kernel/src/drivers/usb/README.md`](../../drivers/usb/README.md)) on top.

**Status:** entirely polling-based, control- and interrupt-transfers only.
Best-effort — QEMU's `raspi3b` machine has no DWC2 model at all, so `init`
logs why and returns immediately there, leaving the UART console fully
usable either way. **Mass storage is compile-tested only**, not yet
verified against real hardware.

| Module | Role |
|---|---|
| `dwc2.rs` | Host controller: BCM2837 on-chip DWC2 bring-up, control/interrupt transfers |
| `hub.rs` | Enumeration and classification of newly connected devices, hot-plug rescan, split transactions for sub-High-speed devices behind a hub |
| `hid.rs` | HID boot-protocol keyboard(s), feeds `drivers::keyboard::src::state` |
| `msc.rs` | Mass Storage Class, Bulk-Only Transport (BOT), SCSI block commands |
| `protocol.rs` | Shared USB wire-format types |

## Version history

- **nightly.2** — Added Mass Storage Class driver (`msc.rs`), same shape as
  the x86_64/UHCI stack. Keyboard driver also gained Shift+PageUp/
  PageDown scrollback (`take_scroll`). Compile-tested only, not yet run
  against physical hardware.
- **nightly.1** — Initial DWC2 driver: root-port bring-up, hub enumeration,
  HID boot-protocol keyboard support.
