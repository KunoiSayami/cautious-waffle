use crate::cloudflare::ApiRequest;
use crate::datastructures::Config;
use crate::file_watcher::FileWatchDog;
use crate::web::{get, get_debug, post};
use axum::http::StatusCode;
use axum::{Extension, Json, Router};
use clap::{arg, command};
use log::{debug, error, info, warn, LevelFilter};
use serde_json::json;
use std::hint::unreachable_unchecked;
use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tap::TapFallible;
use tokio::sync::RwLock;
use tower::ServiceBuilder;
use tower_http::trace::TraceLayer;

mod cloudflare;
mod datastructures;
mod file_watcher;
mod web;

const DEFAULT_CONFIG_LOCATION: &str = "config.toml";

async fn async_main(
    config_location: String,
    file_watchdog: bool,
    query_enabled: bool,
) -> anyhow::Result<()> {
    let config = Config::try_from_file(&config_location).await?;

    let bind = config.get_bind();
    info!("Version: {}", env!("CARGO_PKG_VERSION"));
    debug!("Server bind to {}", &bind);

    let query_enabled = query_enabled || config.enable_query();

    let request = ApiRequest::try_from(config)?;

    if request.is_relay() {
        debug!("Server is running on relay mode");
    }

    let relay_flag = Arc::new(AtomicBool::new(request.is_relay()));
    let request = Arc::new(RwLock::new(request));

    let router = Router::new()
        .route("/:sub_id", axum::routing::get(get).post(post))
        .route(
            "/",
            axum::routing::get(|| async {
                Json(json!({ "version": env!("CARGO_PKG_VERSION"), "status": 200 }))
            }),
        )
        .fallback(|| async { (StatusCode::FORBIDDEN, "403 Forbidden") })
        .with_state(request.clone())
        .layer(Extension(relay_flag.clone()))
        .layer(ServiceBuilder::new().layer(TraceLayer::new_for_http()));

    let router = if query_enabled {
        if !std::env::var("DISABLE_QUERY_WARNING")
            .map(|v| v.eq("1"))
            .unwrap_or_default()
        {
            warn!("Route query is enabled, it may cause some security issue. Set DISABLE_QUERY_WARNING=1 to disable this warning.");
        }
        router.route("/query", axum::routing::get(get_debug))
    } else {
        router
    };

    let server_handler = axum_server::Handle::new();
    let server = tokio::spawn(
        axum_server::bind(bind.parse().unwrap())
            .handle(server_handler.clone())
            .serve(router.into_make_service()),
    );

    let file_watcher_handler = if file_watchdog {
        Some(FileWatchDog::start(config_location, request, relay_flag))
    } else {
        None
    };

    tokio::select! {
        _ = async {
            tokio::signal::ctrl_c().await.unwrap();
            info!("Recv Control-C send graceful shutdown command.");
            server_handler.graceful_shutdown(None);
            tokio::signal::ctrl_c().await.unwrap();
            warn!("Force to exit!");
            std::process::exit(137)
        } => {
            unsafe { unreachable_unchecked() }
        },
        ret = server => {
            ret??;
        }
    }

    if file_watchdog {
        tokio::task::spawn_blocking(|| file_watcher_handler.unwrap().stop())
            .await
            .tap_err(|e| {
                error!(
                    "[Can be safely ignored] Unable to spawn stop file watcher thread {:?}",
                    e
                )
            })
            .ok();
    }

    Ok(())
}

fn main() -> anyhow::Result<()> {
    let matches = command!()
        .args(&[
            arg!(--config [configure_file] "Specify configure location")
                .default_value(DEFAULT_CONFIG_LOCATION),
            arg!(--systemd "Disable log output in systemd"),
            arg!(--"disable-watcher" "Disable configuration file watcher"),
            arg!(--"enable-query" "Enable query response"),
        ])
        .get_matches();

    let mut binding = env_logger::Builder::from_default_env();
    binding
        .filter_module("rustls", LevelFilter::Warn)
        .filter_module("reqwest", LevelFilter::Warn)
        .filter_module("h2", LevelFilter::Warn)
        .filter_module("hyper::proto::h1", LevelFilter::Warn);
    if matches.get_flag("systemd") {
        binding.format(|buf, record| writeln!(buf, "[{}] {}", record.level(), record.args()));
    }
    binding.init();

    tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async_main(
            matches
                .get_one("config")
                .map(|s: &String| s.to_string())
                .unwrap(),
            !matches.get_flag("disable-watcher"),
            matches.get_flag("enable-query"),
        ))
}
