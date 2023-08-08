use derive_new::new;
use minio_rsc::provider::StaticProvider;
use minio_rsc::Minio;
use secrecy::{ExposeSecret, Secret};
use serde::{Deserialize, Deserializer};
use std::collections::HashMap;

use crate::error::Error;

const DEFAULT_SERVER_HOST: &str = "0.0.0.0";
const DEFAULT_SERVER_PORT: u16 = 8080;

#[derive(Debug, serde::Deserialize, Getters)]
#[getset(get = "pub")]
pub struct Config {
    server: ServerConfig,
    s3: S3Config,
    bucket: BucketConfig,
}

#[derive(Debug, serde::Deserialize, Getters)]
#[getset(get = "pub")]
pub struct S3Config {
    access_key: Secret<String>,
    secret_key: Secret<String>,
    host: String,
    region: String,
}

#[derive(Debug, serde::Deserialize, Getters)]
#[getset(get = "pub")]
pub struct ServerConfig {
    host: String,
    port: u16,
}

#[derive(Debug, serde::Deserialize, Getters)]
#[getset(get = "pub")]
pub struct BucketConfig {
    #[serde(deserialize_with = "deserialize_buckets_from_string")]
    entries: HashMap<String, BucketEntry>,
}

#[derive(Debug, Clone, serde::Deserialize, Getters, new)]
#[getset(get = "pub")]
pub struct BucketEntry {
    bucket: String,
    file: String,
}

impl Config {
    pub fn get_configuration() -> crate::Result<Self> {
        config::Config::builder()
            .add_source(config::Environment::default().try_parsing(true))
            .set_default("server.host", DEFAULT_SERVER_HOST)?
            .set_default("server.port", DEFAULT_SERVER_PORT)?
            .build()
            .map_err(|e| Error::custom(format!("Can't parse config: {e}")))?
            .try_deserialize::<Config>()
            .map_err(|e| Error::custom(format!("Failed to deserialize configuration: {e}")))
    }
}

impl S3Config {
    pub fn create_minio(&self) -> crate::Result<Minio> {
        let provider = StaticProvider::new(
            self.access_key.expose_secret(),
            self.secret_key.expose_secret(),
            None,
        );

        info!(
            "Creating s3 client for {} and region {}",
            self.host, self.region
        );
        let minio = Minio::builder()
            .host(self.host.clone())
            .region(self.region.clone())
            .provider(provider)
            .secure(true)
            .build()?;

        Ok(minio)
    }
}
pub fn deserialize_buckets_from_string<'de, D>(
    deserializer: D,
) -> Result<HashMap<String, BucketEntry>, D::Error>
where
    D: Deserializer<'de>,
{
    let string: String = Deserialize::deserialize(deserializer)?;

    let values: Result<HashMap<String, BucketEntry>, D::Error> = string
        .split("; ")
        .map(|s| {
            let mut split = s.split(':');

            let identifier = match split.next() {
                Some(s) => Ok(s),
                None => Err(serde::de::Error::custom("Identifier is missing")),
            }?;

            let bucket_data = match split.next() {
                Some(s) => Ok(s),
                None => Err(serde::de::Error::custom("BucketData is missing")),
            }?;

            let mut raw_bucket_entry = bucket_data.split(',');

            let bucket = match raw_bucket_entry.next() {
                Some(s) => Ok(s),
                None => Err(serde::de::Error::custom("Bucket is missing")),
            }?;
            let file = match raw_bucket_entry.next() {
                Some(s) => Ok(s),
                None => Err(serde::de::Error::custom("Bucket file is missing")),
            }?;

            Ok((
                identifier.to_string(),
                BucketEntry::new(bucket.to_string(), file.to_string()),
            ))
        })
        .collect();

    values
}
