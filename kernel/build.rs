use std::process::Command;
use std::env;
use std::path::PathBuf;

fn main() {
    let out = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("CARGO_CFG_TARGET_ARCH").unwrap();
    let dir = board_dir(&target);

    match target.as_str() {
        "x86_64" => assemble_x86(&out, dir),
        "riscv64" => assemble_riscv(&out, dir),
        _ => {}
    }

    emit_linker_script(dir);
    emit_git_hash();
}

/// Directory holding the active board's boot glue + linker script.
///
/// `.cargo/config.toml` rustflags are keyed by target triple only, so they
/// can't vary by Cargo feature — board selection (`board-*` features, see
/// `kernel/Cargo.toml`) has to be resolved here instead. Cargo exposes each
/// enabled feature to build scripts as `CARGO_FEATURE_<NAME>` (uppercased,
/// dashes -> underscores).
fn board_dir(target_arch: &str) -> &'static str {
    match target_arch {
        "aarch64" => {
            if env::var_os("CARGO_FEATURE_BOARD_RASPBERRYPI3").is_some() {
                "src/boards/raspberrypi3"
            } else {
                "src/boards/qemu_virt_aarch64"
            }
        }
        "x86_64" => {
            if env::var_os("CARGO_FEATURE_BOARD_VIRTUALBOX").is_some() {
                "src/boards/virtualbox"
            } else {
                "src/boards/qemu_pc"
            }
        }
        "riscv64" => "src/boards/qemu_virt_riscv64",
        _ => "",
    }
}

fn emit_linker_script(dir: &str) {
    if dir.is_empty() { return; }
    // `rerun-if-changed` is resolved relative to this build script's own cwd
    // (the `kernel` package root), but `rustc-link-arg` is passed straight
    // through to the linker invocation, whose cwd is wherever `cargo` itself
    // was run from (the workspace root, per the top-level Makefile) — hence
    // the extra `kernel/` prefix only on the link-arg variant.
    let rel_path = format!("{dir}/linker.ld");
    println!("cargo:rustc-link-arg=-Tkernel/{rel_path}");
    println!("cargo:rerun-if-changed={rel_path}");
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

fn assemble_x86(out: &PathBuf, dir: &str) {
    let src = format!("{dir}/boot.s");
    let obj = out.join("boot_x86.o");
    let status = Command::new("as")
        .args(["--64", &src, "-o", obj.to_str().unwrap()])
        .status()
        .expect("'as' (binutils) not found install binutils");
    assert!(status.success(), "boot.s assembly failed");
    println!("cargo:rustc-link-arg={}", obj.display());
    println!("cargo:rerun-if-changed={src}");
}

fn assemble_riscv(out: &PathBuf, dir: &str) {
    let src = format!("{dir}/boot.s");
    let obj = out.join("boot_riscv.o");
    let as_bin = find_riscv_as();
    let status = Command::new(&as_bin)
        .args(["-march=rv64gc", "-mabi=lp64d", &src, "-o", obj.to_str().unwrap()])
        .status()
        .expect("RISC-V assembler not found");
    assert!(status.success(), "riscv boot.s assembly failed");
    println!("cargo:rustc-link-arg={}", obj.display());
    println!("cargo:rerun-if-changed={src}");
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
