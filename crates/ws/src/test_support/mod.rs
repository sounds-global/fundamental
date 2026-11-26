//! What a suite driving a real [`WsServer`](crate::WsServer) needs.
//!
//! Debug builds only, whatever the feature says: a feature alone could reach a release build.

mod close_frame;
mod connect;
mod connect_to;
mod next_text;
mod serve;
mod socket;
mod until;

pub use close_frame::close_frame;
pub use connect::connect;
pub use connect_to::connect_to;
pub use next_text::next_text;
pub use serve::serve;
pub use socket::Socket;
pub use until::{PATIENCE, until};
