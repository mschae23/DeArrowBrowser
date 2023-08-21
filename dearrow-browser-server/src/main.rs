use std::{fs::{File, Permissions, set_permissions}, io::{self, Read, Write}, os::unix::prelude::PermissionsExt, sync::RwLock};

use actix_files::{Files, NamedFile};
use actix_web::{App, dev::{fn_service, ServiceRequest, ServiceResponse}, HttpServer, web};
use anyhow::{bail, Context};
use chrono::Utc;
use tokio_postgres::NoTls;

use state::*;

mod utils;
mod routes;
mod state;

const CONFIG_PATH: &str = "config.toml";

#[actix_web::main]
async fn main() -> anyhow::Result<()> {
    let config: web::Data<AppConfig> = web::Data::new(match File::open(CONFIG_PATH) {
        Ok(mut file) => {
            let mut contents = String::new();
            file.read_to_string(&mut contents).with_context(|| format!("Failed to read {CONFIG_PATH}"))?;
            let cfg: AppConfig = toml::from_str(&contents).with_context(|| format!("Failed to deserialize contents of {CONFIG_PATH}"))?;
            if cfg.listen.tcp.is_none() && cfg.listen.unix.is_none() {
                bail!("Invalid configuration - no tcp port or unix socket path specified");
            }
            cfg
        },
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let cfg = AppConfig::default();
            let serialized = toml::to_string(&cfg).context("Failed to serialize default AppConfig as TOML")?;
            let mut file = File::options().write(true).create_new(true).open(CONFIG_PATH).with_context(|| format!("Failed to create {CONFIG_PATH}"))?;
            write!(file, "{serialized}").with_context(|| format!("Failed to write serialized default AppConfig to {CONFIG_PATH}"))?;
            cfg
        },
        Err(e) => {
            return Err(e).context(format!("Failed to open {CONFIG_PATH}"));
        }
    });
    let db: web::Data<RwLock<DatabaseState>> = {
        // Connect to the database.
        let (client, connection) =
            tokio_postgres::Config::new()
                .host(&config.database.host).user(&config.database.user).password(&config.database.password).dbname(&config.database.name)
                .connect(NoTls).await?;

        // The connection object performs the actual communication with the database,
        // so spawn it off to run on its own.
        actix_web::rt::spawn(async move {
            if let Err(e) = connection.await {
                eprintln!("Connection error: {}", e);
            }
        });

        let index_titles = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"title\", t.\"userID\", t.\"timeSubmitted\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", tv.\"verification\", u.\"userName\", v.\"userID\" from \"titles\" t join \"titleVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" order by t.\"timeSubmitted\" desc limit 50")
            .await?;

        let unverified_titles = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"title\", t.\"userID\", t.\"timeSubmitted\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", tv.\"verification\", u.\"userName\", v.\"userID\" from \"titles\" t join \"titleVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where tv.\"verification\" = -1 and tv.\"locked\" != 1 and tv.\"shadowHidden\" != 1 order by t.\"timeSubmitted\"")
            .await?;

        let uuid_titles = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"title\", t.\"userID\", t.\"timeSubmitted\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", tv.\"verification\", u.\"userName\", v.\"userID\" from \"titles\" t join \"titleVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where t.\"UUID\" = $1 limit 1")
            .await?;

        let video_titles = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"title\", t.\"userID\", t.\"timeSubmitted\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", tv.\"verification\", u.\"userName\", v.\"userID\" from \"titles\" t join \"titleVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where t.\"videoID\" = $1 order by t.\"timeSubmitted\"")
            .await?;

        let user_titles = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"title\", t.\"userID\", t.\"timeSubmitted\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", tv.\"verification\", u.\"userName\", v.\"userID\" from \"titles\" t join \"titleVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where t.\"userID\" = $1 order by t.\"timeSubmitted\"")
            .await?;

        let index_thumbnails = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"userID\", t.\"timeSubmitted\", tt.\"timestamp\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", u.\"userName\", v.\"userID\" from \"thumbnails\" t join \"thumbnailVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"thumbnailTimestamps\" tt on t.\"UUID\" = tt.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" order by t.\"timeSubmitted\" desc limit 50")
            .await?;

        let uuid_thumbnails = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"userID\", t.\"timeSubmitted\", tt.\"timestamp\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", u.\"userName\", v.\"userID\" from \"thumbnails\" t join \"thumbnailVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"thumbnailTimestamps\" tt on t.\"UUID\" = tt.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where t.\"UUID\" = $1 limit 1")
            .await?;

        let video_thumbnails = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"userID\", t.\"timeSubmitted\", tt.\"timestamp\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", u.\"userName\", v.\"userID\" from \"thumbnails\" t join \"thumbnailVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"thumbnailTimestamps\" tt on t.\"UUID\" = tt.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where t.\"videoID\" = $1 order by t.\"timeSubmitted\"")
            .await?;

        let user_thumbnails = client.prepare("select t.\"UUID\", t.\"videoID\", t.\"userID\", t.\"timeSubmitted\", tt.\"timestamp\", tv.\"votes\", t.\"original\", tv.\"locked\", tv.\"shadowHidden\", u.\"userName\", v.\"userID\" from \"thumbnails\" t join \"thumbnailVotes\" tv on t.\"UUID\" = tv.\"UUID\" left join \"thumbnailTimestamps\" tt on t.\"UUID\" = tt.\"UUID\" left join \"userNames\" u on t.\"userID\" = u.\"userID\" left join \"vipUsers\" v on t.\"userID\" = v.\"userID\" where t.\"userID\" = $1 order by t.\"timeSubmitted\"")
            .await?;

        web::Data::new(RwLock::new(DatabaseState {
            db: client,
            statements: PreparedQueries {
                index_titles, unverified_titles, uuid_titles, video_titles, user_titles,
                index_thumbnails, uuid_thumbnails, video_thumbnails, user_thumbnails,
            },
            last_error: None,
            last_updated: Utc::now().timestamp_millis(),
            last_modified: utils::get_mtime(&config.mirror_path.join("last-modified")),
            updating_now: false
        }))
    };

    let mut server = {
        let config = config.clone();
        HttpServer::new(move || {
            let config2 = config.clone();
            App::new()
                .service(web::scope("/api")
                    .configure(routes::configure_routes)
                    .app_data(config.clone())
                    .app_data(db.clone())
                )
                .service(
                    Files::new("/", config.static_content_path.as_path())
                        .index_file("index.html")
                        .default_handler(fn_service(move |req: ServiceRequest| {
                            let config = config2.clone();
                            async move {
                                let (req, _) = req.into_parts();
                                let index_file = config.static_content_path.join("index.html");
                                let file = NamedFile::open_async(index_file.as_path()).await?;
                                let resp = file.into_response(&req);
                                Ok(ServiceResponse::new(req, resp))
                            }
                        }))
                )
        })
    };
    server = match config.listen.tcp {
        None => server,
        Some((ref ip, port)) => {
            let ip_str = ip.as_str();
            let srv = server.bind((ip_str, port)).with_context(|| format!("Failed to bind to tcp port {ip_str}:{port}"))?;
            println!("Listening on {ip_str}:{port}");
            srv
        }
    };
    server = match config.listen.unix {
        None => server,
        Some(ref path) => {
            let path_str = path.as_str();
            let srv = server.bind_uds(path_str).with_context(|| format!("Failed to bind to unix socket {path_str}"))?;
            match config.listen.unix_mode {
                None => (),
                Some(mode) => {
                    let perms = Permissions::from_mode(mode);
                    set_permissions(path_str, perms).with_context(|| format!("Failed to change mode of unix socket {path_str} to {mode}"))?
                }
            }
            println!("Listening on {path_str}");
            srv
        }
    };
    server.run()
    .await
    .context("Error while running the server")
}
