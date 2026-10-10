# MVP Smoke Test

This smoke test verifies the current AssetXChain MVP path without assuming
production IPFS, XCM, or storage-chain integration.

## 1. Build And Run

```sh
cargo check -p assetxchain-node
cargo build --release
./target/release/assetxchain-node --dev --tmp
```

Connect Polkadot-JS Apps to:

```text
ws://localhost:9944
```

## 2. Register A Data Asset

Use `dataAssets.registerAssetWithMetadata` from Alice.

Example values:

- `name`: `0x6173736574`
- `description`: `0x6465736372697074696f6e`
- `raw_data_hash`: `0x1111111111111111111111111111111111111111111111111111111111111111`
- `data_size_bytes`: `7340032`
- `metadata_cid`: `0x626166796265696d65746164617461`
- `data_cid`: `0x6261667962656964617461`
- `encryption_info`:
  - `algorithm`: `0x4145532d3235362d47434d`
  - `key_length`: `256`
  - `parameters_hash`: `0x5555555555555555555555555555555555555555555555555555555555555555`
  - `is_encrypted`: `true`

Expected result: the extrinsic succeeds and emits `dataAssets.AssetRegistered`.

## 3. Query Runtime APIs

From Polkadot-JS developer console:

```js
const assetId = new Uint8Array(32).fill(7);

await api.call.dataAssetsApi.getAsset(assetId);
await api.call.dataAssetsApi.getAssetByTokenId(0);
await api.call.dataAssetsApi.getAssetRoot();
```

Use the actual `asset_id` from the `AssetRegistered` event or from local test
fixtures. The `[7; 32]` value is only a placeholder.

## 4. Query The Custom Proof RPC

The current proof RPC accepts `asset_id` as a JSON array of 32 bytes and `at` as
`null` for best block:

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

Expected result: a JSON-RPC response with `result` containing trie proof nodes
or an empty proof path if the placeholder asset id is not present.

## 5. Market Flow Reminder

For market transfer demos, register or instantiate a market contract, authorize
the market account with `dataAssets.authorizeMarket`, then invoke the market
contract path that calls the runtime chain extension. Owner transfer remains
blocked while a market authorization exists.

## Current Boundaries

- `storage_ipfs` is checked as a workspace prototype but is not runtime-wired.
- Real IPFS availability checks are deferred behind `IpfsAvailabilityVerifier`.
- XCM or storage-chain availability checks are deferred behind
  `XcmAvailabilityVerifier`.
- `pallet-template` remains in the runtime as a clearly labeled placeholder and
  is not part of the data asset MVP.
