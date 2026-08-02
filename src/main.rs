use axum::{
    Router,
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    response::{Html, IntoResponse},
    routing::get,
};
use chrono::{DateTime, Utc};
use log::{LevelFilter, info, warn};
use minecraft_pinger::MinecraftPinger;
use minecraft_pinger::config::{JavaPingConfig, PingConfig};
use serde::{Deserialize, Serialize};
use std::{
    env,
    net::IpAddr,
    sync::Arc,
    time::{Duration, SystemTime},
};
use ticker::Ticker;
use tokio::sync::{Mutex, broadcast};

const VERSION_26_2: i32 = 776;
const PING_TYPE_MINECRAFT: &str = "MINECRAFT";
const PING_TYPE_NETWORK: &str = "NETWORK";

const INDEX_HTML: &str = include_str!("../index.html");

#[derive(Clone)]
struct PingResult {
    timestamp: SystemTime,
    connectivity: u32,
    latency: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct Ping {
    pub ping_type: String,
    pub timestamp: DateTime<Utc>,
    pub connectivity: u32,
    pub latency: u32,
}

#[derive(Clone, Serialize)]
#[serde(tag = "type")]
enum WsMessage {
    #[serde(rename = "snapshot")]
    Snapshot { host: String, history: Vec<Ping> },
    #[serde(rename = "ping")]
    Ping {
        timestamp: DateTime<Utc>,
        connectivity: u32,
        latency: u32,
    },
}

#[derive(Clone)]
struct AppState {
    host: String,
    minecraft_ping_results: Arc<Mutex<Vec<PingResult>>>,
    minecraft_tx: broadcast::Sender<Ping>,
    network_ping_results: Arc<Mutex<Vec<PingResult>>>,
    network_tx: broadcast::Sender<Ping>,
}

#[derive(Clone)]
struct SeriesState {
    host: String,
    results: Arc<Mutex<Vec<PingResult>>>,
    tx: broadcast::Sender<Ping>,
}

async fn index() -> impl IntoResponse {
    Html(INDEX_HTML)
}

async fn ws_minecraft_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let series = SeriesState {
        host: state.host.clone(),
        results: state.minecraft_ping_results.clone(),
        tx: state.minecraft_tx.clone(),
    };
    ws.on_upgrade(move |socket| handle_series_socket(socket, series))
}

async fn ws_network_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl IntoResponse {
    let series = SeriesState {
        host: state.host.clone(),
        results: state.network_ping_results.clone(),
        tx: state.network_tx.clone(),
    };
    ws.on_upgrade(move |socket| handle_series_socket(socket, series))
}

