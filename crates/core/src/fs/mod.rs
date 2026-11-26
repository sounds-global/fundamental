//! Writing files so a reader never sees half of one, and reading only the regular file a path
//! names.

mod open_regular;
mod read_regular;
mod replace_file;

pub use open_regular::open_regular;
pub use read_regular::read_regular;
pub use replace_file::replace_file;
