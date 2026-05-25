//! ADB domain layer.
//!
//! Module shape:
//!
//! - [`resolver`] — locate the `adb` binary, probe its version. No
//!   network or device traffic.
//! - [`transport`] — the [`AdbTransport`] trait abstracting "talk to a
//!   device", a [`ShellTransport`] that shells out to the resolved
//!   binary, and a [`MockTransport`] used by tests.
//! - [`device`] — value types (`Device`, `DeviceState`) returned by the
//!   transport.
//! - [`packages`] — `pm list packages` parsing and the typed
//!   [`AppPackage`] / [`PackageFilter`] surfaces.
//!
//! All Tauri `#[command]` glue lives in `crate::commands`, not here.
//! The domain layer is `cargo test`-able without a Tauri runtime.
//!
//! Why shell out today instead of `adb_client`: the pure-Rust crate
//! (v3.x) still lacks Android 11+ wireless `adb pair` mTLS. For v0.1 we
//! lean on the upstream binary so every flow matches the user's
//! command line; the crate can slot in as an optimization once R-011
//! is past the smoke-test phase.

pub mod actions;
pub mod device;
pub mod packages;
pub mod resolver;
pub mod transport;

// Public re-exports for cross-module consumers (`commands`, tests).
// Items in `actions` are reached via `crate::adb::actions::*` so no
// short alias is needed here.
pub use device::Device;
pub use packages::{list_packages, AppPackage, PackageFilter};
pub use resolver::{locate_adb, AdbResolution};
pub use transport::{AdbTransport, ShellTransport, TransportError};
