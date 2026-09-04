use std::io::{self, Read};
use std::process::Child;
use std::sync::mpsc::{self, Receiver};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::{Duration, Instant};

pub(super) struct CapturedOutput {
    stdout: Option<Receiver<CapturedStream>>,
    stderr: Option<Receiver<CapturedStream>>,
    stdout_result: Option<CapturedStream>,
    stderr_result: Option<CapturedStream>,
    cancel: Arc<AtomicBool>,
}

impl CapturedOutput {
    pub(super) fn from_child(child: &mut Child) -> Self {
        let cancel = Arc::new(AtomicBool::new(false));
        Self {
            stdout: child
                .stdout
                .take()
                .map(|reader| spawn_reader(reader, Arc::clone(&cancel))),
            stderr: child
                .stderr
                .take()
                .map(|reader| spawn_reader(reader, Arc::clone(&cancel))),
            stdout_result: None,
            stderr_result: None,
            cancel,
        }
    }

    pub(super) fn collect(&mut self, timeout: Duration) -> CaptureResult {
        let deadline = Instant::now() + timeout;
        if self.stdout_result.is_none() {
            self.stdout_result =
                receive_stream(&mut self.stdout, &self.cancel, remaining(deadline));
        }
        if self.stderr_result.is_none() {
            self.stderr_result =
                receive_stream(&mut self.stderr, &self.cancel, remaining(deadline));
        }
        CaptureResult {
            output: combine_streams(
                self.stdout_result.clone().unwrap_or_default(),
                self.stderr_result.clone().unwrap_or_default(),
            ),
            incomplete: self.stdout.is_some() || self.stderr.is_some(),
        }
    }

    pub(super) fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

fn remaining(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

#[derive(Debug, Clone, Default)]
struct CapturedStream {
    bytes: Vec<u8>,
    truncated: bool,
}

pub(super) struct CaptureResult {
    pub(super) output: String,
    pub(super) incomplete: bool,
}

#[cfg(unix)]
fn spawn_reader<R>(reader: R, cancel: Arc<AtomicBool>) -> Receiver<CapturedStream>
where
    R: Read + std::os::fd::AsRawFd + Send + 'static,
{
    let nonblocking = super::set_nonblocking(reader.as_raw_fd()).is_ok();
    spawn_reader_loop(reader, cancel, nonblocking)
}

#[cfg(not(unix))]
fn spawn_reader<R>(reader: R, cancel: Arc<AtomicBool>) -> Receiver<CapturedStream>
where
    R: Read + Send + 'static,
{
    spawn_reader_loop(reader, cancel, false)
}

fn spawn_reader_loop<R>(
    reader: R,
    cancel: Arc<AtomicBool>,
    nonblocking: bool,
) -> Receiver<CapturedStream>
where
    R: Read + Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let (bytes, truncated) = read_stream(reader, &cancel, nonblocking);
        let _ = sender.send(CapturedStream { bytes, truncated });
    });
    receiver
}

fn read_stream<R: Read>(
    mut reader: R,
    cancel: &Arc<AtomicBool>,
    nonblocking: bool,
) -> (Vec<u8>, bool) {
    let mut bytes = Vec::with_capacity(super::MAX_STREAM_BYTES);
    let mut truncated = false;
    let mut buffer = [0_u8; 4096];
    while !cancel.load(Ordering::Relaxed) {
        match read_step(&mut reader, &mut buffer, nonblocking) {
            ReaderStep::Done => break,
            ReaderStep::Chunk(read) => append_chunk(&mut bytes, &mut truncated, &buffer[..read]),
            ReaderStep::Wait => thread::sleep(Duration::from_millis(10)),
        }
    }
    (bytes, truncated)
}

enum ReaderStep {
    Done,
    Chunk(usize),
    Wait,
}

fn read_step<R: Read>(reader: &mut R, buffer: &mut [u8], nonblocking: bool) -> ReaderStep {
    match reader.read(buffer) {
        Ok(0) => ReaderStep::Done,
        Ok(read) => ReaderStep::Chunk(read),
        Err(error) if nonblocking && error.kind() == io::ErrorKind::WouldBlock => ReaderStep::Wait,
        Err(_) => ReaderStep::Done,
    }
}

fn append_chunk(bytes: &mut Vec<u8>, truncated: &mut bool, chunk: &[u8]) {
    let remaining = super::MAX_STREAM_BYTES.saturating_sub(bytes.len());
    let keep = chunk.len().min(remaining);
    bytes.extend_from_slice(&chunk[..keep]);
    *truncated |= keep < chunk.len();
}

fn receive_stream(
    receiver: &mut Option<Receiver<CapturedStream>>,
    cancel: &Arc<AtomicBool>,
    timeout: Duration,
) -> Option<CapturedStream> {
    let channel = receiver.as_ref()?;
    match channel.recv_timeout(timeout) {
        Ok(stream) => {
            *receiver = None;
            Some(stream)
        }
        Err(mpsc::RecvTimeoutError::Timeout) => {
            cancel.store(true, Ordering::Relaxed);
            None
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            *receiver = None;
            None
        }
    }
}

fn combine_streams(stdout: CapturedStream, stderr: CapturedStream) -> String {
    let stdout = stream_text(stdout);
    let stderr = stream_text(stderr);
    let combined = match (stdout.is_empty(), stderr.is_empty()) {
        (true, true) => String::new(),
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
    };
    super::truncate_output(combined)
}

fn stream_text(stream: CapturedStream) -> String {
    let mut text = String::from_utf8_lossy(&stream.bytes).trim().to_string();
    if stream.truncated {
        text.push_str("\n[output truncated after 32768 bytes]");
    }
    text
}
