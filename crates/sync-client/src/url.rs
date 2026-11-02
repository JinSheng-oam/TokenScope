use std::net::IpAddr;
use url::Url;

use crate::client::HubClientError;

/// Validated Hub origin. Credentials, paths, query strings and fragments are
/// forbidden so callers cannot smuggle secrets or redirect API paths.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HubUrl {
    base: Url,
    origin: String,
}

impl HubUrl {
    pub fn parse(input: &str) -> Result<Self, HubClientError> {
        let value = input.trim();
        if value.is_empty() || value.len() > 2048 {
            return Err(HubClientError::InvalidHubUrl);
        }
        let explicit_userinfo = value
            .split_once("://")
            .map(|(_, rest)| {
                let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
                rest[..authority_end].contains('@')
            })
            .unwrap_or(true);
        let mut parsed = Url::parse(value).map_err(|_| HubClientError::InvalidHubUrl)?;
        if explicit_userinfo
            || !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !matches!(parsed.path(), "" | "/")
        {
            return Err(HubClientError::InvalidHubUrl);
        }
        let host = parsed.host_str().ok_or(HubClientError::InvalidHubUrl)?;
        match parsed.scheme() {
            "https" => {}
            "http" if is_loopback_host(host) => {}
            _ => return Err(HubClientError::InsecureTransport),
        }
        parsed.set_path("");
        let origin = parsed.origin().ascii_serialization();
        Ok(Self {
            base: parsed,
            origin,
        })
    }

    pub fn endpoint(&self, path: &str) -> Result<Url, HubClientError> {
        if !path.starts_with('/') || path.starts_with("//") {
            return Err(HubClientError::InvalidHubUrl);
        }
        self.base
            .join(path)
            .map_err(|_| HubClientError::InvalidHubUrl)
    }

    pub fn origin(&self) -> &str {
        &self.origin
    }
}

fn is_loopback_host(host: &str) -> bool {
    let ip_text = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    host.eq_ignore_ascii_case("localhost")
        || ip_text
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_https_and_loopback_http_only() {
        assert!(HubUrl::parse("https://tokens.example.com").is_ok());
        assert!(HubUrl::parse("http://127.0.0.1:8787").is_ok());
        assert!(HubUrl::parse("http://[::1]:8787").is_ok());
        assert!(HubUrl::parse("http://localhost:8787").is_ok());
        assert!(HubUrl::parse("http://192.168.1.10:8787").is_err());
    }

    #[test]
    fn rejects_userinfo_paths_queries_and_fragments() {
        for input in [
            "https://user:pass@example.com",
            "https://@example.com",
            "https://example.com/prefix",
            "https://example.com/?token=secret",
            "https://example.com/#fragment",
            "file:///tmp/hub",
        ] {
            assert!(HubUrl::parse(input).is_err(), "accepted {input}");
        }
    }

    #[test]
    fn canonicalizes_origin_for_credential_scope() {
        let url = HubUrl::parse("https://EXAMPLE.com:443/").unwrap();
        assert_eq!(url.origin(), "https://example.com");
        assert_eq!(
            url.endpoint("/api/v1/pair").unwrap().as_str(),
            "https://example.com/api/v1/pair"
        );
    }
}
