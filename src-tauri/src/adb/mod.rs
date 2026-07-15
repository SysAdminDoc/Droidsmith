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
//! - [`wireless`] — Android 11+ pairing/connect helpers around
//!   `adb pair`, `adb connect`, and mDNS service discovery.
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
pub mod device_info;
pub mod health;
pub mod packages;
pub mod parsers;
pub mod resolver;
pub mod transport;
pub mod users;
pub mod version_policy;
pub mod wireless;

// Public re-exports for cross-module consumers (`commands`, tests).
// Items in `actions` are reached via `crate::adb::actions::*` so no
// short alias is needed here.
pub use device::{
    attach_transport_provenance, observe_connection_generations, Device, DeviceTarget,
    DeviceTransportKind,
};
pub use device_info::{get_device_info, DeviceInfo};
pub use packages::{list_packages, AppPackage, PackageFilter};
pub use resolver::{locate_adb, AdbResolution};
pub use transport::{validate_device_target, AdbTransport, ShellTransport, TransportError};
pub use users::{list_users, AndroidUser};
pub use wireless::{
    connect as connect_wireless, list_mdns_services, pair as pair_wireless, WirelessAdbService,
    WirelessCommandResult, WirelessConnectRequest, WirelessPairRequest,
};
