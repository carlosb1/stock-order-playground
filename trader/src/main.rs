mod intrumentdata;
mod strategies;
mod wal;
mod db;
mod models;

use barter::{engine::{
    clock::LiveClock,
    state::{
        global::DefaultGlobalData,
        instrument::filter::InstrumentFilter,
        trading::TradingState,
    },
}, logging::init_logging, risk::DefaultRiskManager, statistic::time::Daily, strategy::DefaultStrategy, system::{
    builder::{AuditMode, EngineFeedMode, SystemArgs, SystemBuilder},
    config::SystemConfig,
}, EngineEvent};
use barter_data::{
    streams::builder::dynamic::indexed::init_indexed_multi_exchange_market_stream,
    subscription::SubKind,
};
use barter_instrument::index::IndexedInstruments;
use barter_integration::Terminal;
use futures::{SinkExt, StreamExt};
use rust_decimal::Decimal;
use std::{fs::File, io::BufReader, time::Duration};
use std::collections::HashMap;
use std::sync::Arc;
use axum::extract::{State, WebSocketUpgrade};
use axum::extract::ws::{Message, WebSocket};
use axum::response::{Html, IntoResponse};
use axum::Router;
use axum::routing::get;
use barter::engine::{EngineOutput};
use barter::engine::audit::{AuditTick, EngineAudit};
use barter::statistic::summary::TradingSummary;
use barter::strategy::close_positions::{close_open_positions_with_market_orders, ClosePositionsStrategy};
use barter::system::System;
use barter_execution::order::id::{OrderId, StrategyId};
use barter_execution::order::request::{OrderRequestOpen, RequestOpen};
use barter_instrument::instrument::{Instrument, InstrumentIndex};
use barter_instrument::{Keyed, Side};
use questdb::ingress::TimestampNanos;
use rust_decimal_macros::dec;
use tokio::sync::{broadcast, mpsc};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, info};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use strategies::dummy_strategy;
use strategies::dummy_strategy::MyStrategy;
use crate::db::DBRepository;
use crate::intrumentdata::InstrumentMarketDataL2;
use crate::models::BackgroundMessage;
use crate::strategies::dummy_strategy::{strategy_id, MockDecider, ModelDecider, MyEngine, MyEvent, OnDisconnectOutput, OnTradingDisabledOutput};
use dyn_clone::DynClone;


const FILE_PATH_SYSTEM_CONFIG: &str = "system_config.json";
const RISK_FREE_RETURN: Decimal = dec!(0.05);

pub struct Worker<S: ModelDecider> where
    S: ModelDecider + Send + Sync + Clone + 'static
{
    system_config: SystemConfig,
    subkinds: Vec<SubKind>,
    strategy: MyStrategy<S>,
    engine_feed_mode: EngineFeedMode,
    audit_mode: AuditMode,
    trading_state: TradingState,
    system: Option<System<MyEngine<S>, MyEvent>>,
    audit_task: Option<AuditTask>,
    db_config: String,
}

pub type AuditTask = JoinHandle<
    UnboundedReceiverStream<
        AuditTick<EngineAudit<EngineEvent, EngineOutput<OnTradingDisabledOutput, OnDisconnectOutput>>>>>;

