use std::fmt;

use proxy_core::{
    ComposedRequest,
    http::{Destination, ParsedRequestHead},
};

const MAX_METHOD_BYTES: usize = 32;
const MAX_HOST_BYTES: usize = 1_024;
const MAX_TARGET_BYTES: usize = 8 * 1_024;
const MAX_HEADERS: usize = 256;
const MAX_HEADER_NAME_BYTES: usize = 128;
const MAX_HEADER_VALUE_BYTES: usize = 16 * 1_024;
const MAX_TOTAL_HEADER_BYTES: usize = 64 * 1_024;
pub const MAX_COMPOSED_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedComposedRequest {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub parsed: ParsedRequestHead,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ComposerPreparationError {
    UnsupportedScheme,
    InvalidHost,
    InvalidPort,
    InvalidMethod,
    InvalidTarget,
    TooManyHeaders,
    HeadersTooLarge,
    InvalidHeader(String),
    EngineOwnedHeader(String),
    BodyTooLarge,
}

impl fmt::Display for ComposerPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedScheme => f.write_str("scheme must be http or https"),
            Self::InvalidHost => f.write_str("host is invalid"),
            Self::InvalidPort => f.write_str("port must be between 1 and 65535"),
            Self::InvalidMethod => f.write_str("method is not a valid HTTP token"),
            Self::InvalidTarget => {
                f.write_str("target must be a bounded origin-form path beginning with /")
            }
            Self::TooManyHeaders => write!(f, "too many headers; maximum is {MAX_HEADERS}"),
            Self::HeadersTooLarge => write!(
                f,
                "headers are too large; maximum combined size is {MAX_TOTAL_HEADER_BYTES} bytes"
            ),
            Self::InvalidHeader(name) => write!(f, "invalid header: {name}"),
            Self::EngineOwnedHeader(name) => write!(
                f,
                "header {name} is managed by the engine and cannot be supplied explicitly"
            ),
            Self::BodyTooLarge => write!(
                f,
                "request body is too large; maximum is {MAX_COMPOSED_BODY_BYTES} bytes"
            ),
        }
    }
}

impl std::error::Error for ComposerPreparationError {}

pub fn prepare(request: &ComposedRequest) -> Result<PreparedComposedRequest, ComposerPreparationError> {
    let scheme = request.scheme.trim().to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" => 80,
        "https" => 443,
        _ => return Err(ComposerPreparationError::UnsupportedScheme),
    };

    let host = normalize_host(&request.host)?;
    let port = request.port.unwrap_or(default_port);
    if port == 0 {
        return Err(ComposerPreparationError::InvalidPort);
    }

    if request.method.len() > MAX_METHOD_BYTES || !valid_token(&request.method) {
        return Err(ComposerPreparationError::InvalidMethod);
    }
    if request.target.len() > MAX_TARGET_BYTES || !valid_target(&request.target) {
        return Err(ComposerPreparationError::InvalidTarget);
    }
    if request.headers.len() > MAX_HEADERS {
        return Err(ComposerPreparationError::TooManyHeaders);
    }

    let body = request.body.as_deref().unwrap_or_default().as_bytes().to_vec();
    if body.len() > MAX_COMPOSED_BODY_BYTES {
        return Err(ComposerPreparationError::BodyTooLarge);
    }

    let mut headers = Vec::with_capacity(request.headers.len() + 2);
    let mut header_bytes = 0usize;
    for header in &request.headers {
        if is_engine_owned_header(&header.name) {
            return Err(ComposerPreparationError::EngineOwnedHeader(
                header.name.clone(),
            ));
        }
        if header.name.len() > MAX_HEADER_NAME_BYTES
            || header.value.len() > MAX_HEADER_VALUE_BYTES
            || !valid_token(&header.name)
            || !valid_header_value(&header.value)
        {
            return Err(ComposerPreparationError::InvalidHeader(
                header.name.clone(),
            ));
        }
        header_bytes = header_bytes
            .saturating_add(header.name.len())
            .saturating_add(header.value.len())
            .saturating_add(4);
        if header_bytes > MAX_TOTAL_HEADER_BYTES {
            return Err(ComposerPreparationError::HeadersTooLarge);
        }
        headers.push((header.name.clone(), header.value.clone()));
    }

    headers.push((
        "Host".into(),
        format_authority(&host, port, default_port),
    ));
    if request.body.is_some() {
        headers.push(("Content-Length".into(), body.len().to_string()));
    }

    let parsed = ParsedRequestHead {
        method: request.method.clone(),
        target: request.target.clone(),
        version: "HTTP/1.1".into(),
        headers,
        destination: Destination {
            host: host.clone(),
            port,
            origin_form_target: request.target.clone(),
            is_connect: false,
        },
    };

    Ok(PreparedComposedRequest {
        scheme,
        host,
        port,
        parsed,
        body,
    })
}

