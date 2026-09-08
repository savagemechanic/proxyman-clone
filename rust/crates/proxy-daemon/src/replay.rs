use std::fmt;

use proxy_core::{
    CapturedTransaction, HeaderField,
    http::{Destination, ParsedRequestHead},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedReplayRequest {
    pub source_transaction_id: u64,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub parsed: ParsedRequestHead,
    pub body: Vec<u8>,
}

impl PreparedReplayRequest {
    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = format!(
            "{} {} HTTP/1.1\r\n",
            self.parsed.method, self.parsed.destination.origin_form_target
        )
        .into_bytes();

        for (name, value) in &self.parsed.headers {
            bytes.extend_from_slice(name.as_bytes());
            bytes.extend_from_slice(b": ");
            bytes.extend_from_slice(value.as_bytes());
            bytes.extend_from_slice(b"\r\n");
        }
        bytes.extend_from_slice(b"Connection: close\r\n\r\n");
        bytes.extend_from_slice(&self.body);
        bytes
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReplayPreparationError {
    UnsupportedScheme(String),
    TunnelTransaction,
    InvalidMethod,
    InvalidTarget,
    InvalidAuthority,
    UnsafeHeader(String),
    UnsupportedTransferEncoding,
    TruncatedBody,
    UnavailableBody,
    BodySizeMismatch,
}

impl fmt::Display for ReplayPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme(scheme) => write!(f, "unsupported replay scheme: {scheme}"),
            Self::TunnelTransaction => {
                f.write_str("CONNECT tunnel transactions cannot be replayed")
            }
            Self::InvalidMethod => f.write_str("captured request method is not safe to serialize"),
            Self::InvalidTarget => f.write_str("captured request target is not valid origin-form"),
            Self::InvalidAuthority => f.write_str("captured request has an invalid Host authority"),
            Self::UnsafeHeader(name) => {
                write!(f, "captured request contains an unsafe header: {name}")
            }
            Self::UnsupportedTransferEncoding => {
                f.write_str("requests using Transfer-Encoding cannot be safely replayed yet")
            }
            Self::TruncatedBody => f.write_str("captured request body is truncated"),
            Self::UnavailableBody => f.write_str("full textual request body is unavailable"),
            Self::BodySizeMismatch => {
                f.write_str("captured request body size does not match its preview")
            }
        }
    }
}

impl std::error::Error for ReplayPreparationError {}

pub fn prepare(
    transaction: &CapturedTransaction,
) -> Result<PreparedReplayRequest, ReplayPreparationError> {
    if transaction.scheme == "tunnel" {
        return Err(ReplayPreparationError::TunnelTransaction);
    }

    let default_port = match transaction.scheme.as_str() {
        "http" => 80,
        "https" => 443,
        other => return Err(ReplayPreparationError::UnsupportedScheme(other.to_owned())),
    };

    if !valid_token(&transaction.method) {
        return Err(ReplayPreparationError::InvalidMethod);
    }
    if !valid_target(&transaction.target) {
        return Err(ReplayPreparationError::InvalidTarget);
    }
    if transaction
        .request_headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("transfer-encoding"))
    {
        return Err(ReplayPreparationError::UnsupportedTransferEncoding);
    }

    let authority = transaction
        .request_headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("host"))
        .map(|header| header.value.as_str())
        .unwrap_or(transaction.host.as_str());
    let (host, port) = parse_authority(authority, default_port)?;
    let body = replay_body(transaction)?;
    let had_content_length = transaction
        .request_headers
        .iter()
        .any(|header| header.name.eq_ignore_ascii_case("content-length"));

    let mut headers = Vec::new();
    for HeaderField { name, value } in &transaction.request_headers {
        if is_reconstructed_header(name) {
            continue;
        }
        if !valid_token(name) || !valid_header_value(value) {
            return Err(ReplayPreparationError::UnsafeHeader(name.clone()));
        }
        headers.push((name.clone(), value.clone()));
    }

    headers.push(("Host".into(), format_authority(&host, port, default_port)));
    if !body.is_empty() || had_content_length {
        headers.push(("Content-Length".into(), body.len().to_string()));
    }

    let parsed = ParsedRequestHead {
        method: transaction.method.clone(),
        target: transaction.target.clone(),
        version: "HTTP/1.1".into(),
        headers,
        destination: Destination {
            host: host.clone(),
            port,
            origin_form_target: transaction.target.clone(),
            is_connect: false,
        },
    };

    Ok(PreparedReplayRequest {
        source_transaction_id: transaction.id,
        scheme: transaction.scheme.clone(),
        host,
        port,
        parsed,
        body,
    })
}

