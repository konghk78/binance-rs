use crate::errors::Result;
use crate::config::Config;
use crate::model::{
    AccountUpdateEvent, AggrTradesEvent, BookTickerEvent, ContinuousKlineEvent, DayTickerEvent,
    DepthOrderBookEvent, IndexKlineEvent, IndexPriceEvent, KlineEvent, LiquidationEvent,
    MarkPriceEvent, MiniTickerEvent, OrderBook, TradeEvent, UserDataStreamExpiredEvent,
};
use crate::futures::model;
use error_chain::bail;
use url::Url;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::net::TcpStream;
use tungstenite::{connect, Message};
use tungstenite::protocol::WebSocket;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::handshake::client::Response;
#[allow(clippy::all)]
enum FuturesWebsocketAPI {
    Default,
    MultiStream,
    Custom(String),
}

pub enum FuturesMarket {
    USDM,
    COINM,
    Vanilla,
}

impl FuturesWebsocketAPI {
    fn params(self, market: &FuturesMarket, subscription: &str, config: &Config) -> String {
        let baseurl = match market {
            FuturesMarket::USDM => config.futures_ws_endpoint.as_str(),
            FuturesMarket::COINM => config.futures_ws_endpoint_coin_m.as_str(),
            FuturesMarket::Vanilla => config.futures_ws_endpoint_vanilla.as_str(),
        };

        match self {
            FuturesWebsocketAPI::Default => {
                format!("{}/ws/{}", baseurl, subscription)
            }
            FuturesWebsocketAPI::MultiStream => {
                format!("{}/stream?streams={}", baseurl, subscription)
            }
            FuturesWebsocketAPI::Custom(url) => format!("{}/{}", url, subscription),
        }
    }
}

#[allow(clippy::large_enum_variant)]
#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum FuturesWebsocketEvent {
    AccountUpdate(AccountUpdateEvent),
    OrderTrade(model::OrderTradeEvent),
    AggrTrades(AggrTradesEvent),
    Trade(TradeEvent),
    OrderBook(OrderBook),
    DayTicker(DayTickerEvent),
    MiniTicker(MiniTickerEvent),
    MiniTickerAll(Vec<MiniTickerEvent>),
    IndexPrice(IndexPriceEvent),
    MarkPrice(MarkPriceEvent),
    MarkPriceAll(Vec<MarkPriceEvent>),
    DayTickerAll(Vec<DayTickerEvent>),
    Kline(KlineEvent),
    ContinuousKline(ContinuousKlineEvent),
    IndexKline(IndexKlineEvent),
    Liquidation(LiquidationEvent),
    DepthOrderBook(DepthOrderBookEvent),
    BookTicker(BookTickerEvent),
    UserDataStreamExpiredEvent(UserDataStreamExpiredEvent),
}

pub struct FuturesWebSockets<'a> {
    pub socket: Option<(WebSocket<MaybeTlsStream<TcpStream>>, Response)>,
    handler: Box<dyn FnMut(FuturesWebsocketEvent) -> Result<()> + 'a>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum FuturesEvents {
    Vec(Vec<DayTickerEvent>),
    DayTickerEvent(DayTickerEvent),
    BookTickerEvent(BookTickerEvent),
    MiniTickerEvent(MiniTickerEvent),
    VecMiniTickerEvent(Vec<MiniTickerEvent>),
    AccountUpdateEvent(AccountUpdateEvent),
    OrderTradeEvent(model::OrderTradeEvent),
    AggrTradesEvent(AggrTradesEvent),
    IndexPriceEvent(IndexPriceEvent),
    MarkPriceEvent(MarkPriceEvent),
    VecMarkPriceEvent(Vec<MarkPriceEvent>),
    TradeEvent(TradeEvent),
    KlineEvent(KlineEvent),
    ContinuousKlineEvent(ContinuousKlineEvent),
    IndexKlineEvent(IndexKlineEvent),
    LiquidationEvent(LiquidationEvent),
    OrderBook(OrderBook),
    DepthOrderBookEvent(DepthOrderBookEvent),
    UserDataStreamExpiredEvent(UserDataStreamExpiredEvent),
}

impl<'a> FuturesWebSockets<'a> {
    pub fn new<Callback>(handler: Callback) -> FuturesWebSockets<'a>
    where
        Callback: FnMut(FuturesWebsocketEvent) -> Result<()> + 'a,
    {
        FuturesWebSockets {
            socket: None,
            handler: Box::new(handler),
        }
    }

    pub fn connect(&mut self, market: &FuturesMarket, subscription: &str) -> Result<()> {
        self.connect_wss(&FuturesWebsocketAPI::Default.params(
            market,
            subscription,
            &Config::default(),
        ))
    }

