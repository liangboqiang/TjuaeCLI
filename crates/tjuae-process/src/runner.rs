use std::io::{Error, Result};
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::task::JoinHandle;

use crate::containment::ChildContainment;

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(120);
pub const DEFAULT_POST_PROCESS_DRAIN: Duration = Duration::from_millis(250);

/// Runs one process and buffers its stdout/stderr while it is running.
pub struct CommandRunner {
    command: Command,
    timeout: Duration,
    post_process_drain: Duration,
    stdin: Option<Vec<u8>>,
}

impl CommandRunner {
    pub fn new(command: Command) -> Self {
        Self {
            command,
            timeout: DEFAULT_TIMEOUT,
            post_process_drain: DEFAULT_POST_PROCESS_DRAIN,
            stdin: None,
        }
    }

    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn post_process_drain(mut self, drain: Duration) -> Self {
        self.post_process_drain = drain;
        self
    }

    /// Write the supplied bytes to the child process standard input and then
    /// close the pipe. This is used by protocol-driven commands such as Hooks.
    pub fn stdin_bytes(mut self, stdin: impl Into<Vec<u8>>) -> Self {
        self.stdin = Some(stdin.into());
        self
    }

    pub async fn run(mut self) -> Result<CommandResult> {
        self.command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if self.stdin.is_some() {
            self.command.stdin(Stdio::piped());
        }
        ChildContainment::configure(&mut self.command);

        let mut child = self.command.spawn()?;
        let child_id = child.id();
        let containment = ChildContainment::attach(&mut child)?;
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));

        let stdout_reader = child
            .stdout
            .take()
            .map(|reader| read_stream(reader, Arc::clone(&stdout)));
        let stderr_reader = child
            .stderr
            .take()
            .map(|reader| read_stream(reader, Arc::clone(&stderr)));

        if let Some(stdin) = self.stdin
            && let Some(mut child_stdin) = child.stdin.take()
        {
            child_stdin.write_all(&stdin).await?;
            child_stdin.shutdown().await?;
        }

        match tokio::time::timeout(self.timeout, child.wait()).await {
            Ok(status) => {
                let status = status?;
                let (stdout_result, stderr_result) = tokio::join!(
                    drain_reader_with_result(stdout_reader, self.post_process_drain),
                    drain_reader_with_result(stderr_reader, self.post_process_drain)
                );
                stdout_result?;
                stderr_result?;

                Ok(CommandResult {
                    exit_code: status.code(),
                    timed_out: false,
                    stdout: take_output(stdout),
                    stderr: take_output(stderr),
                })
            }
            Err(_) => {
                containment.terminate(&mut child, child_id)?;
                child.wait().await?;
                // Terminating the containment closes every writer, so these readers must reach
                // EOF. A bounded drain can discard output held by Tokio's Windows pipe reader.
                let (stdout_result, stderr_result) =
                    tokio::join!(finish_reader(stdout_reader), finish_reader(stderr_reader));
                stdout_result?;
                stderr_result?;

                Ok(CommandResult {
                    exit_code: None,
                    timed_out: true,
                    stdout: take_output(stdout),
                    stderr: take_output(stderr),
                })
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandResult {
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

fn read_stream<R>(mut reader: R, output: Arc<Mutex<Vec<u8>>>) -> JoinHandle<Result<()>>
where
    R: AsyncRead + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        let mut buffer = [0_u8; 8192];

        loop {
            let read = reader.read(&mut buffer).await?;
            if read == 0 {
                return Ok(());
            }

            output
                .lock()
                .expect("process output mutex should not be poisoned")
                .extend_from_slice(&buffer[..read]);
        }
    })
}

async fn finish_reader(reader: Option<JoinHandle<Result<()>>>) -> Result<()> {
    if let Some(reader) = reader {
        reader
            .await
            .map_err(|error| Error::other(format!("读取进程输出失败：{error}")))?
    } else {
        Ok(())
    }
}

async fn drain_reader_with_result(reader: Option<JoinHandle<Result<()>>>, drain: Duration) -> Result<()> {
    if let Some(mut reader) = reader {
        tokio::select! {
            _ = tokio::time::sleep(drain) => {
                reader.abort();
                let _abort_join_result = reader.await;
                Ok(())
            }
            result = &mut reader => {
                result
                    .map_err(|error| Error::other(format!("读取进程输出失败：{error}")))?
            }
        }
    } else {
        Ok(())
    }
}

fn take_output(output: Arc<Mutex<Vec<u8>>>) -> Vec<u8> {
    output
        .lock()
        .expect("process output mutex should not be poisoned")
        .clone()
}

#[cfg(test)]
#[path = "runner_test.rs"]
mod runner_test;
