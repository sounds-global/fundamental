use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use fundamental_config::AppSettings;
use fundamental_core::AnyError;
use fundamental_nats::{
    MessageDescriptor, NatsClient, NatsConfig, Worker, WorkerContext, WorkersFuture,
};
use fundamental_runtime::DrainPhase;
use fundamental_secrets::VaultClient;

use crate::Served;
use crate::deferred::{Built, Deferred};

/// The surface name, as `[apps.<name>.worker]` declares it.
pub(crate) const WORKER: &str = "worker";

/// The step that starts every handler's listener: `mount_workers!`'s `register_workers`.
pub(crate) type Register =
    Arc<dyn for<'a> Fn(&'a WorkerContext) -> WorkersFuture<'a> + Send + Sync>;

/// A value given to every handler: what it adds to the worker's context, once built.
type Given = Box<dyn FnOnce(WorkerContext) -> WorkerContext + Send>;
/// The future that builds a value given to every handler.
type GivenFuture = Pin<Box<dyn Future<Output = Result<Given, AnyError>> + Send>>;
/// The step that builds a value given to every handler, from what the app is served with.
pub(crate) type Provide = Arc<dyn Fn(&Served) -> GivenFuture + Send + Sync>;

/// What an app's [`worker`](crate::App::worker) declares.
#[derive(Default)]
pub(crate) struct Workers {
    register: Option<Register>,
    provided: Vec<Provide>,
    messages: Option<fn() -> Vec<MessageDescriptor>>,
}

impl Workers {
    /// Whether the app declares a worker.
    pub(crate) fn declared(&self) -> bool {
        self.register.is_some()
    }

    /// Declares the handlers `register` starts.
    pub(crate) fn register(&mut self, register: Register) {
        self.register = Some(register);
    }

    /// Gives every handler the value `provide` builds when the worker starts.
    pub(crate) fn provide<T, F, Fut, E>(&mut self, provide: F)
    where
        T: Send + Sync + 'static,
        F: Fn(&Served) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T, E>> + Send + 'static,
        E: Into<AnyError> + 'static,
    {
        self.provided.push(Arc::new(move |served| {
            let built = provide(served);
            Box::pin(async move {
                let value = built.await.map_err(Into::into)?;
                Ok(Box::new(move |context: WorkerContext| context.provide(value)) as Given)
            })
        }));
    }

    /// Declares the messages `describe` lists for the dev tools.
    pub(crate) fn messages(&mut self, describe: fn() -> Vec<MessageDescriptor>) {
        self.messages = Some(describe);
    }

    /// The messages the dev tools describe; none when the app lists none.
    #[cfg_attr(
        not(feature = "introspect"),
        expect(dead_code, reason = "read by the dev tools")
    )]
    pub(crate) fn described(&self) -> Vec<MessageDescriptor> {
        self.messages.map(|describe| describe()).unwrap_or_default()
    }

    /// The worker, its settings resolved now; connected and subscribed only when started.
    pub(crate) fn surface(
        &self,
        settings: &AppSettings,
        served: &Served,
    ) -> Result<Option<Deferred>, AnyError> {
        let Some(register) = self.register.clone() else {
            return Ok(None);
        };
        let start = Start {
            register,
            provided: self.provided.clone(),
            served: served.clone(),
            nats: settings.nats_settings()?.clone(),
            secrets: settings.config().secrets.clone(),
            environment: settings.environment(),
        };
        Ok(Some(Deferred::new(
            WORKER,
            DrainPhase::Work,
            None,
            move || Box::pin(start.run()),
        )))
    }
}

/// What a worker is started from: its handlers, the values they are given, and its settings.
struct Start {
    register: Register,
    provided: Vec<Provide>,
    served: Served,
    nats: fundamental_config::settings::NatsSettings,
    secrets: fundamental_config::settings::VaultSettings,
    environment: fundamental_core::Environment,
}

impl Start {
    /// Connects to NATS, builds what the handlers are given, and subscribes every handler.
    async fn run(self) -> Result<Built, AnyError> {
        let vault = VaultClient::from_settings(&self.secrets).await?;
        let config =
            NatsConfig::from_settings(&self.nats, self.environment, vault.as_ref()).await?;
        let mut context = WorkerContext::new(NatsClient::connect(config).await?);
        for provide in &self.provided {
            context = provide(&self.served).await?(context);
        }
        let listeners = (self.register)(&context).await?;
        Ok(Built::new(
            Worker::new(context.into_client()).with_listeners(listeners),
        ))
    }
}
