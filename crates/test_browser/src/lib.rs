//! A real browser for a workspace's tests: a pinned headless Chrome driven over DevTools.

#![forbid(unsafe_code)]
#![deny(missing_docs)]

mod ax_node;
mod browser;
mod cdp_event;
mod connection;
mod console_line;
mod image_format;
mod page;
mod seen_request;
mod test_browser_error;

pub use ax_node::AxNode;
pub use browser::Browser;
pub use cdp_event::CdpEvent;
pub use console_line::ConsoleLine;
pub use image_format::ImageFormat;
pub use page::Page;
pub use seen_request::SeenRequest;
pub use test_browser_error::TestBrowserError;
