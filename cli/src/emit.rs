//! The machine-readable half of what `fun` prints.

use std::io::{self, Write};

/// Write one unstyled line of machine-readable output to stdout, ignoring `--quiet`.
///
/// Not `println!`, which panics when the reader closes the pipe (`| head`).
pub(crate) fn emit(line: &str) {
    if let Err(cause) = write_line(&mut io::stdout().lock(), line) {
        eprintln!("could not write to stdout: {cause}");
    }
}

fn write_line(out: &mut impl Write, line: &str) -> io::Result<()> {
    match writeln!(out, "{line}") {
        Err(cause) if cause.kind() == io::ErrorKind::BrokenPipe => Ok(()),
        written => written,
    }
}

#[cfg(test)]
mod tests {
    use super::write_line;
    use std::io::{self, Write};

    struct Failing(io::ErrorKind);

    impl Write for Failing {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(self.0))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_closed_pipe_ends_the_write_quietly() {
        assert!(write_line(&mut Failing(io::ErrorKind::BrokenPipe), "enough").is_ok());
    }

    #[test]
    fn any_other_write_failure_is_reported() {
        let error = write_line(&mut Failing(io::ErrorKind::PermissionDenied), "line")
            .expect_err("a failure that is not a closed pipe must surface");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn a_line_is_written_with_its_newline() {
        let mut out = Vec::new();
        write_line(&mut out, "answer").expect("a Vec accepts every write");
        assert_eq!(out, b"answer\n");
    }
}
