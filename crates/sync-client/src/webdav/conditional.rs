use super::{WebDavClient, WebDavClientError};
use reqwest::{header, Method, StatusCode};
use std::io::Read;

impl WebDavClient {
    pub fn read_versioned_document(
        &self,
        path: &str,
        limit: usize,
    ) -> Result<Option<(String, Option<String>)>, WebDavClientError> {
        let response = self
            .auth_request(Method::GET, &self.base_url().join(path)?)
            .send()
            .map_err(transport)?;
        match response.status() {
            StatusCode::NOT_FOUND => return Ok(None),
            StatusCode::OK => {}
            status => return Err(rejected(status)),
        }
        let tag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| {
                v.len() >= 2
                    && v.starts_with('"')
                    && v.ends_with('"')
                    && !v[1..v.len() - 1].contains('"')
            })
            .map(str::to_owned);
        let mut bytes = Vec::new();
        response
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| WebDavClientError::Transport(e.to_string()))?;
        if bytes.len() > limit {
            return Err(WebDavClientError::Serialization("文档超过大小限制".into()));
        }
        let body = String::from_utf8(bytes)
            .map_err(|e| WebDavClientError::Serialization(e.to_string()))?;
        Ok(Some((body, tag)))
    }

    pub fn write_versioned_document(
        &self,
        path: &str,
        body: String,
        tag: Option<&str>,
    ) -> Result<(), WebDavClientError> {
        let request = self
            .auth_request(Method::PUT, &self.base_url().join(path)?)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
        let response = match tag {
            Some(tag) => request.header(header::IF_MATCH, tag),
            None => request.header(header::IF_NONE_MATCH, "*"),
        }
        .send()
        .map_err(transport)?;
        if response.status().is_success() {
            Ok(())
        } else {
            Err(rejected(response.status()))
        }
    }
}

fn transport(error: reqwest::Error) -> WebDavClientError {
    WebDavClientError::Transport(error.to_string())
}

fn rejected(status: StatusCode) -> WebDavClientError {
    WebDavClientError::ServerRejected(
        status.as_u16(),
        if status == StatusCode::PRECONDITION_FAILED {
            "远端版本已变化，请重新同步".into()
        } else {
            "统一指令请求失败".into()
        },
    )
}
