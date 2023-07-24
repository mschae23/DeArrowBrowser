use std::sync::{RwLock, Arc};
use actix_web::{Responder, get, post, web, http::StatusCode, CustomizeResponder, HttpResponse, rt::task::spawn_blocking};
use anyhow::{anyhow, bail};
use chrono::Utc;
use dearrow_parser::{StringSet, DearrowDB};
use dearrow_browser_api::*;
use serde::Deserialize;

use crate::{utils::{self, MapInto}, state::*};

pub fn configure_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(helo)
       .service(get_random_titles)
       .service(get_title_by_uuid)
       .service(get_titles_by_video_id)
       .service(get_titles_by_user_id)
       .service(get_random_thumbnails)
       .service(get_thumbnail_by_uuid)
       .service(get_thumbnails_by_video_id)
       .service(get_thumbnails_by_user_id)
       .service(get_status)
       .service(get_errors)
       .service(request_reload);
}

type JsonResult<T> = utils::Result<web::Json<T>>;
type CustomizedJsonResult<T> = utils::Result<CustomizeResponder<web::Json<T>>>;

#[get("/")]
async fn helo() -> impl Responder {
    "hi"
}

#[get("/status")]
async fn get_status(db_lock: web::Data<RwLock<DatabaseState>>, string_set: web::Data<RwLock<StringSet>>) -> JsonResult<StatusResponse> {
    let strings = match string_set.try_read() {
        Err(_) => None,
        Ok(set) => Some(set.set.len()),
    };
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    Ok(web::Json(StatusResponse {
        last_updated: db.last_updated,
        last_modified: db.last_modified,
        updating_now: db.updating_now,
        titles: db.db.titles.len(),
        thumbnails: db.db.thumbnails.len(),
        errors: db.errors.len(),
        last_error: db.last_error.as_ref().map(|e| format!("{e:?}")),
        string_count: strings,
    }))
}

#[derive(Deserialize, Debug)]
struct Auth {
    auth: Option<String>
}

fn do_reload(db_lock: web::Data<RwLock<DatabaseState>>, string_set_lock: web::Data<RwLock<StringSet>>, config: web::Data<AppConfig>) -> anyhow::Result<()> {
    {
        let mut db_state = db_lock.write().map_err(|_| anyhow!("Failed to acquire DatabaseState for writing"))?;
        if db_state.updating_now {
            bail!("Already updating!");
        }
        db_state.updating_now = true;
    }
    let mut string_set_clone = string_set_lock.read().map_err(|_| anyhow!("Failed to acquire StringSet for reading"))?.clone();
    let (new_db, errors) = DearrowDB::load_dir(config.mirror_path.as_path(), &mut string_set_clone)?;
    let last_updated = Utc::now().timestamp_millis();
    let last_modified = utils::get_mtime(&config.mirror_path.join("titles.csv"));
    {
        let mut string_set = string_set_lock.write().map_err(|_| anyhow!("Failed to acquire StringSet for writing"))?;
        let mut db_state = db_lock.write().map_err(|_| anyhow!("Failed to acquire DatabaseState for writing"))?;
        *string_set = string_set_clone;
        db_state.db = new_db;
        db_state.errors = errors.into();
        db_state.last_updated = last_updated;
        db_state.last_modified = last_modified;
        db_state.updating_now = false;
        string_set.clean();
    }
    Ok(())
}

#[post("/reload")]
async fn request_reload(db_lock: web::Data<RwLock<DatabaseState>>, string_set_lock: web::Data<RwLock<StringSet>>, config: web::Data<AppConfig>, auth: web::Query<Auth>) -> HttpResponse {
    let provided_hash = match auth.auth.as_deref() {
        None => { return HttpResponse::NotFound().finish(); },
        Some(s) => utils::sha256(s),
    };
    let actual_hash = utils::sha256(config.auth_secret.as_str());

    if provided_hash != actual_hash {
        return HttpResponse::Forbidden().finish();
    }
    match spawn_blocking(move || do_reload(db_lock, string_set_lock, config)).await {
        Ok(..) => HttpResponse::Ok().finish(),
        Err(e) => HttpResponse::InternalServerError().body(format!("{e:?}")),
    }
}

