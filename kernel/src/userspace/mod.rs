// ── Embedded ELF64 binaries ───────────────────────────────────────────────────
//
// Minimal PIE (ET_DYN, e_entry=0) ELF64 binaries.  The ELF loader relocates
// them so the first PT_LOAD page lands at USER_BASE_VA (arch-specific).
//
// Layout per binary:
//   offset 0x00 (64 B): ELF header   — ET_DYN, e_entry=0, e_phoff=64
//   offset 0x40 (56 B): PT_LOAD PHDR — p_vaddr=0, p_offset=0x78
//   offset 0x78       : code + message
//
// Syscall ABI:
//   RISC-V: ecall, a7=nr, a0/a1/a2=args
//   x86_64: int 0x80, rax=nr, rdi=a0, rsi=a1, rdx=a2

/// Minimal PIE ELF64 for RISC-V: writes "[elf] Hello from ELF!\n" then exits.
///
/// Code at offset 0x78 (loaded at USER_BASE_VA = 0x1_0000_0000):
///   addi a7, x0, 1       SYS_WRITE
///   addi a0, x0, 1       fd=stdout
///   auipc a1, 0          a1 = PC (= entry_VA + 8)
///   addi a1, a1, 28      a1 = entry_VA + 36 = &msg
///   addi a2, x0, 22      len=22
///   ecall
///   addi a7, x0, 60      SYS_EXIT
///   addi a0, x0, 0       code=0
///   ecall
///   "[elf] Hello from ELF!\n"   (21 bytes)
#[cfg(target_arch = "riscv64")]
#[rustfmt::skip]
pub static HELLO_ELF_RV64: [u8; 178] = [
    // ── ELF header (64 bytes) ────────────────────────────────────────────
    0x7f, 0x45, 0x4c, 0x46,  // EI_MAG
    0x02,                     // ELFCLASS64
    0x01,                     // ELFDATA2LSB
    0x01,                     // EI_VERSION
    0x00,                     // OSABI = none
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // padding
    0x03, 0x00,               // e_type = ET_DYN
    0xf3, 0x00,               // e_machine = EM_RISCV (243)
    0x01, 0x00, 0x00, 0x00,  // e_version
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // e_entry = 0
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // e_phoff = 64
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // e_shoff = 0
    0x00, 0x00, 0x00, 0x00,  // e_flags
    0x40, 0x00,               // e_ehsize = 64
    0x38, 0x00,               // e_phentsize = 56
    0x01, 0x00,               // e_phnum = 1
    0x40, 0x00,               // e_shentsize = 64
    0x00, 0x00,               // e_shnum = 0
    0x00, 0x00,               // e_shstrndx = 0
    // ── Program header (56 bytes) ────────────────────────────────────────
    0x01, 0x00, 0x00, 0x00,  // p_type = PT_LOAD
    0x05, 0x00, 0x00, 0x00,  // p_flags = PF_R | PF_X
    0x78, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_offset = 0x78
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_vaddr = 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_paddr = 0
    0x3a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_filesz = 58 (36 code + 22 msg)
    0x3a, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_memsz = 58
    0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_align = 0x1000
    // ── Code at offset 0x78 (9 × 4 B = 36 bytes) ────────────────────────
    0x93, 0x08, 0x10, 0x00,  // addi a7, x0, 1    (SYS_WRITE)
    0x13, 0x05, 0x10, 0x00,  // addi a0, x0, 1    (stdout)
    0x97, 0x05, 0x00, 0x00,  // auipc a1, 0
    0x93, 0x85, 0xc5, 0x01,  // addi a1, a1, 28   (&msg = PC+8+28 = entry+36)
    0x13, 0x06, 0x60, 0x01,  // addi a2, x0, 22   (len)
    0x73, 0x00, 0x00, 0x00,  // ecall
    0x93, 0x08, 0xc0, 0x03,  // addi a7, x0, 60   (SYS_EXIT)
    0x13, 0x05, 0x00, 0x00,  // addi a0, x0, 0
    0x73, 0x00, 0x00, 0x00,  // ecall
    // ── Message at offset 0x9C (21 bytes) ───────────────────────────────
    b'[', b'e', b'l', b'f', b']', b' ',
    b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r', b'o', b'm',
    b' ', b'E', b'L', b'F', b'!', b'\n',
];

