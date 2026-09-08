use crate::BodyPreview;

pub const DEFAULT_BODY_PREVIEW_LIMIT: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct BodyCapture {
    limit: usize,
    captured: Vec<u8>,
    total_bytes: u64,
}

impl Default for BodyCapture {
    fn default() -> Self {
        Self::new(DEFAULT_BODY_PREVIEW_LIMIT)
    }
}

impl BodyCapture {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            captured: Vec::with_capacity(limit.min(16 * 1024)),
            total_bytes: 0,
        }
    }

    pub fn ingest(&mut self, bytes: &[u8]) {
        self.total_bytes = self.total_bytes.saturating_add(bytes.len() as u64);
        let remaining = self.limit.saturating_sub(self.captured.len());
        if remaining > 0 {
            self.captured
                .extend_from_slice(&bytes[..bytes.len().min(remaining)]);
        }
    }

    pub fn finish(self, content_type: Option<String>) -> BodyPreview {
        let captured_bytes = self.captured.len() as u64;
        let text = if is_textual_content_type(content_type.as_deref()) || content_type.is_none() {
            String::from_utf8(self.captured).ok()
        } else {
            None
        };

        BodyPreview {
            content_type,
            text,
            captured_bytes,
            total_bytes: self.total_bytes,
            truncated: self.total_bytes > captured_bytes,
        }
    }
}

pub fn content_type(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| value.clone())
}

fn is_textual_content_type(content_type: Option<&str>) -> bool {
    let Some(value) = content_type else {
        return true;
    };
    let mime = value
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    mime.starts_with("text/")
        || mime.contains("json")
        || mime.contains("xml")
        || mime.contains("javascript")
        || mime.contains("graphql")
        || mime == "application/x-www-form-urlencoded"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_is_bounded_but_tracks_total_size() {
        let mut capture = BodyCapture::new(4);
        capture.ingest(b"abc");
        capture.ingest(b"def");
        let preview = capture.finish(Some("text/plain".into()));
        assert_eq!(preview.text.as_deref(), Some("abcd"));
        assert_eq!(preview.captured_bytes, 4);
        assert_eq!(preview.total_bytes, 6);
        assert!(preview.truncated);
    }

    #[test]
    fn binary_content_does_not_create_text_preview() {
        let mut capture = BodyCapture::new(32);
        capture.ingest(&[0, 159, 146, 150]);
        let preview = capture.finish(Some("application/octet-stream".into()));
        assert!(preview.text.is_none());
        assert_eq!(preview.total_bytes, 4);
    }
}