impl<S> Worker<S>  where
S: ModelDecider + Send + Sync + Clone + 'static, {
    pub fn new(system_config: &SystemConfig, db_config: String, decider: S) -> Self {
        let subkinds = [SubKind::PublicTrades, SubKind::OrderBooksL1, SubKind::OrderBooksL2];
        Self{system_config: system_config.clone(),
            db_config: db_config.clone(),
            subkinds: subkinds.to_vec(),
            strategy: MyStrategy{id: strategy_id(), wrapper: decider},
            engine_feed_mode: EngineFeedMode::Iterator,
            audit_mode: AuditMode::Enabled,
            trading_state: TradingState::Disabled,
            system: None,
            audit_task: None
        }
    }
    pub async fn start(&mut self) -> anyhow::Result<()> {
        let instruments = IndexedInstruments::new(self.system_config.instruments.clone());
        let market_stream = init_indexed_multi_exchange_market_stream(
            &instruments,
            &self.subkinds,
        )
            .await?;
        // Instrument data factory: clonamos L2 por instrumento
        let instrument_data = InstrumentMarketDataL2::default();

        let args = SystemArgs::new(
            &instruments,
            self.system_config.executions.clone(),
            LiveClock,
            self.strategy.clone(),
            //DefaultStrategy::default(),
            DefaultRiskManager::default(),
            market_stream,
            DefaultGlobalData::default(),
            // |_| DefaultInstrumentMarketData::default(),
            |_| instrument_data.clone(),
        );

        let system = SystemBuilder::new(args)
            // Engine feed in Sync mode (Iterator input)
            .engine_feed_mode(self.engine_feed_mode.clone())
            // Audit feed is enabled (Engine sends audits)
            .audit_mode(self.audit_mode.clone())
            // Engine starts with TradingState::Disabled
            .trading_state(self.trading_state)
            // Build System, but don't start spawning tasks yet
            .build()?
            // Init System, spawning component tasks on the current runtime
            .init_with_runtime(tokio::runtime::Handle::current())
            .await?;
        self.system = Some(system);
        Ok(())
    }
    pub fn enable_trading(&mut self) -> anyhow::Result<()> {
        let Some(system) = &mut self.system else {
            return Err(anyhow::Error::msg("system is not set"));
        };
        system.trading_state(TradingState::Enabled);
        Ok(())
    }

    fn db_worker( mut rx: Receiver<BackgroundMessage>, db_config: &str) -> anyhow::Result<JoinHandle<()>> {
        let audit_task = tokio::spawn({
            //Add clones id it is necessary

            let mut db_inner = DBRepository::new(db_config);
            async move {
                while let Some(val) = rx.recv().await {
                    db_inner.write_bg_msg(&val, TimestampNanos::now()).expect("db worker error");
                    /*
                    match val {
                        BackgroundMessage::Account(_) | BackgroundMessage::Market(_) => todo!(),
                    }
                    
                     */
                }

            }
        });
        Ok(audit_task)
    }

    pub async fn start_audit(&mut self) -> anyhow::Result<()> {
        let Some(system) = &mut self.system else {
            return Err(anyhow::Error::msg("system is not set"));
        };
        if self.audit_mode == AuditMode::Disabled {
            return Err(anyhow::Error::msg("Audit mode is disabled"));
        };

        let (tx, rx): (Sender<BackgroundMessage>, Receiver<BackgroundMessage>) = mpsc::channel(1024);

        Worker::<S>::db_worker(rx, self.db_config.as_str())?;

        let audit = system.audit.take().unwrap();
        let audit_task = tokio::spawn(async move {
            let mut audit_stream = audit.updates.into_stream();
            while let Some(audit) = audit_stream.next().await {
                debug!(?audit, "AuditStream consumed AuditTick");
                if audit.event.is_terminal() {
                    break;
                }
                match audit.event {
                    EngineAudit::Process(process) => {
                        let event = process.event;
                        match event {
                            EngineEvent::Account(account) => {
                                tx.send(BackgroundMessage::Account(account)).await.unwrap();
                            }
                            EngineEvent::Market(market) => {
                                tx.send(BackgroundMessage::Market(market)).await.unwrap();
                            }
                            _ => {

                            }
                        }
                    }
                    _ => {
                        
                    }
                    
                    
                }
            }
            audit_stream
        });
        self.audit_task = Some(audit_task);
        Ok(())
    }
    
    pub async fn stop(self, risk_free_return: Option<Decimal>) -> anyhow::Result<TradingSummary<Daily>> {
        let Some(system) = self.system else {
            return Err(anyhow::Error::msg("system is not set"));
        };
        let Some(audit_task) = self.audit_task else {
            return Err(anyhow::Error::msg("audit task is not set"));
        };
        
        system.cancel_orders(InstrumentFilter::None);
        system.close_positions(InstrumentFilter::None);

        // Shutdown
        let (engine, _shutdown_audit) = system.shutdown().await?;
        let _audit_stream = audit_task.await?;

        let risk_free_return = risk_free_return.unwrap_or(RISK_FREE_RETURN);
        // Generate TradingSummary<Daily>
        let trading_summary = engine
            .trading_summary_generator(risk_free_return)
            .generate(Daily);
        Ok(trading_summary)
    }
    
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
    //TODO open this port
    let db_config = std::env::var("QUESTDB_CONFIG").unwrap_or("tcp::addr=0.0.0.0:9009;protocol_version=2".to_string());

    // Load SystemConfig
    let system_config: SystemConfig = load_config()?;
    let mut worker = Worker::new(&system_config, db_config, MockDecider{});
    worker.start().await?;
    worker.start_audit().await?;
    worker.enable_trading()?;
    tokio::time::sleep(Duration::from_secs(60)).await;
    worker.stop(None).await?;
    Ok(())
}

