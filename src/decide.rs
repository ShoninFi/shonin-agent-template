//! What the rebalancer decides, as plain functions of what it sees.
//!
//! Nothing in this file waits, reads the venue or sends anything. Each
//! function takes the account as the venue reports it and returns the orders
//! it would place, so every decision can be tested with plain `cargo test`,
//! on your own machine, before any money is involved.

use std::collections::BTreeMap;

use strategy_sdk::{Account, AccountState, Action, Decimal, Order, Side, from_payload};

/// The manifest's `[config]`, as the agent reads it.
#[derive(Clone, Debug)]
pub struct Settings {
    /// The one market it trades.
    pub market: String,
    /// The share of the vault's value to hold in that market, long.
    pub target_exposure: Decimal,
    /// The most it will ever hold, as a share of the vault's value.
    pub max_exposure: Decimal,
    /// How far exposure may drift from the target before it trades.
    pub rebalance_band: Decimal,
    /// The largest single order, as a share of the vault's value.
    pub max_order_fraction: Decimal,
}

impl Settings {
    /// Read what `run` is handed: the manifest's `[config]` as a map of
    /// strings, with decimals as text so two machines can't round the same
    /// number differently. Anything missing or unreadable falls back to the
    /// default written here, rather than stopping the agent.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, String> {
        let map: BTreeMap<String, String> = if bytes.is_empty() {
            BTreeMap::new()
        } else {
            from_payload(bytes)?
        };
        let number = |key: &str, fallback: &str| -> Decimal {
            map.get(key)
                .and_then(|text| text.parse().ok())
                .unwrap_or_else(|| fallback.parse().expect("a default written down here"))
        };
        Ok(Self {
            market: map
                .get("market")
                .cloned()
                .unwrap_or_else(|| "ETH-PERP".to_string()),
            target_exposure: number("target_exposure", "0.5"),
            max_exposure: number("max_exposure", "1.0"),
            rebalance_band: number("rebalance_band", "0.05"),
            max_order_fraction: number("max_order_fraction", "0.05"),
        })
    }

    /// The target, kept inside the limits: never short, never above the most
    /// the agent will hold, whatever the config says.
    pub fn target(&self) -> Decimal {
        clamp(self.target_exposure, zero(), self.max_exposure)
    }
}

/// How much of the vault's value is held in the market, at mark: `0.5` means
/// half the vault. `None` when there's nothing to measure against yet.
pub fn exposure(state: &AccountState, market: &str) -> Option<Decimal> {
    let price = state.market(market)?.mark;
    if state.vault_value <= zero() || price <= zero() {
        return None;
    }
    Some(state.position(market) * price / state.vault_value)
}

/// One step back towards the target, inside every limit, or nothing at all
/// when exposure is close enough.
///
/// It only ever takes one step: a single order no bigger than the order limit.
/// The next event is the next step, so a large gap closes over several orders
/// rather than one that moves the market.
pub fn rebalance(state: &AccountState, settings: &Settings) -> Vec<Action> {
    let (Some(quote), Some(now)) = (
        state.market(&settings.market),
        exposure(state, &settings.market),
    ) else {
        // The venue hasn't listed the market the manifest names. Trading
        // something else wouldn't be this agent, so it waits.
        return Vec::new();
    };

    let gap = settings.target() - now;
    if abs(gap) < settings.rebalance_band {
        return Vec::new();
    }
    let notional = min(
        abs(gap) * state.vault_value,
        state.vault_value * settings.max_order_fraction,
    );

    if gap > zero() {
        if quote.ask <= zero() {
            return Vec::new();
        }
        let mut actions = Vec::new();
        // The venue holds the money it trades with, so bring some from home
        // first. The vault contract never lets it take cash below the floor
        // its Author set, however much is asked for.
        let moving = min(notional, state.idle);
        if moving > zero() {
            actions.push(Action::MoveCapital {
                from: Account::Idle,
                to: Account::Venue,
                amount: moving,
            });
        }
        actions.push(Action::Place(Order {
            market: settings.market.clone(),
            side: Side::Buy,
            size: notional / quote.ask,
            limit_price: quote.ask,
            reduce_only: false,
        }));
        actions
    } else {
        // Selling only ever shrinks what's held: this agent never goes short.
        let held = state.position(&settings.market);
        if held <= zero() || quote.bid <= zero() {
            return Vec::new();
        }
        vec![Action::Place(Order {
            market: settings.market.clone(),
            side: Side::Sell,
            size: min(notional / quote.bid, held),
            limit_price: quote.bid,
            reduce_only: true,
        })]
    }
}

/// Close whatever is held, all at once. Used when the vault is stopping: the
/// order limit is set aside, because leaving a position open would cost the
/// vault's Depositors more than a larger order will.
pub fn flatten(state: &AccountState, settings: &Settings) -> Vec<Action> {
    let held = state.position(&settings.market);
    let Some(quote) = state.market(&settings.market) else {
        return Vec::new();
    };
    if held <= zero() || quote.bid <= zero() {
        return Vec::new();
    }
    vec![Action::Place(Order {
        market: settings.market.clone(),
        side: Side::Sell,
        size: held,
        limit_price: quote.bid,
        reduce_only: true,
    })]
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
