use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use anyhow::Error;
use getrandom::getrandom;
use std::path::PathBuf;
use serde::{Serialize, Deserialize};
use tokio_postgres::Statement;

#[derive(Serialize, Deserialize)]
pub struct AppConfig {
    pub mirror_path: PathBuf,
    pub static_content_path: PathBuf,
    pub listen: ListenConfig,
    pub auth_secret: String,
    pub database_host: String,
    pub database_user: String,
    pub database_password: String,
    pub database_name: String,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut buffer: Vec<u8> = (0..32).map(|_| 0u8).collect();
        getrandom(&mut buffer[..]).unwrap();
        Self {
            mirror_path: PathBuf::from("./mirror"),
            static_content_path: PathBuf::from("./static"),
            listen: ListenConfig::default(),
            auth_secret: URL_SAFE_NO_PAD.encode(buffer),
            database_host: String::from("localhost"),
            database_user: String::from("sponsortimes"),
            database_password: String::from(""),
            database_name: String::from("sponsortimes"),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct ListenConfig {
    pub tcp: Option<(String, u16)>,
    pub unix: Option<String>,
    pub unix_mode: Option<u32>,
}

impl Default for ListenConfig {
    fn default() -> Self {
        Self {
            tcp: Some(("0.0.0.0".to_owned(), 9292)),
            unix: None,
            unix_mode: None,
        }
    }
}

pub struct PreparedQueries {
    pub index_titles: Statement,
    pub uuid_titles: Statement,
    pub video_titles: Statement,
    pub user_titles: Statement,

    pub index_thumbnails: Statement,
    pub uuid_thumbnails: Statement,
    pub video_thumbnails: Statement,
    pub user_thumbnails: Statement,
}

pub struct DatabaseState {
    pub db: tokio_postgres::Client,
    pub statements: PreparedQueries,
    pub last_error: Option<Error>,
    pub last_updated: i64,
    pub last_modified: i64,
    pub updating_now: bool,
}

