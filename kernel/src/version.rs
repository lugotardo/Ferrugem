/// Kernel version single source of truth.
///
/// The version number lives in `Cargo.toml` (`package.version`).
/// All other crates and modules must read from this module; never
/// hardcode version strings elsewhere.
///
/// Build script (`build.rs`) resolves the git commit hash at compile
/// time and exposes it via the `KERNEL_GIT_HASH` environment variable.

/// Kernel name.
pub const NAME: &str = "Ferrugem";

/// Semantic version string, sourced from `Cargo.toml` at compile time.
/// Format: `MAJOR.MINOR.PATCH`
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Short git commit hash (8 hex digits), resolved by `build.rs`.
/// Falls back to `"00000000"` when git is unavailable (e.g. CI tarballs).
pub const GIT_HASH: &str = env!("KERNEL_GIT_HASH");

/// Full version string shown in banners and uname output.
/// Format: `MAJOR.MINOR.PATCH-GITHASH`  →  e.g. `0.1.0-a3f8c12d`
pub const VERSION_FULL: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "-",
    env!("KERNEL_GIT_HASH"),
);

/// One-line banner printed at boot.
pub const BANNER: &str = concat!(
    "Ferrugem v",
    env!("CARGO_PKG_VERSION"),
    "-",
    env!("KERNEL_GIT_HASH"),
    "\n",
);
