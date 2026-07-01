use std::process::Command;
use std::env;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("CARGO_CFG_TARGET_ARCH").unwrap();

    match target.as_str() {
        "x86_64" => assemble_x86(&out),
        "riscv64" => assemble_riscv(&out),
        _ => {}
    }

    emit_git_hash();

    println!("cargo:rerun-if-changed=src/arch/x86_64/boot.s");
    println!("cargo:rerun-if-changed=src/arch/riscv64/boot.s");
}

/// Bake the short git commit hash into KERNEL_GIT_HASH so version.rs can
/// expose it as a compile-time constant without depending on runtime state.
fn emit_git_hash() {
    let hash = Command::new("git")
        .args(["rev-parse", "--short=8", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "00000000".to_string());

    println!("cargo:rustc-env=KERNEL_GIT_HASH={}", hash);
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/refs/heads");
}

fn assemble_x86(out: &PathBuf) {
    let obj = out.join("boot_x86.o");
    let status = Command::new("as")
        .args([
            "--64",
            "src/arch/x86_64/boot.s",
            "-o",
            obj.to_str().unwrap(),
        ])
        .status()
        .expect("'as' (binutils) not found install binutils");
    assert!(status.success(), "boot.s assembly failed");
    println!("cargo:rustc-link-arg={}", obj.display());
}

fn assemble_riscv(out: &PathBuf) {
    let obj = out.join("boot_riscv.o");
    let as_bin = find_riscv_as();
    let status = Command::new(&as_bin)
        .args([
            "-march=rv64gc",
            "-mabi=lp64d",
            "src/arch/riscv64/boot.s",
            "-o",
            obj.to_str().unwrap(),
        ])
        .status()
        .expect("RISC-V assembler not found");
    assert!(status.success(), "riscv boot.s assembly failed");
    println!("cargo:rustc-link-arg={}", obj.display());
}

fn find_riscv_as() -> String {
    for candidate in &[
        "riscv64-unknown-elf-as",
        "riscv64-linux-gnu-as",
        "riscv64-elf-as",
    ] {
        if Command::new(candidate).arg("--version").output().is_ok() {
            return candidate.to_string();
        }
    }
    panic!("No RISC-V assembler found. Install: sudo apt install gcc-riscv64-linux-gnu");
}
