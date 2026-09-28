//! The rebalancer's decisions, tested without a venue.
//!
//! Each test builds the account as a venue would report it, asks the agent
//! what it would do, and checks the answer. Add your own the same way.

use std::collections::BTreeMap;

use rebalancer::decide::{Settings, exposure, flatten, rebalance};
use strategy_sdk::{
    AccountState, Action, MarketState, Order, PositionState, Side, decimal, to_payload,
};

/// The settings the template ships with.
fn settings() -> Settings {
    Settings::from_bytes(&[]).expect("the defaults read")
}

/// A vault worth `vault` in total, with `idle` of it at home, holding
/// `position` of ETH-PERP at a price of 2,000.
fn account(vault: &str, idle: &str, position: &str) -> AccountState {
    AccountState {
        cash: decimal("0"),
        value: decimal("0"),
        exit_value: decimal("0"),
        vault_value: decimal(vault),
        idle: decimal(idle),
        margin_bps: 1000,
        positions: vec![PositionState {
            market: "ETH-PERP".to_string(),
            size: decimal(position),
        }],
        markets: vec![MarketState {
            market: "ETH-PERP".to_string(),
            is_perp: true,
            mark: decimal("2000"),
            oracle: decimal("2000"),
            bid: decimal("1999"),
            ask: decimal("2001"),
            funding_index: decimal("0"),
            funding_rate: decimal("0"),
        }],
    }
}

fn the_order(actions: &[Action]) -> &Order {
    actions
        .iter()
        .find_map(|action| match action {
            Action::Place(order) => Some(order),
            _ => None,
        })
        .expect("an order")
}

#[test]
fn close_enough_to_the_target_does_nothing() {
    // 5 ETH at 2,000 is 10,000: exactly half of a 20,000 vault.
    let state = account("20000", "1000", "5");
    assert_eq!(exposure(&state, "ETH-PERP"), Some(decimal("0.5")));
    assert!(rebalance(&state, &settings()).is_empty());
}

#[test]
fn below_the_target_it_buys_one_capped_step() {
    // Nothing held yet: the gap is half the vault, 10,000, but one order is
    // at most 5% of the vault, 1,000.
    let state = account("20000", "5000", "0");
    let actions = rebalance(&state, &settings());

    assert!(matches!(
        actions.first(),
        Some(Action::MoveCapital { amount, .. }) if *amount == decimal("1000")
    ));
    let order = the_order(&actions);
    assert_eq!(order.side, Side::Buy);
    // 1,000 worth, at the ask.
    assert_eq!(order.size, decimal("1000") / decimal("2001"));
    assert!(!order.reduce_only);
}

#[test]
fn above_the_target_it_sells_without_going_short() {
    // 8 ETH is 16,000: 80% of the vault, well above half.
    let state = account("20000", "1000", "8");
    let order = the_order(&rebalance(&state, &settings())).clone();
    assert_eq!(order.side, Side::Sell);
    assert!(order.reduce_only);
    assert!(order.size <= decimal("8"));
}

#[test]
fn a_target_above_the_limit_is_held_at_the_limit() {
    let mut greedy = settings();
    greedy.target_exposure = decimal("3");
    assert_eq!(greedy.target(), greedy.max_exposure);
}

#[test]
fn a_negative_target_never_opens_a_short() {
    let mut short = settings();
    short.target_exposure = decimal("-1");
    let state = account("20000", "1000", "0");
    assert!(rebalance(&state, &short).is_empty());
}

#[test]
fn a_market_the_venue_does_not_list_is_left_alone() {
    let mut elsewhere = settings();
    elsewhere.market = "DOGE-PERP".to_string();
    assert!(rebalance(&account("20000", "1000", "0"), &elsewhere).is_empty());
}

#[test]
fn closing_out_sells_everything_at_once() {
    let order = the_order(&flatten(&account("20000", "1000", "5"), &settings())).clone();
    assert_eq!(order.size, decimal("5"));
    assert!(order.reduce_only);
}

/// The defaults in `decide.rs` are what the agent falls back to, and the
/// manifest is what it promises. This fails if the two ever drift apart.
#[test]
fn the_manifest_and_the_defaults_agree() {
    let manifest = include_str!("../shonin.toml");
    let config: BTreeMap<String, String> = manifest
        .split("[config]")
        .nth(1)
        .expect("a [config] table")
        .lines()
        .take_while(|line| !line.trim_start().starts_with('['))
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            let value = value.split('#').next()?.trim().trim_matches('"');
            Some((key.trim().to_string(), value.to_string()))
        })
        .collect();

    let read = Settings::from_bytes(&to_payload(&config)).expect("the manifest reads");
    let defaults = settings();
    assert_eq!(read.market, defaults.market);
    for (name, a, b) in [
        (
            "target_exposure",
            read.target_exposure,
            defaults.target_exposure,
        ),
        ("max_exposure", read.max_exposure, defaults.max_exposure),
        (
            "rebalance_band",
            read.rebalance_band,
            defaults.rebalance_band,
        ),
        (
            "max_order_fraction",
            read.max_order_fraction,
            defaults.max_order_fraction,
        ),
    ] {
        assert_eq!(a, b, "{name} differs between shonin.toml and decide.rs");
    }
}