fn replay_body(transaction: &CapturedTransaction) -> Result<Vec<u8>, ReplayPreparationError> {
    if transaction.request_body_bytes == 0 {
        return Ok(Vec::new());
    }

    let preview = transaction
        .request_body_preview
        .as_ref()
        .ok_or(ReplayPreparationError::UnavailableBody)?;
    if preview.truncated
        || preview.captured_bytes != preview.total_bytes
        || preview.total_bytes != transaction.request_body_bytes
    {
        return Err(ReplayPreparationError::TruncatedBody);
    }
    let text = preview
        .text
        .as_ref()
        .ok_or(ReplayPreparationError::UnavailableBody)?;
    let body = text.as_bytes().to_vec();
    if body.len() as u64 != transaction.request_body_bytes {
        return Err(ReplayPreparationError::BodySizeMismatch);
    }
    Ok(body)
}

fn parse_authority(
    authority: &str,
    default_port: u16,
) -> Result<(String, u16), ReplayPreparationError> {
    let authority = authority.trim();
    if authority.is_empty()
        || authority
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return Err(ReplayPreparationError::InvalidAuthority);
    }

    if authority.starts_with('[') {
        let close = authority
            .find(']')
            .ok_or(ReplayPreparationError::InvalidAuthority)?;
        let host = authority[1..close].to_owned();
        if host.is_empty() {
            return Err(ReplayPreparationError::InvalidAuthority);
        }
        let suffix = &authority[close + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else {
            suffix
                .strip_prefix(':')
                .ok_or(ReplayPreparationError::InvalidAuthority)?
                .parse::<u16>()
                .map_err(|_| ReplayPreparationError::InvalidAuthority)?
        };
        return Ok((host, port));
    }

    if authority.matches(':').count() > 1 {
        return Ok((authority.to_owned(), default_port));
    }

    match authority.rsplit_once(':') {
        Some((host, port_text)) => {
            if host.is_empty() {
                return Err(ReplayPreparationError::InvalidAuthority);
            }
            let port = port_text
                .parse::<u16>()
                .map_err(|_| ReplayPreparationError::InvalidAuthority)?;
            Ok((host.to_owned(), port))
        }
        None => Ok((authority.to_owned(), default_port)),
    }
}

fn format_authority(host: &str, port: u16, default_port: u16) -> String {
    let host = if host.contains(':') && !host.starts_with('[') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    if port == default_port {
        host
    } else {
        format!("{host}:{port}")
    }
}

fn is_reconstructed_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "host"
            | "content-length"
            | "connection"
            | "proxy-connection"
            | "keep-alive"
            | "transfer-encoding"
            | "te"
            | "trailer"
            | "upgrade"
    )
}

fn valid_target(value: &str) -> bool {
    value.starts_with('/')
        && !value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
}

fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric()
                || matches!(
                    byte,
                    b'!' | b'#'
                        | b'$'
                        | b'%'
                        | b'&'
                        | b'\''
                        | b'*'
                        | b'+'
                        | b'-'
                        | b'.'
                        | b'^'
                        | b'_'
                        | b'`'
                        | b'|'
                        | b'~'
                )
        })
}

fn valid_header_value(value: &str) -> bool {
    !value
        .bytes()
        .any(|byte| byte == b'\r' || byte == b'\n' || byte == 0)
}

#[cfg(test)]
mod tests {
    use proxy_core::{BodyPreview, TransactionState};

    use super::*;

