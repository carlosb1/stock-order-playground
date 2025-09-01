use std::sync::Arc;
use barter::strategy::algo::AlgoStrategy;
use barter::engine::state::EngineState;
use barter::engine::state::global::DefaultGlobalData;
use barter_execution::order::request::{OrderRequestCancel, OrderRequestOpen, RequestOpen};
use barter_instrument::exchange::{ExchangeId, ExchangeIndex};
use barter_instrument::instrument::InstrumentIndex;
use barter::engine::state::instrument::filter::InstrumentFilter;
use barter_execution::order::{OrderKey, OrderKind, TimeInForce};
use barter_instrument::Side;
use barter_execution::trade::TradeId;
use barter::strategy::on_disconnect::OnDisconnectStrategy;
use barter::engine::clock::LiveClock;
use barter::engine::Engine;
use barter::engine::execution_tx::MultiExchangeTxMap;
use barter::EngineEvent;
use barter::execution::request::ExecutionRequest;
use barter::risk::DefaultRiskManager;
use barter_execution::order::id::{ClientOrderId, OrderId, StrategyId};
use barter::strategy::on_trading_disabled::OnTradingDisabled;
use barter::strategy::close_positions::{close_open_positions_with_market_orders, ClosePositionsStrategy};
use barter_data::event::DataKind;
use barter_instrument::asset::AssetIndex;
use barter_integration::channel::UnboundedTx;
use dyn_clone::DynClone;
use rust_decimal::Decimal;
use crate::intrument_data::InstrumentMarketDataL2;
use rust_decimal_macros::dec;
use crate::strategies::dummy_strategy::Operation::Buy;

pub type MyState  = EngineState<DefaultGlobalData, InstrumentMarketDataL2>;
pub type MyRisk   = DefaultRiskManager<MyState>;
pub type MyExecTx  = MultiExchangeTxMap<UnboundedTx<ExecutionRequest>>;
pub type MyEvent  = EngineEvent<DataKind>;
//pub type MyEngine = Engine<LiveClock, MyState, MyExecTx, MyStrategy, MyRisk>;
pub type MyEngine = Engine<LiveClock, MyState, MyExecTx, MyStrategy, MyRisk>;

pub enum Operation {
    Buy(Decimal, Decimal),
    Mocked,
    Other,
}

pub trait ModelDecider: Send + Sync {
    fn run(&self, bids: &[(Decimal, Decimal)], asks: &[(Decimal, Decimal)]) -> Operation;
}

#[derive(Clone)]
pub struct DummyDecider;
impl ModelDecider for DummyDecider {
    fn run(&self, _bids: &[(Decimal, Decimal)], _asks: &[(Decimal, Decimal)]) -> Operation {
        Operation::Buy(dec!(0.0), dec!(0.0))
    }
}

#[derive(Clone)]
pub struct MockDecider;
impl ModelDecider for MockDecider {
    fn run(&self, _bids: &[(Decimal, Decimal)], _asks: &[(Decimal, Decimal)]) -> Operation {
        Operation::Mocked
    }
}


#[derive(Clone)]
pub struct MyStrategy {
    pub id: StrategyId,
    pub wrapper: Arc<dyn ModelDecider + Send + Sync>,
}

impl MyStrategy {
    pub fn new<D>(decide: D) -> Self where
        D: ModelDecider + 'static, {
        Self { id: strategy_id(), wrapper: Arc::new(decide) }
    }
}

pub fn strategy_id() -> StrategyId {
    let str = smol_str::SmolStr::from("MyStrategy");
    StrategyId::from(str)
}

fn gen_cid(instrument: usize) -> ClientOrderId {
    ClientOrderId::new(InstrumentIndex(instrument).to_string())
}

fn gen_trade_id(instrument: usize) -> TradeId {
    TradeId::new(InstrumentIndex(instrument).to_string())
}

fn gen_order_id(instrument: usize) -> OrderId {
    OrderId::new(InstrumentIndex(instrument).to_string())
}

impl AlgoStrategy for MyStrategy {
    type State = MyState;

    fn generate_algo_orders(&self, state: &Self::State) -> (impl IntoIterator<Item=OrderRequestCancel<ExchangeIndex, InstrumentIndex>>, impl IntoIterator<Item=OrderRequestOpen<ExchangeIndex, InstrumentIndex>>) {

        let opens = state
            .instruments
            .instruments(&InstrumentFilter::None)
            .filter_map(|state| {
                if let Some((asks, bids)) = state.data.l2() {
                    self.wrapper.run(asks, bids);
                    //println!("/////////////////////////////////");
                    //println!("-->{:?}", asks);
                    //println!("-->{:?}", bids);
                    //println!("/////////////////////////////////");

                }

//                let ob=state.data.l1.clone();
//                println!("->{:?}", ob);
                // Don't open more if we have a Position already
                if state.position.current.is_some() {
                    return None;
                }

                // Don't open more orders if there are already some InFlight
                if !state.orders.0.is_empty() {
                    return None;
                }

                // Don't open if there is no instrument market price available
                let price = state.data.price()?;

                // Generate Market order to buy the minimum allowed quantity
                Some(OrderRequestOpen {
                    key: OrderKey {
                        exchange: state.instrument.exchange,
                        instrument: state.key,
                        strategy: self.id.clone(),
                        cid: gen_cid(state.key.index()),
                    },
                    state: RequestOpen {
                        side: Side::Buy,
                        kind: OrderKind::Market,
                        time_in_force: TimeInForce::ImmediateOrCancel, // inmend
                        price,
                        quantity: dec!(3),
                    },
                })
            });

        // cancel order, execute orders
        //(std::iter::empty(), std::iter::empty())
        (std::iter::empty(), opens)
    }
}

// Cerrar posiciones: usa helper listo (market + IOC por cada posición abierta)
impl ClosePositionsStrategy for MyStrategy {
    type State = MyState;

    fn close_positions_requests<'a>(
        &'a self,
        state: &'a Self::State,
        filter: &'a InstrumentFilter<ExchangeIndex, AssetIndex, InstrumentIndex>,
    ) -> (
        impl IntoIterator<Item = OrderRequestCancel<ExchangeIndex, InstrumentIndex>> + 'a,
        impl IntoIterator<Item = OrderRequestOpen<ExchangeIndex, InstrumentIndex>> + 'a,
    )
    where
        ExchangeIndex: 'a,
        AssetIndex: 'a,
        InstrumentIndex: 'a,
    {
        close_open_positions_with_market_orders(&self.id, state, filter, |state| {
            ClientOrderId::new(state.key.to_string())
        })
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct OnDisconnectOutput;

// Al desconectar del exchange: cancela y cierra de forma segura (puedes dejar no-op si prefieres)
impl OnDisconnectStrategy<
    LiveClock,
    EngineState<DefaultGlobalData, InstrumentMarketDataL2>,
    MyExecTx,
    MyRisk,
> for MyStrategy{
    type OnDisconnect = OnDisconnectOutput;

    fn on_disconnect(
        engine: &mut MyEngine,
        _exchange: ExchangeId,
    ) -> Self::OnDisconnect {
        OnDisconnectOutput
    }
}

#[derive(Debug, PartialEq, Clone)]
pub struct OnTradingDisabledOutput;

// Al deshabilitar trading: idem
impl OnTradingDisabled<
    LiveClock,
    MyState,
    MyExecTx,
    MyRisk,
> for MyStrategy {
    type OnTradingDisabled = OnTradingDisabledOutput;

    fn on_trading_disabled(
        engine: &mut MyEngine,
    ) -> Self::OnTradingDisabled {
        OnTradingDisabledOutput
    }
}