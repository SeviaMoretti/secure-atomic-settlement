# AssetXChain

AssetXChain is a Substrate-based solo chain for data asset ownership,
certificate issuance, collateral management, incentive distribution, and market
contract integration.

The chain is built from the Substrate solochain template, but the runtime has
been extended with custom pallets for data asset state, right-token
certificates, market registration, ecosystem incentives, block rewards,
collateral roles, and validator management.

## Project Status

This repository is an active development chain. It already contains the core
runtime and pallet architecture for data asset registration and circulation, but
some modules remain prototypes or placeholders.

Implemented core capabilities:

- Register data assets as on-chain ownership certificates.
- Issue and transfer right-token certificates for existing data assets.
- Store data asset and certificate state in independent child tries.
- Compute asset and certificate state roots and write them into block digests.
- Lock, release, and slash collateral associated with data asset registration.
- Register market contracts and let approved market contracts transfer assets
  through a runtime chain extension.
- Maintain an ecosystem incentive pool and block reward model.
- Run a BABE + GRANDPA solo chain with session-based validators.
- Expose custom RPC support for data asset storage proofs.

Known in-progress areas:

- The IPFS storage pallet is present as a prototype and is not currently wired
  into the runtime.
- The market contract path supports asset transfer; certificate transfer through
  the chain extension is not fully implemented.
- A custom header type with an `asset_root` field exists, but the current runtime
  still uses the standard Substrate generic header and exposes asset roots
  through digests.
- Some incentive, storage-availability, and governance workflows contain
  placeholder logic that still needs production integration.

For a fuller architecture description, see
[docs/project-overview.md](./docs/project-overview.md).

## Repository Layout

```text
assetxchain/
  node/                 Substrate node service, CLI, chain specs, RPC wiring
  runtime/              Runtime composition, pallet configs, runtime APIs
  pallets/
    dataassets/         Core data asset and certificate state pallet
    incentive/          Ecosystem incentive pool and reward statistics
    rewards/            Per-block issuance rewards
    markets/            Market contract registration and verification
    collaterals/        Role-based collateral and slashing logic
    validator/          Validator set management for sessions
    shared_traits/      Cross-pallet traits
    storage_ipfs/       IPFS/storage-order prototype, not runtime-wired
    template/           Original Substrate example pallet, placeholder only
  contracts/
    market_standard/    ink! market interface and chain extension bindings
    market_orderbook/   ink! order-book market prototype
    zhushui/            Traditional single-contract asset storage prototype
  docs/                 Project and environment documentation
  env-setup/            Rust and Nix environment setup files
```

Generated output and local chain data, such as `target/`, `geth_data_1m/`, and
prebuilt binaries under `docker_bin/`, are not the main source of project logic.

## Build

Install the Rust/Substrate build environment first. The existing environment
notes are in [docs/rust-setup.md](./docs/rust-setup.md).

Build the node:

```sh
cargo build --release
```

The node binary is named for AssetXChain:

```sh
./target/release/assetxchain-node --help
```

Use `assetxchain-node` as the development and release binary for AssetXChain.

## Run A Development Chain

Start a temporary single-node development chain:

```sh
./target/release/assetxchain-node --dev --tmp
```

Start with persistent state:

```sh
mkdir -p ./dev-chain-state
./target/release/assetxchain-node --dev --base-path ./dev-chain-state
```

Purge development state:

```sh
./target/release/assetxchain-node purge-chain --dev
```

Run with debug logs:

```sh
RUST_BACKTRACE=1 ./target/release/assetxchain-node -ldebug --dev
```

After the node starts, connect Polkadot-JS Apps to:

```text
ws://localhost:9944
```

## Local Testnet

The node supports the standard `dev` and `local` chain spec presets:

```sh
./target/release/assetxchain-node --chain local --alice --validator \
  --base-path /tmp/assetx-alice \
  --port 30333 \
  --rpc-port 9944 \
  --rpc-methods unsafe \
  --rpc-cors all
```

A second local validator can be started with Bob and a bootnode pointing to the
Alice node.

## Core Runtime Modules

### Data Assets

`pallet-dataassets` is the central business pallet. It manages:

- Data asset registration.
- Data asset transfer and lock/unlock state.
- Right-token certificate issuance, revocation, and transfer.
- Asset-to-market authorization.
- Asset child trie and certificate child trie storage.
- Asset root and certificate root digest updates.
- Data asset collateral locking, phased release, and slashing.

### Market Contracts

`pallet-markets` registers ink! market contracts. A market creator locks market
operator collateral, then the pallet verifies the contract by calling its
`is_assetx_market()` message through `pallet-contracts`.

Market contracts can call the runtime chain extension to transfer an authorized
data asset to a buyer. Asset ownership still changes in the runtime, not inside
the contract storage.

### Incentives And Rewards

`pallet-rewards` mints per-block rewards to the current block author until the
configured mining reward supply is exhausted.

`pallet-incentive` manages the ecosystem incentive pool. It supports dynamic
pool release, first-create rewards, quality data rewards, market rewards,
trader rebates, liquidity rewards, and governance reward bookkeeping.

### Collateral And Validators

`pallet-collaterals` provides role-based collateral for market operators, IPFS
providers, governance pledges, and related slashing distribution.

`pallet-validator` manages the validator set used by `pallet-session`, and the
runtime uses BABE for block production and GRANDPA for finality.

## Runtime APIs And RPC

The runtime exposes a `DataAssetsApi` with methods for:

- `get_asset(asset_id)`
- `get_asset_by_token_id(token_id)`
- `get_certificate(asset_id, cert_id)`
- `get_asset_root()`

The node also registers a custom RPC method:

```text
dataAssets_getAssetProof(asset_id, at)
```

This RPC generates a read proof for an asset stored in the asset child trie.
Because the current RPC signature uses `[u8; 32]`, pass `asset_id` as a JSON
array of 32 bytes. Use `null` for `at` to query the best block.

```sh
curl -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "dataAssets_getAssetProof",
    "params": [
      [7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7],
      null
    ]
  }' \
  http://localhost:9944
```

Runtime API query examples from Polkadot-JS:

```js
const assetId = new Uint8Array(32).fill(7);
const certId = new Uint8Array(32).fill(9);

const asset = await api.call.dataAssetsApi.getAsset(assetId);
const byToken = await api.call.dataAssetsApi.getAssetByTokenId(0);
const cert = await api.call.dataAssetsApi.getCertificate(assetId, certId);
const root = await api.call.dataAssetsApi.getAssetRoot();
```

## MVP Smoke Test

See [docs/mvp-smoke-test.md](./docs/mvp-smoke-test.md) for a concise manual
flow covering node startup, asset registration with off-chain metadata,
runtime API queries, and the custom proof RPC.

## Benchmarks

Build with runtime benchmarks:

```sh
cargo build --release --features runtime-benchmarks
```

Example pallet benchmark command:

```sh
./target/release/assetxchain-node benchmark pallet \
  --chain dev \
  --pallet pallet_dataassets \
  --extrinsic "*" \
  --steps 50 \
  --repeat 20 \
  --output pallets/dataassets/src/weights.rs
```

## Documentation

- [docs/project-overview.md](./docs/project-overview.md): architecture and
  module responsibilities.
- [docs/development-automation.md](./docs/development-automation.md): execution
  guide for Codex or other development agents.
- [docs/rust-setup.md](./docs/rust-setup.md): Rust/Substrate development
  environment setup.
- [note.txt](./note.txt): development notes and command snippets.
- [sto_ipfs.note](./sto_ipfs.note): IPFS/storage-chain design notes.
