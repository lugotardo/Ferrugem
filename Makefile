ARCH ?= x86_64

# BOARD selects which BSP (board-* Cargo feature, see kernel/Cargo.toml) is
# built for the chosen ARCH. Each ARCH has one default board that matches
# today's behavior when BOARD isn't set; pass BOARD=<name> to pick another.
ifeq ($(ARCH), x86_64)
    TARGET  := x86_64-unknown-none
    QEMU    := qemu-system-x86_64
    BOARD   ?= qemu-pc
    QFLAGS_COMMON := -machine q35 -cpu qemu64 -m 128M \
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
# shell (prompt, comandos, erros) só vai para o terminal via serial — nenhuma
# delas escreve no framebuffer VGA hoje (x86 só desenha ali em caso de pane do
# kernel; RISC-V não tem driver de console gráfico, só serial via SBI). Ou
# seja: a janela SDL fica em branco durante uso normal — digite e leia sempre
# no terminal onde você rodou o `make`, não na janela.
QFLAGS_DISPLAY := $(QFLAGS_COMMON) -serial stdio -display sdl

CARGO_FLAGS := --target $(TARGET) --release -p kernel $(BOARD_FEATURES)

ISO      := ferrugem.iso
KERNEL   := target/$(TARGET)/release/kernel

INIT_ELF := userspace/init/target/x86_64-unknown-linux-musl/release/init

.PHONY: all build run run-display iso iso-virtualbox clean clippy fmt userspace x86 x86-display riscv riscv-display aarch64 aarch64-display virtualbox raspberrypi3

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
	@echo "*** Digite e leia aqui no terminal — a janela SDL nao mostra a shell. ***"
	$(QEMU) $(QFLAGS_DISPLAY)

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

riscv:
	$(MAKE) ARCH=riscv64 run

riscv-display:
	$(MAKE) ARCH=riscv64 run-display

aarch64:
	$(MAKE) ARCH=aarch64 run

aarch64-display:
	$(MAKE) ARCH=aarch64 run-display

# BSP: x86_64 PC platform, VirtualBox — builds the GRUB ISO with the
# board-virtualbox feature instead of qemu-pc's default.
virtualbox:
	$(MAKE) ARCH=x86_64 BOARD=virtualbox iso-virtualbox

# BSP: Raspberry Pi 3 (BCM2837), run under QEMU's `raspi3b` machine (which
# emulates it closely enough to serve as the day-to-day test target — real
# hardware needs a `kernel8.img` on the SD card's boot partition instead).
raspberrypi3:
	$(MAKE) ARCH=aarch64 BOARD=raspberrypi3 run
