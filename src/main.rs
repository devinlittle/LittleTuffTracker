use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::Result;
use dashmap::DashMap;
use tokio::{signal, time::sleep};
use tracing::{error, info};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

use crate::{
    env::ENV_VARS,
    structs::{AppState, Connections, InfoHashes},
};

mod env;
mod http;
mod structs;
mod udp;

#[tokio::main]
async fn main() -> Result<()> {
    let env_filter = EnvFilter::from_default_env();

    let stdout_layer = tracing_subscriber::fmt::layer().with_ansi(true).compact();
    std::fs::create_dir_all(&ENV_VARS.log_dir)?;
    let file_appender =
        tracing_appender::rolling::daily(&ENV_VARS.log_dir, "little_tuff_tracker.log");

    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let file_layer = tracing_subscriber::fmt::layer()
        .json()
        .with_writer(non_blocking);

    Registry::default()
        .with(env_filter)
        .with(stdout_layer)
        .with(file_layer)
        .init();

    let connections: Connections = DashMap::new();
    let info_hashes: InfoHashes = DashMap::new();

    let app_state = Arc::new(AppState {
        connections,
        info_hashes,
    });

    // Garbage Collector
    let _garbage_app_state = Arc::clone(&app_state);
    tokio::spawn(async move {});

    // This is the UDP Router
    let udp_app_state = Arc::clone(&app_state);
    tokio::spawn(async move {
        let Ok(ip) = ENV_VARS.host.parse::<IpAddr>() else {
            error!("Failed to associate HOST string with IpAddress, check if HOST provided was a valid IP?");
            panic!("IP Parsing failed during UDP Socket Serve")
        };
        let host_on = SocketAddr::new(ip, ENV_VARS.port);

        let _ = udp::start_utp_serve(host_on, udp_app_state).await;
    });

    // This is the Axum Router
    tokio::spawn(async move {
        info!("This would be the Axum Router");
    });

    signal::ctrl_c().await?;
    println!("\nCtrl-C Recieved; Shutting down Little Tuff Tracker");
    sleep(Duration::from_millis(150)).await;

    Ok(())
}
