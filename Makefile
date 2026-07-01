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
else
    $(error Unsupported ARCH=$(ARCH). Use x86_64 or riscv64)
endif

# Headless: serial via stdin/stdout do terminal (padrão)
QFLAGS         := $(QFLAGS_COMMON) -serial stdio -display none
# Com janela VGA: serial ainda vai para stdin/stdout do terminal que iniciou o QEMU.
# A janela SDL exibe saída VGA; PS/2 funciona após clicar na janela (Ctrl+Alt libera).
QFLAGS_DISPLAY := $(QFLAGS_COMMON) -serial stdio -display sdl

CARGO_FLAGS := --target $(TARGET) --release -p kernel

ISO      := ferrugem.iso
KERNEL   := target/$(TARGET)/release/kernel

.PHONY: all build run run-display iso clean clippy fmt

all: build

build:
	cargo build $(CARGO_FLAGS)

run: build
	$(QEMU) $(QFLAGS)

# Roda com janela VGA aberta; digita no terminal que iniciou o QEMU (serial=stdio).
# Na janela SDL: Ctrl+Alt libera o mouse | PS/2 ativo após clicar na janela.
run-display: build
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

# Convenience targets
x86:
	$(MAKE) ARCH=x86_64 run

x86-display:
	$(MAKE) ARCH=x86_64 run-display

riscv:
	$(MAKE) ARCH=riscv64 run
