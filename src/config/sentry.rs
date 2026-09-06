//! The `telemetry.sentry` block: whether to report, where, and how much.
//!
//! Present only under the `sentry` feature. With the feature off these keys do not exist in any
//! layer, so a deployment that sets one is told so at boot rather than starting a service that
//! reports nothing — see the feature's note in `Cargo.toml`.
//!
//! The runtime that reads this block is [`crate::telemetry::sentry`]. Nothing here talks to the
//! SDK; this is the description of the surface, and the module split is what lets the
//! configuration reference be generated on a machine that never initialises a client.

use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use terrace_config::schema::Describe;

/// How much of the `tracing` record stream one Sentry sink takes.
///
/// Ordered by severity, so a threshold names the *least* severe record it accepts: `warn` means
/// `error` and `warn`. Deliberately its own type rather than a reuse of
/// [`TelemetryConfig::log_level`](super::TelemetryConfig): `off` is a value here and not one
/// there, and the two thresholds are independent — see [`SentryConfig::breadcrumb_level`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize, Describe)]
#[serde(rename_all = "lowercase")]
pub enum SentryLevel {
    /// Take nothing.
    Off,
    /// `error` only.
    #[default]
    Error,
    /// `error` and `warn`.
    Warn,
    /// Down to `info`.
    Info,
    /// Down to `debug`.
    Debug,
    /// Everything.
    Trace,
}

