use serde::{Deserialize, Serialize};

use crate::http::{ParsedRequestHead, ParsedResponseHead};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteRule {
    pub id: String,
    pub enabled: bool,
    pub host_contains: Option<String>,
    pub path_prefix: Option<String>,
    pub actions: Vec<RewriteAction>,
    #[serde(default)]
    pub response_actions: Vec<ResponseRewriteAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RewriteAction {
    SetPath { value: String },
    SetHeader { name: String, value: String },
    RemoveHeader { name: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ResponseRewriteAction {
    SetStatus { value: u16 },
    SetHeader { name: String, value: String },
    RemoveHeader { name: String },
}

impl RewriteRule {
    pub fn matches(&self, request: &ParsedRequestHead) -> bool {
        if !self.enabled {
            return false;
        }

        if let Some(host_contains) = self.host_contains.as_deref() {
            if !request
                .destination
                .host
                .to_ascii_lowercase()
                .contains(&host_contains.to_ascii_lowercase())
            {
                return false;
            }
        }

        if let Some(path_prefix) = self.path_prefix.as_deref() {
            if !request
                .destination
                .origin_form_target
                .starts_with(path_prefix)
            {
                return false;
            }
        }

        true
    }
}

pub fn apply_request_rules(request: &mut ParsedRequestHead, rules: &[RewriteRule]) -> Vec<String> {
    let mut applied = Vec::new();

    for rule in rules {
        if !rule.matches(request) {
            continue;
        }

        let mut changed = false;
        for action in &rule.actions {
            changed |= apply_request_action(request, action);
        }
        if changed {
            applied.push(rule.id.clone());
        }
    }

    applied
}

pub fn apply_response_rules(
    request: &ParsedRequestHead,
    response: &mut ParsedResponseHead,
    rules: &[RewriteRule],
) -> Vec<String> {
    let mut applied = Vec::new();

    for rule in rules {
        if !rule.matches(request) {
            continue;
        }

        let mut changed = false;
        for action in &rule.response_actions {
            changed |= apply_response_action(response, action);
        }
        if changed {
            applied.push(rule.id.clone());
        }
    }

    applied
}

fn apply_request_action(request: &mut ParsedRequestHead, action: &RewriteAction) -> bool {
    match action {
        RewriteAction::SetPath { value } => {
            let Some(value) = normalize_path(value) else {
                return false;
            };
            request.target = value.clone();
            request.destination.origin_form_target = value;
            true
        }
        RewriteAction::SetHeader { name, value } => set_header(&mut request.headers, name, value),
        RewriteAction::RemoveHeader { name } => remove_header(&mut request.headers, name),
    }
}

fn apply_response_action(
    response: &mut ParsedResponseHead,
    action: &ResponseRewriteAction,
) -> bool {
    match action {
        ResponseRewriteAction::SetStatus { value } => {
            if !valid_status_override(*value) || response.status_code == *value {
                return false;
            }
            response.status_code = *value;
            response.reason = reason_phrase(*value).to_owned();
            true
        }
        ResponseRewriteAction::SetHeader { name, value } => {
            set_header(&mut response.headers, name, value)
        }
        ResponseRewriteAction::RemoveHeader { name } => remove_header(&mut response.headers, name),
    }
}

fn set_header(headers: &mut Vec<(String, String)>, name: &str, value: &str) -> bool {
    if !valid_rewrite_header_name(name) || !valid_header_value(value) {
        return false;
    }
    let existing = headers
        .iter()
        .find(|(existing, _)| existing.eq_ignore_ascii_case(name));
    if existing.is_some_and(|(_, existing_value)| existing_value == value) {
        return false;
    }
    headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
    headers.push((name.to_owned(), value.to_owned()));
    true
}

fn remove_header(headers: &mut Vec<(String, String)>, name: &str) -> bool {
    if !valid_rewrite_header_name(name) {
        return false;
    }
    let before = headers.len();
    headers.retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
    headers.len() != before
}

fn normalize_path(value: &str) -> Option<String> {
    if value.is_empty()
        || value
            .bytes()
            .any(|byte| byte.is_ascii_control() || byte == b' ')
    {
        return None;
    }

    Some(if value.starts_with('/') {
        value.to_owned()
    } else {
        format!("/{value}")
    })
}

fn valid_rewrite_header_name(name: &str) -> bool {
    valid_header_name(name)
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "content-length" | "transfer-encoding" | "connection" | "proxy-connection"
        )
}

fn valid_header_name(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|byte| {
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

fn valid_status_override(status: u16) -> bool {
    (200..=599).contains(&status) && !matches!(status, 204 | 205 | 304)
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::{parse_request_head, parse_response_head};

    fn request() -> ParsedRequestHead {
        parse_request_head(
            b"GET http://api.example.com/v1/users HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: Bearer old\r\nX-Debug: 1\r\n\r\n",
        )
        .unwrap()
    }

    fn base_rule() -> RewriteRule {
        RewriteRule {
            id: "rule".into(),
            enabled: true,
            host_contains: Some("example.com".into()),
            path_prefix: Some("/v1".into()),
            actions: Vec::new(),
            response_actions: Vec::new(),
        }
    }

    #[test]
    fn disabled_rule_never_matches() {
        let mut rule = base_rule();
        rule.enabled = false;
        assert!(!rule.matches(&request()));
        rule.enabled = true;
        assert!(rule.matches(&request()));
    }

    #[test]
    fn request_actions_are_applied_in_rule_order() {
        let mut request = request();
        let mut first = base_rule();
        first.id = "first".into();
        first.host_contains = Some("API.EXAMPLE".into());
        first.actions = vec![
            RewriteAction::SetPath {
                value: "v2/users".into(),
            },
            RewriteAction::RemoveHeader {
                name: "authorization".into(),
            },
        ];
        let second = RewriteRule {
            id: "second".into(),
            enabled: true,
            host_contains: None,
            path_prefix: Some("/v2".into()),
            actions: vec![RewriteAction::SetHeader {
                name: "X-Debug".into(),
                value: "2".into(),
            }],
            response_actions: Vec::new(),
        };

        let applied = apply_request_rules(&mut request, &[first, second]);
        assert_eq!(applied, vec!["first", "second"]);
        assert_eq!(request.destination.origin_form_target, "/v2/users");
        assert!(
            request
                .headers
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("authorization"))
        );
    }

    #[test]
    fn response_actions_change_status_and_headers() {
        let request = request();
        let mut response = parse_response_head(
            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Remove: yes\r\n\r\n",
        )
        .unwrap();
        let mut rule = base_rule();
        rule.response_actions = vec![
            ResponseRewriteAction::SetStatus { value: 418 },
            ResponseRewriteAction::SetHeader {
                name: "X-Debug".into(),
                value: "rewritten".into(),
            },
            ResponseRewriteAction::RemoveHeader {
                name: "X-Remove".into(),
            },
        ];

        let applied = apply_response_rules(&request, &mut response, &[rule]);
        assert_eq!(applied, vec!["rule"]);
        assert_eq!(response.status_code, 418);
        assert_eq!(response.reason, "");
        assert!(
            response.headers.iter().any(|(name, value)| {
                name.eq_ignore_ascii_case("x-debug") && value == "rewritten"
            })
        );
        assert!(
            response
                .headers
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("x-remove"))
        );
    }

    #[test]
    fn unsafe_values_bodyless_statuses_and_framing_headers_are_ignored() {
        let request = request();
        let original_request = request.clone();
        let mut mutable_request = request;
        let mut rule = base_rule();
        rule.actions = vec![
            RewriteAction::SetPath {
                value: "/ok\r\nInjected: yes".into(),
            },
            RewriteAction::SetHeader {
                name: "Content-Length".into(),
                value: "999".into(),
            },
        ];
        assert!(apply_request_rules(&mut mutable_request, &[rule.clone()]).is_empty());
        assert_eq!(mutable_request, original_request);

        let mut response =
            parse_response_head(b"HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\n").unwrap();
        let original_response = response.clone();
        rule.response_actions = vec![
            ResponseRewriteAction::SetStatus { value: 204 },
            ResponseRewriteAction::RemoveHeader {
                name: "Content-Length".into(),
            },
            ResponseRewriteAction::SetHeader {
                name: "X-Test".into(),
                value: "bad\r\nInjected: yes".into(),
            },
        ];
        assert!(apply_response_rules(&original_request, &mut response, &[rule]).is_empty());
        assert_eq!(response, original_response);
    }
}
