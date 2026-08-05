//! The one place a local runtime's address is checked.
//!
//! `cortex-ollama` enforced loopback inside its own client. With several
//! backends that check would have been copied several times, and the copy
//! that gets forgotten is the one that ships. Every provider in this crate
//! constructs its address through [`LoopbackUrl`], which cannot be built from
//! a remote host — there is no flag, no environment variable, and no
//! constructor that skips it.
//!
//! The reason is not paranoia about the network. It is that the README and
//! `docs/architecture.md` claim local-first with no cloud egress, and shadow
//! and usage telemetry travel through these calls. A single remote base URL
//! would make that claim false without anything in the code looking wrong.

use std::fmt::{Display, Formatter};

use serde::{Deserialize, Serialize};

/// Hosts that are the local machine and nothing else.
const LOOPBACK_HOSTS: &[&str] = &["localhost", "127.0.0.1", "[::1]", "::1"];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndpointError {
    NotHttp(String),
    NotLoopback(String),
    Malformed(String),
}

impl Display for EndpointError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotHttp(value) => {
                write!(
                    formatter,
                    "local runtime URL must be http or https: {value}"
                )
            }
            Self::NotLoopback(host) => write!(
                formatter,
                "local runtime host must be loopback, got {host}; \
                 Cortex Loom does not call a model over the network"
            ),
            Self::Malformed(value) => write!(formatter, "unusable runtime URL: {value}"),
        }
    }
}

impl std::error::Error for EndpointError {}

/// A base URL that has been proved to point at this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LoopbackUrl(String);

impl LoopbackUrl {
    /// # Errors
    ///
    /// Returns [`EndpointError`] when the URL is not http(s), has no host, or
    /// points anywhere other than loopback.
    pub fn parse(value: &str) -> Result<Self, EndpointError> {
        let trimmed = value.trim().trim_end_matches('/');
        let rest = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .ok_or_else(|| EndpointError::NotHttp(trimmed.to_owned()))?;
        let authority = rest.split('/').next().unwrap_or_default();
        if authority.is_empty() {
            return Err(EndpointError::Malformed(trimmed.to_owned()));
        }
        // Credentials in the authority would let `user@evil.example` read as
        // loopback to a careless split; reject rather than interpret.
        if authority.contains('@') {
            return Err(EndpointError::Malformed(trimmed.to_owned()));
        }
        let host = host_of(authority);
        if !LOOPBACK_HOSTS.contains(&host.as_str()) {
            return Err(EndpointError::NotLoopback(host));
        }
        Ok(Self(trimmed.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Join a path onto the base, keeping exactly one separator.
    #[must_use]
    pub fn join(&self, path: &str) -> String {
        format!("{}/{}", self.0, path.trim_start_matches('/'))
    }
}

impl Display for LoopbackUrl {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.0)
    }
}

/// The host part of an authority, with the port removed and IPv6 brackets
/// kept, because `[::1]:8000` and `127.0.0.1:8000` split differently.
fn host_of(authority: &str) -> String {
    if let Some(end) = authority.find(']') {
        return authority[..=end].to_ascii_lowercase();
    }
    authority
        .split(':')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{EndpointError, LoopbackUrl};

    #[test]
    fn loopback_addresses_in_every_spelling_are_accepted() {
        for value in [
            "http://127.0.0.1:11434",
            "http://localhost:8000/",
            "https://LOCALHOST:443",
            "http://[::1]:8000",
        ] {
            assert!(LoopbackUrl::parse(value).is_ok(), "{value} should parse");
        }
    }

    #[test]
    fn nothing_off_this_machine_can_be_addressed() {
        for value in [
            "http://api.openai.com/v1",
            "http://192.168.1.10:8000",
            "https://127.0.0.1.evil.example",
            "http://10.0.0.1",
        ] {
            assert!(
                matches!(
                    LoopbackUrl::parse(value),
                    Err(EndpointError::NotLoopback(_))
                ),
                "{value} must be refused"
            );
        }
    }

    #[test]
    fn credentials_in_the_authority_are_refused_rather_than_interpreted() {
        // `127.0.0.1@evil.example` reads as loopback to a naive parser.
        assert!(matches!(
            LoopbackUrl::parse("http://127.0.0.1@evil.example/v1"),
            Err(EndpointError::Malformed(_))
        ));
    }

    #[test]
    fn a_scheme_that_is_not_http_is_refused() {
        assert!(matches!(
            LoopbackUrl::parse("file:///etc/passwd"),
            Err(EndpointError::NotHttp(_))
        ));
        assert!(matches!(
            LoopbackUrl::parse("127.0.0.1:11434"),
            Err(EndpointError::NotHttp(_))
        ));
    }

    #[test]
    fn joining_keeps_exactly_one_separator() {
        let base = LoopbackUrl::parse("http://127.0.0.1:8000/").unwrap();
        assert_eq!(
            base.join("/v3/chat/completions"),
            "http://127.0.0.1:8000/v3/chat/completions"
        );
        assert_eq!(
            base.join("v3/embeddings"),
            "http://127.0.0.1:8000/v3/embeddings"
        );
    }
}
