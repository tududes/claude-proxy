use std::collections::HashMap;

/// Simple SSE event parser that accumulates lines until a blank line, then yields the combined `data:` payload.
/// This follows the SSE spec: multiple `data:` lines per event are joined by `\n`.
pub struct SseEventParser {
    buf: String,
    // Accumulates data: lines for the current event until blank line.
    cur_data_lines: Vec<String>,
}

impl SseEventParser {
    pub fn new() -> Self {
        Self {
            buf: String::with_capacity(16 * 1024),
            cur_data_lines: Vec::with_capacity(4),
        }
    }

    /// Feed bytes and extract zero or more complete SSE event payloads (already joined).
    pub fn push_and_drain_events(&mut self, chunk: &[u8]) -> Vec<String> {
        let s = String::from_utf8_lossy(chunk);
        self.buf.push_str(&s);
        let mut out = Vec::new();

        loop {
            // Find next newline
            let Some(pos) = self.buf.find('\n') else { break };
            // Take one line (retain possible preceding \r, we'll trim)
            let mut line = self.buf.drain(..=pos).collect::<String>();
            if line.ends_with('\n') {
                line.pop();
            }
            if line.ends_with('\r') {
                line.pop();
            }
            let trimmed = line.as_str();

            // Blank line => event terminator
            if trimmed.is_empty() {
                if !self.cur_data_lines.is_empty() {
                    let payload = self.cur_data_lines.join("\n");
                    self.cur_data_lines.clear();
                    out.push(payload);
                }
                continue;
            }

            // Only collect `data:` lines, ignore others (e.g., `event:`/`id:`)
            if let Some(rest) = trimmed.strip_prefix("data:") {
                self.cur_data_lines.push(rest.trim_start().to_string());
            }
        }

        out
    }

    /// Flush at end-of-stream (if the server doesn't send a final blank line).
    pub fn flush(self) -> Option<String> {
        if !self.cur_data_lines.is_empty() {
            let payload = self.cur_data_lines.join("\n");
            // self.cur_data_lines.clear(); // Not needed since we're consuming self
            Some(payload)
        } else {
            None
        }
    }
}

#[derive(Clone)]
pub struct ToolBuf {
    pub block_index: i32,
    pub id: String,
    pub name: String,
}

pub type ToolsMap = HashMap<usize, ToolBuf>;