/// Sentry error reporting and distributed tracing.
///
/// Off by default and off in `config.example.toml`: a DSN is an egress destination for whatever
/// a log record happens to carry, so switching it on is a decision an operator makes once per
/// deployment rather than one that arrives with an image.
///
/// When [`Self::enabled`] is set the service refuses to boot without a usable [`Self::dsn`],
/// rather than installing a client that reports nowhere. A reporter that silently reports
/// nothing is discovered during the incident it was installed for.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each bool is a configuration key an operator writes and the contract schema               reads by name; the enum the lint asks for would rename all six"
)]
#[derive(Debug, Deserialize, Serialize, Getters, Describe)]
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct SentryConfig {
    /// Initialise the Sentry client. `false` installs no client, no panic hook, no subscriber
    /// layer and no HTTP middleware, so every other key in this block is inert.
    #[serde(default)]
    enabled: bool,
    /// Ingest URL, `https://<key>@<host>/<project>`.
    ///
    /// A [`SecretString`]: the embedded key is a bearer credential for the project it names, and
    /// this struct is nested inside a [`Config`](super::Config) the reload supervisor logs with
    /// `{:?}`. Mount it like the S3 credentials rather than committing it — the same `_FILE`
    /// indirection and secrets-directory layers apply.
    ///
    /// Absent while [`Self::enabled`] is set is a boot failure, not a silent no-op.
    #[config(secret)]
    #[serde(default, skip_serializing)]
    dsn: Option<SecretString>,
    /// Environment tag on every event, e.g. `production` or `staging`.
    ///
    /// Unset resolves to `production` for a release build and `development` for a debug one —
    /// the same rule the SDK applies, but applied *here*, so that `SENTRY_ENVIRONMENT` is never
    /// consulted. That variable is a second configuration channel that bypasses this loader and
    /// its shadow-key rejection, and a field the service always sets is one it cannot reach.
    #[serde(default)]
    environment: Option<String>,
    /// Release tag on every event.
    ///
    /// Unset resolves to `s3-bucket-perma-link@<crate version>`, which is what makes a
    /// regression attributable to a deploy. It is also the name the Dockerfile's `sentry-cli
    /// debug-files upload` symbolicates against, so overriding it here without matching that
    /// upload leaves events with no symbols.
    #[serde(default)]
    release: Option<String>,
    /// Host tag on every event. Unset reports none: the identity of one replica is
    /// infrastructure detail, and in a container it is a pod name that is gone by the time
    /// anybody reads the issue.
    #[serde(default)]
    server_name: Option<String>,
    /// Fraction of captured events actually sent, `0.0`–`1.0`.
    ///
    /// A blunt volume cap: it drops whole issues rather than repetitions of one, so a rare
    /// error is exactly what it loses. Leave it at `1.0` unless quota forces otherwise.
    // The interval is not decoration: `telemetry::sentry::check_rate` refuses a value outside
    // `0.0..=1.0` at boot, so a schema saying so rejects exactly the files the loader rejects.
    // Float literals because the field is an `f32` — `min = 0` would publish the integer `0`.
    #[config(range(min = 0.0, max = 1.0))]
    #[serde(default = "SentryConfig::default_sample_rate")]
    sample_rate: f32,
    /// Fraction of traces this service **starts** that are recorded, `0.0`–`1.0`.
    ///
    /// `0.0` — the default — means it starts none of its own, which is the sensible setting for
    /// a service whose whole job is one redirect: performance data on it is a cost with no
    /// question behind it. It does not remove this service from a trace that reaches it already
    /// sampled: an inbound `sentry-trace` header is continued regardless, so a caller that does
    /// trace still sees the hop.
    #[config(range(min = 0.0, max = 1.0))]
    #[serde(default)]
    traces_sample_rate: f32,
    /// Least severe `tracing` level reported as a Sentry **issue**.
    // `values` rather than a literal list: `SentryLevel` is this crate's own unit-variant enum
    // deriving `Describe`, and its `Deserialize` is serde's derived one, so the spellings the
    // schema publishes are read off the type that accepts them and cannot drift from it.
    #[config(values)]
    #[serde(default)]
    capture_level: SentryLevel,
    /// Least severe `tracing` level kept as a **breadcrumb** — the trail attached to the next
    /// issue. Records at or above `capture_level` become issues instead.
    ///
    /// Spelled as a key rather than as a rustdoc link to [`Self::capture_level`]: this first
    /// paragraph is a cell in the generated README table, where an intra-doc link renders as
    /// its own source.
    ///
    /// Independent of [`TelemetryConfig::log_level`](super::TelemetryConfig): the two sinks
    /// carry their own filters, so tightening the console to `warn` does not silently empty
    /// every breadcrumb trail. The cost is symmetric — asking for `debug` breadcrumbs makes the
    /// process evaluate `debug` records it does not print.
    #[config(values)]
    #[serde(default = "SentryConfig::default_breadcrumb_level")]
    breadcrumb_level: SentryLevel,
    /// How many breadcrumbs one event carries.
    #[serde(default = "SentryConfig::default_max_breadcrumbs")]
    max_breadcrumbs: usize,
    /// Attach a stack trace to events that carry none of their own.
    #[serde(default = "SentryConfig::default_attach_stacktrace")]
    attach_stacktrace: bool,
    /// Send personally identifying data with every event: the client IP, the full request header
    /// set, and the request body where one was buffered.
    ///
    /// **Off, and worth leaving off.** This service serves public links; the IP address of
    /// whoever followed one is not what makes a failed S3 fetch actionable, and Sentry is a
    /// third party. On, it also widens what the HTTP middleware records, because the actix
    /// integration reads this same flag to decide whether to redact sensitive headers.
    #[serde(default)]
    send_default_pii: bool,
    /// Record one Sentry transaction per request, named by the matched route.
    ///
    /// Whether a started transaction is *kept* is [`Self::traces_sample_rate`]'s decision; this
    /// is the switch for a deployment that wants error reporting and no performance data at all.
    /// The per-request hub is installed either way — without it, breadcrumbs from concurrently
    /// served requests all land on the main hub and every issue arrives with a trail belonging
    /// to whoever else was in flight.
    #[serde(default = "SentryConfig::default_http_transactions")]
    http_transactions: bool,
    /// Copy `tracing` span fields onto the Sentry span as attributes.
    ///
    /// Off: the span fields here carry request paths and object keys, and a transaction is
    /// stored under a longer retention than a log line.
    #[serde(default)]
    span_attributes: bool,
    /// How long process exit waits for queued events to drain, in seconds.
    ///
    /// Paid on every shutdown, including the rolling ones a deploy produces, so it trades
    /// against how quickly a replica goes away. `0` discards whatever is still queued.
    #[serde(default = "SentryConfig::default_shutdown_timeout_secs")]
    shutdown_timeout_secs: u64,
    /// Print the SDK's own diagnostics to stderr. For proving a DSN works, not for running.
    #[serde(default)]
    debug: bool,
}

impl SentryConfig {
    fn default_sample_rate() -> f32 {
        1.0
    }

    fn default_breadcrumb_level() -> SentryLevel {
        SentryLevel::Info
    }

    fn default_max_breadcrumbs() -> usize {
        100
    }

    fn default_attach_stacktrace() -> bool {
        true
    }

    fn default_http_transactions() -> bool {
        true
    }

    fn default_shutdown_timeout_secs() -> u64 {
        2
    }
}