// Our shared state

struct AppState {
    // Channel used to send messages to all connected clients.
    tx: broadcast::Sender<String>,
    workers: HashMap<String,Box<dyn ModelDecider>>
}

// Include utf-8 file at **compile** time.
async fn index() -> Html<&'static str> {
    Html("hello world")
}

async fn websocket_handler(
    ws: WebSocketUpgrade,
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket, state))
}

pub fn list_available_workers() -> Vec<&'static str> {
    vec!["MOCK_DECIDED"]
}

pub fn new_worker_by_id(id: &str) -> Option<Box<dyn ModelDecider>> {
     match id {
        "MOCK_DECIDED" => Some(Box::new(MockDecider{})),
        _ => None,
    }
}


pub enum Command {
    NEW_WORKER(String),

}

// This function deals with a single websocket connection, i.e., a single
// connected client / user, for which we will spawn two independent tasks (for
// receiving / sending chat messages).
async fn websocket(stream: WebSocket, state: Arc<AppState>) {
    // By splitting, we can send and receive at the same time.
    let (mut sender, mut receiver) = stream.split();
    // Loop until a text message is found.
    while let Some(Ok(message)) = receiver.next().await {
        if let Message::Text(command) = message {


            // If username that is sent by client is not taken, fill username string.
            //check_username(&state, &mut username, name.as_str());
        }
    }
    // We subscribe *before* sending the "joined" message, so that we will also
    // display it to our client.
    let mut rx = state.tx.subscribe();

    // Now send the "joined" message to all subscribers.
    let msg = format!("joined.");
    tracing::debug!("{msg}");
    let _ = state.tx.send(msg);

    // Spawn the first task that will receive broadcast messages and send text
    // messages over the websocket to our client.
    let mut send_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // In any websocket error, break loop.
            if sender.send(Message::text(msg)).await.is_err() {
                break;
            }
        }
    });

    // Clone things we want to pass (move) to the receiving task.
    let tx = state.tx.clone();

    // Spawn a task that takes messages from the websocket, prepends the user
    // name, and sends them to all broadcast subscribers.
    let mut recv_task = tokio::spawn(async move {
        while let Some(Ok(Message::Text(text))) = receiver.next().await {
            // Add username before message.
            let _ = tx.send(format!(" {text}"));
        }
    });

    // If any one of the tasks run to completion, we abort the other.
    tokio::select! {
        _ = &mut send_task => recv_task.abort(),
        _ = &mut recv_task => send_task.abort(),
    };

    // Send "user left" message (similar to "joined" above).
    let msg = format!(" left.");
    tracing::debug!("{msg}");
    let _ = state.tx.send(msg);

}


