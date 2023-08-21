use std::sync::RwLock;

use actix_web::{CustomizeResponder, get, http::StatusCode, HttpResponse, post, Responder, rt::task::spawn_blocking, web};
use anyhow::{anyhow, bail};
use chrono::Utc;
use serde::Deserialize;

use dearrow_browser_api::*;

use crate::{state::*, utils};

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(helo)
       .service(get_random_titles)
       .service(get_unverified_titles)
       .service(get_title_by_uuid)
       .service(get_titles_by_video_id)
       .service(get_titles_by_user_id)
       .service(get_random_thumbnails)
       .service(get_thumbnail_by_uuid)
       .service(get_thumbnails_by_video_id)
       .service(get_thumbnails_by_user_id)
       .service(get_status)
       .service(request_reload);;
}

type JsonResult<T> = utils::Result<web::Json<T>>;
type CustomizedJsonResult<T> = utils::Result<CustomizeResponder<web::Json<T>>>;

#[get("/")]
async fn helo() -> impl Responder {
    "hello"
}

#[get("/status")]
async fn get_status(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<StatusResponse> {
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    Ok(web::Json(StatusResponse {
        last_updated: db.last_updated,
        last_modified: db.last_modified,
        updating_now: db.updating_now,
        // titles: db.csv_db.titles.len(),
        // thumbnails: db.csv_db.thumbnails.len(),
        // vip_users: db.csv_db.vip_users.len(),
        // usernames: db.csv_db.usernames.len(),
        last_error: db.last_error.as_ref().map(|e| format!("{e:?}")),
    }))
}

#[derive(Deserialize, Debug)]
struct Auth {
    auth: Option<String>
}

fn do_reload(db_lock: web::Data<RwLock<DatabaseState>>, config: web::Data<AppConfig>) -> anyhow::Result<()> {
    {
        let mut db_state = db_lock.write().map_err(|_| anyhow!("Failed to acquire DatabaseState for writing"))?;
        if db_state.updating_now {
            bail!("Already updating!");
        }
        db_state.updating_now = true;
    }
    let last_updated = Utc::now().timestamp_millis();
    let last_modified = utils::get_mtime(&config.mirror_path.join("last-modified"));
    {
        let mut db_state = db_lock.write().map_err(|_| anyhow!("Failed to acquire DatabaseState for writing"))?;
        db_state.last_updated = last_updated;
        db_state.last_modified = last_modified;
        db_state.updating_now = false;
    }
    Ok(())
}

#[post("/reload")]
async fn request_reload(db_lock: web::Data<RwLock<DatabaseState>>, config: web::Data<AppConfig>, auth: web::Query<Auth>) -> HttpResponse {
    let provided_hash = match auth.auth.as_deref() {
        None => { return HttpResponse::NotFound().finish(); },
        Some(s) => utils::sha256(s),
    };
    let actual_hash = utils::sha256(config.auth_secret.as_str());

    if provided_hash != actual_hash {
        return HttpResponse::Forbidden().finish();
    }
    match spawn_blocking(move || do_reload(db_lock, config)).await {
        Ok(..) => HttpResponse::Ok().body("Reload complete"),
        Err(e) => HttpResponse::InternalServerError().body(format!("{e:?}")),
    }
}

#[get("/titles")]
async fn get_random_titles(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<Vec<ApiTitle>> {
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let titles = db.db.query(&db.statements.index_titles, &[]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?.into_iter()
        .map(utils::parse_api_title_from_database_row).collect();
    Ok(web::Json(titles))
}

#[get("/titles/unverified")]
async fn get_unverified_titles(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<Vec<ApiTitle>> {
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    Ok(web::Json(
        db.db.titles.values()
            .filter(|t| t.flags.contains(TitleFlags::Unverified) && !t.flags.intersects(TitleFlags::Locked | TitleFlags::ShadowHidden))
            .map(|t| t.into_with_db(&db.db)).collect()
    ))
}

#[get("/titles/uuid/{uuid}")]
async fn get_title_by_uuid(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> JsonResult<ApiTitle> {
    let uuid = path.into_inner();
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let mut rows = db.db.query(&db.statements.uuid_titles, &[&uuid]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?;

    if rows.is_empty() {
        Err(utils::Error::EmptyStatus(StatusCode::NOT_FOUND))
    } else {
        Ok(web::Json(utils::parse_api_title_from_database_row(rows.swap_remove(0))))
    }
}

#[get("/titles/video_id/{video_id}")]
async fn get_titles_by_video_id(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiTitle>> {
    let video_id = path.into_inner();
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let titles: Vec<_> = db.db.query(&db.statements.video_titles, &[&video_id]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?.into_iter()
        .map(utils::parse_api_title_from_database_row).collect();
    let status = if titles.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    Ok(web::Json(titles).customize().with_status(status))
}

#[get("/titles/user_id/{user_id}")]
async fn get_titles_by_user_id(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiTitle>> {
    let user_id = path.into_inner();
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let titles: Vec<_> = db.db.query(&db.statements.user_titles, &[&user_id]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?.into_iter()
        .map(utils::parse_api_title_from_database_row).collect();
    let status = if titles.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    Ok(web::Json(titles).customize().with_status(status))
}

#[get("/thumbnails")]
async fn get_random_thumbnails(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<Vec<ApiThumbnail>> {
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let thumbnails = db.db.query(&db.statements.index_thumbnails, &[]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?.into_iter()
        .map(utils::parse_api_thumbnail_from_database_row).collect();
    Ok(web::Json(thumbnails))
}

#[get("/thumbnails/uuid/{uuid}")]
async fn get_thumbnail_by_uuid(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> JsonResult<ApiThumbnail> {
    let uuid = path.into_inner();
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let mut rows = db.db.query(&db.statements.uuid_thumbnails, &[&uuid]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?;

    if rows.is_empty() {
        Err(utils::Error::EmptyStatus(StatusCode::NOT_FOUND))
    } else {
        Ok(web::Json(utils::parse_api_thumbnail_from_database_row(rows.swap_remove(0))))
    }
}

#[get("/thumbnails/video_id/{video_id}")]
async fn get_thumbnails_by_video_id(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiThumbnail>> {
    let video_id = path.into_inner();
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let thumbnails: Vec<_> = db.db.query(&db.statements.video_thumbnails, &[&video_id]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?.into_iter()
        .map(utils::parse_api_thumbnail_from_database_row).collect();
    let status = if thumbnails.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    Ok(web::Json(thumbnails).customize().with_status(status))
}

#[get("/thumbnails/user_id/{video_id}")]
async fn get_thumbnails_by_user_id(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiThumbnail>> {
    let user_id = path.into_inner();
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    let thumbnails: Vec<_> = db.db.query(&db.statements.user_thumbnails, &[&user_id]).await.map_err(|err| anyhow!("Failed to query database: {}", err))?.into_iter()
        .map(utils::parse_api_thumbnail_from_database_row).collect();
    let status = if thumbnails.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    Ok(web::Json(thumbnails).customize().with_status(status))
}
