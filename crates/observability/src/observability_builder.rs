//! [`ObservabilityBuilder`] — assembles the process's tracing subscriber.

use fundamental_config::AppSettings;
use fundamental_config::settings::ObservabilitySettings;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

use crate::nats_connect_ceiling::NatsConnectCeiling;
use crate::{ObservabilityError, ObservabilityGuard};

const JSON_FORMAT: &str = "json";

/// The floor on the process-lifecycle target, applied after every other directive.
///
/// A literal because the runtime depends on this crate; a runtime test pins the two together.
const LIFECYCLE_FLOOR: &str = "fundamental::lifecycle=info";

/// The subscriber a caller's layer is composed against: under the process filter and the
/// [`NatsConnectCeiling`].
pub type ObservabilitySubscriber = tracing_subscriber::layer::Layered<
    NatsConnectCeiling,
    tracing_subscriber::layer::Layered<EnvFilter, tracing_subscriber::Registry>,
>;

/// A caller's layer, erased to the one shape the builder can hold.
pub type BoxedLayer = Box<dyn Layer<ObservabilitySubscriber> + Send + Sync + 'static>;

/// Builds and installs the process's subscriber from `[observability]`.
///
/// Directives passed to [`with_directives`] apply after `log_level` and `RUST_LOG`, so a
/// privacy cap on a target cannot be lifted by the environment.
///
/// ```rust,no_run
/// # extern crate fundamental_observability;
/// # extern crate fundamental_config;
/// # use fundamental_config::AppSettings;
/// # use fundamental_observability::ObservabilityBuilder;
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// # let app = AppSettings::load("layer4")?;
/// const DRIVER_LOG_CAPS: [&str; 2] = ["scylla=info", "sqlx=info"];
///
/// let _telemetry = ObservabilityBuilder::from_app(&app)
///     .with_directives(DRIVER_LOG_CAPS)
///     .init()?;
/// # Ok(())
/// # }
/// ```
///
/// [`with_directives`]: Self::with_directives
#[must_use = "an ObservabilityBuilder installs nothing until `init` is called"]
pub struct ObservabilityBuilder {
    log_level: String,
    span_level: String,
    log_format: String,
    service_name: String,
    directives: Vec<String>,
    extra: Vec<BoxedLayer>,
    #[cfg(feature = "otlp")]
    otlp: crate::otlp::OtlpPlan,
    #[cfg(not(feature = "otlp"))]
    otlp_asked: bool,
}

impl ObservabilityBuilder {
    /// Reads the active app's `[observability]` settings; the service name defaults to the app's.
    pub fn from_app(app: &AppSettings) -> Self {
        Self::from_settings(&app.config().observability, app.name())
    }

    /// Reads `settings` for the service `app_name`.
    pub fn from_settings(settings: &ObservabilitySettings, app_name: &str) -> Self {
        Self {
            log_level: settings.log_level.clone(),
            span_level: settings.span_level.clone(),
            log_format: settings.log_format.clone(),
            service_name: settings
                .service_name
                .clone()
                .unwrap_or_else(|| app_name.to_string()),
            directives: Vec::new(),
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::from_settings(settings),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: settings.otlp.enabled,
        }
    }

    /// Whether [`init`](Self::init) would install this telemetry, checked without installing
    /// it. With the `otlp` feature it needs a Tokio reactor, as `init` does.
    ///
    /// # Errors
    /// - [`ObservabilityError::InvalidDirective`] if a directive cannot be parsed.
    /// - [`ObservabilityError::Exporter`] if an enabled OTLP exporter cannot be built.
    /// - [`ObservabilityError::OtlpNotBuilt`] if `[observability.otlp]` is enabled in a build
    ///   without the `otlp` feature.
    pub fn check(&self) -> Result<(), ObservabilityError> {
        #[cfg(not(feature = "otlp"))]
        if self.otlp_asked {
            return Err(ObservabilityError::OtlpNotBuilt {
                service: self.service_name.clone(),
            });
        }
        // Checked strictly: the lenient read below drops a bad directive with a line on stderr.
        EnvFilter::try_new(&self.log_level).map_err(|error| {
            ObservabilityError::InvalidDirective {
                directive: self.log_level.clone(),
                reason: error.to_string(),
            }
        })?;
        self.global_filter()?;
        self.log_filter()?;
        #[cfg(feature = "otlp")]
        self.span_filter()?;
        #[cfg(feature = "otlp")]
        self.otlp.check()?;
        Ok(())
    }