/// Minimal PIE ELF64 for x86_64: writes "[elf] Hello from ELF!\n" then exits.
///
/// Code at offset 0x78 (loaded at USER_BASE_VA = 0x0000_0080_0000_0000):
///   mov rax, 1           SYS_WRITE
///   mov rdi, 1           fd=stdout
///   lea rsi, [rip+0x19]  rip=entry+21, msg at entry+46, 46-21=25=0x19
///   mov rdx, 22          len=22
///   int 0x80
///   mov rax, 60          SYS_EXIT
///   mov rdi, 0           code=0
///   int 0x80
///   "[elf] Hello from ELF!\n"  (21 bytes)
#[cfg(target_arch = "x86_64")]
#[rustfmt::skip]
pub static HELLO_ELF_X86: [u8; 188] = [
    // ── ELF header (64 bytes) ────────────────────────────────────────────
    0x7f, 0x45, 0x4c, 0x46,
    0x02, 0x01, 0x01, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x03, 0x00,               // ET_DYN
    0x3e, 0x00,               // EM_X86_64 (62)
    0x01, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // e_entry = 0
    0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // e_phoff = 64
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
    0x00, 0x00, 0x00, 0x00,
    0x40, 0x00, 0x38, 0x00, 0x01, 0x00, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00,
    // ── Program header (56 bytes) ────────────────────────────────────────
    0x01, 0x00, 0x00, 0x00,  // PT_LOAD
    0x05, 0x00, 0x00, 0x00,  // PF_R | PF_X
    0x78, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_offset = 0x78
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_vaddr = 0
    0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_paddr = 0
    0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_filesz = 68 (46 code + 22 msg)
    0x44, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_memsz = 68
    0x00, 0x10, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,  // p_align = 0x1000
    // ── Code at offset 0x78 (46 bytes) ──────────────────────────────────
    0x48, 0xc7, 0xc0, 0x01, 0x00, 0x00, 0x00,  // mov rax, 1
    0x48, 0xc7, 0xc7, 0x01, 0x00, 0x00, 0x00,  // mov rdi, 1
    0x48, 0x8d, 0x35, 0x19, 0x00, 0x00, 0x00,  // lea rsi, [rip+0x19]
    0x48, 0xc7, 0xc2, 0x16, 0x00, 0x00, 0x00,  // mov rdx, 22
    0xcd, 0x80,                                 // int 0x80
    0x48, 0xc7, 0xc0, 0x3c, 0x00, 0x00, 0x00,  // mov rax, 60
    0x48, 0xc7, 0xc7, 0x00, 0x00, 0x00, 0x00,  // mov rdi, 0
    0xcd, 0x80,                                 // int 0x80
    // ── Message at offset 0xA6 (21 bytes) ───────────────────────────────
    b'[', b'e', b'l', b'f', b']', b' ',
    b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r', b'o', b'm',
    b' ', b'E', b'L', b'F', b'!', b'\n',
];

// ── Raw machine-code programs (no ELF wrapper) ────────────────────────────────

