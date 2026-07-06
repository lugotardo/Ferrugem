ARCH ?= x86_64

# BOARD selects which BSP (board-* Cargo feature, see kernel/Cargo.toml) is
# built for the chosen ARCH. Each ARCH has one default board that matches
# today's behavior when BOARD isn't set; pass BOARD=<name> to pick another.
ifeq ($(ARCH), x86_64)
    TARGET  := x86_64-unknown-none
    QEMU    := qemu-system-x86_64
    BOARD   ?= qemu-pc
    # piix3-usb-uhci works as a plain PCI function regardless of chipset
    # (q35's own USB controllers are EHCI, UHCI is only there as an ICH9
    # companion controller normally), so this is the simplest way to give
    # `drivers::usb::uhci` a real UHCI controller to enumerate under QEMU;
    # usb-kbd is a synthetic USB HID boot-protocol keyboard, letting
    # `make run` exercise that whole driver end-to-end without real hardware.
    QFLAGS_COMMON := -machine q35 -cpu qemu64 -m 128M \
                     -device piix3-usb-uhci -device usb-kbd \
                     -kernel target/$(TARGET)/release/kernel \
                     -no-reboot -no-shutdown
    ifeq ($(BOARD), qemu-pc)
        BOARD_FEATURES :=
    else ifeq ($(BOARD), virtualbox)
        BOARD_FEATURES := --no-default-features --features board-virtualbox
    else
        $(error Unsupported BOARD=$(BOARD) for ARCH=x86_64. Use qemu-pc or virtualbox)
    endif
else ifeq ($(ARCH), riscv64)
    TARGET  := riscv64gc-unknown-none-elf
    QEMU    := qemu-system-riscv64
    BOARD   ?= qemu-virt
    QFLAGS_COMMON := -machine virt -cpu rv64 -m 128M \
                     -bios default \
                     -kernel target/$(TARGET)/release/kernel \
                     -no-reboot -no-shutdown
    ifeq ($(BOARD), qemu-virt)
        BOARD_FEATURES :=
    else
        $(error Unsupported BOARD=$(BOARD) for ARCH=riscv64. Use qemu-virt)
    endif
else ifeq ($(ARCH), aarch64)
    TARGET  := aarch64-unknown-none
    QEMU    := qemu-system-aarch64
    BOARD   ?= qemu-virt
    ifeq ($(BOARD), qemu-virt)
        BOARD_FEATURES :=
        # No -bios: booting a raw ELF via -kernel on this QEMU version jumps
        # straight to the ELF entry point at EL1, no firmware needed. Passing
        # `-bios none` errors out ("Could not find ROM image 'none'") instead
        # of behaving like a no-op sentinel.
        QFLAGS_COMMON := -machine virt,gic-version=2 -cpu cortex-a72 -m 128M \
                         -kernel target/$(TARGET)/release/kernel \
                         -no-reboot -no-shutdown
    else ifeq ($(BOARD), raspberrypi3)
        BOARD_FEATURES := --no-default-features --features board-raspberrypi3
        # QEMU's raspi3b machine models a fixed 1 GiB of RAM and rejects any
        # other -m value; the kernel itself still only uses its Fase-1
        # 128 MiB hardcode (see boards::raspberrypi3::RAM_FALLBACK_SIZE).
        QFLAGS_COMMON := -machine raspi3b -cpu cortex-a53 -m 1024M \
                         -kernel target/$(TARGET)/release/kernel \
                         -no-reboot -no-shutdown
    else
        $(error Unsupported BOARD=$(BOARD) for ARCH=aarch64. Use qemu-virt or raspberrypi3)
    endif
else
    $(error Unsupported ARCH=$(ARCH). Use x86_64, riscv64 or aarch64)
endif

# Headless: serial via stdin/stdout do terminal (padrão)
QFLAGS         := $(QFLAGS_COMMON) -serial stdio -display none
# Com janela VGA aberta. IMPORTANTE: em ambas as arquiteturas, TODA a saída do
# shell (prompt, comandos, erros) só vai para o terminal via serial, nenhuma
# delas escreve no framebuffer VGA hoje (x86 só desenha ali em caso de pane do
# kernel; RISC-V não tem driver de console gráfico, só serial via SBI). Ou
# seja: a janela SDL fica em branco durante uso normal, digite e leia sempre
# no terminal onde você rodou o `make`, não na janela.
QFLAGS_DISPLAY := $(QFLAGS_COMMON) -serial stdio -display sdl
# Sessão gráfica real via VNC (porta 5900+N, N=VNC_DISPLAY) em vez de uma
# janela SDL local: útil para testar teclado PS/2/USB de um cliente VNC de
# verdade (vinculado a hardware de teclado emulado de verdade, ao contrário
# do QMP `sendkey`) sem precisar de um display local — inclusive de outra
# máquina via `vncviewer host:N` ou um túnel SSH. Mesma ressalva do SDL: a
# tela fica em branco, digite/leia sempre no terminal (serial=stdio).
VNC_DISPLAY   ?= 5
QFLAGS_VNC     := $(QFLAGS_COMMON) -serial stdio -vnc :$(VNC_DISPLAY)

CARGO_FLAGS := --target $(TARGET) --release -p kernel $(BOARD_FEATURES)

