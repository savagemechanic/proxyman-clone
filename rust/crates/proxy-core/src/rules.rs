use serde::{Deserialize, Serialize};

use crate::http::ParsedRequestHead;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RewriteRule {
    pub id: String,
    pub enabled: bool,
    pub host_contains: Option<String>,
    pub path_prefix: Option<String>,
    pub actions: Vec<RewriteAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RewriteAction {
    SetPath { value: String },
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
            if !request.destination.origin_form_target.starts_with(path_prefix) {
                return false;
            }
        }

        true
    }
}

pub fn apply_request_rules(
    request: &mut ParsedRequestHead,
    rules: &[RewriteRule],
) -> Vec<String> {
    let mut applied = Vec::new();

    for rule in rules {
        if !rule.matches(request) {
            continue;
        }

        for action in &rule.actions {
            apply_action(request, action);
        }
        applied.push(rule.id.clone());
    }

    applied
}

fn apply_action(request: &mut ParsedRequestHead, action: &RewriteAction) {
    match action {
        RewriteAction::SetPath { value } => {
            let value = normalize_path(value);
            request.target = value.clone();
            request.destination.origin_form_target = value;
        }
        RewriteAction::SetHeader { name, value } => {
            request
                .headers
                .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
            request.headers.push((name.clone(), value.clone()));
        }
        RewriteAction::RemoveHeader { name } => {
            request
                .headers
                .retain(|(existing, _)| !existing.eq_ignore_ascii_case(name));
        }
    }
}

fn normalize_path(value: &str) -> String {
    if value.starts_with('/') {
        value.to_owned()
    } else {
        format!("/{value}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::http::parse_request_head;

    fn request() -> ParsedRequestHead {
        parse_request_head(
            b"GET http://api.example.com/v1/users HTTP/1.1\r\nHost: api.example.com\r\nAuthorization: Bearer old\r\nX-Debug: 1\r\n\r\n",
        )
        .unwrap()
    }

    #[test]
    fn disabled_rule_never_matches() {
        let mut rule = RewriteRule {
            id: "disabled".into(),
            enabled: false,
            host_contains: Some("example.com".into()),
            path_prefix: Some("/v1".into()),
            actions: vec![RewriteAction::SetPath {
                value: "/changed".into(),
            }],
        };
        assert!(!rule.matches(&request()));
        rule.enabled = true;
        assert!(rule.matches(&request()));
    }

    #[test]
    fn actions_are_applied_in_rule_order() {
        let mut request = request();
        let rules = vec![
            RewriteRule {
                id: "first".into(),
                enabled: true,
                host_contains: Some("API.EXAMPLE".into()),
                path_prefix: Some("/v1".into()),
                actions: vec![
                    RewriteAction::SetPath {
                        value: "v2/users".into(),
                    },
                    RewriteAction::RemoveHeader {
                        name: "authorization".into(),
                    },
                ],
            },
            RewriteRule {
                id: "second".into(),
                enabled: true,
                host_contains: None,
                path_prefix: Some("/v2".into()),
                actions: vec![RewriteAction::SetHeader {
                    name: "X-Debug".into(),
                    value: "2".into(),
                }],
            },
        ];

        let applied = apply_request_rules(&mut request, &rules);
        assert_eq!(applied, vec!["first", "second"]);
        assert_eq!(request.destination.origin_form_target, "/v2/users");
        assert!(
            request
                .headers
                .iter()
                .all(|(name, _)| !name.eq_ignore_ascii_case("authorization"))
        );
        assert_eq!(
            request
                .headers
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case("x-debug"))
                .map(|(_, value)| value.as_str()),
            Some("2")
        );
    }
}
