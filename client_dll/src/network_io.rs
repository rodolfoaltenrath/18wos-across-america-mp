//! UDP failures caused by a disconnected LAN do not terminate the DLL worker.
use std::io::{Error, ErrorKind};

pub(crate) fn transient(error: &Error) -> bool {
    matches!(
        error.kind(),
        ErrorKind::WouldBlock
            | ErrorKind::Interrupted
            | ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionAborted
            | ErrorKind::NetworkUnreachable
            | ErrorKind::HostUnreachable
    ) || cfg!(windows)
        && matches!(
            error.raw_os_error(),
            Some(10050 | 10051 | 10053 | 10054 | 10061 | 10065)
        )
}
pub(crate) fn oversized(error: &Error) -> bool {
    cfg!(windows) && error.raw_os_error() == Some(10040)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_outage_is_retryable_but_configuration_and_permission_errors_are_not() {
        for kind in [
            ErrorKind::WouldBlock,
            ErrorKind::Interrupted,
            ErrorKind::ConnectionRefused,
            ErrorKind::ConnectionReset,
            ErrorKind::ConnectionAborted,
            ErrorKind::NetworkUnreachable,
            ErrorKind::HostUnreachable,
        ] {
            assert!(transient(&Error::from(kind)), "{kind:?}");
        }
        for kind in [
            ErrorKind::PermissionDenied,
            ErrorKind::InvalidInput,
            ErrorKind::AddrNotAvailable,
        ] {
            assert!(!transient(&Error::from(kind)), "{kind:?}");
        }
        if cfg!(windows) {
            for code in [10050, 10051, 10053, 10054, 10061, 10065] {
                assert!(transient(&Error::from_raw_os_error(code)), "{code}");
            }
            assert!(oversized(&Error::from_raw_os_error(10040)));
            assert!(!transient(&Error::from_raw_os_error(10040)));
            assert!(!transient(&Error::from_raw_os_error(10013)));
        }
    }
}
