use crate::DomainError;
use sha2::{Digest, Sha256};

const MAX_IDENTITY_BYTES: usize = 16 * 1024;
const RECORD_DOMAIN: &[u8] = b"tokenscope/record-id/v1\0";
const ACCOUNT_DOMAIN: &[u8] = b"tokenscope/account-id/v1\0";
const PROJECT_DOMAIN: &[u8] = b"tokenscope/project-id/v1\0";
const SESSION_DOMAIN: &[u8] = b"tokenscope/session-id/v1\0";

pub(crate) fn hex_sha256(bytes: &[u8]) -> String {
    let mut hash = Sha256::new();
    hash.update(bytes);
    to_lower_hex(&hash.finalize())
}

fn to_lower_hex(data: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(data.len() * 2);
    for &b in data {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

pub(crate) fn valid_lower_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|x| x.is_ascii_hexdigit() && !x.is_ascii_uppercase())
}

pub(crate) fn valid_tool_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    !bytes.is_empty()
        && bytes.len() <= 64
        && bytes[0].is_ascii_lowercase()
        && bytes.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn update_part(hasher: &mut Sha256, bytes: &[u8], name: &'static str) -> Result<(), DomainError> {
    if bytes.is_empty() {
        return Err(DomainError::MissingField(name));
    }
    if bytes.len() > MAX_IDENTITY_BYTES {
        return Err(DomainError::TooLong(name));
    }
    // Length prefix avoids ambiguities between source and native identifiers.
    let len = u32::try_from(bytes.len()).map_err(|_| DomainError::TooLong(name))?;
    hasher.update(len.to_be_bytes());
    hasher.update(bytes);
    Ok(())
}

fn opaque_id(
    domain: &[u8],
    prefix: &str,
    tool_id: &str,
    parts: &[(&'static str, &[u8])],
) -> Result<String, DomainError> {
    if !valid_tool_id(tool_id) {
        return Err(DomainError::InvalidField("tool_id"));
    }
    let mut h = Sha256::new();
    h.update(domain);
    update_part(&mut h, tool_id.as_bytes(), "tool_id")?;
    for (name, bytes) in parts {
        update_part(&mut h, bytes, name)?;
    }
    Ok(format!("{}{}", prefix, to_lower_hex(&h.finalize())))
}

/// Stable per-tool record identity. Source/native byte identities MUST be stable
/// for the logical record, not an arbitrary display path or changing content.
/// The device is a separate composite-key partition and is not hashed here.
pub fn stable_record_id(
    tool_id: &str,
    canonical_source_identity: &[u8],
    native_record_identity: &[u8],
) -> Result<String, DomainError> {
    opaque_id(RECORD_DOMAIN, "", tool_id, &[
        ("source_identity", canonical_source_identity),
        ("native_record_identity", native_record_identity),
    ])
}

/// Opaque hashed account identifier. Never persist raw email/cookie/API keys.
pub fn account_key(tool_id: &str, stable_native_account_id: &[u8]) -> Result<String, DomainError> {
    opaque_id(ACCOUNT_DOMAIN, "acct_", tool_id, &[("account_identity", stable_native_account_id)])
}

/// Opaque hashed project identifier. Raw absolute paths never leave the adapter.
pub fn project_key(tool_id: &str, canonical_project_identity: &[u8]) -> Result<String, DomainError> {
    opaque_id(PROJECT_DOMAIN, "prj_", tool_id, &[("project_identity", canonical_project_identity)])
}

/// Opaque hashed session identifier; a session can contain private metadata.
pub fn session_key(tool_id: &str, stable_native_session_id: &[u8]) -> Result<String, DomainError> {
    opaque_id(SESSION_DOMAIN, "sess_", tool_id, &[("session_identity", stable_native_session_id)])
}
