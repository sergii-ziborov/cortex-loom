//! Loopback-only bind unless the operator opts into remote.

use std::net::SocketAddr;

/// Refuse a non-loopback address unless `allow_remote` is set.
pub fn check_bind(address: SocketAddr, allow_remote: bool) -> Result<(), String> {
    if address.ip().is_loopback() || allow_remote {
        return Ok(());
    }
    Err(format!(
        "refusing to bind {address}: not loopback. Pass --allow-remote and put TLS \
         plus authentication in front of the listener"
    ))
}

#[cfg(test)]
mod tests {
    use super::check_bind;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};

    #[test]
    fn loopback_is_always_allowed() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 8080);
        assert!(check_bind(addr, false).is_ok());
    }

    #[test]
    fn unspecified_requires_allow_remote() {
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), 8080);
        assert!(check_bind(addr, false).is_err());
        assert!(check_bind(addr, true).is_ok());
    }
}
