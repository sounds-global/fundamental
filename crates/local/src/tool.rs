use std::ffi::OsStr;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::Instant;

use tracing::{debug, info, warn};

use crate::LocalError;

/// How much of a failed tool's stderr an error carries.
const STDERR_TAIL: usize = 2000;

/// Runs `program` with `args`, never through a shell, writing `stdin` when given, and answers its stdout.
///
/// # Errors
///
/// [`LocalError::ToolFailed`] (`LOCAL_012`) when the program cannot start or fails, with its argv and stderr tail.
pub(crate) fn run<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    stdin: Option<&[u8]>,
) -> Result<String, LocalError> {
    execute(program, args, stdin, false)
}

/// Asks `program` a question whose "no" is an answer, so a refusal is logged at `debug`.
///
/// # Errors
///
/// As [`run`].
pub(crate) fn ask<S: AsRef<OsStr>>(program: &str, args: &[S]) -> Result<String, LocalError> {
    execute(program, args, None, true)
}

#[expect(clippy::cognitive_complexity, reason = "readability baseline")]
fn execute<S: AsRef<OsStr>>(
    program: &str,
    args: &[S],
    stdin: Option<&[u8]>,
    question: bool,
) -> Result<String, LocalError> {
    let argv = std::iter::once(program.to_owned())
        .chain(
            args.iter()
                .map(|arg| arg.as_ref().to_string_lossy().into_owned()),
        )
        .collect::<Vec<_>>()
        .join(" ");
    let started = Instant::now();
    let failed = |status: String, stderr: String| LocalError::ToolFailed {
        argv: argv.clone(),
        status,
        stderr,
    };
    let mut child = Command::new(program)
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| failed("not started".to_owned(), error.to_string()))?;
    if let (Some(bytes), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(bytes)
            .map_err(|error| failed("stdin not written".to_owned(), error.to_string()))?;
    }
    let output = child
        .wait_with_output()
        .map_err(|error| failed("not awaited".to_owned(), error.to_string()))?;
    let elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
    let status = output.status.to_string();
    if output.status.success() {
        info!(argv = %argv, status = %status, elapsed_ms, "local.tool");
        return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let tail: String = stderr
        .chars()
        .rev()
        .take(STDERR_TAIL)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if question {
        debug!(argv = %argv, status = %status, elapsed_ms, stderr = %tail.trim(), "local.tool: answered no");
    } else {
        warn!(argv = %argv, status = %status, elapsed_ms, stderr = %tail.trim(), "local.tool");
    }
    Err(failed(status, tail.trim().to_owned()))
}
