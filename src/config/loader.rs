//! The `s3-bucket-perma-link` dialect of [`terrace_config`].
//!
//! The layering itself — the TOML fragments, the `S3_PERMA_LINK_*` environment layer, the
//! secrets-directory provider, the `_FILE` indirection and the shadow-key rejection — belongs
//! to `terrace-config`. What stays here is the one thing that is ours: which environment names
//! this deployment spells.

use serde::de::DeserializeOwned;
use terrace_config::Terrace;
use terrace_config::explain::Explanation;

pub use terrace_config::{Error as ConfigError, Loaded, Sources};

/// The prefix every configuration variable carries.
const PREFIX: &str = "S3_PERMA_LINK_";

/// The loader the service boots through.
///
/// Layers, lowest precedence first: struct defaults, TOML at `$S3_PERMA_LINK_CONFIG` (a file,
/// or every `*.toml` in it if it names a directory, falling back to `config.toml` in the
/// working directory), `S3_PERMA_LINK_`-prefixed `__`-nested environment variables,
/// `$S3_PERMA_LINK_SECRETS_DIR`, and `S3_PERMA_LINK_<KEY>_FILE` indirection. The last three are
/// mutually exclusive per key: a key supplied by two of them is refused at boot rather than
/// resolved by precedence, because a stale environment variable shadowing a rotated mounted
/// secret keeps the service running on the old credential.
///
/// Nothing is reserved beyond `S3_PERMA_LINK_CONFIG` and `S3_PERMA_LINK_SECRETS_DIR`, which
/// `terrace-config` reserves itself: every value this service reads, the Sentry DSN and the log
/// level included, is read out of the layers rather than out of the environment directly, so
/// there is no key a mounted file could supply to nobody.
///
/// One function, used by all four callers — the boot in `main`, the reload the supervisor
/// drives, the schema the README is generated from, and the tests — so none of them can be
/// reading a dialect the others do not.
#[must_use]
pub fn terrace() -> Terrace {
    Terrace::new(PREFIX)
}

/// Loads `T` through [`terrace`].
///
/// # Errors
/// Returns [`ConfigError`] if a required value is missing, a value fails to parse, a
/// file-backed source cannot be read, or one key is supplied by more than one of the last
/// three layers.
pub fn load<T: DeserializeOwned>() -> Result<T, ConfigError> {
    terrace().load()
}

/// Loads `T` together with the sources a reload watches to read it again.
///
/// # Errors
/// As [`load`].
pub fn load_watched<T: DeserializeOwned>() -> Result<Loaded<T>, ConfigError> {
    terrace().load_watched()
}

/// Which layer supplied each key, as it stands right now.
///
/// Holds no configuration value — not redacted on the way out, never recorded — so the report is
/// safe to log in full. That is what makes it worth logging at all: "the mounted secret is not
/// being picked up" is answered by a report naming the mount and the stale environment variable
/// sitting on top of it, without a debugger and without a redeploy.
///
/// It re-reads at the moment it is called, so calling it from inside a rebuilt runtime describes
/// the layers that rebuild saw rather than the ones the process booted on.
///
/// # Errors
/// Returns [`ConfigError`] if a file-backed layer cannot be read. A configuration [`load`]
/// *refuses* still explains: a key supplied twice is reported as one key with two sources.
pub fn explain() -> Result<Explanation, ConfigError> {
    terrace().explain()
}

#[cfg(test)]
mod tests {
    use super::terrace;
    use crate::config::Config;
    use secrecy::ExposeSecret;
    use terrace_config::explain::{Layer, Origin};
    use terrace_config::testing::{Harness, Jail};
    use tracing::Level;

    /// The sandbox every test below runs in: an empty environment, a temporary working directory,
    /// and *this* crate's loader — so no test can pass against a variable name the service does
    /// not read.
    fn harness() -> Harness {
        Harness::over(terrace())
    }

    /// Sets the four values every load needs, so a test can say only what it is about.
    fn set_credentials(jail: &mut Jail<'_>) {
        jail.env_key("s3.access_key", "access");
        jail.env_key("s3.secret_key", "secret");
        jail.env_key("s3.host", "s3.example.com");
        jail.env_key("s3.region", "eu-central-1");
    }

