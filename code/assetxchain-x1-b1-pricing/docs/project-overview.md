# AssetXChain Project Overview

AssetXChain is a Substrate solo-chain project for data asset ownership,
circulation, and market interaction. It extends the original Substrate
solochain template with custom runtime pallets, child-trie-backed asset state,
ink! market contracts, and economic-model pallets.

This document summarizes the current architecture for future development work.

## High-Level Architecture

AssetXChain has four main layers:

```text
Node
  CLI, chain specs, BABE/GRANDPA service, RPC, custom proof endpoint

Runtime
  FRAME runtime composition, pallet configuration, runtime APIs, chain extension

Pallets
  Data asset state, incentives, rewards, markets, collateral, validators

Contracts
  ink! market standard and order-book market prototype
```

The current node binary is named `assetxchain-node`, and the runtime package is
named `assetxchain-runtime`.

## Data Asset Model

The project separates two asset concepts:

- Data asset, also called the ownership certificate or meta certificate.
- Right-token certificate, representing a usage or access right derived from a
  parent data asset.

The core data structures are defined in `pallets/dataassets/src/types.rs`.

`DataAsset` contains:

- Core identity: `asset_id`, `token_id`, owner, raw data hash, status, nonce.
- Metadata: name, description, labels, metadata CID placeholder fields.
- Characteristics and statistics.
- Encryption and pricing configuration.

`RightToken` contains:

- `certificate_id` and `token_id`.
- Right type: usage or access.
- Owner, issuer, parent asset ID, validity period, status, nonce.

## Child Trie State Design

`pallet-dataassets` stores asset and certificate state in child tries:

- `:asset_trie:` stores data assets and token ID mappings.
- `:certificate_trie:` stores right-token certificates.

On every write to either child trie, the pallet marks the trie as modified. At
block finalization, it recomputes:

- Asset state root.
- Certificate state root.

The roots are written into block digests as `ASSET_ROOT` and `CERT_ROOT` digest
items. This is the current mechanism for exposing independent asset state roots.

A custom header type with an `asset_root` field exists in
`runtime/src/custom_header.rs`, but the runtime currently uses Substrate's
standard generic header.

## Core Data Asset Flows

### Register Data Asset

`pallet_dataassets::register_asset`:

1. Validates name and description length.
2. Generates an `asset_id` from owner, timestamp, and raw data hash.
3. Calculates collateral from data size.
4. Reserves the collateral from the creator.
5. Creates the `DataAsset` and writes it to the asset child trie.
6. Assigns a sequential `token_id`.
7. Attempts to distribute the first-create incentive reward.
8. Emits `AssetRegistered`.

There is also `register_asset_core`, an experiment-only path that bypasses
collateral locking and incentive payout.

### Issue Right-Token Certificate

`pallet_dataassets::issue_certificate`:

1. Loads the parent asset.
2. Checks that the caller is either the asset owner or an authorized market
   account.
3. Converts the input right type into usage or access.
4. Assigns a per-asset certificate token ID.
5. Writes the `RightToken` into the certificate child trie.
6. Registers an asset trade statistic in the incentive pallet.
7. Emits `CertificateIssued`.

### Transfer Data Asset

There are two transfer paths:

- Owner transfer through `transfer_asset`.
- Market transfer through `transfer_asset_by_market` or
  `transfer_by_market_internal`.

The market path checks that the market account has been authorized in
`AssetApprovals`. After a successful transfer, authorization is removed.

### Transfer Right-Token Certificate

`transfer_certificate` verifies the current right-token owner, updates the
certificate owner and nonce, writes it back into the certificate child trie, and
emits `CertificateTransferred`.

## Collateral Design

There are two collateral-related areas.

### Asset Registration Collateral

`pallet-dataassets` handles data asset registration collateral:

- Base amount: configured in the runtime.
- Per-MB amount: configured in the runtime.
- Maximum cap: configured in the runtime.

Collateral is reserved from the asset creator and released in phases:

- 50 percent after 24 hours with verification condition.
- 30 percent after 30 days with usage condition.
- 20 percent after 90 days with availability condition.

Release processing uses a block-indexed agenda to avoid scanning all assets on
every block.

### Role-Based Collateral

`pallet-collaterals` handles broader role collateral:

- Data creator.
- Market operator.
- IPFS provider.
- Governance pledge.

It supports pledge, unbond, and `slash_and_distribute`. Slashed funds can be
split between a destruction account, incentive pool, compensation pool, and IPFS
pool depending on the violation type.

## Market Integration

Market integration has three parts:

1. `pallet-contracts` runs ink! contracts.
2. `pallet-markets` registers and verifies market contracts.
3. `runtime/src/asset_market_extension.rs` lets contracts call runtime asset
   transfer logic.

Market registration flow:

1. A user deploys an ink! market contract.
2. The user calls `pallet_markets::register_market`.
3. The pallet reserves market operator collateral.
4. The pallet calls the contract's `is_assetx_market()` message.
5. If the contract returns true, the market is stored in `RegisteredMarkets`.

The order-book contract prototype in `contracts/market_orderbook` can list and
buy assets. When a purchase succeeds, it calls the chain extension so runtime
asset ownership changes in `pallet-dataassets`.

The current chain extension implements data asset transfer for function ID `1`.
The certificate transfer function ID exists but is not implemented beyond a
success return.

## Incentive And Reward Model

### Block Rewards

`pallet-rewards` mints block rewards to the block author:

- Initial reward: `5 DAT` per block.
- Adjusted reward: `1 DAT` per block after the configured mining threshold.
- Maximum mining supply is capped in runtime configuration.

### Ecosystem Incentives

`pallet-incentive` manages the incentive pool:

- The pool receives its initial balance at genesis.
- A portion is dynamically released each month.
- Released funds can be spent on configured rewards.

Supported reward categories:

- First data asset creation reward.
- Quality data reward.
- Top market monthly reward.
- Trader rebate.
- Liquidity reward.
- Governance voting reward.
- Governance proposal reward.

`pallet-dataassets` calls the incentive pallet through
`pallet-shared-traits`, which avoids direct circular dependencies.

Some reward paths still use placeholder assumptions, such as decoding a market
ID into an operator account instead of resolving the operator from the market
registry.

## Validator And Consensus Model

The runtime uses:

- BABE for block production.
- GRANDPA for finality.
- `pallet-session` for session keys.
- `pallet-validator` as the session validator set manager.
- `pallet-im-online` for online checks.

`pallet-validator` supports Root-controlled validator add/remove operations and
reserves validator bond when a validator is added.

The genesis preset configures Alice for the development chain and Alice/Bob for
the local testnet preset.

## Runtime APIs And RPC

`runtime/src/runtime_api.rs` declares `DataAssetsApi`:

- `get_asset(asset_id)`
- `get_asset_by_token_id(token_id)`
- `get_certificate(asset_id, cert_id)`
- `get_asset_root()`

`runtime/src/apis.rs` implements those methods through `pallet-dataassets`.

`node/src/data_asset_rpc.rs` adds:

```text
dataAssets_getAssetProof(asset_id, at)
```

This RPC builds a child-trie read proof for key:

```text
"assets/" + asset_id
```

inside the `:asset_trie:` child trie.

