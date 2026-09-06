//! The typed configuration surface, and the dialect of the layered loader it is read through.
//!
//! The layering is [`terrace_config`]'s. Lowest precedence first: struct defaults, TOML at
//! `$S3_PERMA_LINK_CONFIG` (`config.toml` in the working directory when unset),
//! `S3_PERMA_LINK_`-prefixed `__`-nested environment variables, `$S3_PERMA_LINK_SECRETS_DIR`,
//! and `S3_PERMA_LINK_<KEY>_FILE` indirection. [`terrace`] is where those names are spelled.
//!
//! Call [`load_watched`] rather than [`load`] when the process should be able to pick the
//! configuration up again after a mounted file changes.

mod loader;
#[cfg(feature = "sentry")]
mod sentry;

use s3::creds::Credentials;
use s3::{Bucket, Region};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;
#[cfg(feature = "sentry")]
use terrace_config::schema::ExternalVar;
use terrace_config::schema::{App, Contract, Describe, External, Schema};
use tracing::Level;

use crate::error::Error;

pub use loader::{ConfigError, Loaded, Sources, explain, load, load_watched, terrace};
#[cfg(feature = "sentry")]
pub use sentry::{SentryConfig, SentryLevel};

/// The service's name, as its image is named.
///
/// Visible to the crate rather than to this module alone because it is also the release tag
/// Sentry events carry ([`crate::telemetry::sentry`]): a build reporting under a name the image
/// is not called is a build whose issues nobody can map back to a deploy.
pub(crate) const APP_NAME: &str = "s3-bucket-perma-link";
/// Where the source this was built from lives.
const SOURCE: &str = "https://github.com/TimSchoenle/s3-bucket-perma-link";

const DEFAULT_SERVER_HOST: &str = "0.0.0.0";
const DEFAULT_SERVER_PORT: u16 = 8080;
const DEFAULT_LOG_LEVEL: &str = "info";

/// Everything the service reads at boot.
///
/// [`Describe`] is what the configuration reference in `README.md` is generated from: the key
/// paths, the environment spellings, the types and the `///` comments below are read off this
/// tree by `examples/config-schema.rs` rather than restated in prose that can drift from it.
#[derive(Debug, Deserialize, Serialize, Getters, Describe)]
// Not rustdoc: where these `///` lines are rendered is above, for whoever reads the reference.
// This is for whoever adds a field. `Getters` copies the `///` onto the getter it generates, so
// leaving one off fails `missing_docs` on a span pointing at the derive, never at the field.
//
// `deny_unknown_fields` closes the root: a key that no field here spells is a boot failure
// rather than a value an operator believes they set. The four blocks below close themselves the
// same way — it shuts one level and no other — and none of them is flattened into or flattens
// another, which is the one shape serde refuses to combine it with. The map under
// `bucket.entries` stays open regardless, because its keys are route names an operator chooses.
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct Config {
    /// Where the service listens. Omit the block for `0.0.0.0:8080`.
    #[config(nested)]
    #[serde(default)]
    server: ServerConfig,
    /// The object store and the credentials for it. The boot fails without this block.
    #[config(nested)]
    s3: S3Config,
    /// The permanent links this instance serves. The boot fails without this block, though an
    /// empty `entries` table loads and then answers 404 to everything but `/health`.
    #[config(nested)]
    bucket: BucketConfig,
    /// Logging and error reporting. Omit the block for `info` and no Sentry.
    #[config(nested)]
    #[serde(default)]
    telemetry: TelemetryConfig,
}

/// What the schema generator reads the `Default` column out of.
///
/// It is not a configuration a service could run on: [`S3Config`]'s fields are required, and a
/// required key reports no default at all, so nothing here reaches the generated table. Only the
/// blocks that really do have defaults — [`ServerConfig`] and [`TelemetryConfig`] — contribute.
impl Default for Config {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            s3: S3Config::default(),
            bucket: BucketConfig::default(),
            telemetry: TelemetryConfig::default(),
        }
    }
}

