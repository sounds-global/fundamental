//! Promotes the active app's surface entries into typed runtime settings.

use super::{merge_api_settings, merge_server_settings};
use crate::FundamentalConfig;

/// Merges the active app's surface entries into `ApiSettings`/`ServerSettings`.
pub(crate) fn merge_surface_entry_config(config: &mut FundamentalConfig, app_name: &str) {
    let Some(app) = config.apps.apps.get(app_name) else {
        return;
    };

    if let Some(api_entry) = app.surfaces.get("api") {
        merge_api_settings(&mut config.api, api_entry);
    }

    let server_entry = app
        .surfaces
        .get("web")
        .or_else(|| app.surfaces.get("ssr"))
        .or_else(|| app.surfaces.get("web-ssr"));
    if let Some(server_entry) = server_entry {
        merge_server_settings(&mut config.server, server_entry);
    }
}
