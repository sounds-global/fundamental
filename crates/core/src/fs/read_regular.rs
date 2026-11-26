use std::io::{self, Read};
use std::path::Path;

use super::open_regular;

/// The bytes of the regular file at `path`, opened as [`open_regular`] opens it, at most `limit`.
///
/// # Errors
///
/// [`io::ErrorKind::FileTooLarge`] past `limit`; the errors of [`open_regular`]; otherwise the
/// error of the read.
pub fn read_regular(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let too_large = || {
        io::Error::new(
            io::ErrorKind::FileTooLarge,
            format!("the file passes {limit} bytes"),
        )
    };
    let file = open_regular(path)?;
    if file.metadata()?.len() > limit {
        return Err(too_large());
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_within_its_limit_is_read_and_one_past_it_is_not() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("desired.json");
        std::fs::write(&path, b"0123456789").expect("write");

        assert_eq!(read_regular(&path, 10).expect("read"), b"0123456789");
        let err = read_regular(&path, 9).expect_err("too large");
        assert_eq!(err.kind(), io::ErrorKind::FileTooLarge);
    }
}