    /// The dialect, end to end: the prefix, the `__` nesting, and the defaults that fill in
    /// around what the environment supplied. `terrace-config` owns the layering and tests it;
    /// what this pins is that the service wires it to the names an operator actually sets.
    #[test]
    fn env_overrides_and_defaults_apply() {
        harness().run(|jail| {
            set_credentials(jail);
            jail.env_key("bucket.entries.docs.bucket", "media");
            jail.env_key("bucket.entries.docs.object", "handbook.pdf");
            jail.env_key("server.port", 9090);

            let config: Config = jail.load()?;

            // `SecretString` has no `PartialEq`; comparing requires `expose_secret()`.
            assert_eq!(config.s3().access_key().expose_secret(), "access");
            assert_eq!(*config.server().port(), 9090);
            // Untouched neighbour in the same block still gets its default.
            assert_eq!(config.server().host(), "0.0.0.0");
            // A block untouched by the environment still materialises with its own defaults.
            assert_eq!(config.telemetry().log_level(), "info");
            #[cfg(feature = "sentry")]
            assert!(!config.telemetry().sentry().enabled());

            let entry = config.bucket().entries().get("docs").expect("docs entry");
            assert_eq!(entry.bucket(), "media");
            assert_eq!(entry.object(), "handbook.pdf");
            Ok(())
        });
    }

    /// The bucket map is a real table in the TOML layer, which is the whole point of the file
    /// layers: the entries used to be squeezed into one `key:bucket,file; ...` string because
    /// an environment variable cannot carry a map.
    #[test]
    fn the_toml_layer_carries_the_bucket_table() {
        harness().run(|jail| {
            set_credentials(jail);
            // No path given: this also pins the `config.toml` fallback, which is what a container
            // with a `ConfigMap` mounted over its working directory uses.
            jail.config(
                r#"
[bucket.entries.docs]
bucket = "media"
object = "handbook.pdf"

[bucket.entries."release-notes"]
bucket = "media"
object = "CHANGELOG.md"
"#,
            )?;

            let config: Config = jail.load()?;

            assert_eq!(config.bucket().entries().len(), 2);
            // A key an environment variable could not have spelled: the environment layer
            // lowercases, and `-` is not expressible in a shell variable name.
            assert_eq!(
                config
                    .bucket()
                    .entries()
                    .get("release-notes")
                    .expect("release-notes entry")
                    .object(),
                "CHANGELOG.md"
            );
            Ok(())
        });
    }

    /// A mounted secret outranks the TOML layer, so a `ConfigMap` carrying a placeholder key
    /// cannot win over the `Secret` that carries the real one — through the variable names
    /// *this* crate configures, which is the half a dependency cannot pin.
    ///
    /// The mount has the shape a projected volume has — a generation directory and a `..data`
    /// entry beside the keys — rather than being a tidy directory of plain files. Which of the
    /// two a provider walks correctly is the difference between reading a `Secret` and booting on
    /// compiled defaults with a perfectly good one mounted beside you.
    #[test]
    fn a_secrets_directory_outranks_the_toml_layer() {
        harness().run(|jail| {
            jail.config(
                r#"
[s3]
access_key = "placeholder"
secret_key = "placeholder"
host = "s3.example.com"
region = "eu-central-1"

[bucket.entries.docs]
bucket = "media"
object = "handbook.pdf"
"#,
            )?;
            jail.secrets_volume()
                .file("s3__secret_key", "rotated\n")
                .projected()
                .create()?;

            let config: Config = jail.load()?;

            assert_eq!(config.s3().secret_key().expose_secret(), "rotated");
            // The key the mount said nothing about still comes from the TOML layer.
            assert_eq!(config.s3().access_key().expose_secret(), "placeholder");
            // The value alone would also be produced by a `config.toml` that happened to say
            // `rotated`. Which layer won is what this test is about.
            let explanation = jail.explain()?;
            let origin = explanation
                .origin("s3.secret_key")
                .expect("the mounted key is reported");
            assert!(
                matches!(origin.effective(), Layer::SecretsFile(_)),
                "the secret must come from the mount, not from {}",
                origin.effective()
            );
            Ok(())
        });
    }

    /// A single value can also be pointed at a file by name, which is the `_FILE` convention a
    /// Docker Compose stack spells rather than mounting a whole secrets directory.
    #[test]
    fn a_file_indirection_variable_supplies_one_key() {
        harness().run(|jail| {
            // Everything but the secret, which the indirection variable supplies instead.
            jail.env_key("s3.access_key", "access");
            jail.env_key("s3.host", "s3.example.com");
            jail.env_key("s3.region", "eu-central-1");
            jail.env_key("bucket.entries.docs.bucket", "media");
            jail.env_key("bucket.entries.docs.object", "handbook.pdf");

            // Writes the file *and* sets the variable naming it, both derived from the key: a
            // test spelling `S3_PERMA_LINK_S3__SECRET_KEY_FILE` out by hand would keep passing
            // after the suffix was renamed, while testing a variable nothing reads.
            jail.indirection("s3.secret_key", "from-a-file\n")?;

            let config: Config = jail.load()?;

            // Trailing newline trimmed: an editor or a `kubectl create secret --from-file`
            // adds one, and it is not part of the credential.
            assert_eq!(config.s3().secret_key().expose_secret(), "from-a-file");
            Ok(())
        });
    }

