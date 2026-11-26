//! The route IR: types and the `pages/` scanner that produces them.

mod address_gen;
mod component_name;
mod hydrate_gen;
mod island_resolve;
mod islands_in_pages;
mod layout;
mod link_gen;
mod loader_resolve;
mod page_data_resolve;
mod page_gen;
mod page_slugs_gen;
mod policy_registration;
// `Route` lives in `route.rs`, which clippy flags as inception.
#[allow(clippy::module_inception)]
#[expect(
    clippy::allow_attributes,
    clippy::allow_attributes_without_reason,
    reason = "readability baseline"
)]
mod route;
mod route_fold;
mod route_kind;
mod route_tree;
mod slug_resolve;
mod special_kind;

pub(crate) use address_gen::render_address_registration;
pub(crate) use hydrate_gen::render_hydrate_entry;
pub(crate) use islands_in_pages::islands_in_pages;
pub use layout::Layout;
pub use link_gen::render_link_builders;
pub(crate) use page_gen::render_page_registration;
pub use page_slugs_gen::render_page_slugs;
pub use route::Route;
pub use route_kind::RouteKind;
pub use route_tree::RouteTree;
