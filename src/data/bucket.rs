use crate::config::BucketEntry;
use derive_new::new;
use minio_rsc::Minio;
use std::collections::HashMap;

#[derive(Getters, new)]
#[getset(get = "pub")]
pub struct DownloadData {
    s3: Minio,
    bucket_config: HashMap<String, BucketEntry>,
}

impl DownloadData {
    pub fn get_entry(&self, key: &str) -> Option<&BucketEntry> {
        self.bucket_config.get(key)
    }
}
