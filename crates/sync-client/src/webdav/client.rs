use std::{fmt, time::Duration};

use reqwest::{blocking::Client, Method, StatusCode};
use serde::{de::DeserializeOwned, Serialize};

use super::{
    credentials::WebDavCredentials,
    url::{WebDavUrl, WebDavUrlError},
};

#[derive(Debug)]
pub enum WebDavClientError {
    Unauthorized,
    NotFound,
    Transport(String),
    ServerRejected(u16, String),
    Serialization(String),
    InvalidUrl(WebDavUrlError),
}

impl fmt::Display for WebDavClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unauthorized => write!(f, "WebDAV 认证失败：用户名或密码错误"),
            Self::NotFound => write!(f, "WebDAV 路径不存在"),
            Self::Transport(msg) => write!(f, "WebDAV 网络传输错误：{msg}"),
            Self::ServerRejected(status, msg) => write!(f, "WebDAV 服务端返回错误 ({status}): {msg}"),
            Self::Serialization(msg) => write!(f, "WebDAV 数据序列化错误：{msg}"),
            Self::InvalidUrl(err) => write!(f, "WebDAV URL 错误：{err}"),
        }
    }
}

impl std::error::Error for WebDavClientError {}

impl From<WebDavUrlError> for WebDavClientError {
    fn from(e: WebDavUrlError) -> Self {
        Self::InvalidUrl(e)
    }
}

pub struct WebDavClient {
    http: Client,
    base_url: WebDavUrl,
    credentials: WebDavCredentials,
}

impl WebDavClient {
    pub fn new(base_url: WebDavUrl, credentials: WebDavCredentials) -> Result<Self, WebDavClientError> {
        let http = Client::builder()
            .timeout(Duration::from_secs(20))
            .connect_timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        Ok(Self {
            http,
            base_url,
            credentials,
        })
    }

    pub fn base_url(&self) -> &WebDavUrl {
        &self.base_url
    }

    fn auth_request(&self, method: Method, url: &url::Url) -> reqwest::blocking::RequestBuilder {
        self.http
            .request(method, url.clone())
            .basic_auth(&self.credentials.username, Some(&*self.credentials.password))
    }

