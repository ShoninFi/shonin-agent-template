//! The rebalancer: holds a target exposure to one market, and trades back
//! towards it when the market moves it too far away.
//!
//! This file is the waiting and the sending. The agent is one long-lived
//! program: it waits on the venue's stream of the vault's own account and on
//! the vault's own events, and each time something happens it reads the
//! account and takes one step. It never polls and never sleeps on a timer, so
//! a quiet market costs nothing.
//!
//! Every decision is in `decide.rs`, as plain functions you can test, and its
//! settings are in `config.rs`.

pub mod config;
pub mod decide;

use futures::StreamExt;
use strategy_sdk::{self as sdk, AccountState, Action, from_payload, to_payload, vault, venues};

use decide::{Settings, flatten, rebalance};

/// The venue this agent trades, as its manifest names it.
const VENUE: &str = "hyperliquid";

/// How many steps closing out may take before leaving the rest to the vault
/// contract, which closes anything left through the same careful path.
const CLOSE_OUT_STEPS: usize = 32;

struct Rebalancer;

/// What woke the agent up.
enum Woke {
    /// Something happened to the vault's account at the venue: a fill, or a
    /// price move.
    Venue,
    /// Something happened to the vault itself.
    Vault(vault::VaultEvent),
}

impl sdk::Strategy for Rebalancer {
    // The SDK still hands `run` its configuration as bytes; the settings now
    // live in `config.rs`, so there's nothing to read here.
    async fn run(_config: Vec<u8>) -> Result<(), String> {
        let settings = Settings::default();

        // Whatever a previous run left behind is still on the venue, so look
        // before doing anything.
        step(&settings).await?;

        let account = venues::account_events(VENUE)
            .into_stream()
            .map(|_| Woke::Venue);
        let vault_events = vault::events().into_stream().map(Woke::Vault);
        let mut waking = futures::stream::select(account, vault_events);

        while let Some(woke) = waking.next().await {
            match woke {
                // A fill or a price move: look again.
                Woke::Venue => step(&settings).await?,
                // A Depositor is leaving, and the vault is closing their share
                // of the position. Let it, then look again at what's left.
                Woke::Vault(vault::VaultEvent::ClaimPending(_)) => step(&settings).await?,
                // The vault is stopping: close out rather than be closed out.
                Woke::Vault(vault::VaultEvent::Halting(_)) => {
                    close_out(&settings).await?;
                    return Ok(());
                }
            }
        }
        Ok(())
    }
}

/// Read the account, decide, and send. There's nothing to remember between
/// steps: every decision starts from what the venue says is there, so a
/// rejected order or a restart never leaves the agent believing something
/// that isn't true.
async fn step(settings: &Settings) -> Result<(), String> {
    let state = read_state().await?;
    submit(&rebalance(&state, settings)).await;
    Ok(())
}

async fn close_out(settings: &Settings) -> Result<(), String> {
    for _ in 0..CLOSE_OUT_STEPS {
        let actions = flatten(&read_state().await?, settings);
        if actions.is_empty() {
            return Ok(());
        }
        submit(&actions).await;
    }
    Ok(())
}

async fn read_state() -> Result<AccountState, String> {
    from_payload(&venues::account_state(VENUE.to_string(), Vec::new()).await?)
}

/// Send what was decided. A rejection isn't a reason to stop: the next event
/// is another chance, and an agent that gave up on one refusal would stop
/// trading the first time a market moved.
async fn submit(actions: &[Action]) {
    if actions.is_empty() {
        return;
    }
    let encoded: Vec<Vec<u8>> = actions.iter().map(to_payload).collect();
    let _verdicts = venues::submit(VENUE.to_string(), encoded).await;
}

sdk::export_strategy!(Rebalancer);