async fn start() -> Result<(), Box<dyn std::error::Error>> {
    init_logging();
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
    let (tx, _rx) = broadcast::channel(100);

    let mut workers: HashMap<String,Box<dyn ModelDecider>> = HashMap::new();
    let app_state = Arc::new(AppState { tx, workers });

    let app = Router::new()
        .route("/", get(index))
        .route("/websocket", get(websocket_handler))
        .with_state(app_state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:3000")
        .await
        .unwrap();
    tracing::debug!("listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();

    // new axum
    let mut worker = Worker::new(&system_config,db_config, MockDecider{});
    worker.start().await?;
    worker.start_audit().await?;
    worker.enable_trading()?;
    tokio::time::sleep(Duration::from_secs(60)).await;
    worker.stop(None).await?;
    Ok(())
}




#[tokio::main]
async fn amain() -> Result<(), Box<dyn std::error::Error>> {
    // Initialise Tracing
    init_logging();

    // Load SystemConfig
    let SystemConfig {
        instruments,
        executions,
    } = load_config()?;

    // Construct IndexedInstruments
    let instruments = IndexedInstruments::new(instruments);

    // Initialise MarketData Stream
    let market_stream = init_indexed_multi_exchange_market_stream(
        &instruments,
        &[SubKind::PublicTrades, SubKind::OrderBooksL1, SubKind::OrderBooksL2],
       // &[SubKind::OrderBooksL2],
    )
        .await?;

    // Construct System Args
    let args = SystemArgs::new(
        &instruments,
        executions,
        LiveClock,
        MyStrategy{id: strategy_id(), wrapper: MockDecider{}},
        //DefaultStrategy::default(),
        DefaultRiskManager::default(),
        market_stream,
        DefaultGlobalData::default(),
        // |_| DefaultInstrumentMarketData::default(),
        |_| InstrumentMarketDataL2::default(),
    );

    // Build & run full system:
    // See SystemBuilder for all configuration options
    let mut system = SystemBuilder::new(args)
        // Engine feed in Sync mode (Iterator input)
        .engine_feed_mode(EngineFeedMode::Iterator)
        // Audit feed is enabled (Engine sends audits)
        .audit_mode(AuditMode::Enabled)
        // Engine starts with TradingState::Disabled
        .trading_state(TradingState::Disabled)
        // Build System, but don't start spawning tasks yet
        .build()?
        // Init System, spawning component tasks on the current runtime
        .init_with_runtime(tokio::runtime::Handle::current())
        .await?;

    // Take ownership of the Engine audit snapshot with updates
    let audit = system.audit.take().unwrap();

    // Run dummy asynchronous AuditStream consumer
    // Note: you probably want to use this Stream to replicate EngineState, or persist events, etc.
    //  --> eg/ see examples/engine_sync_with_audit_replica_engine_state
    let audit_task = tokio::spawn(async move {
        let mut audit_stream = audit.updates.into_stream();
        while let Some(audit) = audit_stream.next().await {
            debug!(?audit, "AuditStream consumed AuditTick");
            if audit.event.is_terminal() {
                break;
            }
        }
        audit_stream
    });

    // Enable trading
    system.trading_state(TradingState::Enabled);

    // Let the example run for 5 seconds...
    tokio::time::sleep(Duration::from_secs(60)).await;

    // Before shutting down, CancelOrders and then ClosePositions
    system.cancel_orders(InstrumentFilter::None);
    system.close_positions(InstrumentFilter::None);

    // Shutdown
    let (engine, _shutdown_audit) = system.shutdown().await?;
    let _audit_stream = audit_task.await?;

    // Generate TradingSummary<Daily>
    let trading_summary = engine
        .trading_summary_generator(RISK_FREE_RETURN)
        .generate(Daily);

    // Print TradingSummary<Daily> to terminal (could save in a file, send somewhere, etc.)
    trading_summary.print_summary();

    Ok(())
}

fn load_config() -> Result<SystemConfig, Box<dyn std::error::Error>> {
    let file = File::open(FILE_PATH_SYSTEM_CONFIG)?;
    let reader = BufReader::new(file);
    let config = serde_json::from_reader(reader)?;
    Ok(config)
}