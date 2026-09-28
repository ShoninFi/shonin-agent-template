# Shonin agent template

A small, complete trading agent for [Shonin](https://docs.shonin.fi): build it, test it and publish it as it is, then change it into your own.

The agent is a **rebalancer**. It holds a target exposure to one market, half the vault's value long ETH by default, and trades back towards it when the market moves it too far away. It's small enough to read in one sitting, and real enough to show everything an agent needs: limits kept in code, a restart that loses nothing, and a clean exit when its vault stops.

The docs page [Write an agent](https://docs.shonin.fi/hands-on/write/) explains the ideas behind it. This README walks through the files.

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

### `shonin.toml`: the manifest

What the agent is and what it promises, in five parts:

- **`[strategy]`**: its `id`, unique among your agents, and every bridge it uses (none here).
- **`[description]`**: its title, a plain description, and its **limits** in plain words. These are promises: the AI Review checks your code keeps them.
- **`[config]`**: the numbers the agent reads when it starts. Here: the market, the target exposure, the most it will ever hold, how far it may drift before trading, and the largest single order.
- **`[venues]`**: where it trades, and which of the vault's accounts it uses.
- **`[runtime.allowances]`**: the computing and gas it may use, which is what its tank can be charged. You can change these on a live vault later.

Everything except the allowances is fixed for the life of the vault once you publish.

### `src/decide.rs`: the decisions

Every decision the agent makes, as plain functions of what the venue reports. Nothing here waits or sends anything, which is what makes it testable.

- **`Settings`** reads the manifest's `[config]`, falling back to defaults written here, and holds the target inside the limits whatever the config says.
- **`exposure`** works out how much of the vault is held in the market.
- **`rebalance`** returns one step back towards the target: a single order no larger than the order limit, and never a short. A big gap closes over several steps, one per event.
- **`flatten`** closes everything at once, for when the vault is stopping.

### `src/lib.rs`: the waiting and the sending

The agent's one long-lived `run`. It waits on two streams, the venue's news about the vault's account and the vault's own events, and on each one it reads the account and takes a step. It remembers nothing between steps: every decision starts from what the venue says is there, so a rejected order or a restart can never leave it believing something false. When the vault says it's halting, it closes out and stops.

### `tests/decide.rs`: the tests

Each test builds an account the way a venue would report it and checks what the agent would do: nothing inside the band, one capped buy below the target, a reduce-only sell above it, no short ever. One more test reads `shonin.toml` and fails if its config ever drifts from the defaults in `decide.rs`.

## Making it yours

1. **Name it.** Change `id` and the `[description]` in `shonin.toml`.
2. **Decide what it does.** Rewrite `src/decide.rs`, and add tests beside the ones here as you go.
3. **Keep your config and your code together.** Every `[config]` key needs a matching field and default in `Settings`; the drift test holds you to it.
4. **Promise only what you enforce.** Every line in `limits` should be something your code makes true. The AI Review reads both and rates you down for a promise the code doesn't keep.
5. **Set your venue and allowances** in `shonin.toml`, and change `VENUE` in `src/lib.rs` to match.

## Publishing

When `cargo test` passes and the component builds, publish it from the web app: pay the $1 deposit plus whatever you want in your agent's tank, and upload this folder. [Publish an agent](https://docs.shonin.fi/hands-on/publish/) has the details.

## License

Apache-2.0. Copy it, change it, publish your own.
