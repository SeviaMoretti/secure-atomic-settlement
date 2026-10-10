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
    use frame_support::traits::{Currency, ReservableCurrency};
    use frame_system::pallet_prelude::*;
    use pallet_shared_traits::IncentiveHandler;
    use sp_core::H256;
    use sp_io::hashing::keccak_256;
    use sp_runtime::traits::{SaturatedConversion, Saturating, Zero};

    use crate::types::*;

    const ASSET_TRIE_ID: &[u8] = b":asset_trie:";
    const CERTIFICATE_TRIE_ID: &[u8] = b":certificate_trie:";
    const METADATA_PREFIX: &[u8] = b"_metadata/";

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
    #[pallet::storage]
    #[pallet::getter(fn market_orders)]
    pub type MarketOrders<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // order_id
        crate::types::MarketOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// 结算记录表——以 order_id 为主键。
    /// 每笔成功结算在此写入一条 SettlementRecord，与 TradeSettlements 双写。
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
        CertificateNotActive,
        OrderNotFound,
        OrderStatusInvalid,
        OrderDigestMismatch,
        AlreadySettled,
        OrderLockFailed,
        OutstandingCertificates,
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
            let (asset_id, timestamp) =
                Self::prepare_asset_registration(&who, &name, &description, &raw_data_hash)?;
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

            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner); // 在未被授权的时候，只有资产所有者才能转移资产
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked); // 锁定的资产不能转移
            ensure!(
                !AssetApprovals::<T>::contains_key(&asset_id),
                Error::<T>::AlreadyAuthorized
            ); // 授权后的资产只有授权的市场可以交易
            ensure!(
                ActiveCertificateCount::<T>::get(&asset_id) == 0,
                Error::<T>::OutstandingCertificates
            ); // 有活跃权证的资产不能直接转移所有权
            let old_owner = asset.core.owner.clone();
            asset.core.owner = new_owner.clone();
            asset.core.nonce += 1;
            asset.core.status = AssetStatus::Private;
            asset.core.updated_at = Self::current_timestamp();
            Self::insert_asset(&asset_id, &asset)?;
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

            let asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let cert = Self::get_certificate(&asset_id, &certificate_id)
                .ok_or(Error::<T>::CertificateNotFound)?;

            ensure!(
                asset.core.owner == who || cert.owner == who,
                Error::<T>::NotOwner
            );

            Self::remove_certificate(&asset_id, &certificate_id)?;
            ActiveCertificateCount::<T>::mutate(&asset_id, |count| *count = count.saturating_sub(1));

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
            // let caller = Self::account_to_h160(&who);

            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner);

            asset.core.status = AssetStatus::Locked;
            asset.core.updated_at = Self::current_timestamp();

            Self::insert_asset(&asset_id, &asset)?;
            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(<T as Config>::WeightInfo::unlock_asset())]
        pub fn unlock_asset(origin: OriginFor<T>, asset_id: [u8; 32]) -> DispatchResult {
            let who = ensure_signed(origin)?;
            // let caller = Self::account_to_h160(&who);

            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == who, Error::<T>::NotOwner);

            asset.core.status = AssetStatus::Private;
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

            // 1. 获取资产
            let mut asset = Self::get_asset(&asset_id).ok_or(Error::<T>::AssetNotFound)?;

            // 2. 验证是否被授权
            let approved_account =
                Self::asset_approvals(&asset_id).ok_or(Error::<T>::NotAuthorized)?;
            ensure!(approved_account == market, Error::<T>::NotAuthorized);

            // 3. 检查资产状态
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked);

            let old_owner = asset.core.owner.clone();

            // 4. 执行转移逻辑
            asset.core.owner = new_owner.clone();
            asset.core.nonce += 1;
            asset.core.updated_at = Self::current_timestamp();
            asset.core.status = AssetStatus::Private;

            // 5. 更新资产树
            Self::insert_asset(&asset_id, &asset)?;

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
            Self::transfer_certificate_internal(
                &asset_id,
                &certificate_id,
                &current_owner,
                &new_owner,
            )
        }
    }

    impl<T: Config> Pallet<T> {
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

        fn mark_trie_modified() {
            TrieModified::<T>::set(true);
        }

        pub fn insert_asset(
            asset_id: &[u8; 32],
            asset: &DataAsset<T::AccountId>,
        ) -> DispatchResult {
            let child_info = Self::asset_trie_info();
            let key = Self::make_asset_key(asset_id);
            child::put(&child_info, &key, asset);
            Self::mark_trie_modified();
            Ok(())
        }

        pub fn get_asset(asset_id: &[u8; 32]) -> Option<DataAsset<T::AccountId>> {
            let child_info = Self::asset_trie_info();
            let key = Self::make_asset_key(asset_id);
            child::get::<DataAsset<T::AccountId>>(&child_info, &key)
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

        fn prepare_asset_registration(
            who: &T::AccountId,
            name: &[u8],
            description: &[u8],
            raw_data_hash: &H256,
        ) -> Result<([u8; 32], u64), DispatchError> {
            ensure!(
                name.len() <= T::MaxNameLength::get() as usize,
                Error::<T>::NameTooLong
            );
            ensure!(
                description.len() <= T::MaxDescriptionLength::get() as usize,
                Error::<T>::DescriptionTooLong
            );

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

        fn put_certificate(
            asset_id: &[u8; 32],
            cert: &RightToken<T::AccountId>,
            count_as_issue: bool,
        ) -> DispatchResult {
            let child_info = Self::certificate_trie_info();
            let storage_key = Self::build_certificate_key(asset_id, &cert.certificate_id);

            child::put(&child_info, &storage_key, cert);

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

        pub fn get_certificate(
            asset_id: &[u8; 32],
            cert_id: &[u8; 32],
        ) -> Option<RightToken<T::AccountId>> {
            let child_info = Self::certificate_trie_info();
            let storage_key = Self::build_certificate_key(asset_id, cert_id);

            child::get::<RightToken<T::AccountId>>(&child_info, &storage_key)
        }

        fn remove_certificate(asset_id: &[u8; 32], cert_id: &[u8; 32]) -> DispatchResult {
            let child_info = Self::certificate_trie_info();
            let storage_key = Self::build_certificate_key(asset_id, cert_id);

            child::kill(&child_info, &storage_key);
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
        ) -> Result<crate::types::MarketOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>, DispatchError> {
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

            if !matches!(order.status, crate::types::MarketOrderStatus::Open | crate::types::MarketOrderStatus::Locked) {
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
                    let approved = AssetApprovals::<T>::get(object_id)
                        .ok_or({
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
                    let approved = AssetApprovals::<T>::get(&parent_asset)
                        .ok_or({
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
                    let cert = Self::get_certificate(&parent_asset, object_id)
                        .ok_or({
                            Self::deposit_event(Event::SettlementRejected {
                                order_id: *order_id,
                                reason: b"CertificateNotFound".to_vec(),
                            });
                            Error::<T>::CertificateNotFound
                        })?;
                    ensure!(cert.owner == order.seller, {
                        Self::deposit_event(Event::SettlementRejected {
                            order_id: *order_id,
                            reason: b"OwnershipMismatch".to_vec(),
                        });
                        Error::<T>::NotOwner
                    });
                    ensure!(
                        cert.is_valid(Self::current_timestamp()),
                        {
                            Self::deposit_event(Event::SettlementRejected {
                                order_id: *order_id,
                                reason: b"ObjectPermanentlyInvalid".to_vec(),
                            });
                            Error::<T>::CertificateNotActive
                        }
                    );
                }
            }

            // 条件 6（签名）由 Substrate 框架保证，无需显式检查

            Ok(order)
        }

        /// 纯写入：转移资产所有权（验证已前置完成）
        pub fn do_transfer_asset(
            asset_id: &[u8; 32],
            _market_account: &T::AccountId,  // 已由 verify_settle_prerequisites 验证授权
            new_owner: &T::AccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let mut asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let old_owner = asset.core.owner.clone();
            asset.core.owner = new_owner.clone();
            asset.core.nonce += 1;
            asset.core.status = AssetStatus::Private;
            asset.core.updated_at = Self::current_timestamp();
            Self::insert_asset(asset_id, &asset)?;
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
        pub fn do_transfer_certificate(
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            _current_owner: &T::AccountId,  // 已由 verify_settle_prerequisites 验证权属
            new_owner: &T::AccountId,
        ) -> Result<T::AccountId, DispatchError> {
            let mut certificate = Self::get_certificate(asset_id, certificate_id)
                .ok_or(Error::<T>::CertificateNotFound)?;
            let old_owner = certificate.owner.clone();
            certificate.owner = new_owner.clone();
            certificate.nonce = certificate.nonce.saturating_add(1);
            Self::update_certificate(asset_id, &certificate)?;
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
            let mut asset = Self::get_asset(asset_id).ok_or(Error::<T>::AssetNotFound)?;

            let approved_account =
                Self::asset_approvals(asset_id).ok_or(Error::<T>::NotAuthorized)?;
            ensure!(
                approved_account == *market_account,
                Error::<T>::NotAuthorized
            );
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked);

            let old_owner = asset.core.owner.clone();
            asset.core.owner = new_owner.clone();
            asset.core.nonce += 1;
            asset.core.status = AssetStatus::Private;
            asset.core.updated_at = Self::current_timestamp();

            Self::insert_asset(asset_id, &asset)?;
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
            let mut certificate = Self::get_certificate(asset_id, certificate_id)
                .ok_or(Error::<T>::CertificateNotFound)?;

            ensure!(certificate.owner == *current_owner, Error::<T>::NotOwner);
            ensure!(
                certificate.is_valid(Self::current_timestamp()),
                Error::<T>::CertificateNotActive
            );

            let old_owner = certificate.owner.clone();
            certificate.owner = new_owner.clone();
            certificate.nonce = certificate.nonce.saturating_add(1);

            Self::update_certificate(asset_id, &certificate)?;
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
        pub fn transfer_by_market_internal(
            asset_id: &[u8; 32],
            market_account: &T::AccountId,
            new_owner: &T::AccountId,
        ) -> DispatchResult {
            let _seller =
                Self::transfer_asset_by_market_state(asset_id, market_account, new_owner)?;
            Ok(())
        }

        pub fn settle_asset_trade_by_market_internal(
            asset_id: &[u8; 32],
            market_account: &T::AccountId,
            buyer: &T::AccountId,
            price: BalanceOf<T>,
            order_id: &[u8; 32],
            order_digest: &[u8; 32],
        ) -> Result<[u8; 32], DispatchError> {
            Self::verify_settle_prerequisites(
                order_id, order_digest, market_account,
                TradeAssetType::DataAsset, asset_id,
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

        pub fn issue_certificate_by_market_internal(
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

        pub fn issue_certificate_internal(
            asset_id: &[u8; 32],
            asset: &DataAsset<T::AccountId>,
            holder: &T::AccountId,
            right_type: u8,
            valid_until: Option<u64>,
        ) -> Result<[u8; 32], DispatchError> {
            ensure!(asset.is_active(), Error::<T>::AssetNotActive);

            let right_type_enum = match right_type {
                1 => RightType::Usage,
                2 => RightType::Access,
                _ => return Err(Error::<T>::InvalidRightType.into()),
            };

            let token_id = Self::get_next_certificate_id(asset_id);
            let current_time = Self::current_timestamp();
            let certificate = RightToken::minimal(
                token_id,
                right_type_enum,
                holder.clone(),
                asset.core.owner.clone(),
                *asset_id,
                current_time,
                valid_until,
            );
            let certificate_id = certificate.certificate_id;

            Self::insert_certificate(asset_id, &certificate)?;
            ActiveCertificateCount::<T>::mutate(asset_id, |count| *count = count.saturating_add(1));
            T::IncentiveHandler::register_asset_trade(asset_id);

            Self::deposit_event(Event::CertificateIssued {
                asset_id: *asset_id,
                certificate_id,
                issuer: asset.core.owner.clone(),
                holder: holder.clone(),
            });

            Ok(certificate_id)
        }

        pub fn transfer_certificate_internal(
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

        pub fn settle_certificate_trade_internal(
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            market_account: &T::AccountId,
            buyer: &T::AccountId,
            price: BalanceOf<T>,
            order_id: &[u8; 32],
            order_digest: &[u8; 32],
        ) -> Result<[u8; 32], DispatchError> {
            Self::verify_settle_prerequisites(
                order_id, order_digest, market_account,
                TradeAssetType::Certificate, certificate_id,
            )?;
            let seller = Self::do_transfer_certificate(
                asset_id, certificate_id, market_account, buyer,
            )?;
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