ISO         := ferrugem.iso
KERNEL      := target/$(TARGET)/release/kernel
KERNEL8_IMG := kernel8.img

INIT_ELF := userspace/init/target/x86_64-unknown-linux-musl/release/init

.PHONY: all build run run-display run-vnc iso iso-virtualbox clean clippy fmt userspace x86 x86-display x86-vnc riscv riscv-display riscv-vnc aarch64 aarch64-display aarch64-vnc virtualbox raspberrypi3 raspberrypi3-img test-x86

all: build

# Build the musl-linked userspace init binary (x86_64 only; RISC-V later).
userspace:
	cd userspace/init && cargo build --release

$(INIT_ELF): userspace

# Build the kernel; on x86_64, ensure userspace/init is built first.
ifeq ($(ARCH), x86_64)
build: $(INIT_ELF)
	cargo build $(CARGO_FLAGS)
else
build:
	cargo build $(CARGO_FLAGS)
endif

run: build
	$(QEMU) $(QFLAGS)

# Roda com janela VGA aberta. A janela fica em branco durante uso normal —
# digite e leia sempre NESTE terminal (serial=stdio), não na janela SDL.
run-display: build
	@echo "*** Digite e leia aqui no terminal, a janela SDL nao mostra a shell. ***"
	$(QEMU) $(QFLAGS_DISPLAY)

# Roda com um servidor VNC de verdade em vez de janela local (ver comentário
# de QFLAGS_VNC acima). Conecte com `vncviewer localhost:$(VNC_DISPLAY)` (ou
# de outra máquina) para ter um teclado real de verdade falando com o
# controlador PS/2/USB emulado.
run-vnc: build
	@echo "*** VNC em localhost:5900+$(VNC_DISPLAY) (porta $$((5900+$(VNC_DISPLAY)))). Digite e leia aqui no terminal, a tela VNC nao mostra a shell. ***"
	$(QEMU) $(QFLAGS_VNC)

iso-virtualbox: build
	cp $(KERNEL) iso/boot/kernel
	objcopy --remove-section=.note.Xen iso/boot/kernel
	grub-mkrescue -o $(ISO) iso/
	@echo "ISO gerada: $(ISO)"
	@echo "No VirtualBox: nova VM → tipo Other/Unknown (64-bit) → boot CD → selecione $(ISO)"

clippy:
	cargo clippy $(CARGO_FLAGS) -- -D warnings

fmt:
	cargo fmt --all

clean:
	cargo clean
	cd userspace/init && cargo clean

# Convenience targets
x86:
	$(MAKE) ARCH=x86_64 run

x86-display:
	$(MAKE) ARCH=x86_64 run-display

x86-vnc:
	$(MAKE) ARCH=x86_64 run-vnc

# In-kernel unit tests (see kernel/src/testing.rs). x86_64 only for now: pass/fail
# is reported via QEMU's isa-debug-exit device, which the runner in
# .cargo/config.toml (kernel/scripts/qemu-test-runner.sh) boots headlessly.
test-x86: $(INIT_ELF)
	cargo test --target x86_64-unknown-none -p kernel

riscv:
	$(MAKE) ARCH=riscv64 run

riscv-display:
	$(MAKE) ARCH=riscv64 run-display

riscv-vnc:
	$(MAKE) ARCH=riscv64 run-vnc

aarch64:
	$(MAKE) ARCH=aarch64 run

aarch64-display:
	$(MAKE) ARCH=aarch64 run-display

aarch64-vnc:
	$(MAKE) ARCH=aarch64 run-vnc

# BSP: x86_64 PC platform, VirtualBox, builds the GRUB ISO with the
# board-virtualbox feature instead of qemu-pc's default.
virtualbox:
	$(MAKE) ARCH=x86_64 BOARD=virtualbox iso-virtualbox

# BSP: Raspberry Pi 3 (BCM2837), run under QEMU's `raspi3b` machine (which
# emulates it closely enough to serve as the day-to-day test target, real
# hardware needs a `kernel8.img` on the SD card's boot partition instead).
raspberrypi3:
	$(MAKE) ARCH=aarch64 BOARD=raspberrypi3 run

# Real Raspberry Pi 3 B hardware: converts the ELF into the raw kernel8.img
# firmware expects on the SD card's boot partition, unlike QEMU's `-kernel`
# (used by the `raspberrypi3` target above), real firmware has no ELF loader.
# Requires `rust-objcopy` (cargo-binutils; `rustup component add llvm-tools`
# + `cargo install cargo-binutils`).
raspberrypi3-img:
	$(MAKE) ARCH=aarch64 BOARD=raspberrypi3 build
	rust-objcopy -O binary target/aarch64-unknown-none/release/kernel $(KERNEL8_IMG)
	@echo "$(KERNEL8_IMG) gerada."
	@echo "Copie para a particao de boot (FAT32) do cartao SD, junto com:"
	@echo "  - kernel/src/boards/raspberrypi3/config.txt"
	@echo "  - bootcode.bin, start.elf, fixup.dat (firmware oficial da Raspberry Pi,"
	@echo "    NAO gerados por este projeto): https://github.com/raspberrypi/firmware/tree/master/boot"