    /// One key supplied by both the environment and a mounted file fails the boot instead of
    /// being resolved by precedence: the environment variable is the one that cannot be
    /// rotated, so silently preferring either is how a service keeps serving on a credential
    /// its operator believes they replaced.
    #[test]
    fn a_key_supplied_twice_is_refused() {
        harness().run(|jail| {
            set_credentials(jail);
            jail.secret_key("s3.secret_key", "rotated")?;

            let error = jail
                .load::<Config>()
                .expect_err("a shadowed key must fail the boot");
            assert!(
                error.to_string().to_lowercase().contains("secret_key"),
                "the error must name the key: {error}"
            );
            Ok(())
        });
    }

    /// Every configuration struct is `#[serde(deny_unknown_fields)]`, so a key no field spells
    /// fails the boot instead of being dropped on the floor. The failure mode it removes is the
    /// quiet one: a typo used to load, take the compiled default, and leave the operator reading
    /// a value they believe they set.
    ///
    /// Spelled against a nested block rather than the root, because that is the level a
    /// misspelling actually happens at — and it is the same attribute on `BucketEntry` that the
    /// generated contract publishes as `additionalProperties: false`, which is what a chart
    /// validating against it enforces one step earlier.
    #[test]
    fn an_undeclared_key_is_refused() {
        harness().run(|jail| {
            set_credentials(jail);
            jail.config(
                r#"
[bucket.entries.docs]
bucket = "media"
objekt = "handbook.pdf"
"#,
            )?;

            let error = jail
                .load::<Config>()
                .expect_err("an undeclared key must fail the boot");
            assert!(
                error.to_string().contains("objekt"),
                "the error must name the key: {error}"
            );
            Ok(())
        });
    }

    /// `telemetry.log_level` takes more spellings than the five it names, which is why the
    /// schema publishes no list of values for it.
    ///
    /// The string is deserialised verbatim and handed to `tracing`'s parser, not `serde`'s: it
    /// folds ASCII case, and it reads anything `usize` parses as `1` to `5` — leading zeros and
    /// a leading `+` included. Every spelling below is one a document may carry today, so the
    /// obvious annotation (`values("trace", "debug", "info", "warn", "error")`) would publish a
    /// schema rejecting a deployment that runs. `config::tests::the_log_level_publishes_no_list_of_values`
    /// holds the other half: that the key stays unannotated.
    ///
    /// Spelled against the document layer because that is the layer a value list would be
    /// checked against. The environment layer accepts less: its text is TOML-parsed before the
    /// field sees it, so `…LOG_LEVEL=3` arrives as an integer and is refused for the type it is
    /// rather than the level it names — which the second half of this test pins, so the day it
    /// changes is not the day the reference is quietly wrong about it.
    #[test]
    fn the_log_level_takes_more_than_the_five_names() {
        harness().run(|jail| {
            set_credentials(jail);

            for spelling in ["info", "INFO", "Info", "3", "+003"] {
                jail.config(format!(
                    r#"
[telemetry]
log_level = "{spelling}"

[bucket.entries.docs]
bucket = "media"
object = "handbook.pdf"
"#
                ))?;

                let config: Config = jail.load()?;
                assert_eq!(
                    config
                        .telemetry()
                        .level()
                        .expect("the parser takes this spelling"),
                    Level::INFO,
                    "`{spelling}` names the level `info` names"
                );
            }

            // The one place the two layers disagree, and the reason the document is what the
            // paragraph above is measured against.
            jail.env_key("telemetry.log_level", 3);
            let refused = jail
                .load::<Config>()
                .expect_err("an integer is not a level the environment layer can carry");
            assert!(
                refused.to_string().contains("expected a string"),
                "the refusal must be about the type, not the level: {refused}"
            );
            Ok(())
        });
    }

    /// The report `main` logs once per generation, on the case it exists for: a key the boot
    /// refuses. A report that named the mount but not the variable shadowing it would leave the
    /// operator exactly where they started.
    #[test]
    fn the_explanation_names_a_contested_key() {
        harness().run(|jail| {
            set_credentials(jail);
            jail.secret_key("s3.secret_key", "rotated")?;

            let explanation = jail.explain()?;
            let contested: Vec<&str> = explanation.contested().map(Origin::key).collect();
            assert_eq!(contested, ["s3.secret_key"]);

            // Assembled under `LastWins` whatever policy is set, so the configuration the test
            // above watches `load` refuse is still explainable — which is the moment it is
            // wanted. Both sources are named, not just the winner.
            let report = explanation.to_string();
            assert!(report.contains("S3_PERMA_LINK_S3__SECRET_KEY"), "{report}");
            assert!(report.contains("s3__secret_key"), "{report}");
            Ok(())
        });
    }
}
