use std::{fmt::{Debug, Display}, path::Path, fs, time::UNIX_EPOCH};
use std::sync::Arc;

use actix_web::{ResponseError, http::{StatusCode, header::ContentType}, HttpResponse};
use sha2::{Sha256, Digest, digest::{typenum::U32, generic_array::GenericArray}};
use tokio_postgres::Row;
use dearrow_browser_api::{ApiThumbnail, ApiTitle};

pub enum Error {
    Anyhow(anyhow::Error),
    EmptyStatus(StatusCode),
}

impl Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Anyhow(ref err) => Debug::fmt(err, f),
            Error::EmptyStatus(status) => f.debug_tuple("Error::EmptyStatus").field(status).finish(),
        }
    }
}
impl Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Anyhow(ref err) => Display::fmt(err, f),
            Error::EmptyStatus(status) => write!(f, "{status}"),
        }
    }
}
impl From<anyhow::Error> for Error {
    fn from(value: anyhow::Error) -> Self {
        Error::Anyhow(value)
    }
}
impl std::error::Error for Error {}
impl ResponseError for Error {
    fn status_code(&self) -> StatusCode {
        match self {
            Error::Anyhow(..) => StatusCode::INTERNAL_SERVER_ERROR,
            Error::EmptyStatus(status) => *status,
        }
    }

    fn error_response(&self) -> HttpResponse {
        let mut builder = HttpResponse::build(self.status_code());
        match self {
            Error::Anyhow(err) => builder.insert_header(ContentType::plaintext()).body(format!("{err:?}")),
            Error::EmptyStatus(..) => builder.finish(),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn sha256(s: &str) -> GenericArray<u8, U32> {
    let mut hasher = Sha256::new();
    hasher.update(s);
    hasher.finalize()
}

pub fn get_mtime(p: &Path) -> i64 {
    fs::metadata(p).ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| d.as_millis().try_into().ok())
        .unwrap_or(0)
}

pub fn parse_api_title_from_database_row(row: Row) -> ApiTitle {
    ApiTitle {
        uuid: Arc::from(row.get::<usize, String>(0)),
        video_id: Arc::from(row.get::<usize, String>(1)),
        title: Arc::from(row.get::<usize, String>(2)),
        user_id: Arc::from(row.get::<usize, String>(3)),
        time_submitted: row.get::<usize, i64>(4),
        votes: row.get::<usize, i32>(5),
        original: row.get::<usize, i32>(6) != 0,
        locked: row.get::<usize, i32>(7) != 0,
        shadow_hidden: row.get::<usize, i32>(8) != 0,
        unverified: row.get::<usize, i32>(9) != 0,
        score: (row.get::<usize, i32>(5) + row.get::<usize, i32>(9)),
        username: row.get::<usize, Option<String>>(10).map(Arc::from),
        vip: row.get::<usize, Option<String>>(11).is_some(),
    }
}

pub fn parse_api_thumbnail_from_database_row(row: Row) -> ApiThumbnail {
    ApiThumbnail {
        uuid: Arc::from(row.get::<usize, String>(0)),
        video_id: Arc::from(row.get::<usize, String>(1)),
        user_id: Arc::from(row.get::<usize, String>(2)),
        time_submitted: row.get::<usize, i64>(3),
        timestamp: row.get::<usize, Option<f64>>(4),
        votes: row.get::<usize, i32>(5),
        original: row.get::<usize, i32>(6) != 0,
        locked: row.get::<usize, i32>(7) != 0,
        shadow_hidden: row.get::<usize, i32>(8) != 0,
        username: row.get::<usize, Option<String>>(9).map(Arc::from),
        vip: row.get::<usize, Option<String>>(10).is_some(),
    }
}
