use std::fmt;

pub const MAX_REQUEST_HEAD_BYTES: usize = 64 * 1024;
pub const MAX_RESPONSE_HEAD_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedRequestHead {
    pub method: String,
    pub target: String,
    pub version: String,
    pub headers: Vec<(String, String)>,
    pub destination: Destination,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedResponseHead {
    pub version: String,
    pub status_code: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub host: String,
    pub port: u16,
    pub origin_form_target: String,
    pub is_connect: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseRequestError {
    HeaderTooLarge,
    Incomplete,
    InvalidUtf8,
    InvalidRequestLine,
    InvalidResponseLine,
    MissingHost,
    InvalidPort,
    InvalidContentLength,
}

impl fmt::Display for ParseRequestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HeaderTooLarge => f.write_str("HTTP head exceeds configured limit"),
            Self::Incomplete => f.write_str("HTTP head is incomplete"),
            Self::InvalidUtf8 => f.write_str("HTTP head is not valid UTF-8/ASCII"),
            Self::InvalidRequestLine => f.write_str("request line is invalid"),
            Self::InvalidResponseLine => f.write_str("response status line is invalid"),
            Self::MissingHost => f.write_str("request does not identify an upstream host"),
            Self::InvalidPort => f.write_str("request contains an invalid port"),
            Self::InvalidContentLength => f.write_str("HTTP message contains an invalid Content-Length"),
        }
    }
}

impl std::error::Error for ParseRequestError {}

pub fn find_request_head_end(bytes: &[u8]) -> Result<Option<usize>, ParseRequestError> {
    find_head_end(bytes, MAX_REQUEST_HEAD_BYTES)
}

pub fn find_response_head_end(bytes: &[u8]) -> Result<Option<usize>, ParseRequestError> {
    find_head_end(bytes, MAX_RESPONSE_HEAD_BYTES)
}

fn find_head_end(bytes: &[u8], limit: usize) -> Result<Option<usize>, ParseRequestError> {
    if bytes.len() > limit {
        return Err(ParseRequestError::HeaderTooLarge);
    }

    Ok(bytes
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4))
}

pub fn parse_request_head(bytes: &[u8]) -> Result<ParsedRequestHead, ParseRequestError> {
    if bytes.len() > MAX_REQUEST_HEAD_BYTES {
        return Err(ParseRequestError::HeaderTooLarge);
    }
    let end = find_request_head_end(bytes)?.ok_or(ParseRequestError::Incomplete)?;
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| ParseRequestError::InvalidUtf8)?;
    let mut lines = text.split("\r\n");
    let request_line = lines.next().ok_or(ParseRequestError::InvalidRequestLine)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(ParseRequestError::InvalidRequestLine)?;
    let target = parts.next().ok_or(ParseRequestError::InvalidRequestLine)?;
    let version = parts.next().ok_or(ParseRequestError::InvalidRequestLine)?;
    if parts.next().is_some() || !version.starts_with("HTTP/") {
        return Err(ParseRequestError::InvalidRequestLine);
    }

    let headers = parse_header_lines(lines)?;
    let destination = resolve_destination(method, target, &headers)?;
    Ok(ParsedRequestHead {
        method: method.to_owned(),
        target: target.to_owned(),
        version: version.to_owned(),
        headers,
        destination,
    })
}

pub fn parse_response_head(bytes: &[u8]) -> Result<ParsedResponseHead, ParseRequestError> {
    if bytes.len() > MAX_RESPONSE_HEAD_BYTES {
        return Err(ParseRequestError::HeaderTooLarge);
    }
    let end = find_response_head_end(bytes)?.ok_or(ParseRequestError::Incomplete)?;
    let text = std::str::from_utf8(&bytes[..end]).map_err(|_| ParseRequestError::InvalidUtf8)?;
    let mut lines = text.split("\r\n");
    let status_line = lines.next().ok_or(ParseRequestError::InvalidResponseLine)?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().ok_or(ParseRequestError::InvalidResponseLine)?;
    let status_code = parts
        .next()
        .ok_or(ParseRequestError::InvalidResponseLine)?
        .parse::<u16>()
        .map_err(|_| ParseRequestError::InvalidResponseLine)?;
    let reason = parts.next().unwrap_or_default();
    if !version.starts_with("HTTP/") || !(100..=999).contains(&status_code) {
        return Err(ParseRequestError::InvalidResponseLine);
    }

    Ok(ParsedResponseHead {
        version: version.to_owned(),
        status_code,
        reason: reason.to_owned(),
        headers: parse_header_lines(lines)?,
    })
}

fn parse_header_lines<'a>(
    lines: impl Iterator<Item = &'a str>,
) -> Result<Vec<(String, String)>, ParseRequestError> {
    let mut headers = Vec::new();
    for line in lines {
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or(ParseRequestError::InvalidRequestLine)?;
        headers.push((name.trim().to_owned(), value.trim().to_owned()));
    }
    Ok(headers)
}

pub fn content_length(headers: &[(String, String)]) -> Result<Option<u64>, ParseRequestError> {
    let Some(value) = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .map(|(_, value)| value)
    else {
        return Ok(None);
    };

    value
        .parse::<u64>()
        .map(Some)
        .map_err(|_| ParseRequestError::InvalidContentLength)
}

