use actix_web::http::Method;
use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Tracing error")]
    Logger(#[from] tracing::metadata::ParseLevelError),
    #[error("IO error")]
    IoError(#[from] std::io::Error),
    #[error("Invalid route")]
    InvalidRoute(String),
    #[error("Config error")]
    Config(#[from] config::ConfigError),
    #[error("Bukkit error")]
    Minio(#[from] minio_rsc::errors::ValueError),
    #[error("{0}")]
    Custom(String),
}

impl Error {
    pub fn custom<S: ToString>(msg: S) -> Self {
        Self::Custom(msg.to_string())
    }

    pub fn invalid_route(route: &Method) -> Self {
        Self::InvalidRoute(route.to_string())
    }
}
