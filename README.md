# Shonin agent template

A small, complete trading agent for [Shonin](https://docs.shonin.fi): build it, test it and publish it as it is, then change it into your own.

The agent is a **rebalancer**. It holds a target exposure to one market, half the vault's value long ETH by default, and trades back towards it when the market moves it too far away. It's small enough to read in one sitting, and real enough to show everything an agent needs: limits kept in code, a restart that loses nothing, and a clean exit when its vault stops.

The [Writing an Agent](https://docs.shonin.fi/writing/getting-started/) section of the docs explains the ideas behind it. This README walks through the files.

## Getting started

1. **Get a copy.** Choose **Use this template** on GitHub, or clone it:

    ```bash
    git clone https://github.com/ShoninFi/shonin-agent-template my-agent
    cd my-agent
    ```

2. **Install Rust** from [rustup.rs](https://rustup.rs). `rust-toolchain.toml` pins the exact compiler and the `wasm32-wasip2` target, and `cargo` fetches both the first time you build.

3. **Point at the SDK.** Until `shonin-strategy-sdk` is published on crates.io, resolve it from a checkout of the Shonin platform by creating `.cargo/config.toml` (it's gitignored):

    ```toml
    [patch.crates-io]
    shonin-strategy-sdk = { path = "/path/to/shonin-platform/packages/strategy-sdk" }
    ```

4. **Test and build:**

    ```bash
    cargo test                                        # the agent's decisions, run natively
    cargo build --release --target wasm32-wasip2      # the component, the way the builder makes it
    ```

## The files

### `shonin.toml`: the configuration

What the agent is and what it promises, for the platform. Your agent never reads it.

- **`[description]`**: its title, a plain description, and its **claims** in plain words. These are promises: the AI Review checks your code keeps them.
- **`[venues.hyperliquid]`**: where it trades, from the [supported venues](https://docs.shonin.fi/reference/venues/).
- **`[bridges.<name>]`**: one for every bridge it uses to move money between chains. It trades on one chain, so it has none.

It gets its id automatically when you publish.

It's fixed once you publish. How much computing and gas the agent may use isn't in here: you choose that when you publish, and can change it on the live vault.

### `src/config.rs`: the settings

The numbers the agent decides with, as constants: the market, the target exposure, the most it will ever hold, how far it may drift before trading, and the largest single order.

### `src/decide.rs`: the decisions

Every decision the agent makes, as plain functions of what the venue reports. Nothing here waits or sends anything, which is what makes it testable.

- **`Settings`** reads `config.rs` as exact decimals, and holds the target inside the limits whatever the settings say.
- **`exposure`** works out how much of the vault is held in the market.
- **`rebalance`** returns one step back towards the target: a single order no larger than the order limit, and never a short. A big gap closes over several steps, one per event.
- **`flatten`** closes everything at once, for when the vault is stopping.

### `src/lib.rs`: the waiting and the sending

The agent's one long-lived `run`. It waits on two streams, the venue's news about the vault's account and the vault's own events, and on each one it reads the account and takes a step. It remembers nothing between steps: every decision starts from what the venue says is there, so a rejected order or a restart can never leave it believing something false. When the vault says it's halting, it closes out and stops.

### `tests/decide.rs`: the tests

Each test builds an account the way a venue would report it and checks what the agent would do: nothing inside the band, one capped buy below the target, a reduce-only sell above it, no short ever.

## Making it yours

1. **Name it.** Change the `[description]` in `shonin.toml`: its `title` is the name people see.
2. **Set its numbers** in `src/config.rs`.
3. **Decide what it does.** Rewrite `src/decide.rs`, and add tests beside the ones here as you go.
4. **Claim only what you enforce.** Every line in `claims` should be something your code makes true. The AI Review reads both and rates you down for a promise the code doesn't keep.
5. **Choose your venue** from the [supported venues](https://docs.shonin.fi/reference/venues/): its table in `shonin.toml`, and `VENUE` in `src/lib.rs`.

## Publishing

When `cargo test` passes and the component builds, publish it from the web app: pay the $1 deposit plus whatever you want in your agent's tank, choose how much computing it may use, and upload this folder. [Publishing](https://docs.shonin.fi/writing/publishing/) has the details.

## License

Apache-2.0. Copy it, change it, publish your own.