pub fn is_chunked(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(name, value)| {
        name.eq_ignore_ascii_case("transfer-encoding")
            && value
                .split(',')
                .any(|token| token.trim().eq_ignore_ascii_case("chunked"))
    })
}

pub fn resolve_destination(
    method: &str,
    target: &str,
    headers: &[(String, String)],
) -> Result<Destination, ParseRequestError> {
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = split_authority(target, 443)?;
        return Ok(Destination {
            host,
            port,
            origin_form_target: target.to_owned(),
            is_connect: true,
        });
    }

    if let Some(rest) = target.strip_prefix("http://") {
        let (authority, path) = split_authority_and_path(rest);
        let (host, port) = split_authority(authority, 80)?;
        return Ok(Destination {
            host,
            port,
            origin_form_target: path.to_owned(),
            is_connect: false,
        });
    }

    let host_header = headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("host"))
        .map(|(_, value)| value.as_str())
        .ok_or(ParseRequestError::MissingHost)?;
    let (host, port) = split_authority(host_header, 80)?;

    Ok(Destination {
        host,
        port,
        origin_form_target: if target.is_empty() { "/" } else { target }.to_owned(),
        is_connect: false,
    })
}

pub fn rewrite_to_origin_form(head: &[u8], parsed: &ParsedRequestHead) -> Vec<u8> {
    if parsed.destination.is_connect || parsed.target == parsed.destination.origin_form_target {
        return head.to_vec();
    }

    let Some(first_line_end) = head.windows(2).position(|window| window == b"\r\n") else {
        return head.to_vec();
    };
    let mut rewritten = format!(
        "{} {} {}",
        parsed.method, parsed.destination.origin_form_target, parsed.version
    )
    .into_bytes();
    rewritten.extend_from_slice(&head[first_line_end..]);
    rewritten
}

fn split_authority_and_path(value: &str) -> (&str, &str) {
    match value.find('/') {
        Some(index) => (&value[..index], &value[index..]),
        None => (value, "/"),
    }
}

fn split_authority(authority: &str, default_port: u16) -> Result<(String, u16), ParseRequestError> {
    if authority.starts_with('[') {
        let close = authority.find(']').ok_or(ParseRequestError::MissingHost)?;
        let host = authority[1..close].to_owned();
        let suffix = &authority[close + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else {
            suffix
                .strip_prefix(':')
                .ok_or(ParseRequestError::InvalidPort)?
                .parse()
                .map_err(|_| ParseRequestError::InvalidPort)?
        };
        return Ok((host, port));
    }

    match authority.rsplit_once(':') {
        Some((host, port_text)) if !host.contains(':') => {
            if host.is_empty() {
                return Err(ParseRequestError::MissingHost);
            }
            let port = port_text
                .parse()
                .map_err(|_| ParseRequestError::InvalidPort)?;
            Ok((host.to_owned(), port))
        }
        _ => {
            if authority.is_empty() {
                Err(ParseRequestError::MissingHost)
            } else {
                Ok((authority.to_owned(), default_port))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_absolute_form_http_request() {
        let bytes =
            b"GET http://example.com:8080/api?q=1 HTTP/1.1\r\nHost: example.com:8080\r\n\r\n";
        let parsed = parse_request_head(bytes).unwrap();
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.destination.host, "example.com");
        assert_eq!(parsed.destination.port, 8080);
        assert_eq!(parsed.destination.origin_form_target, "/api?q=1");
        assert!(!parsed.destination.is_connect);
    }

    #[test]
    fn parses_connect_authority() {
        let bytes = b"CONNECT api.example.com:443 HTTP/1.1\r\nHost: api.example.com:443\r\n\r\n";
        let parsed = parse_request_head(bytes).unwrap();
        assert_eq!(parsed.destination.host, "api.example.com");
        assert_eq!(parsed.destination.port, 443);
        assert!(parsed.destination.is_connect);
    }

    #[test]
    fn resolves_origin_form_using_host_header() {
        let bytes = b"POST /v1/items HTTP/1.1\r\nHost: localhost:9000\r\nContent-Length: 0\r\n\r\n";
        let parsed = parse_request_head(bytes).unwrap();
        assert_eq!(parsed.destination.host, "localhost");
        assert_eq!(parsed.destination.port, 9000);
        assert_eq!(parsed.destination.origin_form_target, "/v1/items");
    }

    #[test]
    fn parses_response_status_and_headers() {
        let bytes = b"HTTP/1.1 201 Created\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        let parsed = parse_response_head(bytes).unwrap();
        assert_eq!(parsed.status_code, 201);
        assert_eq!(parsed.reason, "Created");
        assert_eq!(content_length(&parsed.headers).unwrap(), Some(2));
    }

    #[test]
    fn detects_chunked_transfer_encoding_case_insensitively() {
        let headers = vec![("Transfer-Encoding".into(), "gzip, Chunked".into())];
        assert!(is_chunked(&headers));
    }

    #[test]
    fn rewrites_absolute_form_to_origin_form_without_extra_blank_lines() {
        let bytes = b"GET http://example.com/test HTTP/1.1\r\nHost: example.com\r\n\r\n";
        let parsed = parse_request_head(bytes).unwrap();
        let rewritten = rewrite_to_origin_form(bytes, &parsed);
        assert_eq!(
            rewritten,
            b"GET /test HTTP/1.1\r\nHost: example.com\r\n\r\n"
        );
    }
}