    fn transaction() -> CapturedTransaction {
        CapturedTransaction {
            id: 7,
            started_at_unix_ms: 1,
            scheme: "https".into(),
            host: "example.com".into(),
            method: "GET".into(),
            target: "/v1/users?limit=2".into(),
            request_headers: vec![
                HeaderField {
                    name: "Host".into(),
                    value: "example.com".into(),
                },
                HeaderField {
                    name: "Authorization".into(),
                    value: "Bearer secret".into(),
                },
                HeaderField {
                    name: "Connection".into(),
                    value: "keep-alive".into(),
                },
            ],
            request_body_bytes: 0,
            request_body_preview: None,
            status_code: Some(200),
            response_headers: Vec::new(),
            response_body_bytes: 0,
            response_body_preview: None,
            state: TransactionState::Complete,
        }
    }

    #[test]
    fn prepares_bodyless_https_request() {
        let prepared = prepare(&transaction()).unwrap();
        assert_eq!(prepared.host, "example.com");
        assert_eq!(prepared.port, 443);
        assert_eq!(
            prepared.parsed.destination.origin_form_target,
            "/v1/users?limit=2"
        );
        assert!(prepared.parsed.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value == "Bearer secret"
        }));
        assert!(
            prepared
                .parsed
                .headers
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("connection"))
        );
        let raw = String::from_utf8(prepared.serialize()).unwrap();
        assert!(raw.starts_with("GET /v1/users?limit=2 HTTP/1.1\r\n"));
        assert!(raw.contains("Host: example.com\r\n"));
        assert!(raw.ends_with("Connection: close\r\n\r\n"));
    }

    #[test]
    fn derives_non_default_port_from_host_header() {
        let mut transaction = transaction();
        transaction.scheme = "http".into();
        transaction.request_headers[0].value = "localhost:8081".into();
        let prepared = prepare(&transaction).unwrap();
        assert_eq!(prepared.host, "localhost");
        assert_eq!(prepared.port, 8081);
        assert!(
            prepared
                .parsed
                .headers
                .iter()
                .any(|(name, value)| name == "Host" && value == "localhost:8081")
        );
    }

    #[test]
    fn replays_full_textual_body_and_regenerates_content_length() {
        let mut transaction = transaction();
        transaction.method = "POST".into();
        transaction.request_headers.push(HeaderField {
            name: "Content-Length".into(),
            value: "999".into(),
        });
        transaction.request_body_bytes = 7;
        transaction.request_body_preview = Some(BodyPreview {
            content_type: Some("application/json".into()),
            text: Some("{\"x\":1}".into()),
            captured_bytes: 7,
            total_bytes: 7,
            truncated: false,
        });

        let prepared = prepare(&transaction).unwrap();
        assert_eq!(prepared.body, b"{\"x\":1}");
        assert!(
            prepared.parsed.headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("content-length") && value == "7"
            })
        );
    }

    #[test]
    fn rejects_truncated_or_binary_body() {
        let mut transaction = transaction();
        transaction.method = "POST".into();
        transaction.request_body_bytes = 10;
        transaction.request_body_preview = Some(BodyPreview {
            content_type: Some("text/plain".into()),
            text: Some("short".into()),
            captured_bytes: 5,
            total_bytes: 10,
            truncated: true,
        });
        assert_eq!(
            prepare(&transaction),
            Err(ReplayPreparationError::TruncatedBody)
        );

        transaction.request_body_bytes = 4;
        transaction.request_body_preview = Some(BodyPreview {
            content_type: Some("application/octet-stream".into()),
            text: None,
            captured_bytes: 4,
            total_bytes: 4,
            truncated: false,
        });
        assert_eq!(
            prepare(&transaction),
            Err(ReplayPreparationError::UnavailableBody)
        );
    }

    #[test]
    fn rejects_transfer_encoding_and_unsafe_headers() {
        let mut transaction = transaction();
        transaction.request_headers.push(HeaderField {
            name: "Transfer-Encoding".into(),
            value: "chunked".into(),
        });
        assert_eq!(
            prepare(&transaction),
            Err(ReplayPreparationError::UnsupportedTransferEncoding)
        );

        transaction.request_headers.pop();
        transaction.request_headers.push(HeaderField {
            name: "X-Test".into(),
            value: "ok\r\nInjected: yes".into(),
        });
        assert_eq!(
            prepare(&transaction),
            Err(ReplayPreparationError::UnsafeHeader("X-Test".into()))
        );
    }
}
