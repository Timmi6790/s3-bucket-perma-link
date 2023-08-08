use s3_bucket_perma_link::config::Config;
use s3_bucket_perma_link::data::DownloadData;
use sentry::ClientInitGuard;
use std::env;
use std::str::FromStr;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{filter, Layer};

use s3_bucket_perma_link::server::Server;
use s3_bucket_perma_link::Result;

#[macro_use]
extern crate tracing;

const ENV_SENTRY_DSN: &str = "SENTRY_DSN";
const ENV_LOG_LEVEL: &str = "LOG_LEVEL";

const DEFAULT_LOG_LEVEL: &str = "info";

#[tokio::main]
async fn main() -> Result<()> {
    setup_tracing()?;

    // Prevents the process from exiting until all events are sent
    let _sentry = setup_sentry();

    let server;
    let download_data;
    {
        let config = Config::get_configuration()?;

        server = Server::new(config.server().host().to_string(), *config.server().port());

        let minio = config.s3().create_minio()?;
        download_data = DownloadData::new(minio, config.bucket().entries().clone());
    }

    server.run_until_stopped(download_data).await?;

    Ok(())
}

fn setup_tracing() -> Result<()> {
    let level = env::var(ENV_LOG_LEVEL).unwrap_or_else(|_| DEFAULT_LOG_LEVEL.to_string());
    let level = tracing::Level::from_str(&level)?;

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_filter(filter::LevelFilter::from_level(level)))
        .init();

    Ok(())
}

fn setup_sentry() -> Option<ClientInitGuard> {
    match env::var(ENV_SENTRY_DSN) {
        Ok(dns) => Some(sentry::init((
            dns,
            sentry::ClientOptions {
                release: sentry::release_name!(),
                attach_stacktrace: true,
                ..Default::default()
            },
        ))),
        Err(_) => {
            info!("{ENV_SENTRY_DSN} not set, skipping Sentry setup");
            return None;
        }
    }
}