/// Where the object store lives and how to authenticate against it.
#[derive(Debug, Deserialize, Serialize, Default, Getters, Describe)]
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct S3Config {
    /// S3 access key. Mount it rather than setting it in a file that is committed.
    ///
    /// A [`SecretString`]: this struct is nested inside a [`Config`] that the reload supervisor
    /// logs, and a credential must not be one `{:?}` away from the log stream. `SecretString`
    /// refuses to implement [`Serialize`] for the same reason, which is why the field is skipped
    /// on the way out — `#[config(secret)]` renders `<redacted>` in its place anyway.
    #[config(secret)]
    #[serde(skip_serializing)]
    access_key: SecretString,
    /// S3 secret key. Mount it rather than setting it in a file that is committed.
    #[config(secret)]
    #[serde(skip_serializing)]
    secret_key: SecretString,
    /// The endpoint, e.g. `s3.eu-central-1.amazonaws.com`.
    host: String,
    /// The region the endpoint serves, e.g. `eu-central-1`.
    region: String,
}

/// The listener the service binds.
#[derive(Debug, Deserialize, Serialize, Getters, Describe)]
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct ServerConfig {
    /// Address to listen on. `0.0.0.0` in a container, which is the deployment this ships as.
    #[serde(default = "ServerConfig::default_host")]
    host: String,
    /// Port to listen on.
    #[serde(default = "ServerConfig::default_port")]
    port: u16,
}

/// The routes the service serves, and the object each one resolves to.
#[derive(Debug, Deserialize, Serialize, Default, Getters, Describe)]
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct BucketConfig {
    /// One `[bucket.entries.<request path>]` block per permanent link, each carrying a `bucket`
    /// and an `object`.
    ///
    /// Still one key rather than `#[config(nested)]`, because the key paths under it are the
    /// operator's route names and no type knows them ahead of time. `#[config(element)]` is the
    /// opt-in this crate takes up instead: it describes the shape one block takes — see
    /// [`BucketEntry`] for the two fields — without inventing a key for the route name, which
    /// stays undocumented on purpose. A table rather than the delimiter-separated string this
    /// used to be: the string existed only because an environment variable cannot carry a map,
    /// and the TOML layer can.
    #[config(element)]
    entries: HashMap<String, BucketEntry>,
}

/// One route's object.
#[derive(Debug, Clone, Deserialize, Serialize, Getters, Describe)]
// The element type `bucket.entries` publishes, so this is the one closure a schema consumer can
// see directly: without it the block renders as an open object and `objekt = "…"` passes every
// validator, then serves 500s from a route whose object key is empty.
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct BucketEntry {
    /// The bucket [`Self::object`] lives in.
    bucket: String,
    /// The object key inside [`Self::bucket`].
    ///
    /// Named `object` rather than `file`, which the loader cannot express: `_FILE` is the
    /// suffix marking file indirection, so `S3_PERMA_LINK_BUCKET__ENTRIES__DOCS__FILE` would be
    /// read as "the value of this key is in the file named by this variable".
    object: String,
}

/// Logging and error reporting.
///
/// Both are installed once, before the reload supervisor is reached, and cannot be reinstalled
/// on a running process — so this is the one block a configuration reload does not apply.
#[derive(Debug, Deserialize, Serialize, Getters, Describe)]
// Closed like the rest, with one consequence worth knowing: `sentry` is a field only under the
// `sentry` feature, so a build without it now refuses a file carrying `telemetry.sentry.*`
// rather than parsing and ignoring it. That is the behaviour the feature's note in `Cargo.toml`
// and the README already claim, applied to the TOML layer as well as to the environment one.
#[serde(deny_unknown_fields)]
#[getset(get = "pub")]
pub struct TelemetryConfig {
    /// How much the service says: `trace`, `debug`, `info`, `warn` or `error`.
    ///
    /// The console sink only. What Sentry takes is `telemetry.sentry.capture_level` and
    /// `telemetry.sentry.breadcrumb_level`, which are filtered independently.
    ///
    /// Matched without regard to case, so `INFO` is the same as `info`. A file may also carry
    /// `"1"` to `"5"`, which name the five levels counting up from `error`; an environment
    /// variable may not, because its text is read as a number before this key sees it. Write
    /// one of the five names and none of that applies.
    ///
    /// Parsed by [`Self::level`], which is also where the schema's silence about this key is
    /// explained.
    #[serde(default = "TelemetryConfig::default_log_level")]
    log_level: String,
    /// Sentry error reporting and distributed tracing. Off unless
    /// `telemetry.sentry.enabled` is set.
    ///
    /// Present only in a build carrying the `sentry` feature, which is in the default set. A
    /// build without it does not accept these keys at all, rather than accepting them and
    /// reporting nowhere.
    #[cfg(feature = "sentry")]
    #[config(nested)]
    #[serde(default)]
    sentry: SentryConfig,
}