/// RISC-V U-mode program: greeting via ecall then exit.
///
/// RV64I encoding — RISC-V Linux ABI: a7=nr, a0-a5=args, a0=retval.
///
/// Byte layout (4-byte instructions, little-endian):
///   0  : addi a7, x0, 1    (SYS_WRITE)
///   4  : addi a0, x0, 1    (stdout)
///   8  : auipc a1, 0       → a1 = code_phys + 8
///   12 : addi a1, a1, 32   → a1 = code_phys + 40 (MSG)
///   16 : addi a2, x0, 26   (len)
///   20 : ecall
///   24 : addi a7, x0, 60   (SYS_EXIT)
///   28 : addi a0, x0, 0    (exit code)
///   32 : ecall
///   36 : jal x0, 0         (infinite loop)
///   40 : MSG "[user] Hello from U-mode!\n" (26 bytes)
#[cfg(target_arch = "riscv64")]
#[rustfmt::skip]
pub static HELLO_USER_RV64: [u8; 66] = [
    0x93, 0x08, 0x10, 0x00,  // addi a7, x0, 1
    0x13, 0x05, 0x10, 0x00,  // addi a0, x0, 1
    0x97, 0x05, 0x00, 0x00,  // auipc a1, 0
    0x93, 0x85, 0x05, 0x02,  // addi a1, a1, 32
    0x13, 0x06, 0xA0, 0x01,  // addi a2, x0, 26
    0x73, 0x00, 0x00, 0x00,  // ecall
    0x93, 0x08, 0xC0, 0x03,  // addi a7, x0, 60
    0x13, 0x05, 0x00, 0x00,  // addi a0, x0, 0
    0x73, 0x00, 0x00, 0x00,  // ecall
    0x6F, 0x00, 0x00, 0x00,  // jal x0, 0  (j .)
    // MSG at offset 40: "[user] Hello from U-mode!\n" (26 bytes)
    b'[', b'u', b's', b'e', b'r', b']', b' ',
    b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r', b'o', b'm',
    b' ', b'U', b'-', b'm', b'o', b'd', b'e', b'!', b'\n',
];

/// x86_64 ring-3 program: greeting via INT 0x80 then exit.
///
/// First userspace program: writes a greeting via INT 0x80 then exits.
///
/// x86_64 machine code assembled by hand.  Runs at ring 3; uses the Linux
/// x86_64 INT 0x80 ABI (rax=nr, rdi=a0, rsi=a1, rdx=a2).
///
/// Byte layout:
///   0  : mov rax, 1          (SYS_WRITE)
///   7  : mov rdi, 1          (stdout)
///   14 : lea rsi, [rip+0x1B] → MSG at offset 48  (rip after instr = 21, 21+27=48)
///   21 : mov rdx, 26         (MSG length)
///   28 : int 0x80
///   30 : mov rax, 60         (SYS_EXIT)
///   37 : mov rdi, 0
///   44 : int 0x80
///   46 : jmp $               (hang if exit ever returns)
///   48 : MSG "[user] Hello from ring 3!\n" (26 bytes)
#[rustfmt::skip]
pub static HELLO_USER: [u8; 74] = [
    // mov rax, 1
    0x48, 0xC7, 0xC0, 0x01, 0x00, 0x00, 0x00,
    // mov rdi, 1
    0x48, 0xC7, 0xC7, 0x01, 0x00, 0x00, 0x00,
    // lea rsi, [rip + 0x1B]  (rip=21, 21+27=48 → MSG)
    0x48, 0x8D, 0x35, 0x1B, 0x00, 0x00, 0x00,
    // mov rdx, 26
    0x48, 0xC7, 0xC2, 0x1A, 0x00, 0x00, 0x00,
    // int 0x80
    0xCD, 0x80,
    // mov rax, 60
    0x48, 0xC7, 0xC0, 0x3C, 0x00, 0x00, 0x00,
    // mov rdi, 0
    0x48, 0xC7, 0xC7, 0x00, 0x00, 0x00, 0x00,
    // int 0x80
    0xCD, 0x80,
    // jmp $  (ends at offset 48 = start of MSG)
    0xEB, 0xFE,
    // MSG at offset 48: "[user] Hello from ring 3!\n" (26 bytes)
    b'[', b'u', b's', b'e', b'r', b']', b' ',
    b'H', b'e', b'l', b'l', b'o', b' ', b'f', b'r', b'o', b'm',
    b' ', b'r', b'i', b'n', b'g', b' ', b'3', b'!', b'\n',
];
