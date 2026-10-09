//! The rebalancer: holds a target exposure to one market, and trades back
//! towards it when the market moves it too far away.
//!
//! This file is the waiting and the sending. The agent is one long-lived
//! program: it waits on the market's book from the venue's shared stream, on
//! the vault's own account at the venue and on the vault's own events, and
//! each time something happens it reads the account and takes one step. It
//! never polls and never sleeps on a timer, so a quiet market costs nothing.
//!
//! Every decision is in `decide.rs`, as plain functions you can test, and its
//! settings are in `config.rs`.

pub mod config;
pub mod decide;

use futures::StreamExt;
use strategy_sdk::hyperliquid::{self as hl, AccountState, Action};
use strategy_sdk::streams::{self, HYPERLIQUID, HyperliquidFeed, HyperliquidValue};
use strategy_sdk::transfers::{self, Account, VenueAccount};
use strategy_sdk::{self as sdk, drivers, from_payload, millionths, to_payload, vault};

use decide::{Quote, Settings, flatten, rebalance};

/// The venue this agent trades, as its manifest names it.
const VENUE: &str = hl::DRIVER;

/// How many steps closing out may take before leaving the rest to the vault
/// contract, which closes anything left through the same careful path.
const CLOSE_OUT_STEPS: usize = 32;

struct Rebalancer;

/// What woke the agent up.
enum Woke {
    /// The market's book moved.
    Book(Quote),
    /// Something happened to the vault's account at the venue: a fill.
    Venue,
    /// Something happened to the vault itself.
    Vault(vault::VaultEvent),
    /// A stream value that didn't read: ignored.
    Nothing,
}

impl sdk::Agent for Rebalancer {
    async fn run() -> Result<(), String> {
        let settings = Settings::default();
        let mut quote = Quote::default();

        let book = streams::subscribe::<_, HyperliquidValue>(
            HYPERLIQUID,
            &HyperliquidFeed::Book {
                coin: settings.market.clone(),
            },
        )?
        .map(|value| match value.map(|stamped| stamped.value) {
            Ok(HyperliquidValue::Book(book)) => Woke::Book(Quote {
                bid: book.bids.first().map(|level| level.price),
                ask: book.asks.first().map(|level| level.price),
            }),
            _ => Woke::Nothing,
        });
        let account = drivers::account_events(VENUE)
            .into_stream()
            .map(|_| Woke::Venue);
        let vault_events = vault::events().into_stream().map(Woke::Vault);
        let mut waking = futures::stream::select_all([
            book.boxed_local(),
            account.boxed_local(),
            vault_events.boxed_local(),
        ]);

        while let Some(woke) = waking.next().await {
            match woke {
                // The book moved: look again.
                Woke::Book(latest) => {
                    quote = latest;
                    step(&settings, &quote).await?;
                }
                // A fill: look again.
                Woke::Venue => step(&settings, &quote).await?,
                // A Depositor is leaving, and the vault is closing their share
                // of the position. Let it, then look again at what's left.
                Woke::Vault(vault::VaultEvent::ClaimPending(_)) => step(&settings, &quote).await?,
                // The vault is stopping: close out rather than be closed out.
                Woke::Vault(vault::VaultEvent::Halting(_)) => {
                    close_out(&settings, &quote).await?;
                    return Ok(());
                }
                Woke::Vault(_) | Woke::Nothing => {}
            }
        }
        Ok(())
    }
}

/// Read the account, decide, and send. There's nothing to remember between
/// steps: every decision starts from what the venue says is there, so a
/// rejected order or a restart never leaves the agent believing something
/// that isn't true.
async fn step(settings: &Settings, quote: &Quote) -> Result<(), String> {
    let state = read_state().await?;
    let step = rebalance(&state, quote, settings);
    if step.fund > sdk::Decimal::from(0) {
        let venue = Account::Venue(VenueAccount {
            chain: transfers::home_chain(),
            driver: VENUE.to_string(),
        });
        // Money only ever moves between the vault's own accounts. A move
        // that doesn't go through leaves the order for the next step.
        if !matches!(
            transfers::move_(Account::Vault, venue, millionths(step.fund)).await,
            transfers::Moved::Done
        ) {
            return Ok(());
        }
    }
    submit(&step.actions).await;
    Ok(())
}

async fn close_out(settings: &Settings, quote: &Quote) -> Result<(), String> {
    for _ in 0..CLOSE_OUT_STEPS {
        let actions = flatten(&read_state().await?, quote, settings);
        if actions.is_empty() {
            return Ok(());
        }
        submit(&actions).await;
    }
    Ok(())
}

async fn read_state() -> Result<AccountState, String> {
    from_payload(&drivers::account_state(VENUE.to_string(), Vec::new()).await?)
}

/// Send what was decided. A rejection isn't a reason to stop: the next event
/// is another chance, and an agent that gave up on one refusal would stop
/// trading the first time a market moved.
async fn submit(actions: &[Action]) {
    if actions.is_empty() {
        return;
    }
    let encoded: Vec<Vec<u8>> = actions.iter().map(to_payload).collect();
    let _verdicts = drivers::submit(VENUE.to_string(), encoded).await;
}

sdk::export_agent!(Rebalancer);
