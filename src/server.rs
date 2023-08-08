use crate::data::DownloadData;
use crate::routes::{download, health_check};
use crate::Result;
use actix_web::{web, App, HttpServer};
use derive_new::new;
use tracing_actix_web::TracingLogger;

#[derive(new)]
pub struct Server {
    host: String,
    port: u16,
}

impl Server {
    pub async fn run_until_stopped(&self, download_data: DownloadData) -> Result<()> {
        info!("Starting server on {}:{}", self.host, self.port,);

        let download_data = web::Data::new(download_data);
        let server = HttpServer::new(move || {
            App::new()
                .wrap(TracingLogger::default())
                .app_data(download_data.clone())
                .configure(health_check::get_config)
                .configure(download::get_config)
        })
        .bind((self.host.clone(), self.port))?;

        server.run().await?;

        Ok(())
    }
}