fn normalize_host(value: &str) -> Result<String, ComposerPreparationError> {
    let value = value.trim();
    if value.is_empty()
        || value.len() > MAX_HOST_BYTES
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ' || byte == b'/' || byte == b'@')
    {
        return Err(ComposerPreparationError::InvalidHost);
    }

    if value.starts_with('[') {
        if !value.ends_with(']') || value.len() <= 2 {
            return Err(ComposerPreparationError::InvalidHost);
        }
        return Ok(value[1..value.len() - 1].to_owned());
    }
    if value.ends_with(']') {
        return Err(ComposerPreparationError::InvalidHost);
    }

    Ok(value.to_owned())
}

fn format_authority(host: &str, port: u16, default_port: u16) -> String {
    let host = if host.contains(':') {
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

fn is_engine_owned_header(name: &str) -> bool {
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
    use proxy_core::HeaderField;

    use super::*;

    fn request() -> ComposedRequest {
        ComposedRequest {
            scheme: "https".into(),
            host: "example.com".into(),
            port: None,
            method: "POST".into(),
            target: "/v1/test?x=1".into(),
            headers: vec![HeaderField {
                name: "Content-Type".into(),
                value: "application/json".into(),
            }],
            body: Some("{\"ok\":true}".into()),
        }
    }

    #[test]
    fn prepares_https_request_with_engine_owned_framing() {
        let prepared = prepare(&request()).unwrap();
        assert_eq!(prepared.scheme, "https");
        assert_eq!(prepared.host, "example.com");
        assert_eq!(prepared.port, 443);
        assert_eq!(prepared.body, b"{\"ok\":true}");
        assert!(prepared
            .parsed
            .headers
            .iter()
            .any(|(name, value)| name == "Host" && value == "example.com"));
        assert!(prepared.parsed.headers.iter().any(|(name, value)| {
            name == "Content-Length" && value == &prepared.body.len().to_string()
        }));
    }

    #[test]
    fn supports_custom_port_and_ipv6_host() {
        let mut request = request();
        request.scheme = "http".into();
        request.host = "[::1]".into();
        request.port = Some(8080);
        request.body = None;
        let prepared = prepare(&request).unwrap();
        assert_eq!(prepared.host, "::1");
        assert_eq!(prepared.port, 8080);
        assert!(prepared
            .parsed
            .headers
            .iter()
            .any(|(name, value)| name == "Host" && value == "[::1]:8080"));
    }

    #[test]
    fn rejects_engine_owned_and_unsafe_headers() {
        let mut request = request();
        request.headers.push(HeaderField {
            name: "Content-Length".into(),
            value: "999".into(),
        });
        assert!(matches!(
            prepare(&request),
            Err(ComposerPreparationError::EngineOwnedHeader(_))
        ));

        request.headers.pop();
        request.headers.push(HeaderField {
            name: "X-Test".into(),
            value: "ok\r\nInjected: yes".into(),
        });
        assert!(matches!(
            prepare(&request),
            Err(ComposerPreparationError::InvalidHeader(_))
        ));
    }

    #[test]
    fn rejects_invalid_target_port_and_oversized_body() {
        let mut request = request();
        request.target = "http://example.com/absolute".into();
        assert_eq!(prepare(&request), Err(ComposerPreparationError::InvalidTarget));

        request.target = "/".into();
        request.port = Some(0);
        assert_eq!(prepare(&request), Err(ComposerPreparationError::InvalidPort));

        request.port = None;
        request.body = Some("x".repeat(MAX_COMPOSED_BODY_BYTES + 1));
        assert_eq!(prepare(&request), Err(ComposerPreparationError::BodyTooLarge));
    }
}
