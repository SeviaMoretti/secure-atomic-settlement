# G3 settlement v2 mutation entry audit

- Audit step: S2.12
- Audit date: 2026-07-27
- Scope: G3 AssetXChain-CRS paper-minimum implementation
- Design anchors: S15, S16, A06, A07, A13

## Production settlement gateway

The only contract-reachable AssetXChain settlement mutations are typed Chain
Extension v2 calls in `runtime/src/asset_market_extension.rs`:

| Extension | Function | Runtime authority |
| --- | --- | --- |
| 2 | 1 `create_order` | `pallet_dataassets::create_order` |
| 2 | 2 `fund_order` | `pallet_dataassets::fund_order` |
| 2 | 3 `settle` | `pallet_dataassets::settle` |
| 2 | 4 `cancel_open` | `pallet_dataassets::cancel_open` |
| 2 | 5 `refund_funded` | `pallet_dataassets::refund_funded` |
| 2 | 6 `query_order` | read-only authoritative query |
| 2 | 7 `query_settlement` | read-only authoritative query |

Extension 1 is retained only as the stable legacy identifier. Runtime routing
returns `Legacy chain extension disabled` for that extension and does not
register or dispatch any v1 function.

## Contract projection boundary

`contracts/market_orderbook/lib.rs` stores submitted `MarketIntent` only after
Runtime accepts `create_order`. Its `OrderProjection` is rebuilt only from
`query_order` and `query_settlement`. `sync_projection` contains no mutation or
settlement call, so a failed sync can be retried and repeated sync cannot cause
another settlement.

The compiled market contracts expose no payable message, private payment
ledger, `transferred_value`, contract balance transfer, direct asset/right
transfer, direct right issuance, direct status mutation, or contract-supplied
authoritative order digest.

## Legacy dispatch closure

The following legacy `pallet-dataassets` dispatchables retain their call index
for compatibility but call `ensure_legacy_mutation_enabled` before reading or
writing domain state:

| Call index | Legacy dispatchable |
| --- | --- |
| 1 | `issue_certificate` |
| 2 | `transfer_asset` |
| 3 | `revoke_certificate` |
| 4 | `lock_asset` |
| 5 | `unlock_asset` |
| 7 | `authorize_market` |
| 8 | `revoke_authorization` |
| 9 | `transfer_asset_by_market` |
| 10 | `transfer_certificate` |

The production Runtime sets `LegacyMutationEnabled = ConstBool<false>`.
Legacy characterization test runtimes set it to `ConstBool<true>` so historical
tests remain explicit and do not define production behavior.

Legacy direct helpers, including `transfer_by_market_internal`,
`settle_asset_trade_by_market_internal`,
`issue_certificate_by_market_internal`, and
`settle_certificate_trade_internal`, are `pub(crate)` inside
`pallet-dataassets`; they are not callable from contracts, Runtime adapters, or
other pallets.

## Retained non-production source

`pallets/markets/src/original_lib.rs` is retained as legacy characterization
source and contains an old direct helper call. It is not declared as a module
by `pallets/markets/src/lib.rs`, is not compiled, and cannot provide a Runtime
entry point. Reintroducing a module declaration for it is a mutation-path audit
failure.

## Search checklist

The S2.12 gate searches the compiled contract and Runtime sources for:

- `chain_extension(extension = 1)`, v1 function registration, and `call_v1`;
- payable messages, `transferred_value`, contract transfers, and legacy
  transfer/issue/status APIs;
- settlement calls inside `sync_projection`;
- production `LegacyMutationEnabled` values other than `ConstBool<false>`;
- direct mutation helpers with visibility wider than `pub(crate)`;
- any compiled reference to `original_lib` or its direct mutation call.

Any positive production-path match blocks S2.12 completion. Historical source
and test-only matches must remain identified as such rather than being counted
as production capability.
