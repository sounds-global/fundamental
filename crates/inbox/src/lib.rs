//! An idempotent-consumer inbox: an event's effect commits in one transaction with the record
//! that it ran, so a redelivered, replayed or concurrent copy of the event never has a second
//! effect.

mod attempt;
mod binding;
mod dispatch;
mod effect_name;
mod effect_row;
mod inbound;
mod inbox;
mod inbox_config;
mod inbox_context;
mod inbox_error;
mod inbox_migrations;
mod inbox_row;
mod inbox_settings;
mod inbox_sweep;
mod park;
mod park_reason;
mod parked_address;
mod parked_command;
mod parked_event;
mod parked_row;
mod process;
mod rehome_command;
mod resolve_command;
mod settlement;
mod step;
mod sweep_report;
mod verdict;
mod watermark;

pub use binding::Binding;
pub use effect_name::EffectName;
pub use inbound::Inbound;
pub use inbox::{Inbox, PARKED_PAGE};
pub use inbox_config::InboxConfig;
pub use inbox_context::InboxContext;
pub use inbox_error::InboxError;
pub use inbox_migrations::INBOX_MIGRATIONS;
pub use inbox_settings::InboxSettings;
pub use inbox_sweep::InboxSweep;
pub use park_reason::ParkReason;
pub use parked_address::ParkedAddress;
pub use parked_command::parked_command;
pub use parked_event::ParkedEvent;
pub use rehome_command::rehome_command;
pub use resolve_command::resolve_command;
pub use step::{Step, StepFuture};
pub use sweep_report::SweepReport;

pub use fundamental_sql::EffectTransaction;