#[get("/errors")]
async fn get_errors(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<ErrorList> {
    let db = db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?;
    Ok(web::Json(db.errors.iter().map(|e| format!("{e:?}")).collect()))
}

#[get("/titles")]
async fn get_random_titles(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<Vec<ApiTitle>> {
    Ok(web::Json(
        db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.titles_by_time_submitted.iter().rev().take(20)
            .map(Into::into).collect()
    ))
}

#[get("/titles/uuid/{uuid}")]
async fn get_title_by_uuid(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> JsonResult<ApiTitle> {
    let uuid = path.into_inner();
    Ok(web::Json(
        db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.titles.get(uuid.as_str()).map_into()
            .ok_or(utils::Error::EmptyStatus(StatusCode::NOT_FOUND))?
    ))
}

#[get("/titles/video_id/{video_id}")]
async fn get_titles_by_video_id(db_lock: web::Data<RwLock<DatabaseState>>, string_set: web::Data<RwLock<StringSet>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiTitle>> {
    let video_id = string_set.read().map_err(|_| anyhow!("Failed to acquire StringSet for reading"))?
        .set.get(path.into_inner().as_str()).cloned();
    let mut titles: Vec<ApiTitle> = match video_id {
        None => vec![],
        Some(id) => db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.titles.values()
            .filter(|title| Arc::ptr_eq(&title.video_id, &id))
            .map(Into::into)
            .collect(),
    };
    let status = if titles.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    titles.sort_by(|t1, t2| t1.time_submitted.cmp(&t2.time_submitted).reverse());
    Ok(web::Json(titles).customize().with_status(status))
}

#[get("/titles/user_id/{user_id}")]
async fn get_titles_by_user_id(db_lock: web::Data<RwLock<DatabaseState>>, string_set: web::Data<RwLock<StringSet>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiTitle>> {
    let user_id = string_set.read().map_err(|_| anyhow!("Failed to acquire StringSet for reading"))?
        .set.get(path.into_inner().as_str()).cloned();
    let mut titles: Vec<ApiTitle> = match user_id {
        None => vec![],
        Some(id) => db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.titles.values()
            .filter(|title| Arc::ptr_eq(&title.user_id, &id))
            .map(Into::into)
            .collect(),
    };
    let status = if titles.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    titles.sort_by(|t1, t2| t1.time_submitted.cmp(&t2.time_submitted).reverse());
    Ok(web::Json(titles).customize().with_status(status))
}

#[get("/thumbnails")]
async fn get_random_thumbnails(db_lock: web::Data<RwLock<DatabaseState>>) -> JsonResult<Vec<ApiThumbnail>> {
    Ok(web::Json(
        db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.thumbnails_by_time_submitted.iter().rev().take(20)
            .map(Into::into).collect()
    ))
}

#[get("/thumbnails/uuid/{uuid}")]
async fn get_thumbnail_by_uuid(db_lock: web::Data<RwLock<DatabaseState>>, path: web::Path<String>) -> JsonResult<ApiThumbnail> {
    let uuid = path.into_inner();
    Ok(web::Json(
        db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.thumbnails.get(uuid.as_str()).map_into()
            .ok_or(utils::Error::EmptyStatus(StatusCode::NOT_FOUND))?
    ))
}

#[get("/thumbnails/video_id/{video_id}")]
async fn get_thumbnails_by_video_id(db_lock: web::Data<RwLock<DatabaseState>>, string_set: web::Data<RwLock<StringSet>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiThumbnail>> {
    let video_id = string_set.read().map_err(|_| anyhow!("Failed to acquire StringSet for reading"))?
        .set.get(path.into_inner().as_str()).cloned();
    let mut titles: Vec<ApiThumbnail> = match video_id {
        None => vec![],
        Some(id) => db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.thumbnails.values()
            .filter(|thumb| Arc::ptr_eq(&thumb.video_id, &id))
            .map(Into::into)
            .collect(),
    };
    let status = if titles.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    titles.sort_by(|t1, t2| t1.time_submitted.cmp(&t2.time_submitted).reverse());
    Ok(web::Json(titles).customize().with_status(status))
}

#[get("/thumbnails/user_id/{video_id}")]
async fn get_thumbnails_by_user_id(db_lock: web::Data<RwLock<DatabaseState>>, string_set: web::Data<RwLock<StringSet>>, path: web::Path<String>) -> CustomizedJsonResult<Vec<ApiThumbnail>> {
    let user_id = string_set.read().map_err(|_| anyhow!("Failed to acquire StringSet for reading"))?
        .set.get(path.into_inner().as_str()).cloned();
    let mut titles: Vec<ApiThumbnail> = match user_id {
        None => vec![],
        Some(id) => db_lock.read().map_err(|_| anyhow!("Failed to acquire DatabaseState for reading"))?
            .db.thumbnails.values()
            .filter(|thumb| Arc::ptr_eq(&thumb.user_id, &id))
            .map(Into::into)
            .collect(),
    };
    let status = if titles.is_empty() {
        StatusCode::NOT_FOUND
    } else {
        StatusCode::OK
    };
    titles.sort_by(|t1, t2| t1.time_submitted.cmp(&t2.time_submitted).reverse());
    Ok(web::Json(titles).customize().with_status(status))
}
