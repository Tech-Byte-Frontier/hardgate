//! Keep compiler diagnostics without letting dependency-artifact chatter consume
//! their output budget. Both the pending line and retained stream are bounded.
pub(super) const LIMIT: usize = 4 * 1024 * 1024;
const LINE_LIMIT: usize = 1024 * 1024;

#[derive(Default)]
pub(super) struct CargoStream {
    line: Vec<u8>,
    output: Vec<u8>,
    truncated: bool,
    oversized_line: bool,
}

impl CargoStream {
    pub(super) fn push(&mut self, chunk: &[u8]) {
        for byte in chunk {
            if *byte == b'\n' {
                self.flush();
            } else if self.line.len() < LINE_LIMIT {
                self.line.push(*byte);
            } else {
                self.oversized_line = true;
            }
        }
    }

    pub(super) fn finish(mut self) -> (Vec<u8>, bool) {
        self.flush();
        (self.output, self.truncated)
    }

    fn flush(&mut self) {
        if self.oversized_line {
            self.truncated = true;
        } else if !self.line.is_empty() {
            self.retain_line();
        }
        self.line.clear();
        self.oversized_line = false;
    }

    fn retain_line(&mut self) {
        let mut line = self.line.clone();
        if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(&line) {
            if matches!(
                value["reason"].as_str(),
                Some("compiler-artifact" | "build-script-executed")
            ) {
                return;
            }
            if let Some(message) = value["message"].as_object_mut() {
                message.remove("rendered");
            }
            if let Ok(compact) = serde_json::to_vec(&value) {
                line = compact;
            }
        }
        line.push(b'\n');
        if self.output.len().saturating_add(line.len()) <= LIMIT {
            self.output.extend(line);
        } else {
            self.truncated = true;
        }
    }
}