async fn handle_series_socket(mut socket: WebSocket, series: SeriesState) {
    // subscribe before reading the snapshot so we never miss a ping that
    // arrives between the snapshot read and the subscription. any ping
    // already captured in the snapshot is deduplicated by timestamp.
    let mut rx = series.tx.subscribe();

    let (history, last_ts) = {
        let results = series.results.lock().await;
        let last_ts = results.last().map(|x| DateTime::<Utc>::from(x.timestamp));
        let history = results
            .iter()
            .map(|x| Ping {
                ping_type: String::new(),
                timestamp: x.timestamp.into(),
                connectivity: x.connectivity,
                latency: x.latency,
            })
            .collect::<Vec<_>>();
        (history, last_ts)
    };

    let snapshot = WsMessage::Snapshot {
        host: series.host.clone(),
        history,
    };
    let snapshot_json = match serde_json::to_string(&snapshot) {
        Ok(s) => s,
        Err(_) => return,
    };
    if socket
        .send(Message::Text(snapshot_json.into()))
        .await
        .is_err()
    {
        return;
    }

    loop {
        match rx.recv().await {
            Ok(ping) => {
                if let Some(ts) = last_ts {
                    if ping.timestamp <= ts {
                        continue;
                    }
                }

                let msg = WsMessage::Ping {
                    timestamp: ping.timestamp,
                    connectivity: ping.connectivity,
                    latency: ping.latency,
                };
                let json = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                if socket.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(n)) => {
                warn!("ws client lagged by {} messages", n);
                continue;
            }
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 8)]
async fn main() {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .init();

    let host = env::var("SERVER").expect("SERVER env var empty or unset");

    let minecraft_pinger = MinecraftPinger::new().expect("failed to created minecraft pinger");
    let minecraft_ping_config = PingConfig::builder()
        .set_timeout(Duration::from_millis(250))
        .set_java_config(
            JavaPingConfig::builder()
                .set_protocol_version(VERSION_26_2)
                .build(),
        )
        .build();

    let minecraft_ping_results: Arc<Mutex<Vec<PingResult>>> = Arc::new(Mutex::new(vec![]));
    let network_ping_results: Arc<Mutex<Vec<PingResult>>> = Arc::new(Mutex::new(vec![]));
    let (minecraft_tx, _minecraft_rx) = broadcast::channel::<Ping>(1024);
    let (network_tx, _network_rx) = broadcast::channel::<Ping>(1024);

    let state = AppState {
        host: host.clone(),
        minecraft_ping_results: minecraft_ping_results.clone(),
        network_ping_results: network_ping_results.clone(),
        minecraft_tx: minecraft_tx.clone(),
        network_tx: network_tx.clone(),
    };

    //
    // resolve host (ip / fqdn / hostname) once at startup. both pingers use
    // the resolved ip directly so we skip per-ping dns lookups inside the
    // minecraft lib and the ping crate.
    //

    let resolved_ip: IpAddr = tokio::net::lookup_host((host.as_str(), 0))
        .await
        .unwrap_or_else(|e| panic!("failed to resolve {host}: {e}"))
        .next()
        .map(|sa| sa.ip())
        .unwrap_or_else(|| panic!("no addresses resolved for {host}"));
    let resolved_ip_str = resolved_ip.to_string();
    info!("resolved {host} -> {resolved_ip}");

    //
    // minecraft pinger runs in a background task
    //

    let minecraft_ping_thread_host = host.clone();
    let minecraft_ping_thread_ip = resolved_ip_str.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create minecraft pinger runtime");

        rt.block_on(async move {
            info!("starting minecraft pinger...");

            let ticker = Ticker::new(0.., Duration::from_millis(250));

            for _ in ticker {
                let now = SystemTime::now();

                let mut minecraft_ping_result = PingResult {
                    timestamp: now,
                    connectivity: 0,
                    latency: 1_000,
                };

                match minecraft_pinger
                    .ping_java_server(&minecraft_ping_thread_ip, 25565, &minecraft_ping_config)
                    .await
                {
                    Ok(response) => {
                        info!(
                            "minecraft ping: {} ({}) @ {} ms - {} - {} / {} players",
                            minecraft_ping_thread_host,
                            minecraft_ping_thread_ip,
                            response.latency,
                            response.version.name,
                            response.players.online,
                            response.players.max,
                        );

                        minecraft_ping_result.connectivity = 1;
                        minecraft_ping_result.latency = response.latency;
                    }
                    Err(_e) => {
                        warn!(
                            "minecraft ping: {} ({}) @ {} ms - {} - {} / {} players",
                            minecraft_ping_thread_host,
                            minecraft_ping_thread_ip,
                            "(timeout)",
                            "?",
                            "?",
                            "?",
                        )
                    }
                }

                let minecraft_ping = Ping {
                    ping_type: PING_TYPE_MINECRAFT.into(),
                    timestamp: minecraft_ping_result.timestamp.into(),
                    connectivity: minecraft_ping_result.connectivity,
                    latency: minecraft_ping_result.latency,
                };

                {
                    let mut results = minecraft_ping_results.lock().await;
                    results.push(minecraft_ping_result);

                    if results.len() > 14400 {
                        let len = results.len();
                        results.drain(..len - 14400);
                    }
                }

                let _ = minecraft_tx.send(minecraft_ping);
            }
        });
    });

    //
    // network pinger runs in a background task
    //

    let network_ping_thread_host = host.clone();
    let network_ping_thread_ipaddr = resolved_ip;
    let network_ping_thread_ip = resolved_ip_str.clone();

    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to create network pinger runtime");

        rt.block_on(async move {
            info!("starting network pinger...");

            let ticker = Ticker::new(0.., Duration::from_millis(250));

            for _ in ticker {
                let now = SystemTime::now();

                let mut network_ping_result = PingResult {
                    timestamp: now,
                    connectivity: 0,
                    latency: 1_000,
                };

                match ping::new(network_ping_thread_ipaddr.clone())
                    .socket_type(ping::SocketType::RAW)
                    .timeout(Duration::from_millis(250))
                    .send_async()
                    .await
                {
                    Ok(response) => {
                        info!(
                            "network ping: {} ({}) @ {} ms",
                            network_ping_thread_host,
                            network_ping_thread_ip,
                            response.rtt.as_millis(),
                        );

                        network_ping_result.connectivity = 1;
                        network_ping_result.latency = response.rtt.as_millis() as u32;
                    }
                    Err(_e) => {
                        warn!(
                            "network ping: {} ({}) @ {} ms",
                            network_ping_thread_host, network_ping_thread_ip, "(timeout)",
                        )
                    }
                }

                let network_ping = Ping {
                    ping_type: PING_TYPE_NETWORK.into(),
                    timestamp: network_ping_result.timestamp.into(),
                    connectivity: network_ping_result.connectivity,
                    latency: network_ping_result.latency,
                };

                {
                    let mut results = network_ping_results.lock().await;
                    results.push(network_ping_result);

                    if results.len() > 14400 {
                        let len = results.len();
                        results.drain(..len - 14400);
                    }
                }

                let _ = network_tx.send(network_ping);
            }
        });
    });

    //
    // http + websocket server on the main runtime
    //

    let app = Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/ws-minecraft", get(ws_minecraft_handler))
        .route("/ws-network", get(ws_network_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080")
        .await
        .expect("failed to bind http server");

    info!("handling http + websocket requests...");

    axum::serve(listener, app).await.expect("server error");
}
