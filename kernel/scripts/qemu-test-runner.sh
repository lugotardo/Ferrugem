#!/usr/bin/env bash
# `cargo test` runner for the x86_64-unknown-none target (see .cargo/config.toml).
# Cargo invokes this as `qemu-test-runner.sh <path-to-test-elf>`; we boot it
# headless in QEMU with the isa-debug-exit device wired up (kernel/src/
# testing.rs writes to it via arch::x86_64::port::outl to report pass/fail),
# then translate QEMU's process exit code back into one cargo understands.
#
# isa-debug-exit semantics: writing byte `v` makes QEMU exit with code
# `(v << 1) | 1`. QemuExitCode::Success = 0x10 -> exit 33, Failed = 0x11 -> 35.
set -euo pipefail

KERNEL_ELF="$1"
TIMEOUT_SECS=30

set +e
timeout "$TIMEOUT_SECS" qemu-system-x86_64 \
    -machine q35 -cpu qemu64 -m 128M \
    -device isa-debug-exit,iobase=0xf4,iosize=0x04 \
    -no-reboot -no-shutdown \
    -serial stdio -display none \
    -kernel "$KERNEL_ELF"
code=$?
set -e

if [ "$code" -eq 124 ]; then
    echo "qemu-test-runner: timed out after ${TIMEOUT_SECS}s (test hung without reaching isa-debug-exit)" >&2
    exit 1
elif [ "$code" -eq 33 ]; then
    exit 0
else
    echo "qemu-test-runner: QEMU exited with code $code (expected 33 for pass)" >&2
    exit 1
fi