    /// Test if WebDAV server is reachable and credentials are valid.
    pub fn test_connection(&self) -> Result<(), WebDavClientError> {
        let root_url = self.base_url.inner().clone();
        let method = Method::from_bytes(b"PROPFIND").unwrap_or(Method::GET);
        let resp = self
            .auth_request(method, &root_url)
            .header("Depth", "0")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8" ?><D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/></D:prop></D:propfind>"#,
            )
            .send()
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        match resp.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(WebDavClientError::Unauthorized),
            s if s.is_success() || s == StatusCode::MULTI_STATUS || s == StatusCode::NOT_FOUND => Ok(()),
            s => {
                // Some servers don't support PROPFIND on root or return Method Not Allowed; fallback to OPTIONS / GET
                let fallback = self
                    .auth_request(Method::OPTIONS, &root_url)
                    .send()
                    .map_err(|e| WebDavClientError::Transport(e.to_string()))?;
                match fallback.status() {
                    StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(WebDavClientError::Unauthorized),
                    f if f.is_success() || f == StatusCode::NOT_FOUND => Ok(()),
                    _ => Err(WebDavClientError::ServerRejected(
                        s.as_u16(),
                        format!("HTTP {}", s.as_u16()),
                    )),
                }
            }
        }
    }

    /// Ensure collection (directory) exists recursively via MKCOL.
    pub fn ensure_collection(&self, relative_path: &str) -> Result<(), WebDavClientError> {
        let trimmed = relative_path.trim_matches('/');
        if trimmed.is_empty() {
            return Ok(());
        }

        let segments: Vec<&str> = trimmed.split('/').filter(|s| !s.is_empty()).collect();
        let mut current = String::new();
        let mkcol_method = Method::from_bytes(b"MKCOL").map_err(|_| {
            WebDavClientError::Transport("failed to construct MKCOL method".into())
        })?;

        for segment in segments {
            if !current.is_empty() {
                current.push('/');
            }
            current.push_str(segment);

            let col_url = self.base_url.join(&format!("{current}/"))?;
            let resp = self
                .auth_request(mkcol_method.clone(), &col_url)
                .send()
                .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

            match resp.status() {
                // 201 Created: newly created
                // 405 Method Not Allowed: RFC 4918 standard response when collection already exists
                // 200 OK / 204 No Content: returned by some WebDAV implementations when exists
                StatusCode::CREATED
                | StatusCode::METHOD_NOT_ALLOWED
                | StatusCode::OK
                | StatusCode::NO_CONTENT => {}
                StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                    return Err(WebDavClientError::Unauthorized);
                }
                status => {
                    return Err(WebDavClientError::ServerRejected(
                        status.as_u16(),
                        format!("MKCOL {current} returned {status}"),
                    ));
                }
            }
        }
        Ok(())
    }

    /// Upload a JSON document to WebDAV via PUT.
    pub fn put_json<T: Serialize>(&self, relative_path: &str, data: &T) -> Result<(), WebDavClientError> {
        let file_url = self.base_url.join(relative_path)?;
        let body = serde_json::to_vec(data)
            .map_err(|e| WebDavClientError::Serialization(e.to_string()))?;

        let resp = self
            .auth_request(Method::PUT, &file_url)
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        match resp.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(WebDavClientError::Unauthorized),
            s if s.is_success() => Ok(()),
            s => Err(WebDavClientError::ServerRejected(
                s.as_u16(),
                format!("PUT {relative_path} returned {s}"),
            )),
        }
    }

    /// Download and parse a JSON document from WebDAV via GET. Returns None if 404.
    pub fn get_json<T: DeserializeOwned>(&self, relative_path: &str) -> Result<Option<T>, WebDavClientError> {
        let file_url = self.base_url.join(relative_path)?;
        let resp = self
            .auth_request(Method::GET, &file_url)
            .send()
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        match resp.status() {
            StatusCode::NOT_FOUND => Ok(None),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(WebDavClientError::Unauthorized),
            s if s.is_success() => {
                let bytes = resp
                    .bytes()
                    .map_err(|e| WebDavClientError::Transport(e.to_string()))?;
                let val: T = serde_json::from_slice(&bytes)
                    .map_err(|e| WebDavClientError::Serialization(e.to_string()))?;
                Ok(Some(val))
            }
            s => Err(WebDavClientError::ServerRejected(
                s.as_u16(),
                format!("GET {relative_path} returned {s}"),
            )),
        }
    }

    /// List JSON filenames in `devices/` directory via PROPFIND Depth: 1.
    pub fn list_device_files(&self) -> Result<Vec<String>, WebDavClientError> {
        let devices_url = self.base_url.join("devices/")?;
        let propfind_method = Method::from_bytes(b"PROPFIND").map_err(|_| {
            WebDavClientError::Transport("failed to construct PROPFIND method".into())
        })?;

        let resp = self
            .auth_request(propfind_method, &devices_url)
            .header("Depth", "1")
            .header("Content-Type", "application/xml; charset=utf-8")
            .body(
                r#"<?xml version="1.0" encoding="utf-8" ?><D:propfind xmlns:D="DAV:"><D:prop><D:resourcetype/></D:prop></D:propfind>"#,
            )
            .send()
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        match resp.status() {
            StatusCode::NOT_FOUND => Ok(Vec::new()),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(WebDavClientError::Unauthorized),
            s if s == StatusCode::MULTI_STATUS || s.is_success() => {
                let xml = resp.text().map_err(|e| WebDavClientError::Transport(e.to_string()))?;
                Ok(parse_propfind_filenames(&xml))
            }
            s => Err(WebDavClientError::ServerRejected(
                s.as_u16(),
                format!("PROPFIND devices/ returned {s}"),
            )),
        }
    }

    /// Delete a file or collection. Returns true if deleted, false if not found.
    pub fn delete_file(&self, relative_path: &str) -> Result<bool, WebDavClientError> {
        let file_url = self.base_url.join(relative_path)?;
        let resp = self
            .auth_request(Method::DELETE, &file_url)
            .send()
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;

        match resp.status() {
            StatusCode::NOT_FOUND => Ok(false),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => Err(WebDavClientError::Unauthorized),
            s if s.is_success() => Ok(true),
            s => Err(WebDavClientError::ServerRejected(
                s.as_u16(),
                format!("DELETE {relative_path} returned {s}"),
            )),
        }
    }
}