    /// Loads `app_name` under a bootstrap subscriber and reads its `[observability]` settings.
    ///
    /// ```rust,no_run
    /// # extern crate fundamental_observability;
    /// # use fundamental_observability::ObservabilityBuilder;
    /// # fn main() -> Result<(), Box<dyn std::error::Error>> {
    /// const DRIVER_LOG_CAPS: [&str; 2] = ["scylla=info", "sqlx=info"];
    ///
    /// let (app, builder) = ObservabilityBuilder::boot("layer4")?;
    /// let _telemetry = builder.with_directives(DRIVER_LOG_CAPS).init()?;
    /// # let _ = app;
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    /// [`ObservabilityError::Config`] if the configuration cannot be loaded.
    pub fn boot(app_name: &str) -> Result<(AppSettings, Self), ObservabilityError> {
        let app = crate::boot::with_bootstrap_logging(|| AppSettings::load(app_name))?;
        let builder = Self::from_app(&app);
        Ok((app, builder))
    }

    /// Composes `layer` into the process subscriber, under the same filter and caps.
    pub fn with_layer<L>(mut self, layer: L) -> Self
    where
        L: Layer<ObservabilitySubscriber> + Send + Sync + 'static,
    {
        self.extra.push(Box::new(layer));
        self
    }

    /// Appends filter directives that override both `log_level` and `RUST_LOG`.
    pub fn with_directives<I, S>(mut self, directives: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        self.directives
            .extend(directives.into_iter().map(|d| d.as_ref().to_string()));
        self
    }

    /// Builds the subscriber and installs it as this process's global default.
    ///
    /// # Errors
    /// - [`ObservabilityError::InvalidDirective`] if a directive cannot be parsed.
    /// - [`ObservabilityError::AlreadyInitialized`] if a subscriber is already installed.
    /// - [`ObservabilityError::Exporter`] if an enabled OTLP exporter cannot be built.
    /// - [`ObservabilityError::OtlpNotBuilt`] if `[observability.otlp]` is enabled in a build
    ///   without the `otlp` feature.
    pub fn init(mut self) -> Result<ObservabilityGuard, ObservabilityError> {
        self.check()?;
        let filter = self.global_filter()?;
        let log_filter = self.log_filter()?;
        #[cfg(feature = "otlp")]
        let span_filter = self.span_filter()?;
        let json = self.log_format.eq_ignore_ascii_case(JSON_FORMAT);
        let extra = std::mem::take(&mut self.extra);

        #[cfg(feature = "otlp")]
        let (otlp_layers, providers) = self.otlp.build(&self.service_name, span_filter)?;

        // Trace context only: a baggage propagator would carry arbitrary values to every backend.
        #[cfg(feature = "otlp")]
        opentelemetry::global::set_text_map_propagator(
            opentelemetry_sdk::propagation::TraceContextPropagator::new(),
        );

        // Boxed: the formats and optional OTLP layers give each branch a distinct type.
        let fmt_layer = tracing_subscriber::fmt::layer();
        let mut layers: Vec<BoxedLayer> = Vec::new();

        // The fmt layer filtered back to the log level, so spans admitted for tracing do not print.
        if json {
            layers.push(Box::new(
                fmt_layer.json().flatten_event(true).with_filter(log_filter),
            ));
        } else {
            layers.push(Box::new(fmt_layer.pretty().with_filter(log_filter)));
        }

        #[cfg(feature = "otlp")]
        layers.extend(otlp_layers);

        layers.extend(extra);

        // Directly over the filter: a ceiling inside `layers` cannot veto a callsite's interest.
        tracing_subscriber::registry()
            .with(filter)
            .with(NatsConnectCeiling)
            .with(layers)
            .try_init()
            .map_err(|_| ObservabilityError::AlreadyInitialized)?;

        crate::install_panic_hook();

        // On the lifecycle floor, so `span_level = "warn"` cannot hide it.
        #[cfg(feature = "otlp")]
        let (traces, logs) = providers
            .as_ref()
            .map_or((false, false), |p| (p.has_tracer(), p.has_logger()));
        #[cfg(not(feature = "otlp"))]
        let (traces, logs) = (false, false);
        tracing::info!(
            target: "fundamental::lifecycle",
            service = %self.service_name,
            format = if json { JSON_FORMAT } else { "pretty" },
            traces,
            logs,
            "telemetry installed"
        );

        Ok(ObservabilityGuard {
            #[cfg(feature = "otlp")]
            providers,
        })
    }

