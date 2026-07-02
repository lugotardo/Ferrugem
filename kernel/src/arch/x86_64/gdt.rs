/// GDT with flat 64-bit segments (null, kernel code, kernel data, user code, user data)
/// plus a TSS descriptor for ring-0 stack on interrupts.

#[repr(C, packed)]
struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    gran: u8,   // flags[7:4] + limit_high[3:0]
    base_high: u8,
}

impl GdtEntry {
    const fn null() -> Self {
        Self { limit_low: 0, base_low: 0, base_mid: 0, access: 0, gran: 0, base_high: 0 }
    }

    const fn new(base: u32, limit: u32, access: u8, gran: u8) -> Self {
        Self {
            limit_low: (limit & 0xFFFF) as u16,
            base_low:  (base & 0xFFFF) as u16,
            base_mid:  ((base >> 16) & 0xFF) as u8,
            access,
            gran: (gran & 0xF0) | (((limit >> 16) & 0x0F) as u8),
            base_high: ((base >> 24) & 0xFF) as u8,
        }
    }
}

// 64-bit TSS descriptor is 16 bytes
#[repr(C, packed)]
struct TssDescriptor {
    low:  GdtEntry,
    base_upper: u32,
    _reserved: u32,
}

#[repr(C, packed)]
pub struct Tss {
    _reserved0: u32,
    pub rsp0: u64,
    pub rsp1: u64,
    pub rsp2: u64,
    _reserved1: u64,
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    pub iomap_base: u16,
}

// NOTE: user data MUST be at 0x18 and user code at 0x20 so that SYSRETQ
// works with IA32_STAR[63:48]=0x10:
//   SS = 0x10+8  | 3 = 0x1B → GDT[0x18] = user data ✓
//   CS = 0x10+16 | 3 = 0x23 → GDT[0x20] = user code ✓
static mut GDT: [GdtEntry; 5] = [
    GdtEntry::null(),                             // 0x00 null
    GdtEntry::new(0, 0xFFFFF, 0x9A, 0xA0),       // 0x08 kernel code (64-bit)
    GdtEntry::new(0, 0xFFFFF, 0x92, 0xC0),       // 0x10 kernel data
    GdtEntry::new(0, 0xFFFFF, 0xF2, 0xC0),       // 0x18 user data  ← data before code for SYSRET
    GdtEntry::new(0, 0xFFFFF, 0xFA, 0xA0),       // 0x20 user code (64-bit)
];

// Kernel stack for ISR stack switches (4KiB)
static mut KERNEL_STACK: [u8; 4096] = [0u8; 4096];

static mut TSS: Tss = Tss {
    _reserved0: 0,
    rsp0: 0,
    rsp1: 0,
    rsp2: 0,
    _reserved1: 0,
    ist: [0u64; 7],
    _reserved2: 0,
    _reserved3: 0,
    iomap_base: core::mem::size_of::<Tss>() as u16,
};

#[repr(C, packed)]
struct GdtPtr {
    limit: u16,
    base: u64,
}

pub fn init() {
    unsafe {
        TSS.rsp0 = KERNEL_STACK.as_ptr().add(KERNEL_STACK.len()) as u64;

        // Extend GDT to 7 entries to hold TSS (16 bytes = 2 descriptors)
        // We use a static array large enough
        static mut FULL_GDT: [u64; 7] = [0u64; 7];

        // Copy flat segments as raw u64
        let gdt_ptr = GDT.as_ptr() as *const u64;
        for i in 0..5 {
            FULL_GDT[i] = gdt_ptr.add(i).read_unaligned();
        }

        // Build TSS descriptor
        let tss_base = &TSS as *const Tss as u64;
        let tss_limit = (core::mem::size_of::<Tss>() - 1) as u64;
        let lo: u64 = ((tss_base & 0xFF_FFFF) << 16)
            | (tss_limit & 0xFFFF)
            | (0x89u64 << 40)           // present, type=9 (64-bit TSS available)
            | (((tss_base >> 24) & 0xFF) << 56)
            | ((tss_limit >> 16) << 48);
        let hi: u64 = (tss_base >> 32) & 0xFF_FFFF_FFFF;
        FULL_GDT[5] = lo;
        FULL_GDT[6] = hi;

        let ptr = GdtPtr {
            limit: (core::mem::size_of_val(&FULL_GDT) - 1) as u16,
            base: FULL_GDT.as_ptr() as u64,
        };

        // lgdt, then far-return to flush CS, then reload data segments
        core::arch::asm!(
            "lgdt [{ptr}]",
            // Far return to flush CS: push CS then RIP of the label below
            "lea {tmp}, [rip + 2f]",
            "push 0x08",
            "push {tmp}",
            ".byte 0x48, 0xcb",   // REX.W RETF = far return in 64-bit mode
            "2:",
            "mov ax, 0x10",
            "mov ds, ax",
            "mov es, ax",
            "mov fs, ax",
            "mov gs, ax",
            "mov ss, ax",
            ptr = in(reg) &ptr,
            tmp = lateout(reg) _,
            options(nostack)
        );

        // Load TSS (selector 0x28 = index 5, TI=0, RPL=0)
        core::arch::asm!("ltr ax", in("ax") 0x28u16, options(nostack));
    }
}

pub const KERNEL_CODE_SEL: u16 = 0x08;
pub const KERNEL_DATA_SEL: u16 = 0x10;
pub const USER_CODE_SEL: u16 = 0x20 | 3;  // GDT[4] user code at byte offset 0x20
pub const USER_DATA_SEL: u16 = 0x18 | 3;  // GDT[3] user data at byte offset 0x18

/// Update TSS.rsp0 the kernel stack the CPU switches to on ring-0 entry.
/// Also updates SYSCALL_KERNEL_RSP so the SYSCALL handler switches to the
/// correct kernel stack for the current task.
/// Call this on every task switch.
pub fn set_rsp0(sp: u64) {
    unsafe {
        TSS.rsp0 = sp;
        crate::arch::x86_64::idt::SYSCALL_KERNEL_RSP = sp;
    }
}