/// Parses XML response from WebDAV PROPFIND and extracts non-collection .json filenames.
pub fn parse_propfind_filenames(xml: &str) -> Vec<String> {
    let mut results = Vec::new();

    // Match each <response> or <D:response> block
    let mut cursor = 0;
    while let Some(start_idx) = find_tag_open(xml, "response", cursor) {
        let end_idx = match find_tag_close(xml, "response", start_idx) {
            Some(i) => i,
            None => break,
        };
        let response_block = &xml[start_idx..end_idx];
        cursor = end_idx;

        // Check if this response represents a collection
        let is_collection = if let Some(res_type) = extract_tag_content(response_block, "resourcetype") {
            contains_tag(&res_type, "collection")
        } else {
            false
        };

        if is_collection {
            continue;
        }

        // Extract href
        if let Some(raw_href) = extract_tag_content(response_block, "href") {
            let trimmed_href = raw_href.trim();
            if trimmed_href.ends_with('/') {
                continue;
            }
            // Extract the filename portion after the last slash
            let filename = trimmed_href.rsplit('/').next().unwrap_or(trimmed_href);
            // URL-decode if needed
            let decoded = urlencoding_decode(filename);
            if decoded.ends_with(".json") && !decoded.is_empty() && !results.contains(&decoded) {
                results.push(decoded);
            }
        }
    }

    results
}

fn find_tag_open(haystack: &str, tag_name: &str, from: usize) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'<' {
            // Check if tag name matches (ignoring namespace prefix like <D:tag> or <tag>)
            let mut j = i + 1;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b':' || bytes[j] == b'_' || bytes[j] == b'-') {
                j += 1;
            }
            let candidate = &haystack[i + 1..j];
            let name = candidate.rsplit(':').next().unwrap_or(candidate);
            if name.eq_ignore_ascii_case(tag_name) {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

fn find_tag_close(haystack: &str, tag_name: &str, from: usize) -> Option<usize> {
    let bytes = haystack.as_bytes();
    let mut i = from;
    while i < bytes.len() {
        if bytes[i] == b'<' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
            let mut j = i + 2;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b':' || bytes[j] == b'_' || bytes[j] == b'-') {
                j += 1;
            }
            let candidate = &haystack[i + 2..j];
            let name = candidate.rsplit(':').next().unwrap_or(candidate);
            if name.eq_ignore_ascii_case(tag_name) {
                // Find closing '>'
                while j < bytes.len() && bytes[j] != b'>' {
                    j += 1;
                }
                if j < bytes.len() {
                    return Some(j + 1);
                }
            }
        }
        i += 1;
    }
    None
}

fn extract_tag_content(block: &str, tag_name: &str) -> Option<String> {
    let start = find_tag_open(block, tag_name, 0)?;
    let bytes = block.as_bytes();
    let mut content_start = start;
    while content_start < bytes.len() && bytes[content_start] != b'>' {
        content_start += 1;
    }
    if content_start >= bytes.len() {
        return None;
    }
    content_start += 1;

    let end = find_tag_close(block, tag_name, content_start)?;
    // The closing tag starts with "</"
    let mut content_end = end;
    while content_end > content_start && bytes[content_end - 1] != b'<' {
        content_end -= 1;
    }
    if content_end > content_start {
        content_end -= 1;
    }

    Some(block[content_start..content_end].to_string())
}

fn contains_tag(xml_snippet: &str, tag_name: &str) -> bool {
    find_tag_open(xml_snippet, tag_name, 0).is_some()
}

fn urlencoding_decode(s: &str) -> String {
    let mut out = Vec::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|_| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_propfind_multistatus_with_namespaces() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<D:multistatus xmlns:D="DAV:">
  <D:response>
    <D:href>/dav/TokenScope/devices/</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype><D:collection/></D:resourcetype>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/TokenScope/devices/node-a.json</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
  <D:response>
    <D:href>/dav/TokenScope/devices/node%20b.json</D:href>
    <D:propstat>
      <D:prop>
        <D:resourcetype/>
      </D:prop>
      <D:status>HTTP/1.1 200 OK</D:status>
    </D:propstat>
  </D:response>
</D:multistatus>"#;

        let files = parse_propfind_filenames(xml);
        assert_eq!(files, vec!["node-a.json", "node b.json"]);
    }

    #[test]
    fn parses_propfind_lowercase_d_and_no_namespace() {
        let xml = r#"<multistatus xmlns="DAV:">
  <response>
    <href>/dav/TokenScope/devices/</href>
    <propstat><prop><resourcetype><collection/></resourcetype></prop></propstat>
  </response>
  <response>
    <href>/dav/TokenScope/devices/pc1.json</href>
    <propstat><prop><resourcetype></resourcetype></prop></propstat>
  </response>
</multistatus>"#;

        let files = parse_propfind_filenames(xml);
        assert_eq!(files, vec!["pc1.json"]);
    }
}