/// The configuration surface, described rather than deserialised.
///
/// The reference tables in `README.md` are rendered from this: the key paths, the environment
/// spellings, the types, the defaults and the `///` comments all come off [`Config`], so the
/// documentation cannot say something the type does not. `examples/config-schema.rs` and
/// `examples/readme-variables.rs` are the two callers; both go through here so that neither can
/// describe a different loader than the service boots with.
///
/// Reads nothing from the environment — the `Default` column is filled from [`Config::default`],
/// so a documentation job produces the same answer on a runner where none of these variables
/// exist. Required keys report no default at all, which is why the empty S3 credentials
/// [`Config::default`] carries never reach the table.
///
/// # Errors
/// Returns [`ConfigError`] if [`Config`] does not serialise.
pub fn schema() -> Result<Schema, ConfigError> {
    terrace()
        .schema::<Config>()
        .with_defaults_from(&Config::default())
}

/// The image's name and where its source lives — everything about this service that no build
/// argument decides.
///
/// The two fields that *do* differ between builds of one source tree — the release and the
/// commit — are deliberately not set here. `examples/config-schema.rs` takes them as flags, so
/// this function reads nothing that a documentation job and a container build could disagree
/// about.
#[must_use]
pub fn app() -> App {
    App::new(APP_NAME).source(SOURCE)
}

/// The whole contract this image publishes: every configuration key, and everything else it
/// reads.
///
/// [`schema`] covers the `S3_PERMA_LINK_` namespace, which a derive can see. [`external`] covers
/// the rest — the variables a dependency reads straight out of the environment before any of
/// this crate's layers exist, and the names a Kubernetes pod carries that belong to nobody here.
/// Without that half the document would claim this image reads nothing outside its own prefix,
/// and a validator believing it would reject a pod for carrying `KUBERNETES_SERVICE_HOST`.
///
/// `app` carries what this particular build is; [`app`] is the part of it that is a constant.
///
/// # Errors
/// Returns [`ConfigError`] if [`Config`] does not serialise, or if the declared external surface
/// is not one a validator could act on — a variable colliding with a configuration key's own
/// environment spelling, for instance.
pub fn contract(app: App) -> Result<Contract, ConfigError> {
    schema()?.into_contract(app).external(external()).build()
}