impl Default for SentryConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            dsn: None,
            environment: None,
            release: None,
            server_name: None,
            sample_rate: Self::default_sample_rate(),
            traces_sample_rate: 0.0,
            capture_level: SentryLevel::Error,
            breadcrumb_level: Self::default_breadcrumb_level(),
            max_breadcrumbs: Self::default_max_breadcrumbs(),
            attach_stacktrace: Self::default_attach_stacktrace(),
            send_default_pii: false,
            http_transactions: Self::default_http_transactions(),
            span_attributes: false,
            shutdown_timeout_secs: Self::default_shutdown_timeout_secs(),
            debug: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::config::{Config, SentryLevel, terrace};
    use secrecy::ExposeSecret;
    use terrace_config::testing::{Harness, Jail};

    fn harness() -> Harness {
        Harness::over(terrace())
    }

    fn set_required(jail: &mut Jail<'_>) {
        jail.env_key("s3.access_key", "access");
        jail.env_key("s3.secret_key", "secret");
        jail.env_key("s3.host", "s3.example.com");
        jail.env_key("s3.region", "eu-central-1");
        jail.env_key("bucket.entries.docs.bucket", "media");
        jail.env_key("bucket.entries.docs.object", "handbook.pdf");
    }

    /// A deployment that says nothing about Sentry gets no client and no egress. The block is
    /// `#[serde(default)]` twice over — once on the field in `TelemetryConfig`, once per key —
    /// so a missing section has to materialise rather than fail the boot of every deployment
    /// that predates this feature.
    #[test]
    fn an_unmentioned_section_is_off() {
        harness().run(|jail| {
            set_required(jail);

            let config: Config = jail.load()?;
            let sentry = config.telemetry().sentry();

            assert!(!sentry.enabled());
            assert!(sentry.dsn().is_none());
            assert!(!sentry.send_default_pii());
            assert!(sentry.traces_sample_rate().abs() < f32::EPSILON);
            assert_eq!(*sentry.capture_level(), SentryLevel::Error);
            assert_eq!(*sentry.breadcrumb_level(), SentryLevel::Info);
            Ok(())
        });
    }

    /// The keys sit two levels deep, which is one deeper than every other block: the loader has
    /// to reach `S3_PERMA_LINK_TELEMETRY__SENTRY__*`, and a DSN mounted as a file has to outrank
    /// the environment the same way an S3 credential does.
    #[test]
    fn the_nested_keys_resolve_through_the_dialect() {
        harness().run(|jail| {
            set_required(jail);
            jail.env_key("telemetry.sentry.enabled", true);
            jail.env_key("telemetry.sentry.traces_sample_rate", "0.25");
            jail.env_key("telemetry.sentry.capture_level", "warn");
            jail.secret_key("telemetry.sentry.dsn", "https://key@sentry.example/42\n")?;

            let config: Config = jail.load()?;
            let sentry = config.telemetry().sentry();

            assert!(*sentry.enabled());
            assert_eq!(
                sentry
                    .dsn()
                    .as_ref()
                    .expect("the mounted DSN is read")
                    .expose_secret(),
                "https://key@sentry.example/42"
            );
            assert!((sentry.traces_sample_rate() - 0.25).abs() < f32::EPSILON);
            assert_eq!(*sentry.capture_level(), SentryLevel::Warn);
            Ok(())
        });
    }

    /// The DSN is a credential and takes the same treatment as one: it has to be mountable, and
    /// the mount has to beat a placeholder left in a committed `ConfigMap`.
    #[test]
    fn a_mounted_dsn_outranks_the_toml_layer() {
        harness().run(|jail| {
            jail.config(
                r#"
[s3]
access_key = "access"
secret_key = "secret"
host = "s3.example.com"
region = "eu-central-1"

[bucket.entries.docs]
bucket = "media"
object = "handbook.pdf"

[telemetry.sentry]
enabled = true
dsn = "https://placeholder@sentry.example/1"
"#,
            )?;
            jail.secrets_volume()
                .file("telemetry__sentry__dsn", "https://real@sentry.example/2\n")
                .projected()
                .create()?;

            let config: Config = jail.load()?;

            assert_eq!(
                config
                    .telemetry()
                    .sentry()
                    .dsn()
                    .as_ref()
                    .expect("the mounted DSN is read")
                    .expose_secret(),
                "https://real@sentry.example/2"
            );
            Ok(())
        });
    }
}
