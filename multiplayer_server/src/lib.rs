//! Shared wire protocol. The DLL uses this library without the `server` feature.
pub mod protocol;

#[cfg(feature = "server")]
pub mod server;
