use serde::{Deserialize, Serialize};

use crate::{
    HeaderField,
    http::{ParsedRequestHead, ParsedResponseHead},
    rules::{
        safe_normalize_path, safe_remove_header, safe_set_header, safe_status_override,
        valid_safe_header_edit,
    },
};

pub const MAX_BREAKPOINT_RULES: usize = 128;
pub const MAX_BREAKPOINT_EDIT_HEADERS: usize = 64;
pub const MAX_BREAKPOINT_MATCHER_BYTES: usize = 1_024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointPhase {
    Request,
    Response,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BreakpointRule {
    pub id: String,
    pub enabled: bool,
    pub phase: BreakpointPhase,
    pub host_contains: Option<String>,
    pub path_prefix: Option<String>,
    pub method: Option<String>,
}

impl BreakpointRule {
    pub fn matches(&self, phase: BreakpointPhase, request: &ParsedRequestHead) -> bool {
        if !self.enabled || self.phase != phase {
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

        if let Some(method) = self.method.as_deref() {
            if !request.method.eq_ignore_ascii_case(method) {
                return false;
            }
        }

        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PausedBreakpoint {
    pub id: u64,
    pub transaction_id: u64,
    pub rule_id: String,
    pub phase: BreakpointPhase,
    pub deadline_unix_ms: u64,
    pub scheme: String,
    pub host: String,
    pub method: String,
    pub target: String,
    pub request_headers: Vec<HeaderField>,
    pub status_code: Option<u16>,
    pub response_headers: Vec<HeaderField>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HeaderEdits {
    #[serde(default)]
    pub set: Vec<HeaderField>,
    #[serde(default)]
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BreakpointDecision {
    Continue,
    Drop,
    EditRequest {
        target: Option<String>,
        #[serde(default)]
        headers: HeaderEdits,
    },
    EditResponse {
        status_code: Option<u16>,
        #[serde(default)]
        headers: HeaderEdits,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BreakpointEditError {
    WrongPhase,
    InvalidTarget,
    TooManyHeaderEdits,
    InvalidHeader(String),
    InvalidStatus(u16),
}

impl std::fmt::Display for BreakpointEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPhase => f.write_str("breakpoint decision does not match the paused phase"),
            Self::InvalidTarget => f.write_str("edited request target is invalid"),
            Self::TooManyHeaderEdits => write!(
                f,
                "too many breakpoint header edits; maximum is {MAX_BREAKPOINT_EDIT_HEADERS}"
            ),
            Self::InvalidHeader(name) => write!(f, "invalid or protected breakpoint header: {name}"),
            Self::InvalidStatus(status) => write!(f, "invalid response status override: {status}"),
        }
    }
}

impl std::error::Error for BreakpointEditError {}

pub fn validate_rule_set(rules: &[BreakpointRule]) -> Result<(), String> {
    if rules.len() > MAX_BREAKPOINT_RULES {
        return Err(format!(
            "too many breakpoint rules; maximum is {MAX_BREAKPOINT_RULES}"
        ));
    }

    let mut ids = std::collections::HashSet::new();
    for rule in rules {
        let id = rule.id.trim();
        if id.is_empty() || id.len() > 128 {
            return Err("breakpoint rule IDs must contain 1-128 characters".into());
        }
        if !ids.insert(id.to_owned()) {
            return Err(format!("duplicate breakpoint rule ID: {id}"));
        }

        for (name, value) in [
            ("host_contains", rule.host_contains.as_deref()),
            ("path_prefix", rule.path_prefix.as_deref()),
            ("method", rule.method.as_deref()),
        ] {
            if let Some(value) = value {
                if value.is_empty() || value.len() > MAX_BREAKPOINT_MATCHER_BYTES {
                    return Err(format!(
                        "breakpoint rule {id} has invalid {name}; maximum is {MAX_BREAKPOINT_MATCHER_BYTES} bytes"
                    ));
                }
                if value.bytes().any(|byte| byte == 0 || byte == b'\r' || byte == b'\n') {
                    return Err(format!("breakpoint rule {id} has unsafe {name}"));
                }
            }
        }
    }
    Ok(())
}

pub fn validate_decision(
    phase: BreakpointPhase,
    decision: &BreakpointDecision,
) -> Result<(), BreakpointEditError> {
    match decision {
        BreakpointDecision::Continue | BreakpointDecision::Drop => Ok(()),
        BreakpointDecision::EditRequest { target, headers } => {
            if phase != BreakpointPhase::Request {
                return Err(BreakpointEditError::WrongPhase);
            }
            if let Some(target) = target {
                if safe_normalize_path(target).is_none() {
                    return Err(BreakpointEditError::InvalidTarget);
                }
            }
            validate_header_edits(headers)
        }
        BreakpointDecision::EditResponse {
            status_code,
            headers,
        } => {
            if phase != BreakpointPhase::Response {
                return Err(BreakpointEditError::WrongPhase);
            }
            if let Some(status) = status_code {
                if safe_status_override(*status).is_none() {
                    return Err(BreakpointEditError::InvalidStatus(*status));
                }
            }
            validate_header_edits(headers)
        }
    }
}

pub fn apply_request_decision(
    request: &mut ParsedRequestHead,
    decision: &BreakpointDecision,
) -> Result<bool, BreakpointEditError> {
    validate_decision(BreakpointPhase::Request, decision)?;
    let BreakpointDecision::EditRequest { target, headers } = decision else {
        return Ok(false);
    };

    let mut changed = false;
    if let Some(target) = target {
        let normalized = safe_normalize_path(target).ok_or(BreakpointEditError::InvalidTarget)?;
        if request.destination.origin_form_target != normalized {
            request.target = normalized.clone();
            request.destination.origin_form_target = normalized;
            changed = true;
        }
    }
    changed |= apply_header_edits(&mut request.headers, headers);
    Ok(changed)
}

pub fn apply_response_decision(
    response: &mut ParsedResponseHead,
    decision: &BreakpointDecision,
) -> Result<bool, BreakpointEditError> {
    validate_decision(BreakpointPhase::Response, decision)?;
    let BreakpointDecision::EditResponse {
        status_code,
        headers,
    } = decision
    else {
        return Ok(false);
    };

    let mut changed = false;
    if let Some(status) = status_code {
        let reason = safe_status_override(*status)
            .ok_or(BreakpointEditError::InvalidStatus(*status))?;
        if response.status_code != *status {
            response.status_code = *status;
            response.reason = reason.to_owned();
            changed = true;
        }
    }
    changed |= apply_header_edits(&mut response.headers, headers);
    Ok(changed)
}

fn validate_header_edits(headers: &HeaderEdits) -> Result<(), BreakpointEditError> {
    if headers.set.len().saturating_add(headers.remove.len()) > MAX_BREAKPOINT_EDIT_HEADERS {
        return Err(BreakpointEditError::TooManyHeaderEdits);
    }

    for header in &headers.set {
        if !valid_safe_header_edit(&header.name, Some(&header.value)) {
            return Err(BreakpointEditError::InvalidHeader(header.name.clone()));
        }
    }
    for name in &headers.remove {
        if !valid_safe_header_edit(name, None) {
            return Err(BreakpointEditError::InvalidHeader(name.clone()));
        }
    }
    Ok(())
}

fn apply_header_edits(headers: &mut Vec<(String, String)>, edits: &HeaderEdits) -> bool {
    let mut changed = false;
    for name in &edits.remove {
        changed |= safe_remove_header(headers, name);
    }
    for header in &edits.set {
        changed |= safe_set_header(headers, &header.name, &header.value);
    }
    changed
}

#[cfg(test)]
mod tests {
    use crate::http::{parse_request_head, parse_response_head};

    use super::*;

    fn request() -> ParsedRequestHead {
        parse_request_head(
            b"GET http://api.example.com/v1/users HTTP/1.1\r\nHost: api.example.com\r\nX-Old: yes\r\nContent-Length: 0\r\n\r\n",
        )
        .unwrap()
    }

    fn rule(phase: BreakpointPhase) -> BreakpointRule {
        BreakpointRule {
            id: "api".into(),
            enabled: true,
            phase,
            host_contains: Some("EXAMPLE.COM".into()),
            path_prefix: Some("/v1".into()),
            method: Some("get".into()),
        }
    }

    #[test]
    fn matches_phase_host_path_and_method() {
        let request = request();
        assert!(rule(BreakpointPhase::Request).matches(BreakpointPhase::Request, &request));
        assert!(!rule(BreakpointPhase::Response).matches(BreakpointPhase::Request, &request));
    }

    #[test]
    fn validates_rule_count_duplicates_and_matchers() {
        let first = rule(BreakpointPhase::Request);
        let mut duplicate = first.clone();
        duplicate.phase = BreakpointPhase::Response;
        assert!(validate_rule_set(&[first.clone(), duplicate]).is_err());

        let mut unsafe_rule = first;
        unsafe_rule.path_prefix = Some("/ok\r\nInjected".into());
        assert!(validate_rule_set(&[unsafe_rule]).is_err());
    }

    #[test]
    fn applies_request_path_and_header_edits_without_touching_framing() {
        let mut request = request();
        let decision = BreakpointDecision::EditRequest {
            target: Some("v2/users".into()),
            headers: HeaderEdits {
                set: vec![HeaderField {
                    name: "X-New".into(),
                    value: "1".into(),
                }],
                remove: vec!["X-Old".into()],
            },
        };
        assert!(apply_request_decision(&mut request, &decision).unwrap());
        assert_eq!(request.destination.origin_form_target, "/v2/users");
        assert!(request.headers.iter().any(|(name, value)| name == "X-New" && value == "1"));
        assert!(request.headers.iter().all(|(name, _)| name != "X-Old"));
        assert!(request.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("Content-Length") && value == "0"
        }));
    }

    #[test]
    fn rejects_wrong_phase_framing_headers_and_bodyless_status() {
        let request_decision = BreakpointDecision::EditRequest {
            target: None,
            headers: HeaderEdits {
                set: vec![HeaderField {
                    name: "Content-Length".into(),
                    value: "99".into(),
                }],
                remove: Vec::new(),
            },
        };
        assert!(matches!(
            validate_decision(BreakpointPhase::Request, &request_decision),
            Err(BreakpointEditError::InvalidHeader(_))
        ));
        assert!(matches!(
            validate_decision(BreakpointPhase::Response, &request_decision),
            Err(BreakpointEditError::WrongPhase)
        ));

        let response_decision = BreakpointDecision::EditResponse {
            status_code: Some(204),
            headers: HeaderEdits::default(),
        };
        assert_eq!(
            validate_decision(BreakpointPhase::Response, &response_decision),
            Err(BreakpointEditError::InvalidStatus(204))
        );
    }

    #[test]
    fn applies_response_status_and_header_edits() {
        let mut response = parse_response_head(
            b"HTTP/1.1 200 OK\r\nX-Old: yes\r\nContent-Length: 2\r\n\r\n",
        )
        .unwrap();
        let decision = BreakpointDecision::EditResponse {
            status_code: Some(503),
            headers: HeaderEdits {
                set: vec![HeaderField {
                    name: "X-New".into(),
                    value: "yes".into(),
                }],
                remove: vec!["X-Old".into()],
            },
        };
        assert!(apply_response_decision(&mut response, &decision).unwrap());
        assert_eq!(response.status_code, 503);
        assert_eq!(response.reason, "Service Unavailable");
        assert!(response.headers.iter().any(|(name, value)| name == "X-New" && value == "yes"));
        assert!(response.headers.iter().any(|(name, value)| {
            name.eq_ignore_ascii_case("Content-Length") && value == "2"
        }));
    }
}

impl Default for HeaderEdits {
    fn default() -> Self {
        Self {
            set: Vec::new(),
            remove: Vec::new(),
        }
    }
}
