//! What the rebalancer decides, as plain functions of what it sees.
//!
//! Nothing in this file waits, reads the venue or sends anything. Each
//! function takes the account as the venue reports it, and the market's best
//! bid and ask, and returns what it would do, so every decision can be tested
//! with plain `cargo test`, on your own machine, before any money is
//! involved.

use strategy_sdk::hyperliquid::{AccountState, Action, Order, Side, Tif};
use strategy_sdk::{Decimal, decimal};

use crate::config;

/// The agent's settings, as decimals. [`Settings::default`] reads them from
/// `config.rs`; tests build their own to try other numbers.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The one market it trades.
    pub market: String,
    /// The size decimals the venue allows on it.
    pub size_decimals: u32,
    /// The share of the vault's value to hold in that market, long.
    pub target_exposure: Decimal,
    /// The most it will ever hold, as a share of the vault's value.
    pub max_exposure: Decimal,
    /// How far exposure may drift from the target before it trades.
    pub rebalance_band: Decimal,
    /// The largest single order, as a share of the vault's value.
    pub max_order_fraction: Decimal,
    /// Below this, in dollars, an order isn't sent: the venue refuses
    /// anything under ten.
    pub minimum_order: Decimal,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            market: config::MARKET.to_string(),
            size_decimals: config::SIZE_DECIMALS,
            target_exposure: decimal(config::TARGET_EXPOSURE),
            max_exposure: decimal(config::MAX_EXPOSURE),
            rebalance_band: decimal(config::REBALANCE_BAND),
            max_order_fraction: decimal(config::MAX_ORDER_FRACTION),
            minimum_order: decimal(config::MINIMUM_ORDER),
        }
    }
}

impl Settings {
    /// The target, kept inside the limits: never short, never above the most
    /// the agent will hold, whatever the settings say.
    pub fn target(&self) -> Decimal {
        clamp(self.target_exposure, zero(), self.max_exposure)
    }
}

/// The market's best prices, as the venue's shared stream last said.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quote {
    pub bid: Option<Decimal>,
    pub ask: Option<Decimal>,
}

impl Quote {
    /// The middle of the book, when both sides are there.
    pub fn mid(&self) -> Option<Decimal> {
        match (self.bid, self.ask) {
            (Some(bid), Some(ask)) if bid > zero() && ask >= bid => {
                Some((bid + ask) / Decimal::from(2))
            }
            _ => None,
        }
    }
}

/// One step: dollars to bring from the vault to the venue first, if any,
/// then the orders.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Step {
    pub fund: Decimal,
    pub actions: Vec<Action>,
}

impl Step {
    pub fn is_empty(&self) -> bool {
        self.fund <= zero() && self.actions.is_empty()
    }
}

/// What the account holds in `market` (positive is long).
pub fn held(state: &AccountState, market: &str) -> Decimal {
    state
        .positions
        .iter()
        .find(|position| position.market == market)
        .map_or_else(zero, |position| position.size)
}

/// How much of the vault's value is held in the market, at the mid: `0.5`
/// means half the vault. `None` when there's nothing to measure against yet.
pub fn exposure(state: &AccountState, market: &str, quote: &Quote) -> Option<Decimal> {
    let mid = quote.mid()?;
    if state.vault_value <= zero() {
        return None;
    }
    Some(held(state, market) * mid / state.vault_value)
}

/// One step back towards the target, inside every limit, or nothing at all
/// when exposure is close enough.
///
/// It only ever takes one step: a single order no bigger than the order limit.
/// The next event is the next step, so a large gap closes over several orders
/// rather than one that moves the market.
pub fn rebalance(state: &AccountState, quote: &Quote, settings: &Settings) -> Step {
    let Some(now) = exposure(state, &settings.market, quote) else {
        // No book yet for the market the manifest names: wait.
        return Step::default();
    };

    let gap = settings.target() - now;
    if abs(gap) < settings.rebalance_band {
        return Step::default();
    }
    let notional = min(
        abs(gap) * state.vault_value,
        state.vault_value * settings.max_order_fraction,
    );
    if notional < settings.minimum_order {
        return Step::default();
    }

    if gap > zero() {
        let Some(ask) = quote.ask else {
            return Step::default();
        };
        // The venue account trades with the dollars it holds, so bring what
        // the order needs from the vault first. The vault never lets its
        // cash fall below the floor its Author set, however much is asked.
        let short = notional - state.withdrawable;
        let fund = if short > zero() {
            min(short, state.idle)
        } else {
            zero()
        };
        let size = round_down(notional / ask, settings.size_decimals);
        if size <= zero() {
            return Step::default();
        }
        Step {
            fund,
            actions: vec![Action::Place(order(settings, Side::Buy, size, ask, false))],
        }
    } else {
        // Selling only ever shrinks what's held: this agent never goes short.
        let held = held(state, &settings.market);
        let Some(bid) = quote.bid else {
            return Step::default();
        };
        if held <= zero() {
            return Step::default();
        }
        let size = round_down(min(notional / bid, held), settings.size_decimals);
        if size <= zero() {
            return Step::default();
        }
        Step {
            fund: zero(),
            actions: vec![Action::Place(order(settings, Side::Sell, size, bid, true))],
        }
    }
}

/// Close whatever is held, all at once. Used when the vault is stopping: the
/// order limit is set aside, because leaving a position open would cost the
/// vault's Depositors more than a larger order will.
pub fn flatten(state: &AccountState, quote: &Quote, settings: &Settings) -> Vec<Action> {
    let held = held(state, &settings.market);
    let Some(bid) = quote.bid else {
        return Vec::new();
    };
    if held <= zero() {
        return Vec::new();
    }
    vec![Action::Place(order(settings, Side::Sell, held, bid, true))]
}

/// An immediate-or-cancel order at `price`: it fills now against the book,
/// up to that price, or not at all.
fn order(
    settings: &Settings,
    side: Side,
    size: Decimal,
    price: Decimal,
    reduce_only: bool,
) -> Order {
    Order {
        market: settings.market.clone(),
        side,
        size,
        limit_price: price,
        tif: Tif::Ioc,
        reduce_only,
        client_id: None,
    }
}

fn zero() -> Decimal {
    Decimal::from(0)
}

fn abs(value: Decimal) -> Decimal {
    if value < zero() { -value } else { value }
}

fn min(a: Decimal, b: Decimal) -> Decimal {
    if a < b { a } else { b }
}

fn clamp(value: Decimal, low: Decimal, high: Decimal) -> Decimal {
    if value < low {
        low
    } else if value > high {
        high
    } else {
        value
    }
}

/// `size` cut to the venue's size decimals, never up.
fn round_down(size: Decimal, decimals: u32) -> Decimal {
    let scale = Decimal::from(10u64.pow(decimals));
    (size * scale).floor() / scale
}
