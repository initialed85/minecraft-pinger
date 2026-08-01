use chrono::{DateTime, Utc};
use log::{LevelFilter, info, warn};
use minecraft_pinger::MinecraftPinger;
use minecraft_pinger::config::{JavaPingConfig, PingConfig};
use serde::{Deserialize, Serialize};
use std::{
    env,
    sync::Arc,
    time::{Duration, SystemTime},
};
use ticker::Ticker;
use tiny_http::{Header, Response, Server};
use tokio::sync::Mutex;

const VERSION_26_2: i32 = 776;

const INDEX_HTML: &str = include_str!("../index.html");

#[derive(Clone)]
struct PingResult {
    timestamp: SystemTime,
    connectivity: u32,
    latency: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct Ping {
    pub timestamp: DateTime<Utc>,
    pub connectivity: u32,
    pub latency: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct Score {
    min: f32,
    avg: f32,
    max: f32,
}

#[derive(Clone, Serialize, Deserialize)]
struct PingResponse {
    pub host: String,
    pub connectivity: Score,
    pub latency: Score,
    pub history: Vec<Ping>,
}

fn cors_headers() -> Vec<Header> {
    vec![
        Header::from_bytes("Access-Control-Allow-Origin", "*").unwrap(),
        Header::from_bytes("Access-Control-Allow-Methods", "GET, OPTIONS").unwrap(),
        Header::from_bytes("Access-Control-Allow-Headers", "*").unwrap(),
    ]
}

fn with_cors<R: std::io::Read>(mut response: Response<R>) -> Response<R> {
    for header in cors_headers() {
        response = response.with_header(header);
    }
    response
}

#[tokio::main]
async fn main() {
    env_logger::Builder::new()
        .filter_level(LevelFilter::Info)
        .init();

    let host = env::var("SERVER").expect("SERVER env var empty or unset");

    let pinger = MinecraftPinger::new().expect("failed to created minecraft pinger");

    let config = PingConfig::builder()
        .set_timeout(Duration::from_secs(3))
        .set_java_config(
            JavaPingConfig::builder()
                .set_protocol_version(VERSION_26_2)
                .build(),
        )
        .build();

    let ping_results: Arc<Mutex<Vec<PingResult>>> = Arc::new(Mutex::new(vec![]));

    let thread_host = host.clone();
    let main_host = host.clone();

    let thread_ping_results = ping_results.clone();
    let main_ping_results = ping_results.clone();

    //
    // pinger runs in a background thread
    //

    tokio::spawn(async move {
        let thread_ping_results = thread_ping_results.clone();

        info!("starting pinger...");

        let ticker = Ticker::new(0.., Duration::from_millis(1_000));

        for _ in ticker {
            let now = SystemTime::now();

            let mut ping = PingResult {
                timestamp: now,
                connectivity: 0,
                latency: 1_000,
            };

            match pinger.ping_java_server(&thread_host, 25565, &config).await {
                Ok(response) => {
                    info!(
                        "{} ({}) @ {} ms - {} / {} players",
                        thread_host,
                        response.version.name,
                        response.latency,
                        response.players.online,
                        response.players.max,
                    );

                    ping.connectivity = 1;
                    ping.latency = response.latency;
                }
                Err(_e) => {
                    warn!(
                        "{} ({}) @ {} ms - {} / {} players",
                        thread_host, "?", "(timeout)", "?", "?",
                    )
                }
            }

            {
                let mut thread_ping_results = thread_ping_results.lock().await;
                thread_ping_results.push(ping);

                if thread_ping_results.len() > 3600 {
                    let len = thread_ping_results.len();
                    thread_ping_results.drain(..len - 3600);
                }
            }
        }
    });

    //
    // http server has the main thread
    //

    let server = Server::http("0.0.0.0:8080").expect("failed to create http server");

    info!("handling http requests...");
    for request in server.incoming_requests() {
        let method = request.method().clone();
        let path = request
            .url()
            .split('?')
            .next()
            .unwrap_or(request.url())
            .to_string();

        if method.as_str() == "OPTIONS" {
            _ = request.respond(with_cors(Response::empty(204)));
            continue;
        }

        if path == "/" || path == "/index.html" {
            _ = request.respond(Response::from_string(INDEX_HTML).with_header(
                Header::from_bytes("Content-Type", "text/html; charset=utf-8").unwrap(),
            ));
            continue;
        }

        if path != "/ping" {
            _ = request.respond(with_cors(
                Response::from_string("not found").with_status_code(404),
            ));
            continue;
        }

        let server_ping_results = main_ping_results.lock().await.clone();

        let latencies: Vec<u32> = server_ping_results.iter().map(|x| x.latency).collect();
        let latency = Score {
            min: latencies.iter().min().copied().unwrap_or(0) as f32,
            avg: if !latencies.is_empty() {
                latencies.iter().sum::<u32>() as f32 / latencies.len() as f32
            } else {
                0.0
            } as f32,
            max: latencies.iter().max().copied().unwrap_or(0) as f32,
        };

        let connectivities: Vec<u32> = server_ping_results.iter().map(|x| x.connectivity).collect();
        let connectivity = Score {
            min: connectivities.iter().min().copied().unwrap_or(0) as f32,
            avg: if !connectivities.is_empty() {
                connectivities.iter().sum::<u32>() as f32 / connectivities.len() as f32
            } else {
                0.0
            } as f32,
            max: connectivities.iter().max().copied().unwrap_or(0) as f32,
        };

        let history = server_ping_results
            .iter()
            .map(|x| Ping {
                timestamp: x.timestamp.into(),
                connectivity: x.connectivity,
                latency: x.latency,
            })
            .collect();

        let ping_response = PingResponse {
            host: main_host.clone(),
            connectivity,
            latency,
            history,
        };

        let body = serde_json::to_string(&ping_response).unwrap();

        _ = request.respond(Response::from_string(body));
    }
}
