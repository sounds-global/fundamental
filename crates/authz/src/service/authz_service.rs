//! [`AuthzService`]: the standalone-service state and its responder wiring.

use std::sync::Arc;

use fundamental_macros::mount_responders;
use fundamental_nats::{Listener, NatsClient, NatsResult};

use super::check_rpc::check_rpc_responder;
use super::delete_rpc::delete_rpc_responder;
use super::expand_rpc::expand_rpc_responder;
use super::list_objects_rpc::list_objects_rpc_responder;
use super::write_rpc::write_rpc_responder;
use crate::authorizer::DynAuthorizer;

/// The state a standalone authorization service serves its RPCs from.
///
/// Holds the same [`DynAuthorizer`] the embedded library runs, and answers every
/// [`AuthzRpc`](super::AuthzRpc) on the [`AuthzWorkers`](super::AuthzWorkers) queue.
///
/// ```no_run
/// # extern crate fundamental_authz;
/// # extern crate fundamental_nats;
/// # use std::sync::Arc;
/// # use fundamental_authz::authorizer::DynAuthorizer;
/// # use fundamental_authz::service::AuthzService;
/// # use fundamental_nats::NatsClient;
/// # async fn run(authorizer: Arc<dyn DynAuthorizer>, client: Arc<NatsClient>) {
/// let service = Arc::new(AuthzService::new(authorizer));
/// // The listeners must be kept alive for the service to keep answering.
/// let _listeners = service.serve(client).await.expect("serve");
/// # }
/// ```
pub struct AuthzService {
    authorizer: Arc<dyn DynAuthorizer>,
}

impl AuthzService {
    /// Builds a service over `authorizer`.
    #[must_use]
    pub fn new(authorizer: Arc<dyn DynAuthorizer>) -> Self {
        Self { authorizer }
    }

    /// The authorizer this service answers from.
    #[must_use]
    pub fn authorizer(&self) -> &dyn DynAuthorizer {
        self.authorizer.as_ref()
    }

    /// Subscribes every authorization RPC on the [`AuthzWorkers`](super::AuthzWorkers)
    /// queue, returning the [`Listener`] guards that keep them running.
    ///
    /// Hold the listeners for the service's lifetime. Several processes load-balance each RPC.
    ///
    /// # Errors
    ///
    /// Returns a [`NatsError`](fundamental_nats::NatsError) if any responder fails to
    /// subscribe.
    pub async fn serve(self: Arc<Self>, client: Arc<NatsClient>) -> NatsResult<Vec<Listener>> {
        Ok(vec![
            check_rpc_responder(Arc::clone(&client), Arc::clone(&self)).await?,
            expand_rpc_responder(Arc::clone(&client), Arc::clone(&self)).await?,
            list_objects_rpc_responder(Arc::clone(&client), Arc::clone(&self)).await?,
            write_rpc_responder(Arc::clone(&client), Arc::clone(&self)).await?,
            delete_rpc_responder(client, self).await?,
        ])
    }
}

// The RPC surface as data, for tooling.
mount_responders! {
    super::check_rpc::check_rpc,
    super::expand_rpc::expand_rpc,
    super::list_objects_rpc::list_objects_rpc,
    super::write_rpc::write_rpc,
    super::delete_rpc::delete_rpc,
}
