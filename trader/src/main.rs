mod intrument_data;
mod strategies;
mod db;
mod models;
mod worker;

use barter::{engine::{
    clock::LiveClock,
    state::{
        global::DefaultGlobalData,
        instrument::filter::InstrumentFilter,
        trading::TradingState,
    },
},  risk::DefaultRiskManager, statistic::time::Daily, strategy::DefaultStrategy, system::{
    builder::{AuditMode, EngineFeedMode, SystemArgs, SystemBuilder},
    config::SystemConfig,
}, EngineEvent};
use barter_data::{
    streams::builder::dynamic::indexed::init_indexed_multi_exchange_market_stream,
    subscription::SubKind,
};
use futures::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use std::{fs::File, io::BufReader, time::Duration};
use std::collections::HashSet;
use std::error::Error;
use std::sync::{Arc};
use axum::extract::{State, WebSocketUpgrade};
use axum::extract::ws::{Message, Utf8Bytes, WebSocket};
use axum::response::{Html, IntoResponse};
use axum::{Json, Router};
use axum::http::StatusCode;
use tower_http::services::{ServeDir, ServeFile};
use axum::routing::{get, get_service, post};
use barter::engine::audit::{AuditTick, EngineAudit};
use barter::strategy::close_positions::{close_open_positions_with_market_orders, ClosePositionsStrategy};
use barter_execution::order::id::{OrderId, StrategyId};
use barter_execution::order::request::{OrderRequestOpen, RequestOpen};
use barter_instrument::instrument::{Instrument, InstrumentIndex};
use barter_instrument::{Keyed, Side};
use rust_decimal_macros::dec;
use tokio::sync::{broadcast, mpsc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use crate::strategies::dummy_strategy::{MockDecider, DummyDecider, ModelDecider, MyEngine, MyEvent, OnDisconnectOutput, OnTradingDisabledOutput};
use serde_json::json;
use dashmap::{DashMap, Entry};
use crate::worker::Worker;

const FILE_PATH_SYSTEM_CONFIG: &str = "system_config.json";
const RISK_FREE_RETURN: Decimal = dec!(0.05);




struct AppState {
    // Channel used to send messages to all connected clients.
    tx: broadcast::Sender<String>,
    workers: Arc<DashMap<String, Worker>>,
}

// Include utf-8 file at **compile** time.
async fn list_workers() -> Html<&'static str> {
    json!({});
    Html("hello world")
}

async fn delete_workers(
    State(arc_state): State<Arc<AppState>>,
    Json(name_workers): Json<Vec<String>>,
) -> Result<Json<Vec<(String, String)>>, StatusCode> {
    let mut result = Vec::with_capacity(name_workers.len());

    for name in name_workers {
        match arc_state.workers.remove(&name) {
            // remove gives you ownership; no guards alive
            Some((_key, mut worker)) => {
                // optional: handle result if stop() returns Result<_, _>
                let report = stop_worker(worker).await;
                result.push((name, "REMOVED".to_string()));
            }
            None => {
                result.push((name, "NOTEXIST".to_string()));
            }
        }
    }

    Ok(Json(result))
}