    /// The filter that decides which spans and events exist at all.
    ///
    /// The registry filter gates span creation, so it admits the more verbose of the log and
    /// span levels; each layer filters itself back down.
    fn global_filter(&self) -> Result<EnvFilter, ObservabilityError> {
        self.global_filter_with(std::env::var("RUST_LOG").ok().as_deref())
    }

    /// [`global_filter`](Self::global_filter) given the environment instead of reading it.
    fn global_filter_with(&self, rust_log: Option<&str>) -> Result<EnvFilter, ObservabilityError> {
        // Uncapped: the lifecycle floor raises `max_level_hint` and would defeat the widening.
        let mut filter = self.base_filter(rust_log);

        let span_level: tracing_subscriber::filter::LevelFilter = self.span_level.parse().map_err(
            |error: tracing_subscriber::filter::LevelParseError| {
                ObservabilityError::InvalidDirective {
                    directive: self.span_level.clone(),
                    reason: error.to_string(),
                }
            },
        )?;
        let current = filter
            .max_level_hint()
            .unwrap_or(tracing_subscriber::filter::LevelFilter::ERROR);
        if span_level > current {
            filter = filter.add_directive(span_level.into());
        }

        // Caps re-applied after the widening, so tracing cannot lift a privacy cap.
        self.capped(filter)
    }

    /// The filter for anything that prints: configured level, then `RUST_LOG`, then the caps.
    fn log_filter(&self) -> Result<EnvFilter, ObservabilityError> {
        self.log_filter_with(std::env::var("RUST_LOG").ok().as_deref())
    }

    /// [`log_filter`](Self::log_filter) given the environment instead of reading it.
    fn log_filter_with(&self, rust_log: Option<&str>) -> Result<EnvFilter, ObservabilityError> {
        self.capped(self.base_filter(rust_log))
    }

    /// `RUST_LOG` if it parses, else the configured level, before any cap.
    fn base_filter(&self, rust_log: Option<&str>) -> EnvFilter {
        rust_log
            .and_then(|directives| EnvFilter::try_new(directives).ok())
            .unwrap_or_else(|| EnvFilter::new(self.log_level.clone()))
    }

