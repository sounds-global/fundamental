//! Where a server listens: an address it binds, or a socket the service manager passed it.

#[cfg(unix)]
mod activated_sockets;
mod activation_fault;
#[expect(
    clippy::module_inception,
    reason = "one file per type: the type is `Listen`, so its file is `listen.rs`"
)]
mod listen;
mod socket_name;

#[cfg(unix)]
pub use activated_sockets::ActivatedSockets;
pub use activation_fault::ActivationFault;
pub use listen::Listen;
pub use socket_name::SocketName;
