use crate::data::DownloadData;
use actix_web::{web, HttpResponse};
use minio_rsc::types::args::ObjectArgs;

pub fn get_config(cfg: &mut web::ServiceConfig) {
    cfg.service(web::resource("{tail:.*}").route(web::get().to(download)));
}

async fn download(
    path: web::Path<String>,
    download_data: web::Data<DownloadData>,
) -> core::result::Result<HttpResponse, actix_web::Error> {
    // Only allow specific paths
    info!("Received request for path: {}", path);
    match download_data.get_entry(&path) {
        Some(bucket) => {
            info!("Valid path request!");

            let minio = download_data.s3();
            match minio
                .get_object(ObjectArgs::new(bucket.bucket(), bucket.file()))
                .await
            {
                Ok(file) => Ok(HttpResponse::Ok().streaming(file.bytes_stream())),
                Err(e) => {
                    error!("Failed to download file from bucket {}", e);
                    Ok(HttpResponse::InternalServerError().finish())
                }
            }
        }
        None => Ok(HttpResponse::NotFound().finish()),
    }
}
