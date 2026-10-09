//! The rebalancer's decisions, tested without a venue.
//!
//! Each test builds the account as the venue would report it, and the
//! market's best prices, asks the agent what it would do, and checks the
//! answer. Add your own the same way.

use rebalancer::decide::{Quote, Settings, exposure, flatten, rebalance};
use strategy_sdk::decimal;
use strategy_sdk::hyperliquid::{AccountState, Action, Order, Position, Side};

/// The settings the template ships with.
fn settings() -> Settings {
    Settings::default()
}

/// ETH's book: 1,999 bid, 2,001 asked, so a mid of 2,000.
fn quote() -> Quote {
    Quote {
        bid: Some(decimal("1999")),
        ask: Some(decimal("2001")),
    }
}

/// A vault worth `vault` in total, with `idle` of it at home, holding
/// `position` of ETH, and nothing free at the venue.
fn account(vault: &str, idle: &str, position: &str) -> AccountState {
    AccountState {
        vault_value: decimal(vault),
        idle: decimal(idle),
        station_idle: decimal("0"),
        account_value: decimal("0"),
        withdrawable: decimal("0"),
        positions: vec![Position {
            market: "ETH".to_string(),
            size: decimal(position),
            entry_price: None,
            leverage: 1,
        }],
        spot: Vec::new(),
        open_orders: Vec::new(),
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
    assert_eq!(exposure(&state, "ETH", &quote()), Some(decimal("0.5")));
    assert!(rebalance(&state, &quote(), &settings()).is_empty());
}

#[test]
fn below_the_target_it_brings_dollars_and_buys_one_capped_step() {
    // Nothing held yet: the gap is half the vault, 10,000, but one order is
    // at most 5% of the vault, 1,000.
    let state = account("20000", "5000", "0");
    let step = rebalance(&state, &quote(), &settings());
    assert_eq!(
        step.fund,
        decimal("1000"),
        "the order's dollars from the vault"
    );
    let order = the_order(&step.actions);
    assert_eq!(order.side, Side::Buy);
    // 1,000 worth at the ask, cut to the venue's four decimals.
    assert_eq!(order.size, decimal("0.4997"));
    assert!(!order.reduce_only);
}

#[test]
fn above_the_target_it_sells_without_going_short() {
    // 8 ETH is 16,000: 80% of the vault, well above half.
    let state = account("20000", "1000", "8");
    let step = rebalance(&state, &quote(), &settings());
    assert_eq!(step.fund, decimal("0"));
    let order = the_order(&step.actions).clone();
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
    assert!(rebalance(&state, &quote(), &short).is_empty());
}

#[test]
fn no_book_yet_means_no_trade() {
    let state = account("20000", "1000", "0");
    assert!(rebalance(&state, &Quote::default(), &settings()).is_empty());
}

#[test]
fn an_order_under_the_venue_s_minimum_is_not_sent() {
    // A vault of 200: a step is at most 10 dollars.
    let state = account("200", "200", "0");
    assert!(rebalance(&state, &quote(), &settings()).is_empty());
}

#[test]
fn closing_out_sells_everything_at_once() {
    let order = the_order(&flatten(
        &account("20000", "1000", "5"),
        &quote(),
        &settings(),
    ))
    .clone();
    assert_eq!(order.size, decimal("5"));
    assert!(order.reduce_only);
}
