<p align="center">
  <img src="img/logo.png" alt="Ferrugem" width="220">
</p>

<h1 align="center">Ferrugem</h1>

<p align="center">
  An experimental kernel written in Rust safety, portability, and long-term sustainability.<br>
  <em>"Forge the future, one instruction at a time."</em>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/language-Rust-orange?style=flat-square&logo=rust" alt="Rust">
  <img src="https://img.shields.io/badge/arch-x86__64%20%7C%20RISC--V-blue?style=flat-square" alt="Architectures">
  <img src="https://img.shields.io/badge/status-experimental-yellow?style=flat-square" alt="Status">
</p>

---

## What is Ferrugem?

Ferrugem is an experimental long-term project dedicated to building a modern operating system kernel in Rust, initially targeting **x86_64** and **RISC-V** architectures.

The goal is not simply to boot and print "Hello World". The ambition is to build a kernel that is genuinely functional, practical, and usable in real-world scenarios exploring how Rust can contribute to safer and more reliable low-level systems.

This project exists as an exercise in engineering, research, and learning. We understand this journey may take many years, and that is perfectly acceptable.

---

## Building & Running

**Requirements:** Rust (nightly), `cargo`, QEMU, `grub-mkrescue`

```bash
# Clone the repository
git clone https://github.com/lugotardo/Ferrugem
cd Ferrugem

# Build and run (headless, serial via terminal)
make x86          # x86_64, QEMU q35
make riscv        # RISC-V, QEMU virt
make aarch64      # aarch64, QEMU virt
make raspberrypi3 # aarch64, Raspberry Pi 3 (BCM2837) via QEMU's raspi3b machine

# Build and run with a VGA display window (x86_64 only)
make x86-display

# Build only
make build

# Generate an ISO for VirtualBox
make virtualbox

# Generate kernel8.img for a real Raspberry Pi 3 B (see below)
make raspberrypi3-img
```

> **Tip:** `make x86` connects serial I/O directly to your terminal. `make x86-display` opens a VGA window; keyboard input still works from the terminal via TCP.

---

## Running on real Raspberry Pi 3 B hardware

`make raspberrypi3` targets QEMU's `raspi3b` machine, which is close enough
to real BCM2837 silicon to serve as the day-to-day test target, but real
hardware needs a raw `kernel8.img` on an SD card, not an ELF handed to a
`-kernel` flag. The console is authoritative over UART0 on the 40-pin header:
**GPIO14/TXD0 = pin 8, GPIO15/RXD0 = pin 10, GND = pin 6**, read from a host
machine with a 3.3V-level USB-TTL serial adapter at **115200 8N1**, this is
the only console QEMU's `raspi3b` machine can show you, and the only one
guaranteed to work if no HDMI display is attached.

On real hardware, boot also requests a 1024x768x32 HDMI framebuffer from
firmware over the VideoCore mailbox (`boards::raspberrypi3::{mailbox,
framebuffer}`) and mirrors every console byte onto it with a small
hand-drawn 5x7 bitmap font (`font5x7.rs`, original artwork for this
project, not copied from any existing font). This is best-effort and purely
a visual mirror: if no display is attached, firmware refuses the request, or
(as on QEMU) the mailbox isn't implemented at all, it silently degrades to
UART-only with no effect on boot. **Not yet verified on physical
hardware**, QEMU can't emulate the VideoCore GPU that answers this mailbox,
so this path has only been exercised by code review and by confirming the
UART-only fallback still boots correctly under QEMU.

1. **Build the image:**
   ```bash
   make raspberrypi3-img
   ```
   This needs `rust-objcopy` (`rustup component add llvm-tools && cargo install cargo-binutils`)
   to turn the ELF into a flat `kernel8.img` binary at the fixed load address
   (`0x80000`) real firmware expects.

2. **Prepare a FAT32-formatted SD card** with, at the root of its boot partition:
   - `kernel8.img` (just built)
   - `kernel/src/boards/raspberrypi3/config.txt` (ships in this repo, sets
     `arm_64bit=1` and brings UART0 up for firmware's side of the handshake;
     see that file's comments)
   - `bootcode.bin`, `start.elf`, `fixup.dat`, official Raspberry Pi
     firmware, **not built by this project**: grab the latest from
     [raspberrypi/firmware's `boot/` directory](https://github.com/raspberrypi/firmware/tree/master/boot)

3. Insert the SD card, connect the serial adapter, and power on the board.

**Known limitation:** Fase 1 hardcodes a conservative 128 MiB RAM map
(`boards::raspberrypi3::RAM_FALLBACK_SIZE`) instead of querying the real 1 GiB
via a VideoCore mailbox call, the board boots and runs, it just doesn't see
all of its RAM yet. Real mailbox support is Fase 2 work.

---

## Supported Architectures & Boards

The kernel (`kernel/src/arch/`) only contains CPU-generic code; everything
specific to one platform, boot glue, linker script, peripheral addresses,
interrupt routing, memory map, lives in a Board Support Package under
`kernel/src/boards/`, selected at compile time by a `board-*` Cargo feature
(see `Makefile`'s `BOARD=` variable). Adding a new board means adding a new
BSP, not touching the generic kernel.

| Architecture | Status | Boards |
|---|---|---|
| x86\_64 | Active | `qemu-pc` (QEMU q35, default), `virtualbox` |
| RISC-V 64 | Active | `qemu-virt` (default) |
| ARM64 (AArch64) | Active | `qemu-virt` (default), `raspberrypi3` (BCM2837, day-to-day tested via QEMU's `raspi3b` machine, prepared to boot on real Raspberry Pi 3 B hardware, see below) |

---


## Project Philosophy

- **Safety by default** leveraging Rust's ownership model at the kernel level
- **Modularity** clean separation between architecture-specific and shared code
- **Simplicity** prefer clear, auditable code over premature cleverness
- **Openness** documented, readable, and welcoming to contributors

---

## Our Vision

We believe the future of systems programming can benefit from tools that eliminate entire classes of memory-related bugs while preserving performance and flexibility.

Ferrugem exists to explore questions such as:

- Can a modern kernel be built in Rust?
- Can it achieve performance comparable to traditional kernels?
- Can memory safety be improved without sacrificing control?
- Can a kernel remain portable across multiple architectures?

Our intention is not to replace existing projects, but to learn from them and contribute new ideas to the ecosystem.

---

## A Message from the Author

Hello,

My name is **Luan**, and I am a technology enthusiast with a passion for operating systems and low-level programming.

Rather than only discussing ideas, I decided to contribute through code. This is my contribution: **Ferrugem**.

Perhaps it will take years to become truly usable. Perhaps it will never be "finished". That is acceptable building the journey is part of the goal itself.

---

## Acknowledgements

Ferrugem is an independent project and is **not affiliated with the Linux kernel project**.

The Linux kernel is one of the greatest engineering achievements in computing history. Ferrugem takes inspiration from publicly available resources including hardware specifications, academic papers, open-source implementations, and official kernel documentation.

Full credit belongs to the Linux community and to the countless researchers, engineers, and maintainers whose work has advanced computing for everyone.

Ferrugem respects all copyrights and software licenses. No copyrighted source code is copied without complying with its respective license terms.

---

## License

Ferrugem is distributed under the license defined in this repository. Contributors are expected to respect third-party licenses and intellectual property rights.

---

<p align="center">
  <em>"The best way to predict the future is to build it."</em>
</p>
