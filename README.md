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
make x86          # x86_64
make riscv        # RISC-V

# Build and run with a VGA display window
make x86-display

# Build only
make build

# Generate an ISO for VirtualBox / real hardware
make iso-virtualbox
```

> **Tip:** `make x86` connects serial I/O directly to your terminal. `make x86-display` opens a VGA window; keyboard input still works from the terminal via TCP.

---

## Supported Architectures

| Architecture | Status     |
|---|---|
| x86\_64      | Active     |
| RISC-V 64    | Active     |
| ARM64 (AArch64) | In Development |

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