    pub fn connect_with_config(
        &mut self, market: &FuturesMarket, subscription: &str, config: &Config,
    ) -> Result<()> {
        self.connect_wss(&FuturesWebsocketAPI::Default.params(market, subscription, config))
    }

    pub fn connect_multiple_streams(
        &mut self, market: &FuturesMarket, endpoints: &[String],
    ) -> Result<()> {
        self.connect_wss(&FuturesWebsocketAPI::MultiStream.params(
            market,
            &endpoints.join("/"),
            &Config::default(),
        ))
    }

    pub fn connect_multiple_streams_with_config(
        &mut self, market: &FuturesMarket, endpoints: &[String], config: &Config,
    ) -> Result<()> {
        self.connect_wss(&FuturesWebsocketAPI::MultiStream.params(
            market,
            &endpoints.join("/"),
            config,
        ))
    }

    fn connect_wss(&mut self, wss: &str) -> Result<()> {
        let url = Url::parse(wss)?;
        match connect(url) {
            Ok(answer) => {
                self.socket = Some(answer);
                Ok(())
            }
            Err(e) => bail!(format!("Error during handshake {}", e)),
        }
    }

    pub fn disconnect(&mut self) -> Result<()> {
        if let Some(ref mut socket) = self.socket {
            socket.0.close(None)?;
            return Ok(());
        }
        bail!("Not able to close the connection");
    }

    pub fn test_handle_msg(&mut self, msg: &str) -> Result<()> {
        self.handle_msg(msg)
    }

    fn handle_msg(&mut self, msg: &str) -> Result<()> {
        let value: serde_json::Value = serde_json::from_str(msg)?;

        if let Some(data) = value.get("data") {
            self.handle_msg(&data.to_string())?;
            return Ok(());
        }

        if let Ok(events) = serde_json::from_value::<FuturesEvents>(value) {
            let action = match events {
                FuturesEvents::Vec(v) => FuturesWebsocketEvent::DayTickerAll(v),
                FuturesEvents::DayTickerEvent(v) => FuturesWebsocketEvent::DayTicker(v),
                FuturesEvents::BookTickerEvent(v) => FuturesWebsocketEvent::BookTicker(v),
                FuturesEvents::MiniTickerEvent(v) => FuturesWebsocketEvent::MiniTicker(v),
                FuturesEvents::VecMiniTickerEvent(v) => FuturesWebsocketEvent::MiniTickerAll(v),
                FuturesEvents::AccountUpdateEvent(v) => FuturesWebsocketEvent::AccountUpdate(v),
                FuturesEvents::OrderTradeEvent(v) => FuturesWebsocketEvent::OrderTrade(v),
                FuturesEvents::IndexPriceEvent(v) => FuturesWebsocketEvent::IndexPrice(v),
                FuturesEvents::MarkPriceEvent(v) => FuturesWebsocketEvent::MarkPrice(v),
                FuturesEvents::VecMarkPriceEvent(v) => FuturesWebsocketEvent::MarkPriceAll(v),
                FuturesEvents::TradeEvent(v) => FuturesWebsocketEvent::Trade(v),
                FuturesEvents::ContinuousKlineEvent(v) => FuturesWebsocketEvent::ContinuousKline(v),
                FuturesEvents::IndexKlineEvent(v) => FuturesWebsocketEvent::IndexKline(v),
                FuturesEvents::LiquidationEvent(v) => FuturesWebsocketEvent::Liquidation(v),
                FuturesEvents::KlineEvent(v) => FuturesWebsocketEvent::Kline(v),
                FuturesEvents::OrderBook(v) => FuturesWebsocketEvent::OrderBook(v),
                FuturesEvents::DepthOrderBookEvent(v) => FuturesWebsocketEvent::DepthOrderBook(v),
                FuturesEvents::AggrTradesEvent(v) => FuturesWebsocketEvent::AggrTrades(v),
                FuturesEvents::UserDataStreamExpiredEvent(v) => {
                    FuturesWebsocketEvent::UserDataStreamExpiredEvent(v)
                }
            };
            (self.handler)(action)?;
        }
        Ok(())
    }

    pub fn event_loop(&mut self, running: &AtomicBool) -> Result<()> {
        while running.load(Ordering::Relaxed) {
            if let Some(ref mut socket) = self.socket {
                let message = socket.0.read_message()?;
                match message {
                    Message::Text(msg) => {
                        if let Err(e) = self.handle_msg(&msg) {
                            bail!(format!("Error on handling stream message: {}", e));
                        }
                    }
                    Message::Ping(_) => {
                        socket.0.write_message(Message::Pong(vec![])).unwrap();
                    }
                    Message::Pong(_) | Message::Binary(_) | Message::Frame(_) => (),
                    Message::Close(e) => bail!(format!("Disconnected {:?}", e)),
                }
            }
        }
        bail!("running loop closed");
    }
}