    /// The filter for the span export: `span_level`, then the caps; never `RUST_LOG`.
    #[cfg_attr(
        not(feature = "otlp"),
        expect(dead_code, reason = "only the `otlp` exporter reads it")
    )]
    fn span_filter(&self) -> Result<EnvFilter, ObservabilityError> {
        self.capped(EnvFilter::new(self.span_level.clone()))
    }

    /// Appends this builder's directives, then the lifecycle floor nothing may override.
    fn capped(&self, mut filter: EnvFilter) -> Result<EnvFilter, ObservabilityError> {
        for directive in &self.directives {
            let parsed =
                directive
                    .parse()
                    .map_err(|error: tracing_subscriber::filter::ParseError| {
                        ObservabilityError::InvalidDirective {
                            directive: directive.clone(),
                            reason: error.to_string(),
                        }
                    })?;
            filter = filter.add_directive(parsed);
        }

        // After the caps so it cannot be turned off; the target carries no request data.
        filter = filter.add_directive(LIFECYCLE_FLOOR.parse().map_err(
            |error: tracing_subscriber::filter::ParseError| ObservabilityError::InvalidDirective {
                directive: LIFECYCLE_FLOOR.to_string(),
                reason: error.to_string(),
            },
        )?);
        Ok(filter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cap must beat `RUST_LOG`, not lose to it.
    #[test]
    fn a_later_directive_overrides_an_earlier_one_for_the_same_target() {
        let filter = EnvFilter::new("info,scylla=trace")
            .add_directive("scylla=info".parse().expect("a valid directive"));

        let rendered = filter.to_string();
        assert!(
            rendered.contains("scylla=info"),
            "the appended cap must win: {rendered}"
        );
        assert!(
            !rendered.contains("scylla=trace"),
            "the capped directive must not survive: {rendered}"
        );
    }

    fn builder_at(log_level: &str, directives: Vec<String>) -> ObservabilityBuilder {
        ObservabilityBuilder {
            log_level: log_level.to_string(),
            span_level: "info".to_string(),
            log_format: "pretty".to_string(),
            service_name: "test".to_string(),
            directives,
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        }
    }

    #[cfg(not(feature = "otlp"))]
    #[test]
    fn otlp_asked_of_a_build_without_the_exporter_is_refused() {
        let mut settings = fundamental_config::settings::ObservabilitySettings::default();
        settings.otlp.enabled = true;
        settings.otlp.endpoint = Some("http://127.0.0.1:4317".to_owned());

        let refused = ObservabilityBuilder::from_settings(&settings, "www").check();

        assert!(
            matches!(refused, Err(ObservabilityError::OtlpNotBuilt { ref service }) if service == "www"),
            "{refused:?}"
        );
    }

    #[test]
    fn telemetry_that_asks_for_nothing_unbuilt_passes_the_check() {
        let settings = fundamental_config::settings::ObservabilitySettings::default();

        let checked = ObservabilityBuilder::from_settings(&settings, "www").check();

        assert!(checked.is_ok(), "{checked:?}");
    }

    #[cfg(feature = "otlp")]
    #[test]
    fn an_otlp_endpoint_no_exporter_can_reach_is_refused_naming_it() {
        let otlp = fundamental_config::settings::OtlpSettings {
            enabled: true,
            endpoint: Some("not an address".to_owned()),
            ..fundamental_config::settings::OtlpSettings::default()
        };
        let settings = fundamental_config::settings::ObservabilitySettings {
            otlp,
            ..fundamental_config::settings::ObservabilitySettings::default()
        };

        let refused = ObservabilityBuilder::from_settings(&settings, "www").check();

        assert!(
            matches!(
                refused,
                Err(ObservabilityError::Exporter { ref endpoint, .. }) if endpoint == "not an address"
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn a_log_level_that_does_not_parse_is_refused_naming_it() {
        let settings = fundamental_config::settings::ObservabilitySettings {
            log_level: "warn,fundamental_api::request_span=loud".to_owned(),
            ..fundamental_config::settings::ObservabilitySettings::default()
        };

        let refused = ObservabilityBuilder::from_settings(&settings, "www").check();

        assert!(
            matches!(
                refused,
                Err(ObservabilityError::InvalidDirective { ref directive, .. })
                    if directive == "warn,fundamental_api::request_span=loud"
            ),
            "{refused:?}"
        );
    }

    #[test]
    fn the_lifecycle_floor_survives_a_production_log_level() {
        let rendered = builder_at("warn", Vec::new())
            .log_filter_with(None)
            .expect("builds")
            .to_string();

        assert!(
            rendered.contains("fundamental::lifecycle=info"),
            "a warn-level service must still record its own lifecycle: {rendered}"
        );
    }

    #[test]
    fn the_lifecycle_floor_cannot_be_lowered_by_a_directive() {
        let rendered = builder_at("info", vec!["fundamental::lifecycle=off".to_string()])
            .log_filter_with(None)
            .expect("builds")
            .to_string();

        assert!(
            rendered.contains("fundamental::lifecycle=info"),
            "the floor must beat a later directive for the same target: {rendered}"
        );
    }

    #[test]
    fn the_floor_does_not_lift_a_cap_on_another_target() {
        let rendered = builder_at("info", vec!["scylla=info".to_string()])
            .log_filter_with(None)
            .expect("builds")
            .to_string();

        assert!(
            rendered.contains("scylla=info") && rendered.contains("fundamental::lifecycle=info"),
            "the cap and the floor are independent and both apply: {rendered}"
        );
    }

    #[test]
    fn the_lifecycle_floor_reaches_the_global_filter() {
        let rendered = builder_at("warn", Vec::new())
            .global_filter_with(None)
            .expect("builds")
            .to_string();

        assert!(
            rendered.contains("fundamental::lifecycle=info"),
            "the creation filter must admit lifecycle events: {rendered}"
        );
    }

    #[test]
    fn a_malformed_directive_names_itself() {
        let builder = ObservabilityBuilder {
            log_level: "info".to_string(),
            span_level: "info".to_string(),
            log_format: "pretty".to_string(),
            service_name: "test".to_string(),
            directives: vec!["not a directive=!!".to_string()],
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        };

        let error = builder
            .log_filter_with(None)
            .expect_err("the directive is malformed");

        match error {
            ObservabilityError::InvalidDirective { directive, .. } => {
                assert_eq!(directive, "not a directive=!!");
            }
            other => panic!("expected InvalidDirective, got {other:?}"),
        }
    }

    #[derive(Clone, Default)]
    struct Recorder(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl<S: tracing::Subscriber> Layer<S> for Recorder {
        fn on_event(
            &self,
            event: &tracing::Event<'_>,
            _ctx: tracing_subscriber::layer::Context<'_, S>,
        ) {
            if let Ok(mut seen) = self.0.lock() {
                seen.push(event.metadata().target().to_string());
            }
        }
    }

    /// The composition `init` performs, minus the global install.
    #[test]
    fn a_caller_layer_receives_events_and_the_filter_still_binds_it() {
        let recorder = Recorder::default();
        let layers: Vec<BoxedLayer> = vec![Box::new(recorder.clone())];
        let subscriber = tracing_subscriber::registry()
            .with(EnvFilter::new("info,capped=off"))
            .with(NatsConnectCeiling)
            .with(layers);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "wanted", "reaches the caller's layer");
            tracing::info!(target: "capped", "must not");
        });

        let seen = recorder.0.lock().expect("the recorder is not poisoned");
        assert_eq!(
            seen.as_slice(),
            ["wanted"],
            "a capped target must not reach a caller-supplied layer either"
        );
    }

    #[test]
    #[expect(clippy::cognitive_complexity, reason = "readability baseline")]
    fn warn_logs_still_record_info_spans() {
        let builder = ObservabilityBuilder {
            log_level: "warn".to_string(),
            span_level: "info".to_string(),
            log_format: "json".to_string(),
            service_name: "test".to_string(),
            directives: Vec::new(),
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        };

        let global = builder.global_filter_with(None).expect("the filter builds");
        assert_eq!(
            global.max_level_hint(),
            Some(tracing_subscriber::filter::LevelFilter::INFO),
            "the registry must admit the spans tracing needs, whatever the log level"
        );

        // Driven by real events: the lifecycle floor raises the hint to `info`.
        let recorder = Recorder::default();
        let layers: Vec<BoxedLayer> = vec![Box::new(recorder.clone())];
        let subscriber = tracing_subscriber::registry()
            .with(
                builder
                    .log_filter_with(None)
                    .expect("the log filter builds"),
            )
            .with(NatsConnectCeiling)
            .with(layers);

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(target: "app", "an ordinary info line, below the log level");
            tracing::warn!(target: "app", "an ordinary warn line, at it");
            tracing::info!(target: LIFECYCLE_TARGET, "the shutdown record");
        });

        let seen = recorder.0.lock().expect("the recorder is not poisoned");
        assert_eq!(
            seen.as_slice(),
            ["app", LIFECYCLE_TARGET],
            "the ordinary info line is filtered, its warn line prints, \
             and the lifecycle line survives the production log level"
        );
    }

    /// Mirrored from `fundamental_runtime`, whose own test asserts the two agree.
    const LIFECYCLE_TARGET: &str = "fundamental::lifecycle";

    #[test]
    fn a_more_verbose_log_level_is_not_lowered() {
        let builder = ObservabilityBuilder {
            log_level: "debug".to_string(),
            span_level: "info".to_string(),
            log_format: "json".to_string(),
            service_name: "test".to_string(),
            directives: Vec::new(),
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        };

        assert_eq!(
            builder
                .global_filter_with(None)
                .expect("the filter builds")
                .max_level_hint(),
            Some(tracing_subscriber::filter::LevelFilter::DEBUG)
        );
    }

    #[test]
    fn a_deployed_service_still_creates_the_info_spans_it_exports() {
        use tracing_subscriber::layer::SubscriberExt;

        let builder = ObservabilityBuilder {
            log_level: "warn".to_string(),
            span_level: "info".to_string(),
            log_format: "json".to_string(),
            service_name: "test".to_string(),
            directives: vec!["scylla=info".to_string()],
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        };

        let subscriber = tracing_subscriber::registry()
            .with(builder.global_filter_with(None).expect("the filter builds"));
        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!("server.dispatch");
            assert!(
                !span.is_disabled(),
                "a service logging at `warn` and exporting spans at `info` did not create \
                 one — so it exports nothing, and every component still reports success"
            );
        });
    }

    #[test]
    fn the_telemetry_record_survives_a_deployment_that_wants_neither_logs_nor_spans_at_info() {
        let builder = ObservabilityBuilder {
            log_level: "warn".to_string(),
            span_level: "warn".to_string(),
            log_format: "json".to_string(),
            service_name: "test".to_string(),
            directives: Vec::new(),
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        };

        let rendered = builder
            .global_filter_with(None)
            .expect("the filter builds")
            .to_string();
        assert!(
            rendered.contains(LIFECYCLE_FLOOR),
            "with both levels at warn the lifecycle floor is the only thing keeping the \
             telemetry record alive, and it is not in the filter that gates creation: \
             {rendered}"
        );
    }

    #[test]
    fn a_cap_survives_the_widening_on_every_filter() {
        let builder = ObservabilityBuilder {
            log_level: "warn".to_string(),
            span_level: "trace".to_string(),
            log_format: "json".to_string(),
            service_name: "test".to_string(),
            directives: vec!["scylla=info".to_string()],
            extra: Vec::new(),
            #[cfg(feature = "otlp")]
            otlp: crate::otlp::OtlpPlan::disabled(),
            #[cfg(not(feature = "otlp"))]
            otlp_asked: false,
        };

        for (name, filter) in [
            ("global", builder.global_filter_with(None)),
            ("log", builder.log_filter_with(None)),
            #[cfg(feature = "otlp")]
            ("span", builder.span_filter()),
        ] {
            let rendered = filter.expect("the filter builds").to_string();
            assert!(
                rendered.contains("scylla=info"),
                "the {name} filter must carry the cap: {rendered}"
            );
            assert!(
                !rendered.contains("scylla=trace"),
                "the {name} filter must not let the cap be widened: {rendered}"
            );
        }
    }
}