/// The environment this image reads that the loader does not own.
///
/// Every entry below is something a dependency reads directly, or something the platform injects.
/// `Unknown::Reject` stays on — the default — so a variable that is neither a configuration key
/// nor named here is a defect rather than a shrug, and the list is what makes that claim
/// survivable.
///
/// `sentry` is the only dependency that reads the environment behind our back, and only in a
/// build carrying the `sentry` feature. The list shrank when `telemetry.sentry` arrived:
/// `sentry::init` fills an *unset* field from `SENTRY_DSN`, `SENTRY_RELEASE` or
/// `SENTRY_ENVIRONMENT`, and `telemetry::sentry` now sets all three from the loader,
/// so none of them is reachable. What is left is the proxy and TLS settings, which no
/// configuration key covers.
///
/// Ignored rather than declared are the names with no owner in this image at all — what the
/// kubelet injects into every container. Nothing here reads them, and an image cannot describe
/// what it does not read.
#[must_use]
pub fn external() -> External {
    let external = External::new();

    #[cfg(feature = "sentry")]
    let external = external
        .var(
            ExternalVar::new("HTTP_PROXY")
                .owner("sentry")
                .ty("String")
                .docs("Proxy for Sentry's transport. Read by `sentry::init`; `http_proxy` is also accepted."),
        )
        .var(
            ExternalVar::new("HTTPS_PROXY")
                .owner("sentry")
                .ty("String")
                .docs("As `HTTP_PROXY`, for TLS. Falls back to `HTTP_PROXY`; `https_proxy` is also accepted."),
        )
        .var(
            ExternalVar::new("SSL_VERIFY")
                .owner("sentry")
                .ty("bool")
                .default("true")
                .docs("Whether Sentry's transport validates certificates. `false` accepts invalid ones."),
        )
        // The lowercase spellings the same reader falls back to. Declared rather than ignored:
        // an ignored name is one a chart can misspell freely, and these have an owner.
        .var(
            ExternalVar::new("http_proxy")
                .owner("sentry")
                .ty("String")
                .docs("Lowercase spelling of `HTTP_PROXY`, read when the uppercase one is unset."),
        )
        .var(
            ExternalVar::new("https_proxy")
                .owner("sentry")
                .ty("String")
                .docs("Lowercase spelling of `HTTPS_PROXY`, read when the uppercase one is unset."),
        );

    // No owner in this image: the kubelet writes them into every container and nothing here
    // reads them. `HOSTNAME` is the pod name; `KUBERNETES_*` is the API server's address.
    external.ignore("KUBERNETES_*").ignore("HOSTNAME")
}

impl ServerConfig {
    fn default_host() -> String {
        DEFAULT_SERVER_HOST.to_string()
    }

    fn default_port() -> u16 {
        DEFAULT_SERVER_PORT
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: Self::default_host(),
            port: Self::default_port(),
        }
    }
}

impl TelemetryConfig {
    fn default_log_level() -> String {
        DEFAULT_LOG_LEVEL.to_string()
    }

    /// The configured level.
    ///
    /// `tracing`'s own parser rather than `serde`'s, which is why [`Self::log_level`] is a
    /// `String` that publishes `{"type": "string"}` and no list of values. The set it accepts is
    /// not a fixed list of spellings: it folds ASCII case, and it takes any text `usize` parses
    /// as `1` to `5`, so `INFO`, `Warn` and — in a document, where the text is not read as a
    /// number on the way in — `"3"` and `"+003"` all boot. Publishing the five lowercase names
    /// would therefore publish a schema that rejects configurations this service starts on,
    /// which is the one thing the contract may never do.
    ///
    /// A hand-written `Describe` cannot say it either, so this is documentation rather than an
    /// omission waiting to be fixed: a `terrace_config::schema::Leaf` narrows a key with a type
    /// spelling, a fixed set of values or numeric bounds, and a case-insensitive alternation is
    /// none of the three. `terrace-config` makes the same call for a `Url` — describe what is
    /// true, or say nothing, but never guess. `the_log_level_publishes_no_list_of_values` pins
    /// both halves of that.
    ///
    /// # Errors
    /// Returns [`Error::Logger`] if [`Self::log_level`] does not name a level.
    pub fn level(&self) -> crate::Result<Level> {
        Ok(Level::from_str(&self.log_level)?)
    }
}

impl Default for TelemetryConfig {
    fn default() -> Self {
        Self {
            log_level: Self::default_log_level(),
            #[cfg(feature = "sentry")]
            sentry: SentryConfig::default(),
        }
    }
}

