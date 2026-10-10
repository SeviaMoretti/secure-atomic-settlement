//! # Data Assets Pallet
//!
//! A pallet for managing data assets with an independent asset state tree.
//!
//! ## Overview
//!
//! This pallet implements a dual-layer MPT structure:
//! - Main Asset Trie: Stores all data assets
//! - Certificate Sub-Tries: Each asset has its own sub-trie for certificates
//!
//! All data is stored in Child Tries, completely independent from the main state_root.

// 元证首次创建奖励+，元证交易数量-
#![cfg_attr(not(feature = "std"), no_std)]

extern crate alloc;
use alloc::vec::Vec;

pub use pallet::*;
pub mod collateral;
pub mod digest_item;
pub mod protocol;
pub mod types;

pub mod weights;
pub use weights::WeightInfo;

#[cfg(test)]
mod tests;

#[cfg(feature = "runtime-benchmarks")]
mod benchmarking;

pub use collateral::BalanceOf;

// 需要和 runtime/src/lib.rs 中的对应值保持一致
pub const MILLI_SECS_PER_BLOCK: u64 = 6000;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use codec::Encode;
    use frame_support::pallet_prelude::*;
    use frame_support::storage::child;
    use frame_support::traits::{BalanceStatus, Currency, ReservableCurrency};
    use frame_system::pallet_prelude::*;
    use pallet_shared_traits::{
        IncentiveHandler, MarketAccessError, MarketOperation, MarketRegistryProvider,
        MarketStateSnapshot, ProtocolAccountIdConverter,
    };
    use sp_core::H256;
    use sp_io::hashing::{blake2_256, keccak_256};
    use sp_runtime::traits::{SaturatedConversion, Saturating, Zero};

    use crate::types::*;

    const ASSET_TRIE_ID: &[u8] = b":asset_trie:";
    const CERTIFICATE_TRIE_ID: &[u8] = b":certificate_trie:";
    const METADATA_PREFIX: &[u8] = b"_metadata/";
    const OWNER_ASSET_INDEX_PREFIX: &[u8] = b"_index/owner_asset/";
    const HOLDER_RIGHT_INDEX_PREFIX: &[u8] = b"_index/holder_right/";
    const ASSET_RIGHT_INDEX_PREFIX: &[u8] = b"_index/asset_right/";

    pub trait WeightInfo {
        fn register_asset() -> Weight;
        fn register_asset_core() -> Weight;
        fn issue_certificate() -> Weight;
        fn transfer_certificate() -> Weight;
        fn transfer_asset() -> Weight;
        fn revoke_certificate() -> Weight;
        fn lock_asset() -> Weight;
        fn unlock_asset() -> Weight;
        fn slash_collateral() -> Weight;
        fn authorize_operator() -> Weight;
        fn revoke_authorization() -> Weight;
        fn create_order() -> Weight;
        fn cancel_open() -> Weight;
        fn expire_open() -> Weight;
        fn fund_order() -> Weight;
        fn refund_funded() -> Weight;
        fn settle() -> Weight;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_timestamp::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Currency type for handling collateral
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;

        /// Base collateral amount (e.g., 2000 DATA)
        #[pallet::constant]
        type BaseCollateral: Get<BalanceOf<Self>>;

        /// Collateral per MB of data (e.g., 100 DATA/MB)
        #[pallet::constant]
        type CollateralPerMB: Get<BalanceOf<Self>>;

        /// Maximum collateral cap (e.g., 75000 DATA)
        #[pallet::constant]
        type MaxCollateral: Get<BalanceOf<Self>>;

        #[pallet::constant]
        type MaxNameLength: Get<u32>;

        #[pallet::constant]
        type MaxDescriptionLength: Get<u32>;

        /// Incentive handler trait
        type IncentiveHandler: IncentiveHandler<Self::AccountId, [u8; 32], BalanceOf<Self>>;
        type MarketRegistry: MarketRegistryProvider<Self::AccountId>;
        type ProtocolAccountIdConverter: ProtocolAccountIdConverter<Self::AccountId>;
        #[pallet::constant]
        type FundedGraceBlocks: Get<u32>;
        /// Compatibility switch for pre-v2 ownership/right/status mutation calls.
        /// Production Runtime must set this to false.
        #[pallet::constant]
        type LegacyMutationEnabled: Get<bool>;
        type WeightInfo: WeightInfo;
    }

    /// Storage for asset collateral information
    #[pallet::storage]
    #[pallet::getter(fn asset_collateral)]
    pub type AssetCollateral<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // asset_id
        CollateralInfo<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
    >;

    #[pallet::storage]
    #[pallet::getter(fn asset_approvals)]
    pub type AssetApprovals<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],     // asset_id
        T::AccountId, // authorized operator (market)
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn issue_right_authorizations)]
    pub type IssueRightAuthorizations<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],
        IssueRightAuthorization<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn transfer_meta_authorizations)]
    pub type TransferMetaAuthorizations<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],
        TransferMetaAuthorization<T::AccountId, BlockNumberFor<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn active_issue_right_authorization)]
    pub type ActiveIssueRightAuthorization<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        [u8; 32],
        Blake2_128Concat,
        [u8; 32],
        [u8; 32],
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn active_transfer_meta_authorization)]
    pub type ActiveTransferMetaAuthorization<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        [u8; 32],
        Blake2_128Concat,
        [u8; 32],
        [u8; 32],
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn authorization_nonce)]
    pub type AuthorizationNonce<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        ([u8; 32], MarketOperation),
        u64,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn seller_nonce)]
    pub type SellerNonce<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        protocol::ProtocolAccountId,
        Blake2_128Concat,
        [u8; 32],
        u64,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn buyer_nonce)]
    pub type BuyerNonce<T: Config> =
        StorageMap<_, Blake2_128Concat, protocol::ProtocolAccountId, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_issue_right_authorization_count)]
    pub type ActiveIssueRightAuthorizationCount<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_transfer_meta_authorization_by_asset)]
    pub type ActiveTransferMetaAuthorizationByAsset<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], [u8; 32], OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn issue_quota_reservations)]
    pub type IssueQuotaReservations<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], IssueQuotaReservation, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn outstanding_issue_quota_count)]
    pub type OutstandingIssueQuotaCount<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], u32, ValueQuery>;

    /// Flag to track whether the child tries have been modified since last root computation.
    /// Set to true on any write to asset or certificate tries; cleared in on_finalize.
    #[pallet::storage]
    pub type TrieModified<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// Experiment 2 toggle: true = hash-scattered keys, false = prefix-aggregated keys
    #[pallet::storage]
    #[pallet::getter(fn use_hash_scattered_keys)]
    pub type UseHashScatteredKeys<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// Experiment 2: per-block cert insert counter (reset per block in on_finalize)
    #[pallet::storage]
    #[pallet::getter(fn block_cert_count)]
    pub type BlockCertCount<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// Experiment 2: per-block child::put calls (trie node writes) — increased manually
    #[pallet::storage]
    #[pallet::getter(fn block_put_calls)]
    pub type BlockPutCalls<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// Experiment 2: if true, use zero weight for issue_certificate
    #[pallet::storage]
    #[pallet::getter(fn use_zero_weight)]
    pub type UseZeroWeight<T: Config> = StorageValue<_, bool, ValueQuery>;

    /// Agenda for collateral release processing, indexed by unlock block number.
    /// Each entry holds asset_ids whose collateral phase unlocks at that block.
    #[pallet::storage]
    pub type CollateralReleaseAgenda<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        BlockNumberFor<T>,
        BoundedVec<[u8; 32], ConstU32<100>>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn trade_settlements)]
    pub type TradeSettlements<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],
        TradeSettlement<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    pub type TradeSettlementNonce<T: Config> = StorageValue<_, u64, ValueQuery>;

    /// 订单投影表——合约创建订单后通过 Chain Extension (func_id=6) 写入此存储。
    /// 运行时结算时读取此投影进行前置验证（验证入口见 verify_settle_prerequisites）。
    /// Legacy v1 contract projection. Settlement v2 must not treat this map or
    /// its contract-supplied digest as authoritative.
    #[pallet::storage]
    #[pallet::getter(fn market_orders)]
    pub type MarketOrders<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // order_id
        crate::types::MarketOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// Legacy v1 order-indexed record. It remains readable for compatibility
    /// but is not authoritative for settlement v2.
    #[pallet::storage]
    #[pallet::getter(fn order_settlements)]
    pub type OrderSettlements<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // order_id
        crate::types::SettlementRecord<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// 每个资产的活跃权证数量。
    /// issue_certificate 时 +1，revoke_certificate 时 −1。
    /// 用于 transfer_asset 中阻止"有活跃权证时所有者直接转移资产"。
    /// 使用 ValueQuery：缺失键默认返回 0。
    #[pallet::storage]
    #[pallet::getter(fn active_certificate_count)]
    pub type ActiveCertificateCount<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // asset_id
        u32,
        ValueQuery,
    >;

    /// Paper-minimum object lock. RuntimeOrders remains the source of state.
    /// The sole authoritative settlement-v2 order record.
    #[pallet::storage]
    #[pallet::getter(fn active_meta_order)]
    pub type ActiveMetaOrderByAsset<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], [u8; 32], OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_right_order)]
    pub type ActiveRightOrderByRight<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], [u8; 32], OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn runtime_order)]
    pub type RuntimeOrders<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], protocol::RuntimeOrder, OptionQuery>;

    /// Immutable price-policy preimage bound by the order and seller hashes.
    /// Retained for terminal orders so audit queries can reconstruct fee checks.
    #[pallet::storage]
    #[pallet::getter(fn order_price_policy)]
    pub type OrderPricePolicySnapshots<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], PricePolicy, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn runtime_escrow)]
    pub type RuntimeEscrows<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], protocol::RuntimeEscrow, OptionQuery>;

    /// Sole authoritative settlement-v2 evidence, keyed by RuntimeOrder id.
    #[pallet::storage]
    #[pallet::getter(fn settlement_record)]
    pub type SettlementRecords<T: Config> =
        StorageMap<_, Blake2_128Concat, [u8; 32], protocol::SettlementRecord, OptionQuery>;

    #[cfg(test)]
    std::thread_local! {
        static TEST_ABORT_META_SETTLEMENT: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static TEST_ABORT_ISSUE_RIGHT_SETTLEMENT: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static TEST_ABORT_TRANSFER_RIGHT_SETTLEMENT: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static TEST_ABORT_AFTER_FIRST_PAYOUT: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        static TEST_ABORT_AFTER_SETTLEMENT_EVIDENCE: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        AssetRegistered {
            asset_id: [u8; 32],
            token_id: u32,
            owner: T::AccountId,
            collateral: BalanceOf<T>,
        },
        CertificateIssued {
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            issuer: T::AccountId,
            holder: T::AccountId,
        },
        AssetTransferred {
            asset_id: [u8; 32],
            from: T::AccountId,
            to: T::AccountId,
        },
        CertificateTransferred {
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            from: T::AccountId,
            to: T::AccountId,
        },
        TradeSettled {
            trade_id: [u8; 32],
            market: T::AccountId,
            seller: T::AccountId,
            buyer: T::AccountId,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            asset_type: TradeAssetType,
            price: BalanceOf<T>,
            settled_at: BlockNumberFor<T>,
        },
        CertificateRevoked {
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        },
        AssetRootUpdated {
            root: H256,
        },
        CertificateRootUpdated {
            root: H256,
        },
        /// Collateral locked for asset
        CollateralLocked {
            asset_id: [u8; 32],
            depositor: T::AccountId,
            amount: BalanceOf<T>,
        },
        /// Collateral released (phase completed)
        CollateralReleased {
            asset_id: [u8; 32],
            amount: BalanceOf<T>,
            phase: u8,
        },
        /// Collateral slashed due to violation
        CollateralSlashed {
            asset_id: [u8; 32],
            amount: BalanceOf<T>,
            percentage: u8,
        },
        CollateralOverCappedHint {
            asset_id: [u8; 32],
            depositor: T::AccountId,
            total_uncapped: BalanceOf<T>,
            capped_amount: BalanceOf<T>,
            max_collateral: BalanceOf<T>,
        },
        /// Asset authorized to a market/operator
        AssetAuthorized {
            asset_id: [u8; 32],
            owner: T::AccountId,
            operator: T::AccountId,
        },
        /// Authorization revoked
        AuthorizationRevoked {
            asset_id: [u8; 32],
            owner: T::AccountId,
        },
        IssueRightAuthorizationCreated {
            auth_id: [u8; 32],
            asset_id: [u8; 32],
            market_id: [u8; 32],
        },
        TransferMetaAuthorizationCreated {
            auth_id: [u8; 32],
            asset_id: [u8; 32],
            market_id: [u8; 32],
        },
        IssueRightAuthorizationRevoked {
            auth_id: [u8; 32],
        },
        TransferMetaAuthorizationRevoked {
            auth_id: [u8; 32],
        },
        IssueRightAuthorizationExpired {
            auth_id: [u8; 32],
        },
        TransferMetaAuthorizationExpired {
            auth_id: [u8; 32],
        },
        OrderCreated {
            order_id: [u8; 32],
            market_id: [u8; 32],
            operation: MarketOperation,
            object_id: [u8; 32],
        },
        OrderCancelled {
            order_id: [u8; 32],
        },
        OrderExpired {
            order_id: [u8; 32],
        },
        OrderFunded {
            order_id: [u8; 32],
            buyer: protocol::ProtocolAccountId,
            escrow_id: [u8; 32],
            gross_price: u128,
            grace_deadline: u32,
        },
        PaymentEscrowed {
            order_id: [u8; 32],
            escrow_id: [u8; 32],
            amount: u128,
        },
        OrderRefunded {
            order_id: [u8; 32],
            buyer: protocol::ProtocolAccountId,
            amount: u128,
        },
        PaymentRefunded {
            order_id: [u8; 32],
            escrow_id: [u8; 32],
            amount: u128,
        },
        SettlementFinalized {
            settlement_id: [u8; 32],
            order_id: [u8; 32],
            post_state_commitment: [u8; 32],
        },
        /// Exp2: per-block cert and put count snapshot
        CountersSnapshot {
            cert_count: u64,
            put_count: u64,
        },
        /// 结算被拒绝事件——每个拒绝原因对应一个 reason 字符串
        SettlementRejected {
            order_id: [u8; 32],
            reason: Vec<u8>,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        AssetNotFound,
        AssetNotActive,
        AssetLocked,
        CertificateNotFound,
        NotOwner,
        InvalidInput,
        NameTooLong,
        DescriptionTooLong,
        InvalidRightType,

        InsufficientBalance,
        CollateralNotFound,
        InvalidSlashPercentage,

        NotAuthorized,
        AlreadyAuthorized,
        MarketNotRegistered,
        MarketRestricted,
        CapabilityDenied,
        AuthorizationMissing,
        AuthorizationRevoked,
        AuthorizationExpired,
        AuthorizationAlreadyActive,
        AuthorizationNotActive,
        InvalidAuthorizationExpiry,
        AuthorizationCounterInvariant,
        AuthorizationNonceMismatch,
        AuthorizationNonceOverflow,
        AuthorizationPolicyMismatch,
        InvalidPricePolicy,
        BelowPriceFloor,
        FeeOutOfBounds,
        InvalidIssueQuota,
        QuotaExceeded,
        QuotaReservationAlreadyExists,
        QuotaReservationMissing,
        QuotaReservationStateInvalid,
        QuotaReservationAuthorizationMismatch,
        QuotaInvariant,
        CertificateNotActive,
        OrderNotFound,
        OrderStatusInvalid,
        OrderDigestMismatch,
        AlreadySettled,
        OrderLockFailed,
        OutstandingCertificates,
        InvalidRightParameters,
        RightAlreadyExists,
        TooManyRightsForAsset,
        RightIndexInconsistent,
        MutualExclusionConflict,
        ActiveObjectOrderExists,
        ActiveObjectOrderMismatch,
        StateVersionOverflow,
        UnsupportedProtocolVersion,
        MalformedCommitment,
        InvalidSignature,
        NonceUsed,
        StateVersionMismatch,
        OrderExpired,
        OrderNotExpired,
        ProtocolNonceOverflow,
        OrderAlreadyExists,
        OwnershipMismatch,
        ParentMismatch,
        RightInvalid,
        ProtocolAccountIdInvalid,
        MarketActiveOrderCapacityExceeded,
        MarketActiveOrderCounterInvariant,
        MarketCapacityExceeded,
        MarketFundedCounterInvariant,
        MarketExposureInvariant,
        PayerBuyerMismatch,
        EscrowMismatch,
        OrderNotFunded,
        GracePeriodActive,
        GraceDeadlineOverflow,
        PaymentAmountInvalid,
        TerminalOrder,
        AuthorizationConsumed,
        LegacyMutationDisabled,
    }

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(n: BlockNumberFor<T>) -> Weight {
            Self::process_releases_at_block(n)
        }

        fn on_finalize(_n: BlockNumberFor<T>) {
            if !TrieModified::<T>::get() {
                return;
            }

            let asset_root = Self::compute_asset_root();
            let certificate_root = Self::compute_certificate_root();

            let asset_digest_item = crate::digest_item::create_asset_root_digest(asset_root);
            let certificate_digest_item =
                crate::digest_item::create_certificate_root_digest(certificate_root);

            frame_system::Pallet::<T>::deposit_log(asset_digest_item);
            frame_system::Pallet::<T>::deposit_log(certificate_digest_item);

            Self::deposit_event(Event::AssetRootUpdated { root: asset_root });
            Self::deposit_event(Event::CertificateRootUpdated {
                root: certificate_root,
            });

            TrieModified::<T>::set(false);
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// Toggle key construction mode for Exp2: true = hash-scattered, false = prefix-aggregated
        #[pallet::call_index(15)]
        #[pallet::weight(0)]
        pub fn set_key_mode(origin: OriginFor<T>, hash_mode: bool) -> DispatchResult {
            ensure_root(origin)?;
            UseHashScatteredKeys::<T>::put(hash_mode);
            Ok(())
        }

        /// Read and reset block cert/put counters for Exp2
        #[pallet::call_index(16)]
        #[pallet::weight(0)]
        pub fn get_and_reset_counters(origin: OriginFor<T>) -> DispatchResultWithPostInfo {
            ensure_root(origin)?;
            let certs = BlockCertCount::<T>::get();
            let puts = BlockPutCalls::<T>::get();
            BlockCertCount::<T>::put(0);
            BlockPutCalls::<T>::put(0);
            Self::deposit_event(Event::CountersSnapshot {
                cert_count: certs,
                put_count: puts,
            });
            Ok(().into())
        }

        /// Toggle zero-weight mode for Exp2 certs/block measurement
        #[pallet::call_index(17)]
        #[pallet::weight(0)]
        pub fn set_zero_weight(origin: OriginFor<T>, enable: bool) -> DispatchResult {
            ensure_root(origin)?;
            UseZeroWeight::<T>::put(enable);
            Ok(())
        }

        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::register_asset())]
        pub fn register_asset(
            origin: OriginFor<T>,
            name: Vec<u8>,
            description: Vec<u8>,
            raw_data_hash: H256,
            data_size_bytes: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_register_asset(
                who,
                name,
                description,
                raw_data_hash,
                data_size_bytes,
                Vec::new(),
                Vec::new(),
                EncryptionInfo::default(),
            )
        }

        #[pallet::call_index(12)]
        #[pallet::weight(<T as Config>::WeightInfo::register_asset())]
        pub fn register_asset_with_metadata(
            origin: OriginFor<T>,
            name: Vec<u8>,
            description: Vec<u8>,
            raw_data_hash: H256,
            data_size_bytes: u64,
            metadata_cid: Vec<u8>,
            data_cid: Vec<u8>,
            encryption_info: EncryptionInfo,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::do_register_asset(
                who,
                name,
                description,
                raw_data_hash,
                data_size_bytes,
                metadata_cid,
                data_cid,
                encryption_info,
            )
        }

        /// Experiment-only asset registration path without collateral locking or incentive payout.
        #[pallet::call_index(11)]
        #[pallet::weight(<T as Config>::WeightInfo::register_asset_core())]
        pub fn register_asset_core(
            origin: OriginFor<T>,
            name: Vec<u8>,
            description: Vec<u8>,
            raw_data_hash: H256,
            data_size_bytes: u64,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::validate_asset_registration_fields(&name, &description)?;
            let timestamp = Self::current_timestamp();
            let registration_block = Self::current_validity_block();
            let asset_id = DataAsset::generate_asset_id(&who, registration_block, &raw_data_hash);
            ensure!(
                Self::get_asset(&asset_id).is_none(),
                Error::<T>::InvalidInput
            );
            let token_id = Self::store_registered_asset(
                &who,
                name,
                description,
                raw_data_hash,
                timestamp,
                asset_id,
                data_size_bytes,
                Vec::new(),
                Vec::new(),
                EncryptionInfo::default(),
            )?;

            Self::deposit_event(Event::AssetRegistered {
                asset_id,
                token_id,
                owner: who,
                collateral: BalanceOf::<T>::zero(),
            });
            Ok(())
        }

        // ！！！！！！！！！！由于双层状态树不使用了，所以需要重新实现，并且发行权证的费用要覆盖权证行权的费用
        // !!!!!!!!!!!!发行权证，交易的发起者要么是资产所有者，要么是被授权的市场账户
        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::issue_certificate())]
        pub fn issue_certificate(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            holder: T::AccountId,
            right_type: u8,
            valid_until: Option<u64>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;
            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;

            let is_owner = asset.core.owner == who;
            let is_approved =
                Self::asset_approvals(&asset_id).map_or(false, |operator| operator == who);

            ensure!(is_owner || is_approved, Error::<T>::NotAuthorized);
            Self::issue_certificate_internal(&asset_id, &asset, &holder, right_type, valid_until)?;
            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::transfer_asset())]
        pub fn transfer_asset(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            new_owner: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;

            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner); // 在未被授权的时候，只有资产所有者才能转移资产
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked); // 锁定的资产不能转移
            ensure!(
                !AssetApprovals::<T>::contains_key(&asset_id),
                Error::<T>::AlreadyAuthorized
            ); // 授权后的资产只有授权的市场可以交易
            let old_owner = Self::transfer_asset_owner_state(&asset_id, &who, &new_owner, None)?;
            // 如果所有者自己转移资产，清除该资产上所有未完成的市场授权。确保授权记录不会残留。
            // 但是这样会导致市场方无法继续操作资产，必须重新授权。
            AssetApprovals::<T>::remove(asset_id);

            T::IncentiveHandler::register_asset_trade(&asset_id);
            Self::deposit_event(Event::AssetTransferred {
                asset_id,
                from: old_owner,
                to: new_owner,
            });
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::revoke_certificate())]
        pub fn revoke_certificate(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;

            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let cert = Self::get_certificate(&asset_id, &certificate_id)
                .ok_or(Error::<T>::CertificateNotFound)?;

            ensure!(
                asset.core.owner == who || cert.holder == who,
                Error::<T>::NotOwner
            );
            Self::ensure_right_order_available(&certificate_id, None)?;

            Self::remove_certificate(&asset_id, &certificate_id)?;
            ActiveCertificateCount::<T>::mutate(&asset_id, |count| {
                *count = count.saturating_sub(1)
            });

            Self::deposit_event(Event::CertificateRevoked {
                asset_id,
                certificate_id,
            });
            Ok(())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(<T as Config>::WeightInfo::lock_asset())]
        pub fn lock_asset(origin: OriginFor<T>, asset_id: [u8; 32]) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;
            // let caller = Self::account_to_h160(&who);

            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner);

            asset.core.status = AssetStatus::Locked;
            asset.core.state_version = asset
                .core
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            asset.core.updated_at = Self::current_timestamp();

            Self::insert_asset(&asset_id, &asset)?;
            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::unlock_asset())]
        pub fn unlock_asset(origin: OriginFor<T>, asset_id: [u8; 32]) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;
            // let caller = Self::account_to_h160(&who);

            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner);

            asset.core.status = AssetStatus::Tradable;
            asset.core.state_version = asset
                .core
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            asset.core.updated_at = Self::current_timestamp();

            Self::insert_asset(&asset_id, &asset)?;
            Ok(())
        }

        /// 手动罚没部分抵押品（仅限 sudo/governance）
        #[pallet::call_index(6)]
        #[pallet::weight(<T as Config>::WeightInfo::slash_collateral())]
        pub fn slash_asset_collateral(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            slash_percentage: u8,
        ) -> DispatchResult {
            // Only sudo can slash
            ensure_root(origin)?;

            Self::slash_collateral(&asset_id, slash_percentage)?;

            Ok(())
        }

        /// 授权资产给市场账户（或其他账户）
        #[pallet::call_index(7)] // 索引号递增，不重复
        #[pallet::weight(<T as Config>::WeightInfo::authorize_operator())]
        pub fn authorize_market(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            market_account: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;

            // 验证资产存在且属于调用者
            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner);
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked); // 锁定资产不允许改变授权状态
            ensure!(
                !AssetApprovals::<T>::contains_key(&asset_id),
                Error::<T>::AlreadyAuthorized
            ); // 已被授权的资产不能再次授权

            // 存储授权信息
            AssetApprovals::<T>::insert(&asset_id, &market_account);

            // 发出事件
            Self::deposit_event(Event::AssetAuthorized {
                asset_id,
                owner: who,
                operator: market_account,
            });

            Ok(())
        }

        /// 撤销对市场的授权
        #[pallet::call_index(8)]
        #[pallet::weight(<T as Config>::WeightInfo::revoke_authorization())]
        pub fn revoke_authorization(origin: OriginFor<T>, asset_id: [u8; 32]) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;

            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner);

            if AssetApprovals::<T>::contains_key(&asset_id) {
                AssetApprovals::<T>::remove(&asset_id);

                Self::deposit_event(Event::AuthorizationRevoked {
                    asset_id,
                    owner: who,
                });
            }

            Ok(())
        }

        /// 市场账户（被授权方）转移资产，还有一个transfer_by_market_internal供链扩展调用
        #[pallet::call_index(9)]
        #[pallet::weight(10_000)]
        pub fn transfer_asset_by_market(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            new_owner: T::AccountId,
        ) -> DispatchResult {
            let market = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;

            // 1. 获取资产
            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;

            // 2. 验证是否被授权
            let approved_account =
                Self::asset_approvals(&asset_id).ok_or(Error::<T>::NotAuthorized)?;
            ensure!(approved_account == market, Error::<T>::NotAuthorized);

            // 3. 检查资产状态
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked);

            let old_owner =
                Self::transfer_asset_owner_state(&asset_id, &asset.core.owner, &new_owner, None)?;

            // 6. 转移后通常清除授权（ERC721标准行为，防止前任市场继续控制）
            AssetApprovals::<T>::remove(&asset_id);

            T::IncentiveHandler::register_asset_trade(&asset_id);

            // 7. 发出事件
            Self::deposit_event(Event::AssetTransferred {
                asset_id,
                from: old_owner,
                to: new_owner,
            });

            Ok(())
        }

        /// 转移权证，只有当前权证持有人可以发起。
        #[pallet::call_index(10)]
        #[pallet::weight(<T as Config>::WeightInfo::transfer_certificate())]
        pub fn transfer_certificate(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            new_owner: T::AccountId,
        ) -> DispatchResult {
            let current_owner = ensure_signed(origin)?;
            Self::ensure_legacy_mutation_enabled()?;
            Self::transfer_certificate_internal(
                &asset_id,
                &certificate_id,
                &current_owner,
                &new_owner,
            )
        }

        #[pallet::call_index(18)]
        #[pallet::weight(<T as Config>::WeightInfo::authorize_operator())]
        pub fn create_issue_right_authorization(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            market_id: [u8; 32],
            terms: IssueRightAuthorizationTerms<BlockNumberFor<T>>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_create_issue_right_authorization(&owner, asset_id, market_id, terms)
        }

        #[pallet::call_index(19)]
        #[pallet::weight(<T as Config>::WeightInfo::authorize_operator())]
        pub fn create_transfer_meta_authorization(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            market_id: [u8; 32],
            terms: TransferMetaAuthorizationTerms<T::AccountId, BlockNumberFor<T>>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_create_transfer_meta_authorization(&owner, asset_id, market_id, terms)
        }

        #[pallet::call_index(20)]
        #[pallet::weight(<T as Config>::WeightInfo::revoke_authorization())]
        pub fn revoke_issue_right_authorization(
            origin: OriginFor<T>,
            auth_id: [u8; 32],
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_revoke_issue_right_authorization(&owner, auth_id)
        }

        #[pallet::call_index(21)]
        #[pallet::weight(<T as Config>::WeightInfo::revoke_authorization())]
        pub fn revoke_transfer_meta_authorization(
            origin: OriginFor<T>,
            auth_id: [u8; 32],
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::do_revoke_transfer_meta_authorization(&owner, auth_id)
        }

        #[pallet::call_index(22)]
        #[pallet::weight(<T as Config>::WeightInfo::revoke_authorization())]
        pub fn expire_issue_right_authorization(
            origin: OriginFor<T>,
            auth_id: [u8; 32],
        ) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_expire_issue_right_authorization(auth_id)
        }

        #[pallet::call_index(23)]
        #[pallet::weight(<T as Config>::WeightInfo::revoke_authorization())]
        pub fn expire_transfer_meta_authorization(
            origin: OriginFor<T>,
            auth_id: [u8; 32],
        ) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_expire_transfer_meta_authorization(auth_id)
        }

        #[pallet::call_index(24)]
        #[pallet::weight(<T as Config>::WeightInfo::create_order())]
        #[frame_support::transactional]
        pub fn create_order(
            origin: OriginFor<T>,
            order: protocol::OrderCommitment,
            price_policy_witness: protocol::PricePolicyWitness,
            seller_commitment: protocol::SellerCommitment,
            seller_signature: protocol::ProtocolSignature,
        ) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_create_order(
                order,
                price_policy_witness,
                seller_commitment,
                seller_signature,
            )
        }

        #[pallet::call_index(25)]
        #[pallet::weight(<T as Config>::WeightInfo::cancel_open())]
        #[frame_support::transactional]
        pub fn cancel_open(origin: OriginFor<T>, order_id: [u8; 32]) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            Self::do_cancel_open(&caller, order_id)
        }

        #[pallet::call_index(26)]
        #[pallet::weight(<T as Config>::WeightInfo::expire_open())]
        #[frame_support::transactional]
        pub fn expire_open(origin: OriginFor<T>, order_id: [u8; 32]) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_expire_open(order_id)
        }

        #[pallet::call_index(27)]
        #[pallet::weight(<T as Config>::WeightInfo::fund_order())]
        #[frame_support::transactional]
        pub fn fund_order(
            origin: OriginFor<T>,
            order_id: [u8; 32],
            buyer_commitment: protocol::BuyerCommitment,
            buyer_signature: protocol::ProtocolSignature,
        ) -> DispatchResult {
            let payer = ensure_signed(origin)?;
            Self::do_fund_order(&payer, order_id, buyer_commitment, buyer_signature)
        }

        #[pallet::call_index(28)]
        #[pallet::weight(<T as Config>::WeightInfo::refund_funded())]
        #[frame_support::transactional]
        pub fn refund_funded(origin: OriginFor<T>, order_id: [u8; 32]) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_refund_funded(order_id)
        }

        #[pallet::call_index(29)]
        #[pallet::weight(<T as Config>::WeightInfo::settle())]
        #[frame_support::transactional]
        pub fn settle(origin: OriginFor<T>, order_id: [u8; 32]) -> DispatchResult {
            ensure_signed(origin)?;
            Self::do_settle(order_id)
        }

        /// Root-only constructor for invalid E2 right fixtures on a fresh
        /// disposable chain. It cannot overwrite an existing right.
        #[pallet::call_index(30)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 4))]
        #[frame_support::transactional]
        pub fn initialize_right_for_experiment(
            origin: OriginFor<T>,
            right: RightToken<T::AccountId>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                right.right_id != [0u8; 32],
                Error::<T>::InvalidRightParameters
            );
            ensure!(right.state_version == 1, Error::<T>::InvalidRightParameters);
            ensure!(
                right.has_valid_parameters(),
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                right.status != CertificateStatus::Exhausted || right.remaining_uses == Some(0),
                Error::<T>::InvalidRightParameters
            );
            let asset = Self::get_asset(&right.asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == right.issuer, Error::<T>::NotOwner);
            ensure!(asset.is_tradable(), Error::<T>::AssetNotActive);
            ensure!(
                Self::get_certificate(&right.asset_id, &right.right_id).is_none(),
                Error::<T>::RightAlreadyExists
            );
            let active = if right.status == CertificateStatus::Active {
                Some(
                    ActiveCertificateCount::<T>::get(&right.asset_id)
                        .checked_add(1)
                        .ok_or(Error::<T>::TooManyRightsForAsset)?,
                )
            } else {
                None
            };
            Self::insert_certificate(&right.asset_id, &right)?;
            if let Some(count) = active {
                ActiveCertificateCount::<T>::insert(right.asset_id, count);
            }
            Self::deposit_event(Event::CertificateIssued {
                asset_id: right.asset_id,
                certificate_id: right.right_id,
                issuer: right.issuer,
                holder: right.holder,
            });
            Ok(())
        }

        /// Root-only one-delta fixture mutation for E3 late-failure cases.
        #[pallet::call_index(31)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn configure_meta_status_for_experiment(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            status: AssetStatus,
        ) -> DispatchResult {
            ensure_root(origin)?;
            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            asset.core.status = status;
            Self::put_asset_status_fixture(&asset_id, &asset)
        }

        /// Root-only one-delta fixture mutation for E3 late-failure cases.
        #[pallet::call_index(32)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn configure_right_status_for_experiment(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            right_id: [u8; 32],
            status: CertificateStatus,
        ) -> DispatchResult {
            ensure_root(origin)?;
            let mut right = Self::get_certificate(&asset_id, &right_id)
                .ok_or(Error::<T>::CertificateNotFound)?;
            right.status = status;
            Self::put_right_status_fixture(&asset_id, &right)
        }
    }

    impl<T: Config> Pallet<T> {
        fn ensure_legacy_mutation_enabled() -> DispatchResult {
            ensure!(
                T::LegacyMutationEnabled::get(),
                Error::<T>::LegacyMutationDisabled
            );
            Ok(())
        }

        fn map_protocol_error(error: protocol::ProtocolError) -> Error<T> {
            match error {
                protocol::ProtocolError::UnsupportedProtocolVersion => {
                    Error::<T>::UnsupportedProtocolVersion
                }
                protocol::ProtocolError::MalformedCommitment => Error::<T>::MalformedCommitment,
                protocol::ProtocolError::InvalidSignature => Error::<T>::InvalidSignature,
                protocol::ProtocolError::NonceUsed => Error::<T>::NonceUsed,
                protocol::ProtocolError::StateVersionMismatch => Error::<T>::StateVersionMismatch,
                protocol::ProtocolError::OrderExpired => Error::<T>::OrderExpired,
                protocol::ProtocolError::NonceOverflow => Error::<T>::ProtocolNonceOverflow,
                protocol::ProtocolError::FeeOutOfBounds => Error::<T>::FeeOutOfBounds,
                protocol::ProtocolError::ArithmeticOverflow => Error::<T>::PaymentAmountInvalid,
            }
        }

        pub fn validate_seller_protocol_commitment(
            genesis_hash: protocol::Hash32,
            order: &protocol::OrderCommitment,
            commitment: &protocol::SellerCommitment,
            signature: &protocol::ProtocolSignature,
            current_state_version: u64,
            current_block: u32,
        ) -> Result<protocol::Hash32, DispatchError> {
            let nonce = SellerNonce::<T>::get(commitment.seller, commitment.object_id);
            protocol::verify_seller_commitment(
                genesis_hash,
                order,
                commitment,
                signature,
                nonce,
                current_state_version,
                current_block,
            )
            .map_err(|error| Self::map_protocol_error(error).into())
        }

        pub fn validate_buyer_protocol_commitment(
            genesis_hash: protocol::Hash32,
            order: &protocol::OrderCommitment,
            commitment: &protocol::BuyerCommitment,
            signature: &protocol::ProtocolSignature,
            current_block: u32,
        ) -> Result<protocol::Hash32, DispatchError> {
            let nonce = BuyerNonce::<T>::get(commitment.buyer);
            protocol::verify_buyer_commitment(
                genesis_hash,
                order,
                commitment,
                signature,
                nonce,
                current_block,
            )
            .map_err(|error| Self::map_protocol_error(error).into())
        }

        #[allow(dead_code)]
        pub(crate) fn consume_seller_nonce(
            seller: &protocol::ProtocolAccountId,
            object_id: &[u8; 32],
            nonce: u64,
        ) -> DispatchResult {
            SellerNonce::<T>::try_mutate(seller, object_id, |current| {
                ensure!(*current == nonce, Error::<T>::NonceUsed);
                *current = protocol::next_nonce(*current)
                    .map_err(|error| Self::map_protocol_error(error))?;
                Ok(())
            })
        }

        #[allow(dead_code)]
        pub(crate) fn consume_buyer_nonce(
            buyer: &protocol::ProtocolAccountId,
            nonce: u64,
        ) -> DispatchResult {
            BuyerNonce::<T>::try_mutate(buyer, |current| {
                ensure!(*current == nonce, Error::<T>::NonceUsed);
                *current = protocol::next_nonce(*current)
                    .map_err(|error| Self::map_protocol_error(error))?;
                Ok(())
            })
        }

        fn map_market_access_error(error: MarketAccessError) -> Error<T> {
            match error {
                MarketAccessError::NotRegistered => Error::<T>::MarketNotRegistered,
                MarketAccessError::Restricted => Error::<T>::MarketRestricted,
                MarketAccessError::CapabilityDenied => Error::<T>::CapabilityDenied,
                MarketAccessError::ActiveOrderCapacityExceeded => {
                    Error::<T>::MarketActiveOrderCapacityExceeded
                }
                MarketAccessError::ActiveOrderCounterOverflow
                | MarketAccessError::ActiveOrderCounterUnderflow => {
                    Error::<T>::MarketActiveOrderCounterInvariant
                }
                MarketAccessError::FundedOrderCapacityExceeded
                | MarketAccessError::ExposureCapacityExceeded => Error::<T>::MarketCapacityExceeded,
                MarketAccessError::FundedOrderCounterOverflow
                | MarketAccessError::FundedOrderCounterUnderflow => {
                    Error::<T>::MarketFundedCounterInvariant
                }
                MarketAccessError::ExposureCounterOverflow
                | MarketAccessError::ExposureCounterUnderflow => {
                    Error::<T>::MarketExposureInvariant
                }
                MarketAccessError::AuthorizationCounterOverflow
                | MarketAccessError::AuthorizationCounterUnderflow => {
                    Error::<T>::AuthorizationCounterInvariant
                }
            }
        }

        fn ensure_market_operation(
            market_id: &[u8; 32],
            operation: MarketOperation,
        ) -> DispatchResult {
            T::MarketRegistry::ensure_market_operation(market_id, operation)
                .map_err(Self::map_market_access_error)?;
            Ok(())
        }

        fn protocol_block_number() -> u32 {
            frame_system::Pallet::<T>::block_number().saturated_into()
        }

        fn protocol_genesis_hash() -> Result<[u8; 32], DispatchError> {
            let block_hash = frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero());
            block_hash
                .as_ref()
                .try_into()
                .map_err(|_| Error::<T>::MalformedCommitment.into())
        }

        pub fn price_policy_commitment_hash(policy: &PricePolicy) -> [u8; 32] {
            blake2_256(&policy.encode())
        }

        pub fn issue_right_parameters_commitment_hash(
            authorization: &IssueRightAuthorization<T::AccountId, BlockNumberFor<T>>,
        ) -> [u8; 32] {
            blake2_256(
                &(
                    authorization.right_type,
                    authorization.data_scope,
                    &authorization.validity_template,
                    authorization.transferable,
                    &authorization.allowed_transfer_markets,
                    authorization.royalty_bps,
                )
                    .encode(),
            )
        }

        pub fn right_parameters_commitment_hash(right: &RightToken<T::AccountId>) -> [u8; 32] {
            blake2_256(
                &(
                    right.right_type,
                    right.data_scope,
                    &right.validity,
                    right.transferable,
                    &right.allowed_transfer_markets,
                    right.royalty_bps,
                )
                    .encode(),
            )
        }

        fn ensure_protocol_seller(
            seller: &T::AccountId,
            protocol_seller: &protocol::ProtocolAccountId,
        ) -> DispatchResult {
            let encoded = T::ProtocolAccountIdConverter::to_protocol_account_id(seller)
                .ok_or(Error::<T>::ProtocolAccountIdInvalid)?;
            ensure!(encoded == *protocol_seller, Error::<T>::OwnershipMismatch);
            Ok(())
        }

        fn current_order_seller_and_version(
            order: &protocol::OrderCommitment,
        ) -> Result<(T::AccountId, u64), DispatchError> {
            match order.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                    let asset =
                        Self::get_asset(&order.object_id).ok_or(Error::<T>::AssetNotFound)?;
                    Ok((asset.core.owner, asset.core.state_version))
                }
                MarketOperation::TransferRight => {
                    let parent_asset_id =
                        order.parent_asset_id.ok_or(Error::<T>::ParentMismatch)?;
                    let indexed_parent = Self::right_parent_asset(&order.object_id)
                        .ok_or(Error::<T>::RightInvalid)?;
                    ensure!(
                        indexed_parent == parent_asset_id,
                        Error::<T>::ParentMismatch
                    );
                    let right = Self::get_certificate(&parent_asset_id, &order.object_id)
                        .ok_or(Error::<T>::RightInvalid)?;
                    ensure!(
                        right.asset_id == parent_asset_id,
                        Error::<T>::ParentMismatch
                    );
                    Ok((right.holder, right.state_version))
                }
            }
        }

        fn protocol_allowed_buyer(
            account: Option<&T::AccountId>,
        ) -> Result<Option<protocol::ProtocolAccountId>, DispatchError> {
            account
                .map(|buyer| {
                    T::ProtocolAccountIdConverter::to_protocol_account_id(buyer)
                        .ok_or(Error::<T>::ProtocolAccountIdInvalid.into())
                })
                .transpose()
        }

        fn protocol_buyer_account(
            buyer: &protocol::ProtocolAccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let account = T::ProtocolAccountIdConverter::from_protocol_account_id(buyer)
                .ok_or(Error::<T>::PayerBuyerMismatch)?;
            let round_trip = T::ProtocolAccountIdConverter::to_protocol_account_id(&account)
                .ok_or(Error::<T>::PayerBuyerMismatch)?;
            ensure!(round_trip == *buyer, Error::<T>::PayerBuyerMismatch);
            Ok(account)
        }

        fn protocol_balance(value: u128) -> Result<BalanceOf<T>, DispatchError> {
            let balance: BalanceOf<T> = value.saturated_into();
            ensure!(
                balance.saturated_into::<u128>() == value,
                Error::<T>::PaymentAmountInvalid
            );
            Ok(balance)
        }

        pub fn query_order(order_id: &[u8; 32]) -> Option<protocol::RuntimeOrder> {
            RuntimeOrders::<T>::get(order_id)
        }

        pub fn query_settlement(order_id: &[u8; 32]) -> Option<protocol::SettlementRecord> {
            SettlementRecords::<T>::get(order_id)
        }

        pub fn query_order_price_policy(order_id: &[u8; 32]) -> Option<PricePolicy> {
            OrderPricePolicySnapshots::<T>::get(order_id)
        }

        pub fn query_meta_token(asset_id: &[u8; 32]) -> Option<DataAsset<T::AccountId>> {
            Self::get_asset(asset_id)
        }

        pub fn query_right_token(
            asset_id: &[u8; 32],
            right_id: &[u8; 32],
        ) -> Option<RightToken<T::AccountId>> {
            Self::get_certificate(asset_id, right_id)
        }

        pub fn query_issue_right_authorization(
            auth_id: &[u8; 32],
        ) -> Option<IssueRightAuthorization<T::AccountId, BlockNumberFor<T>>> {
            IssueRightAuthorizations::<T>::get(auth_id)
        }

        pub fn query_transfer_meta_authorization(
            auth_id: &[u8; 32],
        ) -> Option<TransferMetaAuthorization<T::AccountId, BlockNumberFor<T>>> {
            TransferMetaAuthorizations::<T>::get(auth_id)
        }

        pub fn query_payment(order_id: &[u8; 32]) -> Option<protocol::RuntimeEscrow> {
            RuntimeOrders::<T>::get(order_id)
                .and_then(|order| order.escrow_id)
                .and_then(RuntimeEscrows::<T>::get)
        }

        pub fn query_market(
            market_id: &[u8; 32],
        ) -> Result<MarketStateSnapshot<T::AccountId>, DispatchError> {
            T::MarketRegistry::market_state(market_id)
                .map_err(|error| Self::map_market_access_error(error).into())
        }

        fn protocol_account(account: &T::AccountId) -> Result<[u8; 32], DispatchError> {
            T::ProtocolAccountIdConverter::to_protocol_account_id(account)
                .ok_or(Error::<T>::ProtocolAccountIdInvalid.into())
        }

        fn normalized_market_snapshot(
            market_id: &[u8; 32],
        ) -> Result<protocol::NormalizedMarketSnapshot, DispatchError> {
            let market = Self::query_market(market_id)?;
            Ok(protocol::NormalizedMarketSnapshot {
                market_id: market.market_id,
                operator: Self::protocol_account(&market.operator)?,
                protocol_version: market.protocol_version,
                capabilities: market.capabilities,
                capacity_tier: market.capacity_tier,
                active_orders: market.active_orders,
                funded_orders: market.funded_orders,
                active_exposure_value: market.active_exposure_value,
                active_authorizations: market.active_authorizations,
                status: market.status,
                market_fee_bps: market.market_fee_bps,
            })
        }

        fn normalized_object_snapshot(
            order: &protocol::RuntimeOrder,
        ) -> Result<protocol::NormalizedObjectSnapshot, DispatchError> {
            let commitment = &order.order_commitment;
            let asset_id = commitment.parent_asset_id.unwrap_or(commitment.object_id);
            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let right_id = match commitment.operation {
                MarketOperation::TransferMeta => None,
                MarketOperation::IssueRight => Some(protocol::right_id(
                    Self::protocol_genesis_hash()?,
                    MarketOperation::IssueRight,
                    order.order_id,
                )),
                MarketOperation::TransferRight => Some(commitment.object_id),
            };
            let right = right_id.and_then(|id| Self::get_certificate(&asset_id, &id));
            let active_order_id = match commitment.operation {
                MarketOperation::TransferMeta => ActiveMetaOrderByAsset::<T>::get(asset_id),
                MarketOperation::IssueRight => None,
                MarketOperation::TransferRight => {
                    ActiveRightOrderByRight::<T>::get(commitment.object_id)
                }
            };
            Ok(protocol::NormalizedObjectSnapshot {
                asset_id,
                asset_owner: Self::protocol_account(&asset.core.owner)?,
                asset_status: asset.core.status.encode()[0],
                asset_state_version: asset.core.state_version,
                right_id,
                right_exists: right.is_some(),
                right_holder: right
                    .as_ref()
                    .map(|value| Self::protocol_account(&value.holder))
                    .transpose()?,
                right_status: right.as_ref().map(|value| value.status.encode()[0]),
                right_state_version: right.as_ref().map(|value| value.state_version),
                right_parameters_hash: right.as_ref().map(Self::right_parameters_commitment_hash),
                active_order_id,
            })
        }

        fn normalized_authorization_snapshot(
            order: &protocol::RuntimeOrder,
        ) -> Result<protocol::NormalizedAuthorizationSnapshot, DispatchError> {
            let commitment = &order.order_commitment;
            match commitment.operation {
                MarketOperation::TransferMeta => {
                    let authorization =
                        TransferMetaAuthorizations::<T>::get(order.authorization_id)
                            .ok_or(Error::<T>::AuthorizationMissing)?;
                    Ok(protocol::NormalizedAuthorizationSnapshot {
                        authorization_id: authorization.auth_id,
                        kind: protocol::NormalizedAuthorizationKind::TransferMeta,
                        asset_id: authorization.asset_id,
                        owner: Self::protocol_account(&authorization.owner)?,
                        market_id: authorization.market_id,
                        policy_hash: Self::price_policy_commitment_hash(
                            &authorization.price_policy,
                        ),
                        right_parameters_hash: None,
                        nonce: authorization.nonce,
                        expiry: authorization.expiry.saturated_into(),
                        status: authorization.status.encode()[0],
                        reserved_count: 0,
                        reserved_use_units: 0,
                        issued_count: 0,
                        issued_use_units: 0,
                        consumed_order_id: authorization.consumed_order_id,
                    })
                }
                MarketOperation::IssueRight => {
                    let authorization = IssueRightAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?;
                    Ok(protocol::NormalizedAuthorizationSnapshot {
                        authorization_id: authorization.auth_id,
                        kind: protocol::NormalizedAuthorizationKind::IssueRight,
                        asset_id: authorization.asset_id,
                        owner: Self::protocol_account(&authorization.owner)?,
                        market_id: authorization.market_id,
                        policy_hash: Self::price_policy_commitment_hash(
                            &authorization.price_policy,
                        ),
                        right_parameters_hash: Some(Self::issue_right_parameters_commitment_hash(
                            &authorization,
                        )),
                        nonce: authorization.nonce,
                        expiry: authorization.expiry.saturated_into(),
                        status: authorization.status.encode()[0],
                        reserved_count: authorization.reserved_count,
                        reserved_use_units: authorization.reserved_use_units,
                        issued_count: authorization.issued_count,
                        issued_use_units: authorization.issued_use_units,
                        consumed_order_id: None,
                    })
                }
                MarketOperation::TransferRight => Ok(protocol::NormalizedAuthorizationSnapshot {
                    authorization_id: commitment.authorization_id,
                    kind: protocol::NormalizedAuthorizationKind::TransferRightPolicy,
                    asset_id: commitment
                        .parent_asset_id
                        .ok_or(Error::<T>::ParentMismatch)?,
                    owner: commitment.seller,
                    market_id: commitment.market_id,
                    policy_hash: commitment.policy_hash,
                    right_parameters_hash: commitment.right_parameters_hash,
                    nonce: commitment.order_nonce,
                    expiry: commitment.expiry,
                    status: 0,
                    reserved_count: 0,
                    reserved_use_units: 0,
                    issued_count: 0,
                    issued_use_units: 0,
                    consumed_order_id: None,
                }),
            }
        }

        pub fn query_normalized_settlement_snapshot(
            order_id: &[u8; 32],
        ) -> Result<protocol::NormalizedSettlementSnapshot, DispatchError> {
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let payment = RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            Ok(protocol::NormalizedSettlementSnapshot {
                object: Self::normalized_object_snapshot(&order)?,
                authorization: Self::normalized_authorization_snapshot(&order)?,
                payment,
                settlement: SettlementRecords::<T>::get(order_id),
                market: Self::normalized_market_snapshot(&order.order_commitment.market_id)?,
                order,
            })
        }

        pub fn normalized_settlement_commitment(
            order_id: &[u8; 32],
        ) -> Result<[u8; 32], DispatchError> {
            Ok(protocol::normalized_state_commitment(
                &Self::query_normalized_settlement_snapshot(order_id)?,
            ))
        }

        fn funding_policy(
            order: &protocol::OrderCommitment,
            buyer: &T::AccountId,
            market_fee_bps: u16,
            price_policy: &PricePolicy,
        ) -> Result<(u16, u16, Option<T::AccountId>), DispatchError> {
            ensure!(
                order.policy_hash == Self::price_policy_commitment_hash(price_policy),
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::validate_price_policy(price_policy, order.price, market_fee_bps)?;
            ensure!(price_policy.safety_fee_bps == 0, Error::<T>::FeeOutOfBounds);
            match order.operation {
                MarketOperation::TransferMeta => {
                    let authorization = Self::validate_transfer_meta_authorization(
                        &order.authorization_id,
                        &order.object_id,
                        &order.market_id,
                    )?;
                    ensure!(
                        authorization
                            .allowed_buyer
                            .as_ref()
                            .map_or(true, |allowed| allowed == buyer),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    ensure!(
                        authorization.price_policy == *price_policy,
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    Ok((0, price_policy.safety_fee_bps, None))
                }
                MarketOperation::IssueRight => {
                    let authorization = Self::validate_issue_right_authorization(
                        &order.authorization_id,
                        &order.object_id,
                        &order.market_id,
                    )?;
                    ensure!(
                        authorization.price_policy == *price_policy,
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    Ok((0, price_policy.safety_fee_bps, None))
                }
                MarketOperation::TransferRight => {
                    let parent_asset_id =
                        order.parent_asset_id.ok_or(Error::<T>::ParentMismatch)?;
                    let right = Self::get_certificate(&parent_asset_id, &order.object_id)
                        .ok_or(Error::<T>::RightInvalid)?;
                    let royalty_recipient = if right.royalty_bps == 0 {
                        None
                    } else {
                        Some(
                            Self::get_asset(&parent_asset_id)
                                .ok_or(Error::<T>::AssetNotFound)?
                                .core
                                .owner,
                        )
                    };
                    Ok((right.royalty_bps, 0, royalty_recipient))
                }
            }
        }

        fn validate_order_authority(
            order: &protocol::OrderCommitment,
            allowed_order_id: Option<&[u8; 32]>,
        ) -> DispatchResult {
            match order.operation {
                MarketOperation::TransferMeta => {
                    let authorization = Self::validate_transfer_meta_authorization(
                        &order.authorization_id,
                        &order.object_id,
                        &order.market_id,
                    )?;
                    ensure!(
                        order.policy_hash
                            == Self::price_policy_commitment_hash(&authorization.price_policy),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    ensure!(
                        order.allowed_buyer
                            == Self::protocol_allowed_buyer(authorization.allowed_buyer.as_ref())?,
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    ensure!(
                        order.expiry == authorization.expiry.saturated_into::<u32>(),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    Self::validate_price_policy(&authorization.price_policy, order.price, 0)?;
                    Self::ensure_meta_transfer_available(&order.object_id, allowed_order_id)?;
                }
                MarketOperation::IssueRight => {
                    let authorization = Self::validate_issue_right_authorization(
                        &order.authorization_id,
                        &order.object_id,
                        &order.market_id,
                    )?;
                    ensure!(
                        order.policy_hash
                            == Self::price_policy_commitment_hash(&authorization.price_policy),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    ensure!(
                        order.right_parameters_hash
                            == Some(Self::issue_right_parameters_commitment_hash(&authorization)),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    ensure!(
                        order.expiry == authorization.expiry.saturated_into::<u32>(),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                    Self::validate_price_policy(&authorization.price_policy, order.price, 0)?;
                }
                MarketOperation::TransferRight => {
                    Self::ensure_market_operation(&order.market_id, order.operation)?;
                    let parent_asset_id =
                        order.parent_asset_id.ok_or(Error::<T>::ParentMismatch)?;
                    let indexed_parent = Self::right_parent_asset(&order.object_id)
                        .ok_or(Error::<T>::RightInvalid)?;
                    ensure!(
                        indexed_parent == parent_asset_id,
                        Error::<T>::ParentMismatch
                    );
                    let right = Self::get_certificate(&parent_asset_id, &order.object_id)
                        .ok_or(Error::<T>::RightInvalid)?;
                    ensure!(
                        right.asset_id == parent_asset_id
                            && Self::is_right_indexed_for_asset(&parent_asset_id, &order.object_id)
                            && Self::is_right_indexed_for_holder(&right.holder, &order.object_id),
                        Error::<T>::RightInvalid
                    );
                    ensure!(
                        right.is_valid(Self::current_validity_block()),
                        Error::<T>::RightInvalid
                    );
                    ensure!(right.transferable, Error::<T>::AuthorizationMissing);
                    ensure!(
                        right.allowed_transfer_markets.contains(&order.market_id),
                        Error::<T>::AuthorizationMissing
                    );
                    ensure!(
                        order.right_parameters_hash
                            == Some(Self::right_parameters_commitment_hash(&right)),
                        Error::<T>::AuthorizationPolicyMismatch
                    );
                }
            }
            Ok(())
        }

        fn resolve_order_price_policy(
            order: &protocol::OrderCommitment,
            witness: protocol::PricePolicyWitness,
        ) -> Result<PricePolicy, DispatchError> {
            let policy = match (order.operation, witness) {
                (MarketOperation::TransferMeta, protocol::PricePolicyWitness::Authorization) => {
                    Self::validate_transfer_meta_authorization(
                        &order.authorization_id,
                        &order.object_id,
                        &order.market_id,
                    )?
                    .price_policy
                }
                (MarketOperation::IssueRight, protocol::PricePolicyWitness::Authorization) => {
                    Self::validate_issue_right_authorization(
                        &order.authorization_id,
                        &order.object_id,
                        &order.market_id,
                    )?
                    .price_policy
                }
                (
                    MarketOperation::TransferRight,
                    protocol::PricePolicyWitness::SellerSnapshot(policy),
                ) => policy,
                _ => return Err(Error::<T>::AuthorizationPolicyMismatch.into()),
            };
            ensure!(
                order.policy_hash == Self::price_policy_commitment_hash(&policy),
                Error::<T>::AuthorizationPolicyMismatch
            );
            Ok(policy)
        }

        fn reserve_order_object(
            order: &protocol::OrderCommitment,
            order_id: [u8; 32],
        ) -> DispatchResult {
            match order.operation {
                MarketOperation::TransferMeta => {
                    Self::reserve_meta_order(&order.object_id, order_id)
                }
                MarketOperation::IssueRight => Ok(()),
                MarketOperation::TransferRight => Self::reserve_right_order(
                    &order.parent_asset_id.ok_or(Error::<T>::ParentMismatch)?,
                    &order.object_id,
                    order_id,
                ),
            }
        }

        fn release_order_object(order: &protocol::RuntimeOrder) -> DispatchResult {
            match order.order_commitment.operation {
                MarketOperation::TransferMeta => {
                    Self::release_meta_order(&order.order_commitment.object_id, &order.order_id)
                }
                MarketOperation::IssueRight => Ok(()),
                MarketOperation::TransferRight => {
                    Self::release_right_order(&order.order_commitment.object_id, &order.order_id)
                }
            }
        }

        fn do_create_order(
            order: protocol::OrderCommitment,
            price_policy_witness: protocol::PricePolicyWitness,
            seller_commitment: protocol::SellerCommitment,
            seller_signature: protocol::ProtocolSignature,
        ) -> DispatchResult {
            let current_block = Self::protocol_block_number();
            let genesis_hash = Self::protocol_genesis_hash()?;
            let order_id = protocol::validate_order_commitment(genesis_hash, &order, current_block)
                .map_err(Self::map_protocol_error)?;
            ensure!(
                !RuntimeOrders::<T>::contains_key(order_id),
                Error::<T>::NonceUsed
            );

            let (seller, state_version) = Self::current_order_seller_and_version(&order)?;
            Self::ensure_protocol_seller(&seller, &order.seller)?;
            Self::validate_seller_protocol_commitment(
                genesis_hash,
                &order,
                &seller_commitment,
                &seller_signature,
                state_version,
                current_block,
            )?;
            Self::validate_order_authority(&order, None)?;
            let price_policy = Self::resolve_order_price_policy(&order, price_policy_witness)?;
            let market_terms = T::MarketRegistry::market_payment_terms(&order.market_id)
                .map_err(Self::map_market_access_error)?;
            Self::validate_price_policy(&price_policy, order.price, market_terms.market_fee_bps)?;
            ensure!(price_policy.safety_fee_bps == 0, Error::<T>::FeeOutOfBounds);
            Self::reserve_order_object(&order, order_id)?;
            T::MarketRegistry::note_active_order_created(&order.market_id)
                .map_err(Self::map_market_access_error)?;
            Self::consume_seller_nonce(&order.seller, &order.object_id, order.order_nonce)?;

            OrderPricePolicySnapshots::<T>::insert(order_id, price_policy);
            RuntimeOrders::<T>::insert(
                order_id,
                protocol::RuntimeOrder {
                    order_id,
                    order_commitment: order.clone(),
                    authorization_id: order.authorization_id,
                    seller_commitment,
                    buyer_commitment: None,
                    payment_asset: order.currency,
                    escrow_id: None,
                    created_at: current_block,
                    funded_at: None,
                    expiry: order.expiry,
                    grace_deadline: None,
                    state: protocol::OrderState::Open,
                },
            );
            Self::deposit_event(Event::OrderCreated {
                order_id,
                market_id: order.market_id,
                operation: order.operation,
                object_id: order.object_id,
            });
            Ok(())
        }

        fn ensure_current_order_seller(
            caller: &T::AccountId,
            order: &protocol::RuntimeOrder,
        ) -> DispatchResult {
            let (current_seller, _) =
                Self::current_order_seller_and_version(&order.order_commitment)?;
            ensure!(*caller == current_seller, Error::<T>::NotOwner);
            Ok(())
        }

        fn do_cancel_open(caller: &T::AccountId, order_id: [u8; 32]) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(
                order.state == protocol::OrderState::Open,
                Error::<T>::OrderStatusInvalid
            );
            Self::ensure_current_order_seller(caller, &order)?;
            Self::release_order_object(&order)?;
            T::MarketRegistry::note_active_order_closed(&order.order_commitment.market_id)
                .map_err(Self::map_market_access_error)?;
            order.state = protocol::OrderState::Cancelled;
            RuntimeOrders::<T>::insert(order_id, order);
            Self::deposit_event(Event::OrderCancelled { order_id });
            Ok(())
        }

        fn do_expire_open(order_id: [u8; 32]) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(
                order.state == protocol::OrderState::Open,
                Error::<T>::OrderStatusInvalid
            );
            ensure!(
                Self::protocol_block_number() > order.expiry,
                Error::<T>::OrderNotExpired
            );
            Self::release_order_object(&order)?;
            T::MarketRegistry::note_active_order_closed(&order.order_commitment.market_id)
                .map_err(Self::map_market_access_error)?;
            order.state = protocol::OrderState::Expired;
            RuntimeOrders::<T>::insert(order_id, order);
            Self::deposit_event(Event::OrderExpired { order_id });
            Ok(())
        }

        fn do_fund_order(
            payer: &T::AccountId,
            order_id: [u8; 32],
            buyer_commitment: protocol::BuyerCommitment,
            buyer_signature: protocol::ProtocolSignature,
        ) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(
                order.state == protocol::OrderState::Open,
                Error::<T>::OrderStatusInvalid
            );

            let current_block = Self::protocol_block_number();
            let genesis_hash = Self::protocol_genesis_hash()?;
            let recomputed_order_id = Self::validate_buyer_protocol_commitment(
                genesis_hash,
                &order.order_commitment,
                &buyer_commitment,
                &buyer_signature,
                current_block,
            )?;
            ensure!(
                recomputed_order_id == order_id,
                Error::<T>::MalformedCommitment
            );
            let buyer = Self::protocol_buyer_account(&buyer_commitment.buyer)?;
            ensure!(payer == &buyer, Error::<T>::PayerBuyerMismatch);

            let (current_seller, current_state_version) =
                Self::current_order_seller_and_version(&order.order_commitment)?;
            Self::ensure_protocol_seller(&current_seller, &order.order_commitment.seller)?;
            ensure!(
                current_state_version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );

            Self::validate_order_authority(&order.order_commitment, Some(&order_id))?;
            let market_terms =
                T::MarketRegistry::market_payment_terms(&order.order_commitment.market_id)
                    .map_err(Self::map_market_access_error)?;
            let price_policy = OrderPricePolicySnapshots::<T>::get(order_id)
                .ok_or(Error::<T>::AuthorizationPolicyMismatch)?;
            let (royalty_bps, safety_fee_bps, royalty_recipient) = Self::funding_policy(
                &order.order_commitment,
                &buyer,
                market_terms.market_fee_bps,
                &price_policy,
            )?;
            let amounts = protocol::amount_split(
                order.order_commitment.price,
                royalty_bps,
                market_terms.market_fee_bps,
                safety_fee_bps,
            )
            .map_err(Self::map_protocol_error)?;
            let grace_deadline = current_block
                .checked_add(T::FundedGraceBlocks::get())
                .ok_or(Error::<T>::GraceDeadlineOverflow)?;
            let escrow_id =
                protocol::escrow_id(genesis_hash, order.order_commitment.operation, order_id);
            ensure!(
                !RuntimeEscrows::<T>::contains_key(escrow_id),
                Error::<T>::EscrowMismatch
            );
            let market_fee_recipient = T::ProtocolAccountIdConverter::to_protocol_account_id(
                &market_terms.market_fee_recipient,
            )
            .ok_or(Error::<T>::ProtocolAccountIdInvalid)?;
            let royalty_recipient = royalty_recipient
                .as_ref()
                .map(|recipient| -> Result<_, DispatchError> {
                    T::ProtocolAccountIdConverter::to_protocol_account_id(recipient)
                        .ok_or(Error::<T>::ProtocolAccountIdInvalid.into())
                })
                .transpose()?;
            T::MarketRegistry::ensure_funding_capacity(
                &order.order_commitment.market_id,
                order.order_commitment.price,
            )
            .map_err(Self::map_market_access_error)?;
            if order.order_commitment.operation == MarketOperation::IssueRight {
                Self::reserve_issue_quota(
                    &order.authorization_id,
                    &order.order_commitment.object_id,
                    &order.order_commitment.market_id,
                    order_id,
                )?;
            }

            let amount = Self::protocol_balance(order.order_commitment.price)?;
            ensure!(
                T::Currency::free_balance(&buyer) >= amount,
                Error::<T>::InsufficientBalance
            );
            T::Currency::reserve(&buyer, amount).map_err(|_| Error::<T>::InsufficientBalance)?;
            Self::consume_buyer_nonce(&buyer_commitment.buyer, buyer_commitment.buyer_nonce)?;
            T::MarketRegistry::note_order_funded(
                &order.order_commitment.market_id,
                order.order_commitment.price,
            )
            .map_err(Self::map_market_access_error)?;
            RuntimeEscrows::<T>::insert(
                escrow_id,
                protocol::RuntimeEscrow {
                    escrow_id,
                    order_id,
                    payer: buyer_commitment.buyer,
                    buyer: buyer_commitment.buyer,
                    currency: order.payment_asset,
                    amounts,
                    royalty_recipient,
                    market_fee_recipient,
                    state: protocol::PaymentState::Escrowed,
                },
            );
            order.buyer_commitment = Some(buyer_commitment.clone());
            order.escrow_id = Some(escrow_id);
            order.funded_at = Some(current_block);
            order.grace_deadline = Some(grace_deadline);
            order.state = protocol::OrderState::Funded;
            RuntimeOrders::<T>::insert(order_id, order);

            Self::deposit_event(Event::OrderFunded {
                order_id,
                buyer: buyer_commitment.buyer,
                escrow_id,
                gross_price: buyer_commitment.price,
                grace_deadline,
            });
            Self::deposit_event(Event::PaymentEscrowed {
                order_id,
                escrow_id,
                amount: buyer_commitment.price,
            });
            Ok(())
        }

        fn do_refund_funded(order_id: [u8; 32]) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            match order.state {
                protocol::OrderState::Funded => {}
                protocol::OrderState::Open => return Err(Error::<T>::OrderNotFunded.into()),
                protocol::OrderState::Settled
                | protocol::OrderState::Cancelled
                | protocol::OrderState::Expired
                | protocol::OrderState::Refunded => return Err(Error::<T>::TerminalOrder.into()),
            }
            let grace_deadline = order.grace_deadline.ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                Self::protocol_block_number() > grace_deadline,
                Error::<T>::GracePeriodActive
            );
            let buyer_commitment = order
                .buyer_commitment
                .clone()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let mut escrow =
                RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.order_id == order_id
                    && escrow.escrow_id == escrow_id
                    && escrow.state == protocol::PaymentState::Escrowed
                    && escrow.currency == order.payment_asset
                    && escrow.amounts.gross_price == order.order_commitment.price
                    && escrow.payer == buyer_commitment.buyer
                    && escrow.buyer == buyer_commitment.buyer,
                Error::<T>::EscrowMismatch
            );
            let buyer = Self::protocol_buyer_account(&buyer_commitment.buyer)?;
            let amount = Self::protocol_balance(escrow.amounts.gross_price)?;
            let remainder = T::Currency::unreserve(&buyer, amount);
            ensure!(remainder.is_zero(), Error::<T>::EscrowMismatch);

            if order.order_commitment.operation == MarketOperation::IssueRight {
                Self::release_issue_quota(&order.authorization_id, &order_id)?;
            }
            T::MarketRegistry::note_funded_order_closed(
                &order.order_commitment.market_id,
                order.order_commitment.price,
            )
            .map_err(Self::map_market_access_error)?;
            T::MarketRegistry::note_active_order_closed(&order.order_commitment.market_id)
                .map_err(Self::map_market_access_error)?;
            Self::release_order_object(&order)?;

            escrow.state = protocol::PaymentState::Refunded;
            RuntimeEscrows::<T>::insert(escrow_id, escrow);
            order.state = protocol::OrderState::Refunded;
            RuntimeOrders::<T>::insert(order_id, order);
            Self::deposit_event(Event::OrderRefunded {
                order_id,
                buyer: buyer_commitment.buyer,
                amount: buyer_commitment.price,
            });
            Self::deposit_event(Event::PaymentRefunded {
                order_id,
                escrow_id,
                amount: buyer_commitment.price,
            });
            Ok(())
        }

        fn validate_transfer_meta_settlement(
            order: &protocol::RuntimeOrder,
            escrow: &protocol::RuntimeEscrow,
            buyer: &T::AccountId,
        ) -> Result<TransferMetaAuthorization<T::AccountId, BlockNumberFor<T>>, DispatchError>
        {
            let commitment = &order.order_commitment;
            let authorization = TransferMetaAuthorizations::<T>::get(order.authorization_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                authorization.consumed_order_id.is_none(),
                Error::<T>::AuthorizationConsumed
            );
            match authorization.status {
                AuthorizationStatus::Active
                | AuthorizationStatus::Revoked
                | AuthorizationStatus::Expired => {}
                AuthorizationStatus::Consumed => {
                    return Err(Error::<T>::AuthorizationConsumed.into())
                }
            }
            ensure!(
                authorization.asset_id == commitment.object_id
                    && authorization.market_id == commitment.market_id
                    && authorization.operation == MarketOperation::TransferMeta,
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::ensure_current_authorization_owner(&commitment.object_id, &authorization.owner)?;
            ensure!(
                commitment.policy_hash
                    == Self::price_policy_commitment_hash(&authorization.price_policy)
                    && commitment.allowed_buyer
                        == Self::protocol_allowed_buyer(authorization.allowed_buyer.as_ref())?
                    && commitment.expiry == authorization.expiry.saturated_into::<u32>(),
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::validate_price_policy(&authorization.price_policy, commitment.price, 0)?;
            ensure!(
                authorization.price_policy.safety_fee_bps == 0,
                Error::<T>::FeeOutOfBounds
            );
            ensure!(
                authorization
                    .allowed_buyer
                    .as_ref()
                    .map_or(true, |allowed| allowed == buyer),
                Error::<T>::AuthorizationPolicyMismatch
            );

            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let protocol_buyer = T::ProtocolAccountIdConverter::to_protocol_account_id(buyer)
                .ok_or(Error::<T>::PayerBuyerMismatch)?;
            ensure!(
                buyer_commitment.order_id == order.order_id
                    && buyer_commitment.buyer == protocol_buyer
                    && buyer_commitment.price == commitment.price
                    && buyer_commitment.currency == commitment.currency
                    && buyer_commitment.expires_at == commitment.expiry
                    && escrow.order_id == order.order_id
                    && escrow.escrow_id == order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?
                    && escrow.currency == order.payment_asset
                    && escrow.payer == protocol_buyer
                    && escrow.buyer == protocol_buyer,
                Error::<T>::EscrowMismatch
            );
            let distributed = escrow
                .amounts
                .seller_net
                .checked_add(escrow.amounts.market_fee)
                .ok_or(Error::<T>::PaymentAmountInvalid)?;
            ensure!(
                escrow.amounts.gross_price == commitment.price
                    && escrow.amounts.royalty == 0
                    && escrow.amounts.safety_fee == 0
                    && escrow.royalty_recipient.is_none()
                    && distributed == escrow.amounts.gross_price,
                Error::<T>::EscrowMismatch
            );
            Ok(authorization)
        }

        fn validate_issue_right_settlement(
            order: &protocol::RuntimeOrder,
            escrow: &protocol::RuntimeEscrow,
            buyer: &T::AccountId,
        ) -> Result<IssueRightAuthorization<T::AccountId, BlockNumberFor<T>>, DispatchError>
        {
            let commitment = &order.order_commitment;
            let authorization = IssueRightAuthorizations::<T>::get(order.authorization_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            match authorization.status {
                AuthorizationStatus::Active
                | AuthorizationStatus::Revoked
                | AuthorizationStatus::Expired => {}
                AuthorizationStatus::Consumed => {
                    return Err(Error::<T>::AuthorizationConsumed.into())
                }
            }
            ensure!(
                authorization.asset_id == commitment.object_id
                    && authorization.market_id == commitment.market_id
                    && authorization.operation == MarketOperation::IssueRight,
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::ensure_current_authorization_owner(&commitment.object_id, &authorization.owner)?;
            ensure!(
                commitment.policy_hash
                    == Self::price_policy_commitment_hash(&authorization.price_policy)
                    && commitment.right_parameters_hash
                        == Some(Self::issue_right_parameters_commitment_hash(&authorization))
                    && commitment.expiry == authorization.expiry.saturated_into::<u32>(),
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::validate_price_policy(&authorization.price_policy, commitment.price, 0)?;
            ensure!(
                authorization.price_policy.safety_fee_bps == 0,
                Error::<T>::FeeOutOfBounds
            );

            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let protocol_buyer = T::ProtocolAccountIdConverter::to_protocol_account_id(buyer)
                .ok_or(Error::<T>::PayerBuyerMismatch)?;
            ensure!(
                commitment
                    .allowed_buyer
                    .map_or(true, |allowed| allowed == protocol_buyer),
                Error::<T>::AuthorizationPolicyMismatch
            );
            ensure!(
                buyer_commitment.order_id == order.order_id
                    && buyer_commitment.buyer == protocol_buyer
                    && buyer_commitment.price == commitment.price
                    && buyer_commitment.currency == commitment.currency
                    && buyer_commitment.expires_at == commitment.expiry
                    && escrow.order_id == order.order_id
                    && escrow.escrow_id == order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?
                    && escrow.currency == order.payment_asset
                    && escrow.payer == protocol_buyer
                    && escrow.buyer == protocol_buyer,
                Error::<T>::EscrowMismatch
            );
            let distributed = escrow
                .amounts
                .seller_net
                .checked_add(escrow.amounts.market_fee)
                .ok_or(Error::<T>::PaymentAmountInvalid)?;
            ensure!(
                escrow.amounts.gross_price == commitment.price
                    && escrow.amounts.royalty == 0
                    && escrow.amounts.safety_fee == 0
                    && escrow.royalty_recipient.is_none()
                    && distributed == escrow.amounts.gross_price,
                Error::<T>::EscrowMismatch
            );

            let reservation = IssueQuotaReservations::<T>::get(order.order_id)
                .ok_or(Error::<T>::QuotaReservationMissing)?;
            let expected_use_units = authorization
                .validity_template
                .max_uses
                .map(u64::from)
                .unwrap_or(0);
            ensure!(
                reservation.auth_id == authorization.auth_id
                    && reservation.status == IssueQuotaReservationStatus::Reserved
                    && reservation.reserved_count == 1
                    && reservation.reserved_use_units == expected_use_units,
                Error::<T>::QuotaReservationStateInvalid
            );
            ensure!(
                authorization.reserved_count >= reservation.reserved_count
                    && authorization.reserved_use_units >= reservation.reserved_use_units
                    && OutstandingIssueQuotaCount::<T>::get(authorization.asset_id)
                        >= reservation.reserved_count,
                Error::<T>::QuotaInvariant
            );
            Ok(authorization)
        }

        fn consume_transfer_meta_authorization(
            authorization: &TransferMetaAuthorization<T::AccountId, BlockNumberFor<T>>,
            order_id: [u8; 32],
        ) -> Result<bool, DispatchError> {
            let mut stored = TransferMetaAuthorizations::<T>::get(authorization.auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                stored.consumed_order_id.is_none(),
                Error::<T>::AuthorizationConsumed
            );
            let was_active = stored.status == AuthorizationStatus::Active;
            if was_active {
                ensure!(
                    ActiveTransferMetaAuthorization::<T>::get(stored.asset_id, stored.market_id,)
                        == Some(stored.auth_id)
                        && ActiveTransferMetaAuthorizationByAsset::<T>::get(stored.asset_id)
                            == Some(stored.auth_id),
                    Error::<T>::AuthorizationPolicyMismatch
                );
                ActiveTransferMetaAuthorization::<T>::remove(stored.asset_id, stored.market_id);
                ActiveTransferMetaAuthorizationByAsset::<T>::remove(stored.asset_id);
            }
            stored.status = AuthorizationStatus::Consumed;
            stored.consumed_order_id = Some(order_id);
            TransferMetaAuthorizations::<T>::insert(stored.auth_id, stored);
            Ok(was_active)
        }

        fn release_primary_sale_payment(
            escrow: &protocol::RuntimeEscrow,
            seller: &T::AccountId,
        ) -> DispatchResult {
            let buyer = Self::protocol_buyer_account(&escrow.buyer)?;
            let seller_amount = Self::protocol_balance(escrow.amounts.seller_net)?;
            let market_amount = Self::protocol_balance(escrow.amounts.market_fee)?;
            let market_recipient = T::ProtocolAccountIdConverter::from_protocol_account_id(
                &escrow.market_fee_recipient,
            )
            .ok_or(Error::<T>::ProtocolAccountIdInvalid)?;
            if !seller_amount.is_zero() {
                let remainder = T::Currency::repatriate_reserved(
                    &buyer,
                    seller,
                    seller_amount,
                    BalanceStatus::Free,
                )
                .map_err(|_| Error::<T>::EscrowMismatch)?;
                ensure!(remainder.is_zero(), Error::<T>::EscrowMismatch);
                #[cfg(test)]
                Self::maybe_abort_after_first_payout()?;
            }
            if !market_amount.is_zero() {
                let remainder = T::Currency::repatriate_reserved(
                    &buyer,
                    &market_recipient,
                    market_amount,
                    BalanceStatus::Free,
                )
                .map_err(|_| Error::<T>::EscrowMismatch)?;
                ensure!(remainder.is_zero(), Error::<T>::EscrowMismatch);
            }
            Ok(())
        }

        fn release_resale_payment(
            escrow: &protocol::RuntimeEscrow,
            seller: &T::AccountId,
        ) -> DispatchResult {
            let buyer = Self::protocol_buyer_account(&escrow.buyer)?;
            let seller_amount = Self::protocol_balance(escrow.amounts.seller_net)?;
            let royalty_amount = Self::protocol_balance(escrow.amounts.royalty)?;
            let market_amount = Self::protocol_balance(escrow.amounts.market_fee)?;
            let market_recipient = T::ProtocolAccountIdConverter::from_protocol_account_id(
                &escrow.market_fee_recipient,
            )
            .ok_or(Error::<T>::ProtocolAccountIdInvalid)?;

            if !seller_amount.is_zero() {
                let remainder = T::Currency::repatriate_reserved(
                    &buyer,
                    seller,
                    seller_amount,
                    BalanceStatus::Free,
                )
                .map_err(|_| Error::<T>::EscrowMismatch)?;
                ensure!(remainder.is_zero(), Error::<T>::EscrowMismatch);
                #[cfg(test)]
                Self::maybe_abort_after_first_payout()?;
            }
            if !royalty_amount.is_zero() {
                let royalty_recipient = escrow
                    .royalty_recipient
                    .as_ref()
                    .ok_or(Error::<T>::EscrowMismatch)?;
                let royalty_recipient =
                    T::ProtocolAccountIdConverter::from_protocol_account_id(royalty_recipient)
                        .ok_or(Error::<T>::ProtocolAccountIdInvalid)?;
                let remainder = T::Currency::repatriate_reserved(
                    &buyer,
                    &royalty_recipient,
                    royalty_amount,
                    BalanceStatus::Free,
                )
                .map_err(|_| Error::<T>::EscrowMismatch)?;
                ensure!(remainder.is_zero(), Error::<T>::EscrowMismatch);
            }
            if !market_amount.is_zero() {
                let remainder = T::Currency::repatriate_reserved(
                    &buyer,
                    &market_recipient,
                    market_amount,
                    BalanceStatus::Free,
                )
                .map_err(|_| Error::<T>::EscrowMismatch)?;
                ensure!(remainder.is_zero(), Error::<T>::EscrowMismatch);
            }
            Ok(())
        }

        #[cfg(test)]
        pub(crate) fn set_test_abort_meta_settlement(abort: bool) {
            TEST_ABORT_META_SETTLEMENT.with(|flag| flag.set(abort));
        }

        #[cfg(test)]
        pub(crate) fn set_test_abort_issue_right_settlement(abort: bool) {
            TEST_ABORT_ISSUE_RIGHT_SETTLEMENT.with(|flag| flag.set(abort));
        }

        #[cfg(test)]
        pub(crate) fn set_test_abort_transfer_right_settlement(abort: bool) {
            TEST_ABORT_TRANSFER_RIGHT_SETTLEMENT.with(|flag| flag.set(abort));
        }

        #[cfg(test)]
        pub(crate) fn set_test_abort_after_first_payout(abort: bool) {
            TEST_ABORT_AFTER_FIRST_PAYOUT.with(|flag| flag.set(abort));
        }

        #[cfg(test)]
        pub(crate) fn set_test_abort_after_settlement_evidence(abort: bool) {
            TEST_ABORT_AFTER_SETTLEMENT_EVIDENCE.with(|flag| flag.set(abort));
        }

        #[cfg(test)]
        fn maybe_abort_after_first_payout() -> DispatchResult {
            if TEST_ABORT_AFTER_FIRST_PAYOUT.with(|flag| flag.replace(false)) {
                return Err(Error::<T>::InvalidInput.into());
            }
            Ok(())
        }

        #[cfg(test)]
        fn maybe_abort_after_settlement_evidence() -> DispatchResult {
            if TEST_ABORT_AFTER_SETTLEMENT_EVIDENCE.with(|flag| flag.replace(false)) {
                return Err(Error::<T>::InvalidInput.into());
            }
            Ok(())
        }

        #[cfg(test)]
        pub(crate) fn test_update_certificate(
            asset_id: &[u8; 32],
            right: &RightToken<T::AccountId>,
        ) -> DispatchResult {
            Self::update_certificate(asset_id, right)
        }

        #[cfg(test)]
        fn maybe_abort_meta_settlement() -> DispatchResult {
            if TEST_ABORT_META_SETTLEMENT.with(|flag| flag.replace(false)) {
                return Err(Error::<T>::InvalidInput.into());
            }
            Ok(())
        }

        #[cfg(test)]
        fn maybe_abort_issue_right_settlement() -> DispatchResult {
            if TEST_ABORT_ISSUE_RIGHT_SETTLEMENT.with(|flag| flag.replace(false)) {
                return Err(Error::<T>::InvalidInput.into());
            }
            Ok(())
        }

        #[cfg(test)]
        fn maybe_abort_transfer_right_settlement() -> DispatchResult {
            if TEST_ABORT_TRANSFER_RIGHT_SETTLEMENT.with(|flag| flag.replace(false)) {
                return Err(Error::<T>::InvalidInput.into());
            }
            Ok(())
        }

        fn finalize_settlement_evidence(
            order_id: [u8; 32],
            pre_state_version: u64,
            pre_state_commitment: [u8; 32],
        ) -> DispatchResult {
            ensure!(
                !SettlementRecords::<T>::contains_key(order_id),
                Error::<T>::AlreadySettled
            );
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(
                order.state == protocol::OrderState::Settled,
                Error::<T>::OrderStatusInvalid
            );
            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let escrow = RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.state == protocol::PaymentState::Released,
                Error::<T>::EscrowMismatch
            );
            let reconciled = escrow
                .amounts
                .seller_net
                .checked_add(escrow.amounts.royalty)
                .and_then(|value| value.checked_add(escrow.amounts.market_fee))
                .and_then(|value| value.checked_add(escrow.amounts.safety_fee))
                .ok_or(Error::<T>::PaymentAmountInvalid)?;
            ensure!(
                reconciled == escrow.amounts.gross_price
                    && escrow.amounts.gross_price == order.order_commitment.price,
                Error::<T>::EscrowMismatch
            );

            let object = Self::normalized_object_snapshot(&order)?;
            let (asset_id, right_id, post_state_version) = match order.order_commitment.operation {
                MarketOperation::TransferMeta => {
                    (object.asset_id, None, object.asset_state_version)
                }
                MarketOperation::IssueRight | MarketOperation::TransferRight => (
                    object.asset_id,
                    object.right_id,
                    object.right_state_version.ok_or(Error::<T>::RightInvalid)?,
                ),
            };
            let genesis_hash = Self::protocol_genesis_hash()?;
            let settlement_id =
                protocol::settlement_id(genesis_hash, order.order_commitment.operation, order_id);
            let record = protocol::SettlementRecord {
                settlement_id,
                order_id,
                operation: order.order_commitment.operation,
                asset_id,
                right_id,
                authorization_id: order.authorization_id,
                market_id: order.order_commitment.market_id,
                policy_hash: order.order_commitment.policy_hash,
                order_commitment_hash: protocol::order_commitment_hash(&order.order_commitment),
                seller: order.order_commitment.seller,
                buyer: buyer_commitment.buyer,
                currency: order.payment_asset,
                gross_price: escrow.amounts.gross_price,
                seller_amount: escrow.amounts.seller_net,
                royalty_recipient: escrow.royalty_recipient,
                royalty_amount: escrow.amounts.royalty,
                market_fee_recipient: escrow.market_fee_recipient,
                market_fee_amount: escrow.amounts.market_fee,
                safety_fee_amount: escrow.amounts.safety_fee,
                pre_state_version,
                post_state_version,
                pre_state_commitment,
                block_number: Self::protocol_block_number(),
                settlement_protocol_version: protocol::SETTLEMENT_PROTOCOL_VERSION,
            };
            SettlementRecords::<T>::insert(order_id, record);
            let post_state_commitment = Self::normalized_settlement_commitment(&order_id)?;
            Self::deposit_event(Event::SettlementFinalized {
                settlement_id,
                order_id,
                post_state_commitment,
            });
            #[cfg(test)]
            Self::maybe_abort_after_settlement_evidence()?;
            Ok(())
        }

        fn do_settle(order_id: [u8; 32]) -> DispatchResult {
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            match order.state {
                protocol::OrderState::Funded => {}
                protocol::OrderState::Open => return Err(Error::<T>::OrderNotFunded.into()),
                protocol::OrderState::Settled => return Err(Error::<T>::AlreadySettled.into()),
                protocol::OrderState::Cancelled
                | protocol::OrderState::Expired
                | protocol::OrderState::Refunded => return Err(Error::<T>::TerminalOrder.into()),
            }
            ensure!(
                !SettlementRecords::<T>::contains_key(order_id),
                Error::<T>::AlreadySettled
            );
            let grace_deadline = order.grace_deadline.ok_or(Error::<T>::EscrowMismatch)?;
            let settle_deadline = order.expiry.min(grace_deadline);
            ensure!(
                Self::protocol_block_number() <= settle_deadline,
                Error::<T>::OrderExpired
            );
            let price_policy = OrderPricePolicySnapshots::<T>::get(order_id)
                .ok_or(Error::<T>::AuthorizationPolicyMismatch)?;
            ensure!(
                order.order_commitment.policy_hash
                    == Self::price_policy_commitment_hash(&price_policy),
                Error::<T>::AuthorizationPolicyMismatch
            );

            match order.order_commitment.operation {
                MarketOperation::TransferMeta => Self::do_settle_transfer_meta(order),
                MarketOperation::IssueRight => Self::do_settle_issue_right(order),
                MarketOperation::TransferRight => Self::do_settle_transfer_right(order),
            }
        }

        fn validate_transfer_right_settlement(
            order: &protocol::RuntimeOrder,
            escrow: &protocol::RuntimeEscrow,
            buyer: &T::AccountId,
        ) -> Result<(RightToken<T::AccountId>, T::AccountId), DispatchError> {
            let commitment = &order.order_commitment;
            ensure!(
                commitment.operation == MarketOperation::TransferRight
                    && commitment.object_type == protocol::ObjectType::RightToken,
                Error::<T>::InvalidInput
            );
            let parent_asset_id = commitment
                .parent_asset_id
                .ok_or(Error::<T>::ParentMismatch)?;
            let indexed_parent =
                Self::right_parent_asset(&commitment.object_id).ok_or(Error::<T>::RightInvalid)?;
            ensure!(
                indexed_parent == parent_asset_id,
                Error::<T>::ParentMismatch
            );
            let right = Self::get_certificate(&parent_asset_id, &commitment.object_id)
                .ok_or(Error::<T>::RightInvalid)?;
            ensure!(
                right.asset_id == parent_asset_id
                    && Self::is_right_indexed_for_asset(&parent_asset_id, &commitment.object_id)
                    && Self::is_right_indexed_for_holder(&right.holder, &commitment.object_id),
                Error::<T>::RightIndexInconsistent
            );
            ensure!(
                right.status == CertificateStatus::Active,
                Error::<T>::CertificateNotActive
            );
            ensure!(
                right.is_valid(Self::current_validity_block()),
                Error::<T>::CertificateNotActive
            );
            ensure!(right.transferable, Error::<T>::AuthorizationMissing);
            ensure!(
                right
                    .allowed_transfer_markets
                    .contains(&commitment.market_id),
                Error::<T>::AuthorizationMissing
            );
            ensure!(
                commitment.right_parameters_hash
                    == Some(Self::right_parameters_commitment_hash(&right)),
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::ensure_right_order_available(&commitment.object_id, Some(&order.order_id))?;

            let (seller, state_version) = Self::current_order_seller_and_version(commitment)?;
            Self::ensure_protocol_seller(&seller, &commitment.seller)?;
            ensure!(
                state_version == commitment.seller_state_version
                    && right.state_version == commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );

            let parent_asset =
                Self::get_asset(&parent_asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let royalty_recipient = if right.royalty_bps == 0 {
                None
            } else {
                Some(
                    T::ProtocolAccountIdConverter::to_protocol_account_id(&parent_asset.core.owner)
                        .ok_or(Error::<T>::ProtocolAccountIdInvalid)?,
                )
            };
            let expected_royalty = commitment
                .price
                .checked_mul(u128::from(right.royalty_bps))
                .ok_or(Error::<T>::PaymentAmountInvalid)?
                / 10_000;

            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let protocol_buyer = T::ProtocolAccountIdConverter::to_protocol_account_id(buyer)
                .ok_or(Error::<T>::PayerBuyerMismatch)?;
            ensure!(
                commitment
                    .allowed_buyer
                    .map_or(true, |allowed| allowed == protocol_buyer),
                Error::<T>::AuthorizationPolicyMismatch
            );
            ensure!(
                buyer_commitment.order_id == order.order_id
                    && buyer_commitment.buyer == protocol_buyer
                    && buyer_commitment.price == commitment.price
                    && buyer_commitment.currency == commitment.currency
                    && buyer_commitment.expires_at == commitment.expiry
                    && escrow.order_id == order.order_id
                    && escrow.escrow_id == order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?
                    && escrow.currency == order.payment_asset
                    && escrow.payer == protocol_buyer
                    && escrow.buyer == protocol_buyer,
                Error::<T>::EscrowMismatch
            );
            let distributed = escrow
                .amounts
                .seller_net
                .checked_add(escrow.amounts.royalty)
                .and_then(|value| value.checked_add(escrow.amounts.market_fee))
                .ok_or(Error::<T>::PaymentAmountInvalid)?;
            ensure!(
                escrow.amounts.gross_price == commitment.price
                    && escrow.amounts.royalty == expected_royalty
                    && escrow.amounts.safety_fee == 0
                    && escrow.royalty_recipient == royalty_recipient
                    && distributed == escrow.amounts.gross_price,
                Error::<T>::EscrowMismatch
            );
            Ok((right, seller))
        }

        fn do_settle_transfer_meta(mut order: protocol::RuntimeOrder) -> DispatchResult {
            let order_id = order.order_id;
            let buyer_commitment = order
                .buyer_commitment
                .clone()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let buyer = Self::protocol_buyer_account(&buyer_commitment.buyer)?;
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let mut escrow =
                RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.state == protocol::PaymentState::Escrowed,
                Error::<T>::EscrowMismatch
            );
            let (seller, state_version) =
                Self::current_order_seller_and_version(&order.order_commitment)?;
            Self::ensure_protocol_seller(&seller, &order.order_commitment.seller)?;
            ensure!(
                state_version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            let authorization = Self::validate_transfer_meta_settlement(&order, &escrow, &buyer)?;
            Self::ensure_meta_transfer_available(
                &order.order_commitment.object_id,
                Some(&order_id),
            )?;
            let pre_state_commitment = Self::normalized_settlement_commitment(&order_id)?;
            let pre_state_version = state_version;

            Self::transfer_asset_owner_state(
                &order.order_commitment.object_id,
                &seller,
                &buyer,
                Some(&order_id),
            )?;
            let authorization_was_active =
                Self::consume_transfer_meta_authorization(&authorization, order_id)?;
            Self::release_primary_sale_payment(&escrow, &seller)?;
            escrow.state = protocol::PaymentState::Released;
            RuntimeEscrows::<T>::insert(escrow_id, escrow);
            order.state = protocol::OrderState::Settled;
            RuntimeOrders::<T>::insert(order_id, order.clone());
            #[cfg(test)]
            Self::maybe_abort_meta_settlement()?;
            if authorization_was_active {
                T::MarketRegistry::note_authorization_closed(&order.order_commitment.market_id)
                    .map_err(Self::map_market_access_error)?;
            }
            T::MarketRegistry::note_funded_order_closed(
                &order.order_commitment.market_id,
                order.order_commitment.price,
            )
            .map_err(Self::map_market_access_error)?;
            T::MarketRegistry::note_active_order_closed(&order.order_commitment.market_id)
                .map_err(Self::map_market_access_error)?;
            Self::release_order_object(&order)?;
            Self::finalize_settlement_evidence(order_id, pre_state_version, pre_state_commitment)?;
            Ok(())
        }

        fn do_settle_issue_right(mut order: protocol::RuntimeOrder) -> DispatchResult {
            let order_id = order.order_id;
            let buyer_commitment = order
                .buyer_commitment
                .clone()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let buyer = Self::protocol_buyer_account(&buyer_commitment.buyer)?;
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let mut escrow =
                RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.state == protocol::PaymentState::Escrowed,
                Error::<T>::EscrowMismatch
            );
            let (seller, state_version) =
                Self::current_order_seller_and_version(&order.order_commitment)?;
            Self::ensure_protocol_seller(&seller, &order.order_commitment.seller)?;
            ensure!(
                state_version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            let authorization = Self::validate_issue_right_settlement(&order, &escrow, &buyer)?;

            let asset_id = order.order_commitment.object_id;
            let genesis_hash = Self::protocol_genesis_hash()?;
            let right_id =
                protocol::right_id(genesis_hash, MarketOperation::IssueRight, order.order_id);
            ensure!(
                Self::get_certificate(&asset_id, &right_id).is_none(),
                Error::<T>::RightAlreadyExists
            );
            let current_block = Self::current_validity_block();
            ensure!(
                authorization
                    .validity_template
                    .valid_until
                    .map_or(true, |until| current_block <= until),
                Error::<T>::RightInvalid
            );
            let pre_state_commitment = Self::normalized_settlement_commitment(&order_id)?;
            let token_id = Self::get_next_certificate_id(&asset_id);
            let mut right = RightToken::minimal(
                token_id,
                authorization.right_type,
                buyer.clone(),
                seller.clone(),
                asset_id,
                current_block,
                authorization.validity_template.valid_until,
            );
            right.right_id = right_id;
            right.data_scope = authorization.data_scope;
            right.validity = authorization.validity_template.clone();
            right.remaining_uses = authorization.validity_template.max_uses;
            right.transferable = authorization.transferable;
            right.allowed_transfer_markets = authorization.allowed_transfer_markets.clone();
            right.royalty_bps = authorization.royalty_bps;

            Self::create_right_object(right)?;
            Self::convert_issue_quota_to_issued(&authorization.auth_id, &order_id)?;
            Self::release_primary_sale_payment(&escrow, &seller)?;
            escrow.state = protocol::PaymentState::Released;
            RuntimeEscrows::<T>::insert(escrow_id, escrow);
            order.state = protocol::OrderState::Settled;
            RuntimeOrders::<T>::insert(order_id, order.clone());
            #[cfg(test)]
            Self::maybe_abort_issue_right_settlement()?;
            T::MarketRegistry::note_funded_order_closed(
                &order.order_commitment.market_id,
                order.order_commitment.price,
            )
            .map_err(Self::map_market_access_error)?;
            T::MarketRegistry::note_active_order_closed(&order.order_commitment.market_id)
                .map_err(Self::map_market_access_error)?;
            Self::release_order_object(&order)?;
            Self::finalize_settlement_evidence(order_id, 0, pre_state_commitment)?;
            Ok(())
        }

        fn do_settle_transfer_right(mut order: protocol::RuntimeOrder) -> DispatchResult {
            let order_id = order.order_id;
            let buyer_commitment = order
                .buyer_commitment
                .clone()
                .ok_or(Error::<T>::EscrowMismatch)?;
            let buyer = Self::protocol_buyer_account(&buyer_commitment.buyer)?;
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let mut escrow =
                RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.state == protocol::PaymentState::Escrowed,
                Error::<T>::EscrowMismatch
            );
            let (right, seller) =
                Self::validate_transfer_right_settlement(&order, &escrow, &buyer)?;
            let parent_asset_id = order
                .order_commitment
                .parent_asset_id
                .ok_or(Error::<T>::ParentMismatch)?;
            ensure!(right.holder == seller, Error::<T>::NotOwner);
            let pre_state_version = right.state_version;
            let pre_state_commitment = Self::normalized_settlement_commitment(&order_id)?;

            Self::transfer_right_holder_state(
                &parent_asset_id,
                &order.order_commitment.object_id,
                &seller,
                &buyer,
                Some(&order_id),
            )?;
            Self::release_resale_payment(&escrow, &seller)?;
            escrow.state = protocol::PaymentState::Released;
            RuntimeEscrows::<T>::insert(escrow_id, escrow);
            order.state = protocol::OrderState::Settled;
            RuntimeOrders::<T>::insert(order_id, order.clone());
            #[cfg(test)]
            Self::maybe_abort_transfer_right_settlement()?;
            T::MarketRegistry::note_funded_order_closed(
                &order.order_commitment.market_id,
                order.order_commitment.price,
            )
            .map_err(Self::map_market_access_error)?;
            T::MarketRegistry::note_active_order_closed(&order.order_commitment.market_id)
                .map_err(Self::map_market_access_error)?;
            Self::release_order_object(&order)?;
            Self::finalize_settlement_evidence(order_id, pre_state_version, pre_state_commitment)?;
            Ok(())
        }

        fn ensure_authorization_owner(owner: &T::AccountId, asset_id: &[u8; 32]) -> DispatchResult {
            let asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == *owner, Error::<T>::NotOwner);
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked);
            Ok(())
        }

        fn authorization_id(
            owner: &T::AccountId,
            asset_id: &[u8; 32],
            market_id: &[u8; 32],
            operation: MarketOperation,
            nonce: u64,
        ) -> [u8; 32] {
            blake2_256(
                &(
                    b"axc/p2/authorization",
                    operation,
                    asset_id,
                    owner,
                    market_id,
                    nonce,
                )
                    .encode(),
            )
        }

        fn ensure_authorization_nonce(
            owner: &T::AccountId,
            asset_id: &[u8; 32],
            operation: MarketOperation,
            nonce: u64,
        ) -> Result<u64, DispatchError> {
            let current = AuthorizationNonce::<T>::get(owner, (*asset_id, operation));
            ensure!(nonce == current, Error::<T>::AuthorizationNonceMismatch);
            current
                .checked_add(1)
                .ok_or(Error::<T>::AuthorizationNonceOverflow.into())
        }

        fn validate_price_policy_terms(policy: &PricePolicy) -> DispatchResult {
            ensure!(
                (1..=10_000).contains(&policy.floor_ratio_bps),
                Error::<T>::InvalidPricePolicy
            );
            ensure!(
                policy.signed_market_fee_cap_bps <= 10_000 && policy.safety_fee_bps <= 10_000,
                Error::<T>::InvalidPricePolicy
            );
            ensure!(
                u32::from(policy.signed_market_fee_cap_bps) + u32::from(policy.safety_fee_bps)
                    <= 10_000,
                Error::<T>::InvalidPricePolicy
            );
            Ok(())
        }

        fn validate_issue_authorization_terms(
            terms: &IssueRightAuthorizationTerms<BlockNumberFor<T>>,
        ) -> DispatchResult {
            Self::validate_price_policy_terms(&terms.price_policy)?;
            ensure!(terms.max_issue_count > 0, Error::<T>::InvalidIssueQuota);
            ensure!(
                terms.royalty_bps <= 10_000,
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                u32::from(terms.royalty_bps)
                    + u32::from(terms.price_policy.signed_market_fee_cap_bps)
                    + u32::from(terms.price_policy.safety_fee_bps)
                    <= 10_000,
                Error::<T>::InvalidPricePolicy
            );
            if let Some(valid_until) = terms.validity_template.valid_until {
                ensure!(
                    valid_until > terms.validity_template.valid_from,
                    Error::<T>::InvalidRightParameters
                );
            }
            match terms.validity_template.max_uses {
                Some(per_right_use_units) => {
                    ensure!(per_right_use_units > 0, Error::<T>::InvalidIssueQuota);
                    ensure!(
                        terms.max_total_use_units >= u64::from(per_right_use_units),
                        Error::<T>::InvalidIssueQuota
                    );
                }
                None => ensure!(
                    terms.max_total_use_units == 0,
                    Error::<T>::InvalidIssueQuota
                ),
            }
            for (index, market_id) in terms.allowed_transfer_markets.iter().enumerate() {
                ensure!(
                    !terms.allowed_transfer_markets[..index].contains(market_id),
                    Error::<T>::InvalidRightParameters
                );
            }
            Ok(())
        }

        fn do_create_issue_right_authorization(
            owner: &T::AccountId,
            asset_id: [u8; 32],
            market_id: [u8; 32],
            terms: IssueRightAuthorizationTerms<BlockNumberFor<T>>,
        ) -> DispatchResult {
            Self::ensure_market_operation(&market_id, MarketOperation::IssueRight)?;
            Self::ensure_authorization_owner(owner, &asset_id)?;
            Self::validate_issue_authorization_terms(&terms)?;
            ensure!(
                terms.expiry > frame_system::Pallet::<T>::block_number(),
                Error::<T>::InvalidAuthorizationExpiry
            );
            ensure!(
                !ActiveIssueRightAuthorization::<T>::contains_key(asset_id, market_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            ensure!(
                !ActiveTransferMetaAuthorization::<T>::iter_prefix(asset_id).any(|_| true),
                Error::<T>::MutualExclusionConflict
            );
            ensure!(
                !ActiveMetaOrderByAsset::<T>::contains_key(asset_id),
                Error::<T>::MutualExclusionConflict
            );

            let next_nonce = Self::ensure_authorization_nonce(
                owner,
                &asset_id,
                MarketOperation::IssueRight,
                terms.nonce,
            )?;
            let auth_id = Self::authorization_id(
                owner,
                &asset_id,
                &market_id,
                MarketOperation::IssueRight,
                terms.nonce,
            );
            ensure!(
                !IssueRightAuthorizations::<T>::contains_key(auth_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            let next_active_count = ActiveIssueRightAuthorizationCount::<T>::get(asset_id)
                .checked_add(1)
                .ok_or(Error::<T>::AuthorizationCounterInvariant)?;
            T::MarketRegistry::note_authorization_created(&market_id)
                .map_err(Self::map_market_access_error)?;

            IssueRightAuthorizations::<T>::insert(
                auth_id,
                IssueRightAuthorization {
                    auth_id,
                    asset_id,
                    owner: owner.clone(),
                    market_id,
                    operation: MarketOperation::IssueRight,
                    right_type: terms.right_type,
                    data_scope: terms.data_scope,
                    validity_template: terms.validity_template,
                    max_issue_count: terms.max_issue_count,
                    max_total_use_units: terms.max_total_use_units,
                    price_policy: terms.price_policy,
                    royalty_bps: terms.royalty_bps,
                    transferable: terms.transferable,
                    allowed_transfer_markets: terms.allowed_transfer_markets,
                    reserved_count: 0,
                    reserved_use_units: 0,
                    issued_count: 0,
                    issued_use_units: 0,
                    expiry: terms.expiry,
                    nonce: terms.nonce,
                    status: AuthorizationStatus::Active,
                },
            );
            ActiveIssueRightAuthorization::<T>::insert(asset_id, market_id, auth_id);
            ActiveIssueRightAuthorizationCount::<T>::insert(asset_id, next_active_count);
            AuthorizationNonce::<T>::insert(
                owner,
                (asset_id, MarketOperation::IssueRight),
                next_nonce,
            );
            Self::deposit_event(Event::IssueRightAuthorizationCreated {
                auth_id,
                asset_id,
                market_id,
            });
            Ok(())
        }

        fn do_create_transfer_meta_authorization(
            owner: &T::AccountId,
            asset_id: [u8; 32],
            market_id: [u8; 32],
            terms: TransferMetaAuthorizationTerms<T::AccountId, BlockNumberFor<T>>,
        ) -> DispatchResult {
            Self::ensure_market_operation(&market_id, MarketOperation::TransferMeta)?;
            Self::ensure_authorization_owner(owner, &asset_id)?;
            Self::validate_price_policy_terms(&terms.price_policy)?;
            ensure!(
                terms.expiry > frame_system::Pallet::<T>::block_number(),
                Error::<T>::InvalidAuthorizationExpiry
            );
            Self::ensure_meta_transfer_available(&asset_id, None)?;
            ensure!(
                !ActiveTransferMetaAuthorization::<T>::iter_prefix(asset_id).any(|_| true),
                Error::<T>::AuthorizationAlreadyActive
            );
            ensure!(
                ActiveIssueRightAuthorizationCount::<T>::get(asset_id) == 0
                    && !ActiveIssueRightAuthorization::<T>::iter_prefix(asset_id).any(|_| true),
                Error::<T>::MutualExclusionConflict
            );

            let next_nonce = Self::ensure_authorization_nonce(
                owner,
                &asset_id,
                MarketOperation::TransferMeta,
                terms.nonce,
            )?;
            let auth_id = Self::authorization_id(
                owner,
                &asset_id,
                &market_id,
                MarketOperation::TransferMeta,
                terms.nonce,
            );
            ensure!(
                !TransferMetaAuthorizations::<T>::contains_key(auth_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            T::MarketRegistry::note_authorization_created(&market_id)
                .map_err(Self::map_market_access_error)?;

            TransferMetaAuthorizations::<T>::insert(
                auth_id,
                TransferMetaAuthorization {
                    auth_id,
                    asset_id,
                    owner: owner.clone(),
                    market_id,
                    operation: MarketOperation::TransferMeta,
                    price_policy: terms.price_policy,
                    allowed_buyer: terms.allowed_buyer,
                    expiry: terms.expiry,
                    nonce: terms.nonce,
                    status: AuthorizationStatus::Active,
                    consumed_order_id: None,
                },
            );
            ActiveTransferMetaAuthorization::<T>::insert(asset_id, market_id, auth_id);
            ActiveTransferMetaAuthorizationByAsset::<T>::insert(asset_id, auth_id);
            AuthorizationNonce::<T>::insert(
                owner,
                (asset_id, MarketOperation::TransferMeta),
                next_nonce,
            );
            Self::deposit_event(Event::TransferMetaAuthorizationCreated {
                auth_id,
                asset_id,
                market_id,
            });
            Ok(())
        }

        fn do_revoke_issue_right_authorization(
            owner: &T::AccountId,
            auth_id: [u8; 32],
        ) -> DispatchResult {
            let authorization = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(authorization.owner == *owner, Error::<T>::NotOwner);
            ensure!(
                authorization.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationNotActive
            );
            let next_active_count =
                ActiveIssueRightAuthorizationCount::<T>::get(authorization.asset_id)
                    .checked_sub(1)
                    .ok_or(Error::<T>::AuthorizationCounterInvariant)?;
            T::MarketRegistry::note_authorization_closed(&authorization.market_id)
                .map_err(Self::map_market_access_error)?;

            IssueRightAuthorizations::<T>::mutate(auth_id, |entry| {
                if let Some(stored) = entry {
                    stored.status = AuthorizationStatus::Revoked;
                }
            });
            ActiveIssueRightAuthorization::<T>::remove(
                authorization.asset_id,
                authorization.market_id,
            );
            ActiveIssueRightAuthorizationCount::<T>::insert(
                authorization.asset_id,
                next_active_count,
            );
            Self::deposit_event(Event::IssueRightAuthorizationRevoked { auth_id });
            Ok(())
        }

        fn do_revoke_transfer_meta_authorization(
            owner: &T::AccountId,
            auth_id: [u8; 32],
        ) -> DispatchResult {
            let authorization = TransferMetaAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(authorization.owner == *owner, Error::<T>::NotOwner);
            ensure!(
                authorization.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationNotActive
            );
            T::MarketRegistry::note_authorization_closed(&authorization.market_id)
                .map_err(Self::map_market_access_error)?;

            TransferMetaAuthorizations::<T>::mutate(auth_id, |entry| {
                if let Some(stored) = entry {
                    stored.status = AuthorizationStatus::Revoked;
                }
            });
            ActiveTransferMetaAuthorization::<T>::remove(
                authorization.asset_id,
                authorization.market_id,
            );
            ActiveTransferMetaAuthorizationByAsset::<T>::remove(authorization.asset_id);
            Self::deposit_event(Event::TransferMetaAuthorizationRevoked { auth_id });
            Ok(())
        }

        fn do_expire_issue_right_authorization(auth_id: [u8; 32]) -> DispatchResult {
            let authorization = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                authorization.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationNotActive
            );
            ensure!(
                frame_system::Pallet::<T>::block_number() > authorization.expiry,
                Error::<T>::AuthorizationNotActive
            );
            let next_active_count =
                ActiveIssueRightAuthorizationCount::<T>::get(authorization.asset_id)
                    .checked_sub(1)
                    .ok_or(Error::<T>::AuthorizationCounterInvariant)?;
            T::MarketRegistry::note_authorization_closed(&authorization.market_id)
                .map_err(Self::map_market_access_error)?;

            IssueRightAuthorizations::<T>::mutate(auth_id, |entry| {
                if let Some(stored) = entry {
                    stored.status = AuthorizationStatus::Expired;
                }
            });
            ActiveIssueRightAuthorization::<T>::remove(
                authorization.asset_id,
                authorization.market_id,
            );
            ActiveIssueRightAuthorizationCount::<T>::insert(
                authorization.asset_id,
                next_active_count,
            );
            Self::deposit_event(Event::IssueRightAuthorizationExpired { auth_id });
            Ok(())
        }

        fn do_expire_transfer_meta_authorization(auth_id: [u8; 32]) -> DispatchResult {
            let authorization = TransferMetaAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                authorization.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationNotActive
            );
            ensure!(
                frame_system::Pallet::<T>::block_number() > authorization.expiry,
                Error::<T>::AuthorizationNotActive
            );
            T::MarketRegistry::note_authorization_closed(&authorization.market_id)
                .map_err(Self::map_market_access_error)?;

            TransferMetaAuthorizations::<T>::mutate(auth_id, |entry| {
                if let Some(stored) = entry {
                    stored.status = AuthorizationStatus::Expired;
                }
            });
            ActiveTransferMetaAuthorization::<T>::remove(
                authorization.asset_id,
                authorization.market_id,
            );
            ActiveTransferMetaAuthorizationByAsset::<T>::remove(authorization.asset_id);
            Self::deposit_event(Event::TransferMetaAuthorizationExpired { auth_id });
            Ok(())
        }

        pub fn validate_issue_right_authorization(
            auth_id: &[u8; 32],
            asset_id: &[u8; 32],
            market_id: &[u8; 32],
        ) -> Result<IssueRightAuthorization<T::AccountId, BlockNumberFor<T>>, DispatchError>
        {
            Self::ensure_market_operation(market_id, MarketOperation::IssueRight)?;
            let authorization = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            Self::validate_authorization_status(authorization.status, &authorization.expiry)?;
            ensure!(
                authorization.asset_id == *asset_id
                    && authorization.market_id == *market_id
                    && authorization.operation == MarketOperation::IssueRight
                    && ActiveIssueRightAuthorization::<T>::get(asset_id, market_id)
                        == Some(*auth_id),
                Error::<T>::AuthorizationMissing
            );
            Self::ensure_current_authorization_owner(asset_id, &authorization.owner)?;
            Ok(authorization)
        }

        pub fn validate_transfer_meta_authorization(
            auth_id: &[u8; 32],
            asset_id: &[u8; 32],
            market_id: &[u8; 32],
        ) -> Result<TransferMetaAuthorization<T::AccountId, BlockNumberFor<T>>, DispatchError>
        {
            Self::ensure_market_operation(market_id, MarketOperation::TransferMeta)?;
            let authorization = TransferMetaAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            Self::validate_authorization_status(authorization.status, &authorization.expiry)?;
            ensure!(
                authorization.asset_id == *asset_id
                    && authorization.market_id == *market_id
                    && authorization.operation == MarketOperation::TransferMeta
                    && ActiveTransferMetaAuthorization::<T>::get(asset_id, market_id)
                        == Some(*auth_id),
                Error::<T>::AuthorizationMissing
            );
            Self::ensure_current_authorization_owner(asset_id, &authorization.owner)?;
            Ok(authorization)
        }

        pub fn validate_price_policy(
            policy: &PricePolicy,
            price: u128,
            market_fee_bps: u16,
        ) -> DispatchResult {
            Self::validate_price_policy_terms(policy)?;
            ensure!(
                market_fee_bps <= policy.signed_market_fee_cap_bps,
                Error::<T>::FeeOutOfBounds
            );

            let ratio = u128::from(policy.floor_ratio_bps);
            let quotient = policy.reference_price / 10_000;
            let remainder = policy.reference_price % 10_000;
            let base = quotient
                .checked_mul(ratio)
                .ok_or(Error::<T>::InvalidPricePolicy)?;
            let rounded_remainder = remainder
                .checked_mul(ratio)
                .and_then(|value| value.checked_add(9_999))
                .ok_or(Error::<T>::InvalidPricePolicy)?
                / 10_000;
            let effective_floor = base
                .checked_add(rounded_remainder)
                .ok_or(Error::<T>::InvalidPricePolicy)?;
            ensure!(price >= effective_floor, Error::<T>::BelowPriceFloor);
            Ok(())
        }

        pub fn validate_issue_right_open(
            auth_id: &[u8; 32],
            asset_id: &[u8; 32],
            market_id: &[u8; 32],
            price: u128,
            market_fee_bps: u16,
        ) -> Result<IssueRightAuthorization<T::AccountId, BlockNumberFor<T>>, DispatchError>
        {
            let authorization =
                Self::validate_issue_right_authorization(auth_id, asset_id, market_id)?;
            Self::validate_price_policy(&authorization.price_policy, price, market_fee_bps)?;
            Ok(authorization)
        }

        pub fn validate_transfer_meta_open(
            auth_id: &[u8; 32],
            asset_id: &[u8; 32],
            market_id: &[u8; 32],
            allowed_buyer: Option<&T::AccountId>,
            price: u128,
            market_fee_bps: u16,
        ) -> Result<TransferMetaAuthorization<T::AccountId, BlockNumberFor<T>>, DispatchError>
        {
            let authorization =
                Self::validate_transfer_meta_authorization(auth_id, asset_id, market_id)?;
            ensure!(
                authorization.allowed_buyer.as_ref() == allowed_buyer,
                Error::<T>::AuthorizationPolicyMismatch
            );
            Self::validate_price_policy(&authorization.price_policy, price, market_fee_bps)?;
            Ok(authorization)
        }

        pub(crate) fn reserve_issue_quota(
            auth_id: &[u8; 32],
            asset_id: &[u8; 32],
            market_id: &[u8; 32],
            order_id: [u8; 32],
        ) -> DispatchResult {
            let mut authorization =
                Self::validate_issue_right_authorization(auth_id, asset_id, market_id)?;
            ensure!(
                !IssueQuotaReservations::<T>::contains_key(order_id),
                Error::<T>::QuotaReservationAlreadyExists
            );

            let reserved_count = 1u32;
            let reserved_use_units = authorization
                .validity_template
                .max_uses
                .map(u64::from)
                .unwrap_or(0);
            let next_reserved_count = authorization
                .reserved_count
                .checked_add(reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_total_count = authorization
                .issued_count
                .checked_add(next_reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            ensure!(
                next_total_count <= authorization.max_issue_count,
                Error::<T>::QuotaExceeded
            );
            let next_reserved_use_units = authorization
                .reserved_use_units
                .checked_add(reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_total_use_units = authorization
                .issued_use_units
                .checked_add(next_reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            ensure!(
                next_total_use_units <= authorization.max_total_use_units,
                Error::<T>::QuotaExceeded
            );
            let next_outstanding_count = OutstandingIssueQuotaCount::<T>::get(asset_id)
                .checked_add(reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;

            authorization.reserved_count = next_reserved_count;
            authorization.reserved_use_units = next_reserved_use_units;
            IssueRightAuthorizations::<T>::insert(auth_id, authorization);
            OutstandingIssueQuotaCount::<T>::insert(asset_id, next_outstanding_count);
            IssueQuotaReservations::<T>::insert(
                order_id,
                IssueQuotaReservation {
                    order_id,
                    auth_id: *auth_id,
                    reserved_count,
                    reserved_use_units,
                    status: IssueQuotaReservationStatus::Reserved,
                },
            );
            Ok(())
        }

        pub(crate) fn convert_issue_quota_to_issued(
            auth_id: &[u8; 32],
            order_id: &[u8; 32],
        ) -> DispatchResult {
            let mut reservation = IssueQuotaReservations::<T>::get(order_id)
                .ok_or(Error::<T>::QuotaReservationMissing)?;
            ensure!(
                reservation.auth_id == *auth_id,
                Error::<T>::QuotaReservationAuthorizationMismatch
            );
            ensure!(
                reservation.status == IssueQuotaReservationStatus::Reserved,
                Error::<T>::QuotaReservationStateInvalid
            );
            let mut authorization = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;

            let next_reserved_count = authorization
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_reserved_use_units = authorization
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_issued_count = authorization
                .issued_count
                .checked_add(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_issued_use_units = authorization
                .issued_use_units
                .checked_add(reservation.reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_outstanding_count =
                OutstandingIssueQuotaCount::<T>::get(authorization.asset_id)
                    .checked_sub(reservation.reserved_count)
                    .ok_or(Error::<T>::QuotaInvariant)?;
            ensure!(
                next_issued_count
                    .checked_add(next_reserved_count)
                    .ok_or(Error::<T>::QuotaInvariant)?
                    <= authorization.max_issue_count
                    && next_issued_use_units
                        .checked_add(next_reserved_use_units)
                        .ok_or(Error::<T>::QuotaInvariant)?
                        <= authorization.max_total_use_units,
                Error::<T>::QuotaInvariant
            );

            authorization.reserved_count = next_reserved_count;
            authorization.reserved_use_units = next_reserved_use_units;
            authorization.issued_count = next_issued_count;
            authorization.issued_use_units = next_issued_use_units;
            reservation.status = IssueQuotaReservationStatus::Issued;
            OutstandingIssueQuotaCount::<T>::insert(authorization.asset_id, next_outstanding_count);
            IssueRightAuthorizations::<T>::insert(auth_id, authorization);
            IssueQuotaReservations::<T>::insert(order_id, reservation);
            Ok(())
        }

        pub(crate) fn release_issue_quota(
            auth_id: &[u8; 32],
            order_id: &[u8; 32],
        ) -> DispatchResult {
            let mut reservation = IssueQuotaReservations::<T>::get(order_id)
                .ok_or(Error::<T>::QuotaReservationMissing)?;
            ensure!(
                reservation.auth_id == *auth_id,
                Error::<T>::QuotaReservationAuthorizationMismatch
            );
            ensure!(
                reservation.status == IssueQuotaReservationStatus::Reserved,
                Error::<T>::QuotaReservationStateInvalid
            );
            let mut authorization = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            let next_reserved_count = authorization
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_reserved_use_units = authorization
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let next_outstanding_count =
                OutstandingIssueQuotaCount::<T>::get(authorization.asset_id)
                    .checked_sub(reservation.reserved_count)
                    .ok_or(Error::<T>::QuotaInvariant)?;

            authorization.reserved_count = next_reserved_count;
            authorization.reserved_use_units = next_reserved_use_units;
            reservation.status = IssueQuotaReservationStatus::Released;
            OutstandingIssueQuotaCount::<T>::insert(authorization.asset_id, next_outstanding_count);
            IssueRightAuthorizations::<T>::insert(auth_id, authorization);
            IssueQuotaReservations::<T>::insert(order_id, reservation);
            Ok(())
        }

        fn validate_authorization_status(
            status: AuthorizationStatus,
            expiry: &BlockNumberFor<T>,
        ) -> DispatchResult {
            match status {
                AuthorizationStatus::Active => {}
                AuthorizationStatus::Revoked => return Err(Error::<T>::AuthorizationRevoked.into()),
                AuthorizationStatus::Expired => return Err(Error::<T>::AuthorizationExpired.into()),
                AuthorizationStatus::Consumed => {
                    return Err(Error::<T>::AuthorizationConsumed.into())
                }
            }
            ensure!(
                frame_system::Pallet::<T>::block_number() <= *expiry,
                Error::<T>::AuthorizationExpired
            );
            Ok(())
        }

        fn ensure_current_authorization_owner(
            asset_id: &[u8; 32],
            owner: &T::AccountId,
        ) -> DispatchResult {
            let asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == *owner, Error::<T>::NotOwner);
            Ok(())
        }

        fn asset_trie_info() -> sp_core::storage::ChildInfo {
            sp_core::storage::ChildInfo::new_default(ASSET_TRIE_ID)
        }

        fn certificate_trie_info() -> sp_core::storage::ChildInfo {
            sp_core::storage::ChildInfo::new_default(CERTIFICATE_TRIE_ID)
        }

        fn make_asset_key(asset_id: &[u8; 32]) -> Vec<u8> {
            let mut key = b"assets/".to_vec();
            key.extend_from_slice(asset_id);
            key
        }

        fn make_owner_asset_index_key(owner: &T::AccountId, asset_id: &[u8; 32]) -> Vec<u8> {
            let mut key = OWNER_ASSET_INDEX_PREFIX.to_vec();
            key.extend_from_slice(&owner.encode());
            key.extend_from_slice(asset_id);
            key
        }

        fn mark_trie_modified() {
            TrieModified::<T>::set(true);
        }

        pub(crate) fn insert_asset(
            asset_id: &[u8; 32],
            asset: &DataAsset<T::AccountId>,
        ) -> DispatchResult {
            let child_info = Self::asset_trie_info();
            let key = Self::make_asset_key(asset_id);
            ensure!(asset.core.asset_id == *asset_id, Error::<T>::InvalidInput);
            ensure!(asset.core.state_version > 0, Error::<T>::InvalidInput);

            let previous = match child::get_raw(&child_info, &key) {
                Some(raw) => Some(
                    decode_data_asset_compat::<T::AccountId>(&raw)
                        .ok_or(Error::<T>::InvalidInput)?,
                ),
                None => None,
            };
            if let Some(previous) = previous {
                if previous.core.owner != asset.core.owner {
                    let old_index =
                        Self::make_owner_asset_index_key(&previous.core.owner, asset_id);
                    child::kill(&child_info, &old_index);
                }
            }

            child::put(&child_info, &key, asset);
            let owner_index = Self::make_owner_asset_index_key(&asset.core.owner, asset_id);
            child::put(&child_info, &owner_index, &());
            Self::mark_trie_modified();
            Ok(())
        }

        fn put_asset_status_fixture(
            asset_id: &[u8; 32],
            asset: &DataAsset<T::AccountId>,
        ) -> DispatchResult {
            ensure!(asset.core.asset_id == *asset_id, Error::<T>::InvalidInput);
            let child_info = Self::asset_trie_info();
            let key = Self::make_asset_key(asset_id);
            child::put(&child_info, &key, asset);
            Self::mark_trie_modified();
            Ok(())
        }

        pub fn get_asset(asset_id: &[u8; 32]) -> Option<DataAsset<T::AccountId>> {
            let child_info = Self::asset_trie_info();
            let key = Self::make_asset_key(asset_id);
            child::get_raw(&child_info, &key)
                .and_then(|raw| decode_data_asset_compat::<T::AccountId>(&raw))
        }

        pub fn is_asset_indexed_for_owner(owner: &T::AccountId, asset_id: &[u8; 32]) -> bool {
            let child_info = Self::asset_trie_info();
            let key = Self::make_owner_asset_index_key(owner, asset_id);
            child::exists(&child_info, &key)
        }

        pub fn account_exists(account: &T::AccountId) -> bool {
            // 方法1：检查是否有余额
            T::Currency::free_balance(account) > BalanceOf::<T>::zero() ||
            // 方法2：检查系统账户存储
            frame_system::Pallet::<T>::account_exists(account)
        }

        pub fn is_zero_account(account: &T::AccountId) -> bool {
            // 方法1：检查编码后是否全为零
            let encoded = account.encode();
            encoded.iter().all(|&b| b == 0)
        }

        pub fn get_asset_by_token_id(token_id: u32) -> Option<DataAsset<T::AccountId>> {
            let asset_id = Self::get_token_mapping(token_id)?;
            Self::get_asset(&asset_id)
        }

        fn validate_asset_registration_fields(name: &[u8], description: &[u8]) -> DispatchResult {
            ensure!(
                name.len() <= T::MaxNameLength::get() as usize,
                Error::<T>::NameTooLong
            );
            ensure!(
                description.len() <= T::MaxDescriptionLength::get() as usize,
                Error::<T>::DescriptionTooLong
            );
            Ok(())
        }

        fn prepare_asset_registration(
            who: &T::AccountId,
            name: &[u8],
            description: &[u8],
            raw_data_hash: &H256,
        ) -> Result<([u8; 32], u64), DispatchError> {
            Self::validate_asset_registration_fields(name, description)?;

            let timestamp = Self::current_timestamp();
            let asset_id = DataAsset::generate_asset_id(who, timestamp, raw_data_hash);
            ensure!(
                Self::get_asset(&asset_id).is_none(),
                Error::<T>::InvalidInput
            );

            Ok((asset_id, timestamp))
        }

        fn do_register_asset(
            who: T::AccountId,
            name: Vec<u8>,
            description: Vec<u8>,
            raw_data_hash: H256,
            data_size_bytes: u64,
            metadata_cid: Vec<u8>,
            data_cid: Vec<u8>,
            encryption_info: EncryptionInfo,
        ) -> DispatchResult {
            let (asset_id, timestamp) =
                Self::prepare_asset_registration(&who, &name, &description, &raw_data_hash)?;
            let (collateral_amount, is_over_capped) = Self::calculate_collateral(data_size_bytes);

            if is_over_capped {
                let max_collateral = T::MaxCollateral::get();
                let data_size_mb = ((data_size_bytes as u128) / (1024 * 1024)).max(1);
                let variable_collateral =
                    T::CollateralPerMB::get().saturating_mul(data_size_mb.saturated_into());
                let total_uncapped = T::BaseCollateral::get().saturating_add(variable_collateral);

                Self::deposit_event(Event::CollateralOverCappedHint {
                    asset_id,
                    depositor: who.clone(),
                    total_uncapped,
                    capped_amount: collateral_amount,
                    max_collateral,
                });
            }

            Self::lock_collateral(&asset_id, &who, collateral_amount)?;
            let token_id = Self::store_registered_asset(
                &who,
                name,
                description,
                raw_data_hash,
                timestamp,
                asset_id,
                data_size_bytes,
                metadata_cid,
                data_cid,
                encryption_info,
            )?;

            if let Err(_) = T::IncentiveHandler::distribute_first_create_reward(&who, &asset_id) {
                log::error!("首次创建奖励发放失败：asset_id={:?}", asset_id);
            }

            Self::deposit_event(Event::AssetRegistered {
                asset_id,
                token_id,
                owner: who,
                collateral: collateral_amount,
            });
            Ok(())
        }

        fn store_registered_asset(
            who: &T::AccountId,
            name: Vec<u8>,
            description: Vec<u8>,
            raw_data_hash: H256,
            timestamp: u64,
            asset_id: [u8; 32],
            data_size_bytes: u64,
            metadata_cid: Vec<u8>,
            data_cid: Vec<u8>,
            encryption_info: EncryptionInfo,
        ) -> Result<u32, DispatchError> {
            let token_id = Self::get_and_increment_token_id();

            let mut asset =
                DataAsset::minimal(who.clone(), name, description, raw_data_hash, timestamp);
            asset.core.asset_id = asset_id;
            asset.core.token_id = token_id;
            asset.metadata.metadata_cid =
                BoundedVec::try_from(metadata_cid).map_err(|_| Error::<T>::InvalidInput)?;
            asset.metadata.data_cid =
                BoundedVec::try_from(data_cid).map_err(|_| Error::<T>::InvalidInput)?;
            asset.metadata.data_size_bytes = data_size_bytes;
            asset.encryption_info = encryption_info;

            Self::insert_asset(&asset_id, &asset)?;
            Self::set_token_mapping(token_id, asset_id);
            Ok(token_id)
        }

        // 获取自增的 token_id，最大2^32-1(42亿)
        fn get_and_increment_token_id() -> u32 {
            let child_info = Self::asset_trie_info();
            let key = [METADATA_PREFIX, b"next_token_id"].concat();

            let current = child::get::<u32>(&child_info, &key).unwrap_or(0);
            let next = current.saturating_add(1);
            child::put(&child_info, &key, &next);
            Self::mark_trie_modified();
            current
        }

        fn set_token_mapping(token_id: u32, asset_id: [u8; 32]) {
            let child_info = Self::asset_trie_info();
            let mut key = METADATA_PREFIX.to_vec();
            key.extend_from_slice(b"token_mappings/");
            key.extend_from_slice(&token_id.to_le_bytes());
            child::put(&child_info, &key, &asset_id);
            Self::mark_trie_modified();
        }

        fn get_token_mapping(token_id: u32) -> Option<[u8; 32]> {
            let child_info = Self::asset_trie_info();
            let mut key = METADATA_PREFIX.to_vec();
            key.extend_from_slice(b"token_mappings/");
            key.extend_from_slice(&token_id.to_le_bytes());

            child::get::<[u8; 32]>(&child_info, &key)
        }

        fn build_certificate_key(asset_id: &[u8; 32], cert_id: &[u8; 32]) -> Vec<u8> {
            if UseHashScatteredKeys::<T>::get() {
                let mut input = asset_id.to_vec();
                input.extend_from_slice(cert_id);
                keccak_256(&input).to_vec()
            } else {
                let mut key = asset_id.to_vec();
                key.extend_from_slice(cert_id);
                key
            }
        }

        fn make_holder_right_index_key(holder: &T::AccountId, right_id: &[u8; 32]) -> Vec<u8> {
            let mut key = HOLDER_RIGHT_INDEX_PREFIX.to_vec();
            key.extend_from_slice(&holder.encode());
            key.extend_from_slice(right_id);
            key
        }

        fn make_asset_right_index_key(asset_id: &[u8; 32], right_id: &[u8; 32]) -> Vec<u8> {
            let mut key = ASSET_RIGHT_INDEX_PREFIX.to_vec();
            key.extend_from_slice(asset_id);
            key.extend_from_slice(right_id);
            key
        }

        fn make_asset_right_ids_key(asset_id: &[u8; 32]) -> Vec<u8> {
            let mut key = METADATA_PREFIX.to_vec();
            key.extend_from_slice(b"asset_right_ids/");
            key.extend_from_slice(asset_id);
            key
        }

        fn make_right_parent_key(right_id: &[u8; 32]) -> Vec<u8> {
            let mut key = METADATA_PREFIX.to_vec();
            key.extend_from_slice(b"right_parent/");
            key.extend_from_slice(right_id);
            key
        }

        fn right_parent_asset(right_id: &[u8; 32]) -> Option<[u8; 32]> {
            let child_info = Self::certificate_trie_info();
            child::get(&child_info, &Self::make_right_parent_key(right_id))
        }

        fn get_asset_right_ids(
            asset_id: &[u8; 32],
        ) -> BoundedVec<[u8; 32], ConstU32<MAX_RIGHTS_PER_ASSET>> {
            let child_info = Self::certificate_trie_info();
            let key = Self::make_asset_right_ids_key(asset_id);
            child::get(&child_info, &key).unwrap_or_default()
        }

        fn put_certificate(
            asset_id: &[u8; 32],
            cert: &RightToken<T::AccountId>,
            count_as_issue: bool,
        ) -> DispatchResult {
            let child_info = Self::certificate_trie_info();
            ensure!(
                cert.asset_id == *asset_id,
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                cert.has_valid_parameters(),
                Error::<T>::InvalidRightParameters
            );
            let storage_key = Self::build_certificate_key(asset_id, &cert.right_id);
            let previous = match child::get_raw(&child_info, &storage_key) {
                Some(raw) => Some(
                    decode_right_token_compat::<T::AccountId>(&raw)
                        .ok_or(Error::<T>::RightIndexInconsistent)?,
                ),
                None => None,
            };

            let parent_key = Self::make_right_parent_key(&cert.right_id);
            if let Some(existing_parent) = child::get::<[u8; 32]>(&child_info, &parent_key) {
                ensure!(existing_parent == *asset_id, Error::<T>::RightAlreadyExists);
            }

            let mut asset_right_ids = Self::get_asset_right_ids(asset_id);
            if !asset_right_ids.contains(&cert.right_id) {
                asset_right_ids
                    .try_push(cert.right_id)
                    .map_err(|_| Error::<T>::TooManyRightsForAsset)?;
            }

            if let Some(previous) = previous {
                ensure!(
                    previous.asset_id == *asset_id,
                    Error::<T>::RightIndexInconsistent
                );
                if previous.holder != cert.holder {
                    let old_holder_index =
                        Self::make_holder_right_index_key(&previous.holder, &cert.right_id);
                    child::kill(&child_info, &old_holder_index);
                }
            }

            child::put(&child_info, &storage_key, cert);
            child::put(&child_info, &parent_key, asset_id);
            let parent_index = Self::make_asset_right_index_key(asset_id, &cert.right_id);
            child::put(&child_info, &parent_index, &());
            let holder_index = Self::make_holder_right_index_key(&cert.holder, &cert.right_id);
            child::put(&child_info, &holder_index, &());
            let asset_right_ids_key = Self::make_asset_right_ids_key(asset_id);
            child::put(&child_info, &asset_right_ids_key, &asset_right_ids);

            if count_as_issue {
                let count = BlockCertCount::<T>::get().saturating_add(1);
                BlockCertCount::<T>::put(count);
            }

            Self::mark_trie_modified();
            Ok(())
        }

        fn insert_certificate(
            asset_id: &[u8; 32],
            cert: &RightToken<T::AccountId>,
        ) -> DispatchResult {
            Self::put_certificate(asset_id, cert, true)
        }

        fn update_certificate(
            asset_id: &[u8; 32],
            cert: &RightToken<T::AccountId>,
        ) -> DispatchResult {
            Self::put_certificate(asset_id, cert, false)
        }

        fn put_right_status_fixture(
            asset_id: &[u8; 32],
            right: &RightToken<T::AccountId>,
        ) -> DispatchResult {
            ensure!(right.asset_id == *asset_id, Error::<T>::InvalidInput);
            let child_info = Self::certificate_trie_info();
            let key = Self::build_certificate_key(asset_id, &right.right_id);
            child::put(&child_info, &key, right);
            Self::mark_trie_modified();
            Ok(())
        }

        pub fn get_certificate(
            asset_id: &[u8; 32],
            cert_id: &[u8; 32],
        ) -> Option<RightToken<T::AccountId>> {
            let child_info = Self::certificate_trie_info();
            let storage_key = Self::build_certificate_key(asset_id, cert_id);

            child::get_raw(&child_info, &storage_key)
                .and_then(|raw| decode_right_token_compat::<T::AccountId>(&raw))
        }

        pub fn is_right_indexed_for_holder(holder: &T::AccountId, right_id: &[u8; 32]) -> bool {
            let child_info = Self::certificate_trie_info();
            let key = Self::make_holder_right_index_key(holder, right_id);
            child::exists(&child_info, &key)
        }

        pub fn is_right_indexed_for_asset(asset_id: &[u8; 32], right_id: &[u8; 32]) -> bool {
            let child_info = Self::certificate_trie_info();
            let key = Self::make_asset_right_index_key(asset_id, right_id);
            child::exists(&child_info, &key)
        }

        fn remove_certificate(asset_id: &[u8; 32], cert_id: &[u8; 32]) -> DispatchResult {
            let child_info = Self::certificate_trie_info();
            let storage_key = Self::build_certificate_key(asset_id, cert_id);
            let certificate =
                Self::get_certificate(asset_id, cert_id).ok_or(Error::<T>::CertificateNotFound)?;
            ensure!(
                certificate.asset_id == *asset_id,
                Error::<T>::RightIndexInconsistent
            );

            let holder_index = Self::make_holder_right_index_key(&certificate.holder, cert_id);
            let parent_index = Self::make_asset_right_index_key(asset_id, cert_id);
            let parent_key = Self::make_right_parent_key(cert_id);
            let mut asset_right_ids = Self::get_asset_right_ids(asset_id);
            asset_right_ids.retain(|right_id| right_id != cert_id);
            let asset_right_ids_key = Self::make_asset_right_ids_key(asset_id);

            child::kill(&child_info, &storage_key);
            child::kill(&child_info, &holder_index);
            child::kill(&child_info, &parent_index);
            child::kill(&child_info, &parent_key);
            child::put(&child_info, &asset_right_ids_key, &asset_right_ids);
            Self::mark_trie_modified();
            Ok(())
        }

        // 机制导致不能通过遍历child trie获取某资产下的所有证书
        // pub fn get_asset_certificates(asset_id: &[u8; 32]) -> Vec<RightToken<T::AccountId>> {

        fn get_next_certificate_id(asset_id: &[u8; 32]) -> u32 {
            let child_info = Self::certificate_trie_info();

            // Key = _metadata/next_token_id/ + asset_id
            let mut key = METADATA_PREFIX.to_vec();
            key.extend_from_slice(b"next_token_id/");
            key.extend_from_slice(asset_id);

            let current = child::get::<u32>(&child_info, &key).unwrap_or(0);
            let next = current.saturating_add(1);
            child::put(&child_info, &key, &next);
            Self::mark_trie_modified();
            current
        }

        pub fn ensure_meta_transfer_available(
            asset_id: &[u8; 32],
            allowed_order_id: Option<&[u8; 32]>,
        ) -> DispatchResult {
            let asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.is_tradable(), Error::<T>::AssetNotActive);

            if let Some(active_order_id) = ActiveMetaOrderByAsset::<T>::get(asset_id) {
                ensure!(
                    allowed_order_id == Some(&active_order_id),
                    Error::<T>::ActiveObjectOrderExists
                );
            }
            ensure!(
                ActiveIssueRightAuthorizationCount::<T>::get(asset_id) == 0
                    && !ActiveIssueRightAuthorization::<T>::iter_prefix(asset_id).any(|_| true),
                Error::<T>::MutualExclusionConflict
            );
            ensure!(
                OutstandingIssueQuotaCount::<T>::get(asset_id) == 0,
                Error::<T>::MutualExclusionConflict
            );

            let right_ids = Self::get_asset_right_ids(asset_id);
            if right_ids.is_empty() && ActiveCertificateCount::<T>::get(asset_id) > 0 {
                // Legacy v1 state has no enumerable parent index. Fail closed
                // until its certificates are revoked or explicitly migrated.
                return Err(Error::<T>::MutualExclusionConflict.into());
            }

            let now = Self::current_validity_block();
            for right_id in right_ids {
                let right = Self::get_certificate(asset_id, &right_id)
                    .ok_or(Error::<T>::RightIndexInconsistent)?;
                ensure!(
                    right.asset_id == *asset_id
                        && Self::is_right_indexed_for_asset(asset_id, &right_id)
                        && Self::is_right_indexed_for_holder(&right.holder, &right_id),
                    Error::<T>::RightIndexInconsistent
                );
                ensure!(!right.is_valid(now), Error::<T>::MutualExclusionConflict);
            }
            Ok(())
        }

        pub(crate) fn reserve_meta_order(
            asset_id: &[u8; 32],
            order_id: [u8; 32],
        ) -> DispatchResult {
            Self::ensure_meta_transfer_available(asset_id, None)?;
            ensure!(
                !ActiveMetaOrderByAsset::<T>::contains_key(asset_id),
                Error::<T>::ActiveObjectOrderExists
            );
            ActiveMetaOrderByAsset::<T>::insert(asset_id, order_id);
            Ok(())
        }

        pub(crate) fn release_meta_order(
            asset_id: &[u8; 32],
            order_id: &[u8; 32],
        ) -> DispatchResult {
            ensure!(
                ActiveMetaOrderByAsset::<T>::get(asset_id) == Some(*order_id),
                Error::<T>::ActiveObjectOrderMismatch
            );
            ActiveMetaOrderByAsset::<T>::remove(asset_id);
            Ok(())
        }

        pub(crate) fn reserve_right_order(
            asset_id: &[u8; 32],
            right_id: &[u8; 32],
            order_id: [u8; 32],
        ) -> DispatchResult {
            let right =
                Self::get_certificate(asset_id, right_id).ok_or(Error::<T>::CertificateNotFound)?;
            ensure!(
                right.asset_id == *asset_id
                    && Self::is_right_indexed_for_asset(asset_id, right_id)
                    && Self::is_right_indexed_for_holder(&right.holder, right_id),
                Error::<T>::RightIndexInconsistent
            );
            ensure!(
                right.is_valid(Self::current_validity_block()),
                Error::<T>::CertificateNotActive
            );
            ensure!(
                !ActiveRightOrderByRight::<T>::contains_key(right_id),
                Error::<T>::ActiveObjectOrderExists
            );
            ActiveRightOrderByRight::<T>::insert(right_id, order_id);
            Ok(())
        }

        pub(crate) fn release_right_order(
            right_id: &[u8; 32],
            order_id: &[u8; 32],
        ) -> DispatchResult {
            ensure!(
                ActiveRightOrderByRight::<T>::get(right_id) == Some(*order_id),
                Error::<T>::ActiveObjectOrderMismatch
            );
            ActiveRightOrderByRight::<T>::remove(right_id);
            Ok(())
        }

        fn ensure_right_order_available(
            right_id: &[u8; 32],
            allowed_order_id: Option<&[u8; 32]>,
        ) -> DispatchResult {
            if let Some(active_order_id) = ActiveRightOrderByRight::<T>::get(right_id) {
                ensure!(
                    allowed_order_id == Some(&active_order_id),
                    Error::<T>::ActiveObjectOrderExists
                );
            }
            Ok(())
        }

        pub fn compute_asset_root() -> H256 {
            let child_info = Self::asset_trie_info();
            let root_bytes = child::root(&child_info, sp_core::storage::StateVersion::V1);
            H256::from_slice(&root_bytes)
        }

        pub fn compute_certificate_root() -> H256 {
            let child_info = Self::certificate_trie_info();
            let root_bytes = child::root(&child_info, sp_core::storage::StateVersion::V1);
            H256::from_slice(&root_bytes)
        }

        fn current_timestamp() -> u64 {
            <pallet_timestamp::Pallet<T>>::get().saturated_into::<u64>()
        }

        fn current_validity_block() -> u64 {
            frame_system::Pallet::<T>::block_number().saturated_into::<u64>()
        }

        fn next_trade_settlement_nonce() -> u64 {
            let current = TradeSettlementNonce::<T>::get();
            TradeSettlementNonce::<T>::put(current.saturating_add(1));
            current
        }

        fn generate_trade_id(
            market: &T::AccountId,
            buyer: &T::AccountId,
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            settled_at: BlockNumberFor<T>,
            nonce: u64,
        ) -> [u8; 32] {
            let input = (market, buyer, asset_id, certificate_id, settled_at, nonce).encode();
            keccak_256(&input)
        }

        fn record_trade_settlement(
            market: &T::AccountId,
            seller: &T::AccountId,
            buyer: &T::AccountId,
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            asset_type: TradeAssetType,
            price: BalanceOf<T>,
            order_id: &[u8; 32],
            order_digest: &[u8; 32],
            right_action: crate::types::RightAction,
        ) -> Result<[u8; 32], DispatchError> {
            let settled_at = frame_system::Pallet::<T>::block_number();
            let nonce = Self::next_trade_settlement_nonce();
            let trade_id =
                Self::generate_trade_id(market, buyer, asset_id, certificate_id, settled_at, nonce);
            let state_root = Self::compute_asset_root();

            let ts = TradeSettlement {
                trade_id,
                market: market.clone(),
                seller: seller.clone(),
                buyer: buyer.clone(),
                asset_id: *asset_id,
                certificate_id: *certificate_id,
                asset_type,
                price,
                settled_at,
            };
            TradeSettlements::<T>::insert(trade_id, ts);

            // 写入 OrderSettlements（论文要求的主索引，以 order_id 为键）
            // Double-check: 使用 try_mutate 原子地检查并写入
            let record = crate::types::SettlementRecord {
                trade_id,
                order_id: *order_id,
                market: market.clone(),
                seller: seller.clone(),
                buyer: buyer.clone(),
                asset_id: *asset_id,
                certificate_id: *certificate_id,
                object_type: asset_type,
                right_action,
                price,
                order_digest: *order_digest,
                settled_at,
                state_root_at_settlement: state_root,
            };
            OrderSettlements::<T>::try_mutate(order_id, |existing| -> Result<(), DispatchError> {
                ensure!(existing.is_none(), Error::<T>::AlreadySettled);
                *existing = Some(record.clone());
                Ok(())
            })?;
            // try_mutate 闭包返回 Err 则存储不变 — 这是 double-check 的原子性保证

            Self::deposit_event(Event::TradeSettled {
                trade_id,
                market: market.clone(),
                seller: seller.clone(),
                buyer: buyer.clone(),
                asset_id: *asset_id,
                certificate_id: *certificate_id,
                asset_type,
                price,
                settled_at,
            });

            Ok(trade_id)
        }

        /// 统一的结算前置验证入口。
        /// 按七条件合取范式顺序检查，任一条件不满足即返回 Err 并 emit SettlementRejected 事件。
        /// 此函数是只读的——不修改任何存储。
        fn verify_settle_prerequisites(
            order_id: &[u8; 32],
            order_digest: &[u8; 32],
            market: &T::AccountId,
            object_type: TradeAssetType,
            object_id: &[u8; 32],
        ) -> Result<
            crate::types::MarketOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
            DispatchError,
        > {
            // 条件 1: NoPriorSettlement — 检查 settlement:{order_id} 已存在则拒绝
            if OrderSettlements::<T>::contains_key(order_id) {
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"AlreadySettled".to_vec(),
                });
                return Err(Error::<T>::AlreadySettled.into());
            }

            // 条件 2: OrderExists + OrderOpenOrLocked
            let order = MarketOrders::<T>::get(order_id).ok_or({
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderNotFound".to_vec(),
                });
                Error::<T>::OrderNotFound
            })?;

            if !matches!(
                order.status,
                crate::types::MarketOrderStatus::Open | crate::types::MarketOrderStatus::Locked
            ) {
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderStatusInvalid".to_vec(),
                });
                return Err(Error::<T>::OrderStatusInvalid.into());
            }

            // 条件 3: MatchOrderDigest
            if order.order_digest != *order_digest {
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderDigestMismatch".to_vec(),
                });
                return Err(Error::<T>::OrderDigestMismatch.into());
            }

            // 条件 4: MatchAuthorization — market 持有有效授权
            match object_type {
                TradeAssetType::DataAsset => {
                    let approved = AssetApprovals::<T>::get(object_id).ok_or({
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"UnauthorizedMarket".to_vec(),
                        });
                        Error::<T>::NotAuthorized
                    })?;
                    ensure!(approved == *market, {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"UnauthorizedMarket".to_vec(),
                        });
                        Error::<T>::NotAuthorized
                    });
                }
                TradeAssetType::Certificate => {
                    let parent_asset = order.parent_asset_id.ok_or({
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"AssetNotFound".to_vec(),
                        });
                        Error::<T>::AssetNotFound
                    })?;
                    let approved = AssetApprovals::<T>::get(&parent_asset).ok_or({
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"UnauthorizedMarket".to_vec(),
                        });
                        Error::<T>::NotAuthorized
                    })?;
                    ensure!(approved == *market, {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"UnauthorizedMarket".to_vec(),
                        });
                        Error::<T>::NotAuthorized
                    });
                }
            }

            // 条件 5: ObjectExists + MatchOwnership
            match object_type {
                TradeAssetType::DataAsset => {
                    let asset = Self::get_asset(object_id).ok_or({
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"AssetNotFound".to_vec(),
                        });
                        Error::<T>::AssetNotFound
                    })?;
                    ensure!(asset.core.owner == order.seller, {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"OwnershipMismatch".to_vec(),
                        });
                        Error::<T>::NotOwner
                    });
                    ensure!(!asset.is_locked(), {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"AssetLocked".to_vec(),
                        });
                        Error::<T>::AssetLocked
                    });
                }
                TradeAssetType::Certificate => {
                    let parent_asset = order.parent_asset_id.ok_or({
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"AssetNotFound".to_vec(),
                        });
                        Error::<T>::AssetNotFound
                    })?;
                    let cert = Self::get_certificate(&parent_asset, object_id).ok_or({
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"CertificateNotFound".to_vec(),
                        });
                        Error::<T>::CertificateNotFound
                    })?;
                    ensure!(cert.holder == order.seller, {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"OwnershipMismatch".to_vec(),
                        });
                        Error::<T>::NotOwner
                    });
                    ensure!(cert.is_valid(Self::current_validity_block()), {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"ObjectPermanentlyInvalid".to_vec(),
                        });
                        Error::<T>::CertificateNotActive
                    });
                }
            }

            // 条件 6（签名）由 Substrate 框架保证，无需显式检查

            Ok(order)
        }

        fn transfer_asset_owner_state(
            asset_id: &[u8; 32],
            expected_owner: &T::AccountId,
            new_owner: &T::AccountId,
            allowed_order_id: Option<&[u8; 32]>,
        ) -> Result<T::AccountId, DispatchError> {
            Self::ensure_meta_transfer_available(asset_id, allowed_order_id)?;
            let mut asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == *expected_owner, Error::<T>::NotOwner);

            let old_owner = asset.core.owner.clone();
            asset.core.owner = new_owner.clone();
            asset.core.nonce = asset
                .core
                .nonce
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            asset.core.state_version = asset
                .core
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            asset.core.status = AssetStatus::Tradable;
            asset.core.updated_at = Self::current_timestamp();
            Self::insert_asset(asset_id, &asset)?;
            Ok(old_owner)
        }

        fn transfer_right_holder_state(
            asset_id: &[u8; 32],
            right_id: &[u8; 32],
            expected_holder: &T::AccountId,
            new_holder: &T::AccountId,
            allowed_order_id: Option<&[u8; 32]>,
        ) -> Result<T::AccountId, DispatchError> {
            Self::ensure_right_order_available(right_id, allowed_order_id)?;
            let mut right =
                Self::get_certificate(asset_id, right_id).ok_or(Error::<T>::CertificateNotFound)?;
            ensure!(
                right.asset_id == *asset_id,
                Error::<T>::RightIndexInconsistent
            );
            ensure!(right.holder == *expected_holder, Error::<T>::NotOwner);
            ensure!(
                right.is_valid(Self::current_validity_block()),
                Error::<T>::CertificateNotActive
            );

            let old_holder = right.holder.clone();
            right.holder = new_holder.clone();
            right.state_version = right
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            Self::update_certificate(asset_id, &right)?;
            Ok(old_holder)
        }

        /// 纯写入：转移资产所有权（验证已前置完成）
        fn do_transfer_asset(
            asset_id: &[u8; 32],
            _market_account: &T::AccountId, // 已由 verify_settle_prerequisites 验证授权
            new_owner: &T::AccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let old_owner =
                Self::transfer_asset_owner_state(asset_id, &asset.core.owner, new_owner, None)?;
            AssetApprovals::<T>::remove(asset_id);
            T::IncentiveHandler::register_asset_trade(asset_id);
            Self::deposit_event(Event::AssetTransferred {
                asset_id: *asset_id,
                from: old_owner.clone(),
                to: new_owner.clone(),
            });
            Ok(old_owner)
        }

        /// 纯写入：转移权证（验证已前置完成）
        fn do_transfer_certificate(
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            _current_owner: &T::AccountId, // 已由 verify_settle_prerequisites 验证权属
            new_owner: &T::AccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let certificate = Self::get_certificate(asset_id, certificate_id)
                .ok_or(Error::<T>::CertificateNotFound)?;
            let old_owner = Self::transfer_right_holder_state(
                asset_id,
                certificate_id,
                &certificate.holder,
                new_owner,
                None,
            )?;
            T::IncentiveHandler::register_asset_trade(asset_id);
            Self::deposit_event(Event::CertificateTransferred {
                asset_id: *asset_id,
                certificate_id: *certificate_id,
                from: old_owner.clone(),
                to: new_owner.clone(),
            });
            Ok(old_owner)
        }

        fn transfer_asset_by_market_state(
            asset_id: &[u8; 32],
            market_account: &T::AccountId,
            new_owner: &T::AccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;

            let approved_account =
                Self::asset_approvals(asset_id).ok_or(Error::<T>::NotAuthorized)?;
            ensure!(
                approved_account == *market_account,
                Error::<T>::NotAuthorized
            );
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked);

            let old_owner =
                Self::transfer_asset_owner_state(asset_id, &asset.core.owner, new_owner, None)?;
            AssetApprovals::<T>::remove(asset_id);
            T::IncentiveHandler::register_asset_trade(asset_id);
            Self::deposit_event(Event::AssetTransferred {
                asset_id: *asset_id,
                from: old_owner.clone(),
                to: new_owner.clone(),
            });

            Ok(old_owner)
        }

        fn transfer_certificate_state(
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            current_owner: &T::AccountId,
            new_owner: &T::AccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let _asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;

            let old_owner = Self::transfer_right_holder_state(
                asset_id,
                certificate_id,
                current_owner,
                new_owner,
                None,
            )?;
            T::IncentiveHandler::register_asset_trade(asset_id);

            Self::deposit_event(Event::CertificateTransferred {
                asset_id: *asset_id,
                certificate_id: *certificate_id,
                from: old_owner.clone(),
                to: new_owner.clone(),
            });

            Ok(old_owner)
        }

        /// 供其他Pallet调用的方法，跳过签名检查，但检查授权
        pub(crate) fn transfer_by_market_internal(
            asset_id: &[u8; 32],
            market_account: &T::AccountId,
            new_owner: &T::AccountId,
        ) -> DispatchResult {
            let _seller =
                Self::transfer_asset_by_market_state(asset_id, market_account, new_owner)?;
            Ok(())
        }

        pub(crate) fn settle_asset_trade_by_market_internal(
            asset_id: &[u8; 32],
            market_account: &T::AccountId,
            buyer: &T::AccountId,
            price: BalanceOf<T>,
            order_id: &[u8; 32],
            order_digest: &[u8; 32],
        ) -> Result<[u8; 32], DispatchError> {
            Self::verify_settle_prerequisites(
                order_id,
                order_digest,
                market_account,
                TradeAssetType::DataAsset,
                asset_id,
            )?;
            let seller = Self::do_transfer_asset(asset_id, market_account, buyer)?;
            let trade_id = Self::record_trade_settlement(
                market_account,
                &seller,
                buyer,
                asset_id,
                &[0u8; 32],
                TradeAssetType::DataAsset,
                price,
                order_id,
                order_digest,
                crate::types::RightAction::TransferAsset,
            )?;
            Ok(trade_id)
        }

        pub(crate) fn issue_certificate_by_market_internal(
            asset_id: &[u8; 32],
            market_account: &T::AccountId,
            issuer: &T::AccountId,
            holder: &T::AccountId,
            right_type: u8,
            valid_until: Option<u64>,
        ) -> Result<[u8; 32], DispatchError> {
            let asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;

            let approved_account =
                Self::asset_approvals(asset_id).ok_or(Error::<T>::NotAuthorized)?;
            ensure!(
                approved_account == *market_account,
                Error::<T>::NotAuthorized
            );
            ensure!(asset.core.owner == *issuer, Error::<T>::NotOwner);

            Self::issue_certificate_internal(asset_id, &asset, holder, right_type, valid_until)
        }

        fn issue_certificate_internal(
            asset_id: &[u8; 32],
            asset: &DataAsset<T::AccountId>,
            holder: &T::AccountId,
            right_type: u8,
            valid_until: Option<u64>,
        ) -> Result<[u8; 32], DispatchError> {
            ensure!(asset.is_active(), Error::<T>::AssetNotActive);

            let right_type_enum = match right_type {
                1 => RightType::LegacyUsage,
                2 => RightType::LegacyAccess,
                _ => return Err(Error::<T>::InvalidRightType.into()),
            };

            let token_id = Self::get_next_certificate_id(asset_id);
            let current_block = Self::current_validity_block();
            let certificate = RightToken::minimal(
                token_id,
                right_type_enum,
                holder.clone(),
                asset.core.owner.clone(),
                *asset_id,
                current_block,
                valid_until,
            );
            let certificate_id = certificate.right_id;

            Self::create_right_object(certificate)?;
            T::IncentiveHandler::register_asset_trade(asset_id);

            Self::deposit_event(Event::CertificateIssued {
                asset_id: *asset_id,
                certificate_id,
                issuer: asset.core.owner.clone(),
                holder: holder.clone(),
            });

            Ok(certificate_id)
        }

        /// Narrow object-layer constructor. Deterministic id generation and
        /// authorization/quota conversion are intentionally owned by later
        /// settlement steps.
        pub(crate) fn create_right_object(right: RightToken<T::AccountId>) -> DispatchResult {
            ensure!(
                right.right_id != [0u8; 32],
                Error::<T>::InvalidRightParameters
            );
            ensure!(right.state_version == 1, Error::<T>::InvalidRightParameters);
            ensure!(
                right.status == CertificateStatus::Active,
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                right.has_valid_parameters(),
                Error::<T>::InvalidRightParameters
            );
            for (index, market_id) in right.allowed_transfer_markets.iter().enumerate() {
                ensure!(
                    !right.allowed_transfer_markets[..index].contains(market_id),
                    Error::<T>::InvalidRightParameters
                );
            }

            let asset = Self::get_asset(&right.asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == right.issuer, Error::<T>::NotOwner);
            ensure!(asset.is_tradable(), Error::<T>::AssetNotActive);
            ensure!(
                Self::get_certificate(&right.asset_id, &right.right_id).is_none(),
                Error::<T>::RightAlreadyExists
            );
            let next_count = ActiveCertificateCount::<T>::get(&right.asset_id)
                .checked_add(1)
                .ok_or(Error::<T>::TooManyRightsForAsset)?;

            Self::insert_certificate(&right.asset_id, &right)?;
            ActiveCertificateCount::<T>::insert(right.asset_id, next_count);
            Ok(())
        }

        pub(crate) fn transfer_certificate_internal(
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            current_owner: &T::AccountId,
            new_owner: &T::AccountId,
        ) -> DispatchResult {
            let _seller = Self::transfer_certificate_state(
                asset_id,
                certificate_id,
                current_owner,
                new_owner,
            )?;
            Ok(())
        }

        pub(crate) fn settle_certificate_trade_internal(
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            market_account: &T::AccountId,
            buyer: &T::AccountId,
            price: BalanceOf<T>,
            order_id: &[u8; 32],
            order_digest: &[u8; 32],
        ) -> Result<[u8; 32], DispatchError> {
            Self::verify_settle_prerequisites(
                order_id,
                order_digest,
                market_account,
                TradeAssetType::Certificate,
                certificate_id,
            )?;
            let seller =
                Self::do_transfer_certificate(asset_id, certificate_id, market_account, buyer)?;
            let trade_id = Self::record_trade_settlement(
                market_account,
                &seller,
                buyer,
                asset_id,
                certificate_id,
                TradeAssetType::Certificate,
                price,
                order_id,
                order_digest,
                crate::types::RightAction::TransferRight,
            )?;
            Ok(trade_id)
        }
    }

    impl<T: Config> Pallet<T> {
        /// Get asset root from a block's digest
        pub fn get_asset_root_from_digest(digest: &sp_runtime::Digest) -> Option<H256> {
            crate::digest_item::extract_asset_root(digest)
        }

        /// Get asset root from current block's digest
        pub fn current_block_asset_root() -> Option<H256> {
            let digest = frame_system::Pallet::<T>::digest();
            Self::get_asset_root_from_digest(&digest)
        }
    }
}

impl<T: Config> pallet_shared_traits::DataAssetProvider<T::AccountId, [u8; 32]> for Pallet<T> {
    fn get_asset_owner(
        asset_id: &[u8; 32],
    ) -> Result<T::AccountId, pallet_shared_traits::AssetQueryError> {
        let asset = Self::get_asset(asset_id)
            .ok_or(pallet_shared_traits::AssetQueryError::AssetNotFound)?;

        if Self::is_zero_account(&asset.core.owner) {
            return Err(pallet_shared_traits::AssetQueryError::InvalidOwner);
        }

        // 可选：检查账户存在性
        if !Self::account_exists(&asset.core.owner) {
            return Err(pallet_shared_traits::AssetQueryError::OwnerAccountDoesNotExist);
        }

        Ok(asset.core.owner)
    }
}
