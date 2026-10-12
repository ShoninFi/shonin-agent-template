//! The rebalancer's settings, all in one place.
//!
//! Change these to change what it does. Numbers are written as text and read
//! as exact decimals, so two machines can never round them differently.

/// The one market it trades: Hyperliquid's ETH perpetual, by its coin.
pub const MARKET: &str = "ETH";
/// The size decimals the venue allows on it.
pub const SIZE_DECIMALS: u32 = 4;
/// How much of the vault's value to hold there, long: 0.5 is half.
pub const TARGET_EXPOSURE: &str = "0.5";
/// The most it will ever hold, whatever the target says.
pub const MAX_EXPOSURE: &str = "1.0";
/// How far exposure may drift from the target before it trades.
pub const REBALANCE_BAND: &str = "0.05";
/// The largest single order, as a share of the vault's value.
pub const MAX_ORDER_FRACTION: &str = "0.05";
/// The venue refuses an order under ten dollars; below this one isn't sent.
pub const MINIMUM_ORDER: &str = "12";