impl S3Config {
    /// Builds one S3 client per request path in `entries`, sharing these credentials and this
    /// endpoint.
    ///
    /// Path-style addressing, so the bucket name goes in the URL path and `host` is spelled as
    /// the endpoint rather than as a per-bucket name. Nothing here reaches the network, so a
    /// credential the store will reject still builds and first fails on a download.
    ///
    /// # Errors
    /// Returns [`Error::Custom`] if the credentials are malformed, or if a bucket named in
    /// `entries` cannot be addressed under `region` and `host`.
    pub fn create_buckets(
        &self,
        entries: &HashMap<String, BucketEntry>,
    ) -> crate::Result<HashMap<String, Bucket>> {
        let region = Region::Custom {
            region: self.region.clone(),
            endpoint: self.host.clone(),
        };

        let credentials = Credentials::new(
            Some(self.access_key.expose_secret()),
            Some(self.secret_key.expose_secret()),
            None,
            None,
            None,
        )
        .map_err(|e| Error::custom(format!("Failed to create credentials: {e}")))?;

        let mut buckets = HashMap::new();
        for (key, entry) in entries {
            let bucket = Bucket::new(&entry.bucket, region.clone(), credentials.clone())
                .map_err(|e| {
                    Error::custom(format!("Failed to create bucket {}: {e}", entry.bucket))
                })?
                .with_path_style();
            buckets.insert(key.clone(), *bucket);
        }

        Ok(buckets)
    }
}

#[cfg(test)]
mod tests {
    use super::{app, contract, schema};
    use std::collections::BTreeMap;
    use terrace_config::schema::{DEFAULT_PATH, LABEL_PATH, LABEL_PREFIX, LABEL_VERSION};

    /// The contract assembles at all.
    ///
    /// Not a formality: `ContractBuilder::build` is what refuses an external surface a validator
    /// could not act on — a variable carrying the loader's own prefix, one declared twice, one
    /// colliding with a key's environment spelling. The container build runs the same code, so
    /// without this the first report of a bad declaration would be a failed image build.
    #[test]
    fn the_contract_assembles() {
        contract(app()).expect("the declared external surface is one a validator can act on");
    }

    /// `telemetry.log_level` publishes its type and nothing else, and must not grow a list of
    /// values.
    ///
    /// The list every reader reaches for — the five names the `///` comment prints — is one this
    /// crate cannot honestly make, because the accepted set is decided by `tracing`'s parser
    /// rather than by `serde`, and that parser folds case and reads `1` to `5`.
    /// `loader::tests::the_log_level_takes_more_than_the_five_names` boots the service on
    /// `INFO`, `Info`, `3` and `+003` to prove it; this pins the consequence, so an annotation
    /// added later fails here instead of failing in an operator's cluster.
    #[test]
    fn the_log_level_publishes_no_list_of_values() {
        let schema = schema().expect("the schema builds");
        let log_level = schema
            .keys
            .iter()
            .find(|key| key.path == "telemetry.log_level")
            .expect("the key is described rather than skipped");

        assert_eq!(log_level.ty.as_deref(), Some("String"));
        assert!(
            log_level.values.is_empty(),
            "a value list here refuses spellings the service boots on: {:?}",
            log_level.values
        );
    }

    /// The labels the Dockerfile carries name *this* loader.
    ///
    /// The block in the Dockerfile is hand-written, because a `LABEL` key cannot be interpolated.
    /// `.github/scripts/check-contract-drift.sh` diffs it against the generator and the build
    /// checks the image against it; this pins the third side of that triangle — that the values
    /// the generator emits are the loader's own prefix and the path the image copies the document
    /// to, rather than whatever a refactor left behind.
    #[test]
    fn the_labels_describe_this_loader() {
        let contract = contract(app()).expect("the contract assembles");
        let labels: BTreeMap<String, String> = contract
            .labels(DEFAULT_PATH)
            .into_iter()
            .map(|(name, value)| ((*name).to_owned(), value))
            .collect();

        assert_eq!(labels[LABEL_VERSION], "1");
        assert_eq!(labels[LABEL_PATH], "/config/contract.json");
        assert_eq!(labels[LABEL_PREFIX], "S3_PERMA_LINK_");

        // The same check the build runs against the image, over the labels the build would paste.
        contract
            .verify_labels(DEFAULT_PATH, &labels)
            .expect("the generated labels satisfy the generated contract");
    }
}
