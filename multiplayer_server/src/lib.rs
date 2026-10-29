//! Shared wire protocol. The DLL uses this library without the `server` feature.
pub mod identity;
pub mod protocol;
pub mod session_client;
pub mod session_hub;
#[cfg(test)]
mod session_hub_tests;
pub mod session_protocol;
pub mod session_timing;

#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub mod session_server;
