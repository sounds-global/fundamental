#![forbid(unsafe_code)]
#![cfg_attr(docsrs, feature(doc_cfg))]
//! Native (Tauri) surface: run the framework's router inside a desktop or mobile app.
//!
//! The app's [`Router`](fundamental_server::Router) serves pages over the in-process
//! `fundamental://` scheme and actions over one invoke command, both through an
//! [`ActionBridge`]; [`NativeBuilder`] hosts it on desktop and mobile. Native targets only.

mod action_bridge;
mod anonymous;
mod bridge_request;
mod bridge_response;
mod bridge_set;
#[cfg(feature = "loopback")]
mod callback_parameters;
#[cfg(feature = "client-identity")]
mod client_identity;
mod current_device;
mod current_platform;
#[cfg(feature = "deep-link")]
mod deep_link;
#[cfg(feature = "sync-host")]
mod device_slice;
#[cfg(feature = "sync-host")]
mod device_slice_registry;
mod exit_action;
mod in_process_bridge;
mod invoke;
#[cfg(feature = "loopback")]
mod loopback_callback;
#[cfg(desktop)]
mod menu_config;
mod native_builder;
mod native_error;
mod native_event;
mod native_handle;
mod page_bridges;
mod page_protocol;
mod platform_hooks;
mod principal_source;
#[cfg(feature = "push")]
mod push;
mod remote_bridge;
mod request_authorizer;
#[cfg(feature = "client-identity")]
mod server_trust;
mod sync_handoff;
#[cfg(feature = "sync-host")]
mod sync_host;
#[cfg(feature = "sync-host")]
mod sync_host_config;
mod token_source;
#[cfg(desktop)]
mod tray_config;
mod window_config;
mod window_event_kind;

pub use action_bridge::ActionBridge;
pub use anonymous::Anonymous;
pub use bridge_request::BridgeRequest;
pub use bridge_response::BridgeResponse;
pub use bridge_set::BridgeSet;
#[cfg(feature = "loopback")]
pub use callback_parameters::CallbackParameters;
#[cfg(feature = "client-identity")]
pub use client_identity::ClientIdentity;
pub use current_device::current_device;
pub use current_platform::current_platform;
#[cfg(feature = "deep-link")]
pub use deep_link::DeepLinkConfig;
#[cfg(feature = "sync-host")]
pub use device_slice::{DeviceSlice, DeviceSliceEntry};
#[cfg(feature = "sync-host")]
pub use device_slice_registry::DeviceSliceRegistry;
pub use exit_action::ExitAction;
pub use in_process_bridge::InProcessBridge;
#[cfg(feature = "loopback")]
pub use loopback_callback::LoopbackCallback;
#[cfg(desktop)]
pub use menu_config::{AppMenuItem, MenuBarItem, MenuConfig, PredefinedItem};
pub use native_builder::NativeBuilder;
pub use native_error::NativeError;
pub use native_event::NativeEvent;
pub use native_handle::NativeHandle;
pub use page_bridges::PageBridges;
pub use platform_hooks::PlatformHooks;
pub use principal_source::PrincipalSource;
#[cfg(feature = "push")]
#[cfg_attr(docsrs, doc(cfg(feature = "push")))]
pub use push::{
    PushAuthorization, PushDelivery, PushEnvironment, PushMessage, PushProvider, PushRegistrar,
    PushRegistration, PushUnregistered,
};
pub use remote_bridge::RemoteBridge;
pub use request_authorizer::{AuthorizedHeaders, RequestAuthorizer};
#[cfg(feature = "client-identity")]
pub use server_trust::ServerTrust;
pub use sync_handoff::SyncHandoff;
#[cfg(feature = "sync-host")]
pub use sync_host::SyncHost;
#[cfg(feature = "sync-host")]
pub use sync_host_config::SyncHostConfig;
pub use token_source::TokenSource;
#[cfg(desktop)]
pub use tray_config::{TrayConfig, TrayMenuItem};
pub use window_config::WindowConfig;
pub use window_event_kind::WindowEventKind;
