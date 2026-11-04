use std::fmt;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WebDavUrlError {
    InvalidScheme,
    MissingHost,
    UserInfoNotAllowed,
    QueryNotAllowed,
    FragmentNotAllowed,
    ParseError(String),
}

impl fmt::Display for WebDavUrlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidScheme => write!(f, "WebDAV URL 必须使用 https:// 或 http:// 协议"),
            Self::MissingHost => write!(f, "WebDAV URL 缺少主机地址"),
            Self::UserInfoNotAllowed => write!(f, "WebDAV URL 不得包含用户名或密码，请在对应字段中输入"),
            Self::QueryNotAllowed => write!(f, "WebDAV URL 不得包含查询参数 (?)"),
            Self::FragmentNotAllowed => write!(f, "WebDAV URL 不得包含哈希片段 (#)"),
            Self::ParseError(msg) => write!(f, "WebDAV URL 解析失败：{msg}"),
        }
    }
}

impl std::error::Error for WebDavUrlError {}

#[derive(Clone, PartialEq, Eq)]
pub struct WebDavUrl {
    inner: Url,
}

impl WebDavUrl {
    pub fn parse(raw: &str) -> Result<Self, WebDavUrlError> {
        let raw = raw.trim();
        let parsed = Url::parse(raw).map_err(|e| WebDavUrlError::ParseError(e.to_string()))?;

        match parsed.scheme() {
            "https" | "http" => {}
            _ => return Err(WebDavUrlError::InvalidScheme),
        }

        if parsed.host_str().is_none() {
            return Err(WebDavUrlError::MissingHost);
        }

        if !parsed.username().is_empty() || parsed.password().is_some() {
            return Err(WebDavUrlError::UserInfoNotAllowed);
        }

        if parsed.query().is_some() {
            return Err(WebDavUrlError::QueryNotAllowed);
        }

        if parsed.fragment().is_some() {
            return Err(WebDavUrlError::FragmentNotAllowed);
        }

        // Normalize trailing slash for directory root
        let mut normalized = parsed;
        let path = normalized.path();
        if !path.ends_with('/') {
            let mut new_path = path.to_string();
            new_path.push('/');
            normalized.set_path(&new_path);
        }

        Ok(Self { inner: normalized })
    }

    pub fn as_str(&self) -> &str {
        self.inner.as_str()
    }

    pub fn inner(&self) -> &Url {
        &self.inner
    }

    pub fn join(&self, relative: &str) -> Result<Url, WebDavUrlError> {
        let trimmed = relative.trim_start_matches('/');
        self.inner
            .join(trimmed)
            .map_err(|e| WebDavUrlError::ParseError(e.to_string()))
    }
}

impl fmt::Display for WebDavUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.inner.as_str())
    }
}

impl fmt::Debug for WebDavUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("WebDavUrl").field(&self.inner.as_str()).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_and_normalizes_trailing_slash() {
        let url = WebDavUrl::parse("https://dav.example.com/remote/TokenScope").unwrap();
        assert_eq!(url.as_str(), "https://dav.example.com/remote/TokenScope/");
        assert_eq!(
            url.join("devices/dev1.json").unwrap().as_str(),
            "https://dav.example.com/remote/TokenScope/devices/dev1.json"
        );
    }

    #[test]
    fn rejects_userinfo_and_query() {
        assert!(matches!(
            WebDavUrl::parse("https://user:pass@dav.example.com/sync"),
            Err(WebDavUrlError::UserInfoNotAllowed)
        ));
        assert!(matches!(
            WebDavUrl::parse("https://dav.example.com/sync?foo=bar"),
            Err(WebDavUrlError::QueryNotAllowed)
        ));
        assert!(matches!(
            WebDavUrl::parse("ftp://dav.example.com/sync"),
            Err(WebDavUrlError::InvalidScheme)
        ));
    }
}