The current RPC signature uses `[u8; 32]`, so JSON-RPC callers should pass
`asset_id` as an array of 32 byte values and `at` as `null` for the best block:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "dataAssets_getAssetProof",
  "params": [
    [7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7],
    null
  ]
}
```

Runtime API queries can be made from Polkadot-JS:

```js
const assetId = new Uint8Array(32).fill(7);
await api.call.dataAssetsApi.getAsset(assetId);
await api.call.dataAssetsApi.getAssetByTokenId(0);
await api.call.dataAssetsApi.getAssetRoot();
```

## Genesis And Economic Accounts

Genesis configuration is in `runtime/src/genesis_config_presets.rs`.

It configures:

- Development and local testnet presets.
- Sudo account.
- Initial balances for development accounts.
- Foundation account allocation.
- Incentive pool allocation.
- Foundation vesting schedule.
- Initial validator list.
- Incentive pallet genesis initialization.

Runtime economic constants include:

- Total supply: `1,000,000,000 DAT`.
- Foundation allocation: `200,000,000 DAT`.
- Incentive pool allocation: `300,000,000 DAT`.
- Mining reward allocation: `500,000,000 DAT`.

`UNIT` is `10^12`, so `1 DAT = 1_000_000_000_000` base units.

## Contracts

### `contracts/market_standard`

Defines:

- `MarketStandard` ink! trait.
- `DataAssetsExt` chain extension binding.
- `CustomEnvironment` for contracts that use the chain extension.

Required market messages include:

- `is_assetx_market`
- `get_market_type`
- `get_fee_ratio`
- `check_admission`
- `can_list_asset`
- `asset_enter`
- `asset_leave`
- `report_trade_result`

### `contracts/market_orderbook`

Prototype fixed-price order-book market:

- Stores active asset orders.
- Lists assets with prices.
- Accepts payment and transfers assets through the chain extension.
- Implements the market standard trait.

### `contracts/zhushui`

Prototype traditional asset contract. It stores assets directly inside contract
storage and is mainly useful as a comparison case for child-trie-based runtime
asset state.

## Prototype And Unwired Areas

### `pallets/storage_ipfs`

The IPFS storage pallet contains prototype structures and local dispatchable
calls for:

- Storage provider registration with `CollateralRole::IpfsProvider` pledge.
- Storage order creation.
- Asset-storage binding.
- Storage proof submission by the bound provider.

It is not listed as a runtime pallet in the current runtime composition. It is
listed as a Cargo workspace member so the prototype can be checked with
`cargo test -p storage_ipfs` and `cargo check -p storage_ipfs` while runtime
integration remains deferred. The pallet exposes no-op `IpfsAvailabilityVerifier`
and `XcmAvailabilityVerifier` extension points for future real IPFS checks and
storage-chain / XCM availability checks.

The design notes in `sto_ipfs.note` suggest a future direction where IPFS or a
storage chain handles physical data availability, while AssetXChain records
asset ownership and storage-order state.

### Custom Header

`runtime/src/custom_header.rs` defines a `CustomHeader` with `asset_root`, but
the active runtime and opaque block types currently use `generic::Header`.

The current usable integration point is digest-based root emission from
`pallet-dataassets`.

### Template Pallet

`pallets/template` remains in the runtime. It is still the original Substrate
example pallet, is clearly labeled as a placeholder, and is not part of the data
asset business model.

## Development Notes

Common commands:

```sh
cargo check -p assetxchain-node
cargo build --release
./target/release/assetxchain-node --dev --tmp
./target/release/assetxchain-node purge-chain --dev
```

For the end-to-end developer smoke path, see
`docs/mvp-smoke-test.md`.

Build benchmark-enabled node:

```sh
cargo build --release --features runtime-benchmarks
```

Generate data assets weights:

```sh
./target/release/assetxchain-node benchmark pallet \
  --chain dev \
  --pallet pallet_dataassets \
  --extrinsic "*" \
  --steps 50 \
  --repeat 20 \
  --output pallets/dataassets/src/weights.rs
```

## Suggested Next Development Areas

The current codebase is ready for incremental development in these areas:

- Complete certificate transfer support in the contract chain extension.
- Wire IPFS/storage-order design into runtime or split it into a dedicated
  storage-chain integration.
- Replace incentive placeholder lookups with explicit market/operator queries.
- Decide whether asset roots should remain digest-based or move into the custom
  header path.
- Remove or isolate the template pallet if it is no longer useful.
- Add end-to-end tests for asset registration, market authorization, contract
  transfer, incentive accounting, and child-trie proof generation.