async fn list_running_workers(State(arc_state): State<Arc<AppState>>) -> Result<Json<HashSet<String>>, StatusCode> {
    let included_name_workers: HashSet<String> =
        arc_state.workers.iter().map(|entry| entry.key().clone()).collect();
    Ok(Json(included_name_workers))
}
async fn post_workers(
    State(state): State<Arc<AppState>>,
    Json(name_workers): Json<Vec<(String, String)>>,
) -> Result<Json<Vec<(String, String)>>, StatusCode> {
    let system_config: SystemConfig = load_config().map_err(|_| StatusCode::BAD_REQUEST)?;
    let db_config = std::env::var("QUESTDB_CONFIG").unwrap_or_else(|_| "tcp::addr=0.0.0.0:9009".into());

    let mut resp = Vec::with_capacity(name_workers.len());

    for (name, _cfg) in name_workers {
        if !list_available_workers().contains(&name.as_str()) {
            resp.push((name, "NOTEXIST".into()));
            continue;
        }

        let Some(mut worker) = new_worker_by_id(&name, system_config.clone(), &db_config) else {
            resp.push((name, "INNER_BAD_NAME".into()));
            continue;
        };

        match state.workers.entry(name.clone()) {
            Entry::Occupied(_) => {
                resp.push((name, "ALREADY".into()));
            }
            Entry::Vacant(v) => {
                start_worker(&mut worker, state.tx.clone()).await.map_err(|_| StatusCode::BAD_REQUEST)?;
                v.insert(worker);
                resp.push((name, "INSERTED".into()));
            }
        }
    }

    Ok(Json(resp))
}


pub fn list_available_workers() -> Vec<&'static str> {
    vec!["MOCK_DECIDED","DUMMY_DECIDED"]
}


pub fn new_worker_by_id(id: &str, system_config: SystemConfig, db_config: &str) -> Option<Worker> {
     match id {
        "MOCK_DECIDED" => Some(Worker::new(&system_config, db_config.to_string(), MockDecider {}, true)),
        "DUMMY_DECIDED" => Some(Worker::new(&system_config, db_config.to_string(), DummyDecider {}, false)),
        _ => None,
    }
}


async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

pub async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    let (mut sender, _receiver) = stream.split();
    let mut rx = state.tx.subscribe(); // broadcast::Sender<String>

    loop {
        match rx.recv().await {
            Ok(msg) => {
                if sender.send(Message::Text(Utf8Bytes::from(msg))).await.is_err() {
                    break;
                }
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {
                continue;
            }
            Err(_) => break,
        }
    }
}



#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //TODO open this port
    let db_config = std::env::var("QUESTDB_CONFIG").unwrap_or("tcp::addr=0.0.0.0:9009;protocol_version=2".to_string());
    // Load SystemConfig
    let system_config: SystemConfig = load_config()?;

    // tracing info
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| format!("{}=trace", env!("CARGO_CRATE_NAME")).into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    // Set up application state for use with with_state().
    // let user_set = Mutex::new(HashSet::new());
    let (tx, _rx) = broadcast::channel(1024);
    
    let workers = Arc::new(DashMap::new());
    let app_state = Arc::new(AppState { tx: tx.clone(), workers });



    let static_dir = get_service(ServeDir::new("../webapp/dist/public"))
        .handle_error(|err| async move {
            (StatusCode::INTERNAL_SERVER_ERROR, format!("serve error: {err}"))
        });
    let mut worker = new_worker_by_id("MOCK_DECIDED", system_config, &*db_config).unwrap();
    start_worker(&mut worker, tx.clone()).await.unwrap();
    app_state.workers.insert("MOCK_DECIDED".to_string(), worker);

    let app = Router::new()
        .route("/workers/list", get(list_workers))
        .route("/workers", post(post_workers))
        .route("/workers/delete",post(delete_workers))
        .route("/websocket", get(websocket_handler))
        .fallback_service(static_dir)
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();
    Ok(())
}

async fn stop_worker(worker:  Worker) -> Result<(), Box<dyn Error>> {
    worker.stop(None).await?;
    Ok(())
}

async fn start_worker(worker: &mut Worker, send_to_ws: broadcast::Sender<String>) -> Result<(), Box<dyn Error>> {
    worker.start().await?;
    worker.start_audit(Some(send_to_ws)).await?;
    worker.enable_trading()?;
    Ok(())
}

fn load_config() -> Result<SystemConfig, Box<dyn std::error::Error>> {
    let file = File::open(FILE_PATH_SYSTEM_CONFIG)?;
    let reader = BufReader::new(file);
    let config = serde_json::from_reader(reader)?;
    Ok(config)
}