use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use tracing_subscriber::fmt::MakeWriter;

use super::log_hook_output_summary;

#[derive(Clone, Default)]
struct SharedWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
}

struct BufferWriter {
    bytes: Arc<Mutex<Vec<u8>>>,
}

impl Write for BufferWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes
            .lock()
            .expect("log buffer lock should be available")
            .extend(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<'writer> MakeWriter<'writer> for SharedWriter {
    type Writer = BufferWriter;

    fn make_writer(&'writer self) -> Self::Writer {
        BufferWriter {
            bytes: Arc::clone(&self.bytes),
        }
    }
}

#[test]
fn hook_output_log_contains_summary_but_not_raw_message() {
    let secret_output = "token=must-not-appear";
    let messages = [secret_output.to_string()];
    let writer = SharedWriter::default();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(writer.clone())
        .finish();

    tracing::subscriber::with_default(subscriber, || {
        log_hook_output_summary("stop", messages.len());
    });

    let output = String::from_utf8(
        writer
            .bytes
            .lock()
            .expect("log buffer lock should be available")
            .clone(),
    )
    .expect("captured logs should be UTF-8");

    assert!(output.contains("hook_kind=\"stop\""));
    assert!(output.contains("output_count=1"));
    assert!(!output.contains(secret_output));
}
