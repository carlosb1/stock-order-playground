use std::sync::Arc;
use barter::engine::audit::{AuditTick, EngineAudit};
use barter::engine::clock::LiveClock;
use barter::engine::EngineOutput;
use barter::engine::state::global::DefaultGlobalData;
use barter::engine::state::instrument::filter::InstrumentFilter;
use barter::engine::state::trading::TradingState;
use barter::EngineEvent;
use barter::risk::DefaultRiskManager;
use barter::statistic::summary::TradingSummary;
use barter::statistic::time::Daily;
use barter::system::builder::{AuditMode, EngineFeedMode, SystemArgs, SystemBuilder};
use barter::system::config::SystemConfig;
use barter::system::System;
use barter_data::books::Level;
use barter_data::event::{DataKind, MarketEvent};
use barter_data::streams::builder::dynamic::indexed::init_indexed_multi_exchange_market_stream;
use barter_data::streams::consumer::MarketStreamEvent;
use barter_data::subscription::SubKind;
use barter_execution::AccountEventKind;
use barter_instrument::index::IndexedInstruments;
use barter_integration::Terminal;
use futures::StreamExt;
use questdb::ingress::TimestampNanos;
use rust_decimal::Decimal;
use tokio::sync::{broadcast, mpsc, Mutex};
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::task::JoinHandle;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::debug;
use crate::db::{DBRepository};
use crate::intrument_data::InstrumentMarketDataL2;
use crate::models::{BackgroundMessage, Fill};
use crate::{tools, RISK_FREE_RETURN};
use barter_data::streams::reconnect::Event as ReEvent;
use barter_data::subscription::book::OrderBookEvent;
use barter_instrument::Side;
use chrono::{Date, DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use crate::strategies::dummy_strategy::{ModelDecider, MyEngine, MyEvent, MyStrategy, OnDisconnectOutput, OnTradingDisabledOutput};
use crate::tools::{extract_book_l2, extract_order_cancel, extract_order_snapshot};



pub struct Worker where
{
    system_config: SystemConfig,
    subkinds: Vec<SubKind>,
    strategy: MyStrategy,
    engine_feed_mode: EngineFeedMode,
    audit_mode: AuditMode,
    trading_state: TradingState,
    system: Option<System<MyEngine, MyEvent>>,
    audit_task: Option<AuditTask>,
    db_config: String,
    pub operations: Arc<Mutex<Vec<Fill>>>,
    is_raw: bool
}

pub type AuditTask = JoinHandle<
    UnboundedReceiverStream<
        AuditTick<EngineAudit<EngineEvent, EngineOutput<OnTradingDisabledOutput, OnDisconnectOutput>>>>>;

impl Worker {
    pub fn new<D>(system_config: &SystemConfig, db_config: String, decider: D, is_raw: bool) -> Self
    where
        D: ModelDecider + 'static {
        //let subkinds = [SubKind::PublicTrades, SubKind::OrderBooksL1, SubKind::OrderBooksL2];
        let subkinds = [SubKind::PublicTrades, SubKind::OrderBooksL2];
        Self{system_config: system_config.clone(),
            db_config: db_config.clone(),
            subkinds: subkinds.to_vec(),
            strategy: MyStrategy::new(decider),
            engine_feed_mode: EngineFeedMode::Iterator,
            audit_mode: AuditMode::Enabled,
            trading_state: TradingState::Disabled,
            system: None,
            audit_task: None,
            operations: Arc::new(Mutex::new(Vec::new())),
            is_raw
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

    fn broadcast_worker(mut rx: Receiver<BackgroundMessage>,
                        db_config: &str,
                        sender_to_ws: Option<broadcast::Sender<String>>,
                        mut operations: Arc<Mutex<Vec<Fill>>>,
                        is_raw: bool) -> anyhow::Result<JoinHandle<()>> {

        let audit_task = tokio::spawn({
            //Add clones id it is necessary
            let mut db_inner = DBRepository::new(db_config);
            let operations = operations.clone();
            async move {
                let mut best_bid: Level = Level::default();
                let mut best_ask: Level = Level::default();
                let mut last_time: DateTime<Utc> = DateTime::default();
                //we reset the operations
                if operations.lock().await.len() >= 1000 {
                    tracing::info!("Cleaning fill operations");
                    operations.lock().await.clear();
                }
                while let Some(val) = rx.recv().await {
                    if is_raw {
                        if let BackgroundMessage::Market(mkt) = val.clone() {
                            //send to websocket
                            if let Some(sender) = sender_to_ws.clone() {
                                if let Err(er) = sender.send(serde_json::to_string(&val).unwrap()) {
                                    println!("Error sending background message: {:?}", er);
                                }
                            }
                            // is a Item event
                            let ReEvent::Item(ev) = mkt else {
                                continue;
                            };


                            /* param for  fill  market events*/
                            let ex = ev.exchange.as_str();                // o mapea a &str
                            let instrument = format!("{}", ev.instrument.0); // o tu símbolo real
                            let ts = ev.time_exchange;
                            let ns = ts.timestamp_nanos_opt().expect("valid ts");
                            match &ev.kind {
                                DataKind::OrderBookL1(l1) => {
                                    //we save the last bestbid and best_ask
                                    best_bid = l1.best_bid.unwrap_or_default();
                                    best_ask = l1.best_ask.unwrap_or_default();
                                    last_time = l1.last_update_time;
                                }
                                DataKind::OrderBook(l2) => {
                                    let (asks, bids, midprice) = match l2 {
                                        OrderBookEvent::Snapshot(snapshot) => {
                                            //we save snapshot
                                            //tools::create_book_snapshot(snapshot.asks(), snapshot.bids());
                                            let (asks, bids) = extract_book_l2(snapshot.asks(), snapshot.bids());
                                            (asks, bids, snapshot.mid_price())
                                        }
                                        OrderBookEvent::Update(update) => {
                                            //tools::apply_update()
                                            let (asks, bids) = extract_book_l2(update.asks(), update.bids());

                                            (asks, bids, update.mid_price())
                                        }
                                    };
                                    let asks_json = serde_json::to_string(&asks).expect("not valid json");
                                    let bids_json = serde_json::to_string(&bids).expect("not valid json");
                                    let prim_mid_price = if let Some(oval) = midprice {
                                        oval.to_f64().unwrap_or(0.0)
                                    } else {
                                        0f64
                                    };
                                    db_inner.db_insert_order_book(
                                        ns, ex,
                                        instrument.as_str(),
                                        prim_mid_price,
                                        asks_json.as_str(), bids_json.as_str(), &ev.kind).expect("Failed to insert order book");
                                }

                                // Trade “simple”
                                DataKind::Trade(tr) => {
                                    let side = if  tr.side == Side::Buy { "bid" } else { "ask" };
                                    db_inner.db_insert_trade(ns, ex,
                                                             instrument.as_str(), side, tr.price, tr.amount, &ev.kind).expect("Failed to insert trade");
                                }

                                // Candle, Ticker, etc.: guarda crudo + lo mínimo
                                other => {
                                    db_inner.db_insert_market_other(ns, ex,
                                                                    instrument.as_str(), other.kind_name(), other).expect("Failed to insert market data");
                                }
                            }

                        }
                    } else {
                        if let BackgroundMessage::Account(acc) = val.clone() {
                            //send to ws
                            if let Some(sender) = sender_to_ws.clone() {
                                if let Err(er) = sender.send(serde_json::to_string(&val).unwrap()) {
                                    println!("Error sending background message: {:?}", er);
                                }
                            }
                            match acc {
                                ReEvent::Item(ac) => {
                                    match ac.kind.clone() {
                                        AccountEventKind::OrderSnapshot(order) => {
                                            let (exchange, instrument, quantity, price, evt_kind, time_to_force, side, strategy, ts) = extract_order_snapshot(order);
                                            let ns = ts.timestamp_nanos_opt().expect("valid ts");
                                            /* save orders that our strategies are doing */
                                            operations.lock().await.push(
                                                Fill{
                                                    price, quantity, fee: 0., side: side.clone(), strategy: strategy.clone(), ts,
                                                    best_bid, best_ask, last_time
                                            });
                                            db_inner.db_insert_order_account(ns, exchange.as_str(), instrument.as_str(), quantity, price, evt_kind.as_str(), time_to_force.as_str(), side.as_str(), strategy.as_str()).expect("db worker error");
                                        }
                                        AccountEventKind::OrderCancelled(cancel) => {
                                            let (ex, instrument, strategy, ts) = extract_order_cancel(cancel);
                                            let ns = ts.timestamp_nanos_opt().expect("valid ts");
                                            db_inner.db_insert_order_cancel(ns, ex.as_str(), instrument.as_str(), strategy.as_str()).expect("db worker error");

                                        }
                                        _ => {}

                                    }
                                }
                                _ => {}
                            };

                         }
                    }
                }

            }
        });
        Ok(audit_task)
    }

    pub async fn start_audit(&mut self, sender_to_ws: Option<broadcast::Sender<String>>) -> anyhow::Result<()> {
        let Some(system) = &mut self.system else {
            return Err(anyhow::Error::msg("system is not set"));
        };
        if self.audit_mode == AuditMode::Disabled {
            return Err(anyhow::Error::msg("Audit mode is disabled"));
        };

        let (tx, rx): (Sender<BackgroundMessage>, Receiver<BackgroundMessage>) = mpsc::channel(1024);

        Worker::broadcast_worker(rx, self.db_config.as_str(), sender_to_ws, self.operations.clone(), self.is_raw)?;

        let audit = system.audit.take().unwrap();
        let audit_task = tokio::spawn(async move {
            let mut audit_stream = audit.updates.into_stream();
            while let Some(audit) = audit_stream.next().await {
                debug!(?audit.context, "AuditStream consumed AuditTick");
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