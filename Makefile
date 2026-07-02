ARCH ?= x86_64

ifeq ($(ARCH), x86_64)
    TARGET  := x86_64-unknown-none
    QEMU    := qemu-system-x86_64
    QFLAGS_COMMON := -machine q35 -cpu qemu64 -m 128M \
                     -kernel target/$(TARGET)/release/kernel \
                     -no-reboot -no-shutdown
else ifeq ($(ARCH), riscv64)
    TARGET  := riscv64gc-unknown-none-elf
    QEMU    := qemu-system-riscv64
    QFLAGS_COMMON := -machine virt -cpu rv64 -m 128M \
                     -bios default \
                     -kernel target/$(TARGET)/release/kernel \
                     -no-reboot -no-shutdown
else ifeq ($(ARCH), aarch64)
    TARGET  := aarch64-unknown-none
    QEMU    := qemu-system-aarch64
    # No -bios: booting a raw ELF via -kernel on this QEMU version jumps
    # straight to the ELF entry point at EL1, no firmware needed. Passing
    # `-bios none` errors out ("Could not find ROM image 'none'") instead
    # of behaving like a no-op sentinel.
    QFLAGS_COMMON := -machine virt,gic-version=2 -cpu cortex-a72 -m 128M \
                     -kernel target/$(TARGET)/release/kernel \
                     -no-reboot -no-shutdown
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

CARGO_FLAGS := --target $(TARGET) --release -p kernel

ISO      := ferrugem.iso
KERNEL   := target/$(TARGET)/release/kernel

INIT_ELF := userspace/init/target/x86_64-unknown-linux-musl/release/init

.PHONY: all build run run-display iso iso-virtualbox clean clippy fmt userspace x86 x86-display riscv riscv-display aarch64 aarch64-display

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
