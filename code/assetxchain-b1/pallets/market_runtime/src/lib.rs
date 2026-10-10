#![cfg_attr(not(feature = "std"), no_std)]

//! # G2 纯运行时市场 Pallet
//!
//! 第5章实验的 G2（纯运行时方案）。订单簿、撮合、授权管理和权属转移全部在
//! Pallet 内闭环完成，不涉及任何合约调用。
//!
//! 与 G1 和 G3 的核心区别：
//! - G1：全部在合约内（无运行时强制验证）
//! - G2：全部在 Pallet 内（无合约参与，无跨域开销）
//! - G3：合约管理订单 + 运行时强制验证结算
//!
//! G2 的实验角色：
//! - 验证"纯运行时方案在安全性上与 G3 一致"（正确性预期）
//! - 暴露"硬编码市场逻辑缺乏可编程性"（灵活性缺陷）
//! - 量化"全部 Pallet 操作 vs G3 合约+CE 的延迟差异"

// ============================================================================
// 重新导出 Substrate 依赖
// ============================================================================

pub use pallet::*;

use frame_support::{
    pallet_prelude::*,
    PalletId,
    Blake2_128Concat,
};
use frame_system::pallet_prelude::*;
use sp_core::H256;
use sp_std::vec::Vec;
use sp_runtime::traits::AccountIdConversion;
use scale_info::TypeInfo;

// ============================================================================
// 类型定义
// ============================================================================

/// 订单状态 — 与 G1/G3 一致
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum OrderStatus {
    Listed,
    Locked,
    Settled,
    Cancelled,
    Failed,
}

/// 交易对象类型 — 与 G1/G3 一致
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum TradeAssetType {
    DataAsset,
    Certificate,
}

/// 权证操作类型 — 与 G3 的 RightAction 一致
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum RightAction {
    TransferAsset,
    IssueRight,
    TransferRight,
}

/// 订单结构 — 与 G3 的 MarketOrder 对齐
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct RuntimeOrder<AccountId, Balance, BlockNumber> {
    pub order_id: [u8; 32],
    pub order_digest: [u8; 32],
    pub seller: AccountId,
    pub buyer: Option<AccountId>,
    pub object_type: TradeAssetType,
    pub object_id: [u8; 32],         // asset_id 或 certificate_id
    pub parent_asset_id: [u8; 32],    // 权证订单关联的父资产 ID
    pub right_type: u8,               // 仅 Certificate 类型有意义
    pub valid_until: Option<u64>,     // 权证有效期
    pub price: Balance,
    pub status: OrderStatus,
    pub created_at: BlockNumber,
    pub settled_at: Option<BlockNumber>,
}

/// 结算记录 — 与 G3 的 SettlementRecord 对齐
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct RuntimeSettlement<AccountId, Balance, BlockNumber> {
    pub trade_id: [u8; 32],
    pub order_id: [u8; 32],
    pub seller: AccountId,
    pub buyer: AccountId,
    pub asset_id: [u8; 32],
    pub certificate_id: [u8; 32],
    pub object_type: TradeAssetType,
    pub right_action: RightAction,
    pub price: Balance,
    pub order_digest: [u8; 32],
    pub settled_at: BlockNumber,
    pub state_root_at_settlement: H256,
}

// ============================================================================
// Pallet 配置
// ============================================================================

#[frame_support::pallet]
pub mod pallet {

    use super::*;

    // ========================================================================
    // 类型别名 — 必须在 Config trait 之前定义（前向引用）
    // ========================================================================

    /// 复用 dataassets pallet 的余额类型
    pub type BalanceOf<T> = pallet_dataassets::BalanceOf<T>;
    // BlockNumberFor<T> — 直接从 frame_system::pallet_prelude::* 导入，不自定义

    #[pallet::config]
    pub trait Config: frame_system::Config<RuntimeEvent: From<Event<Self>>> + pallet_dataassets::Config {
        /// Pallet 账户 ID（用于 do_transfer_asset 的 market_account 参数）
        #[pallet::constant]
        type PalletId: Get<PalletId>;

        /// 资产与权证 Pallet — G2 直接调用其内部 API
        type DataAssets: DataAssetsAccess<
            Self::AccountId,
            BalanceOf<Self>,
            BlockNumberFor<Self>,
        >;

        /// 每个资产的最大活跃订单数（默认为 1，与 G3 的"每资产仅一个活跃订单"模型一致）
        #[pallet::constant]
        type MaxActiveOrdersPerAsset: Get<u32>;
    }

    // ========================================================================
    // 存储
    // ========================================================================

    #[pallet::storage]
    #[pallet::getter(fn orders)]
    pub type Orders<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],                                          // order_id
        RuntimeOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// 辅助索引：asset_id → order_id（MetaToken 订单）
    #[pallet::storage]
    #[pallet::getter(fn asset_order_index)]
    pub type AssetOrderIndex<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],                                          // asset_id
        [u8; 32],                                          // order_id
        OptionQuery,
    >;

    /// 辅助索引：(asset_id, certificate_id) → order_id（RightToken 转让订单）
    #[pallet::storage]
    #[pallet::getter(fn cert_order_index)]
    pub type CertOrderIndex<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        ([u8; 32], [u8; 32]),                             // (asset_id, certificate_id)
        [u8; 32],                                          // order_id
        OptionQuery,
    >;

    /// 辅助索引：(asset_id, right_type) → order_id（RightToken 发行订单）
    #[pallet::storage]
    #[pallet::getter(fn issue_order_index)]
    pub type IssueOrderIndex<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        ([u8; 32], u8),                                   // (asset_id, right_type)
        [u8; 32],                                          // order_id
        OptionQuery,
    >;

    /// 结算记录（以 order_id 为主键）
    #[pallet::storage]
    #[pallet::getter(fn settlements)]
    pub type Settlements<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32],                                          // order_id
        RuntimeSettlement<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// 订单计数器（用于生成唯一 order_id 和 nonce）
    #[pallet::storage]
    #[pallet::getter(fn order_nonce)]
    pub type OrderNonce<T: Config> = StorageValue<_, u64, ValueQuery>;

    // ========================================================================
    // 事件
    // ========================================================================

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        /// MetaToken 上架
        AssetListed {
            order_id: [u8; 32],
            asset_id: [u8; 32],
            seller: T::AccountId,
            price: BalanceOf<T>,
        },
        /// RightToken 上架
        CertificateListed {
            order_id: [u8; 32],
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            seller: T::AccountId,
            price: BalanceOf<T>,
            right_type: u8,
        },
        /// 订单被锁定
        OrderLocked {
            order_id: [u8; 32],
            buyer: T::AccountId,
        },
        /// 交易结算成功
        TradeSettled {
            order_id: [u8; 32],
            trade_id: [u8; 32],
            seller: T::AccountId,
            buyer: T::AccountId,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            object_type: TradeAssetType,
            right_action: RightAction,
            price: BalanceOf<T>,
            settled_at: BlockNumberFor<T>,
        },
        /// 订单取消
        OrderCancelled {
            order_id: [u8; 32],
        },
        /// 订单失败
        OrderFailed {
            order_id: [u8; 32],
            reason: Vec<u8>,
        },
        /// 结算被拒绝
        SettlementRejected {
            order_id: [u8; 32],
            reason: Vec<u8>,
        },
    }

    // ========================================================================
    // 错误
    // ========================================================================

    #[pallet::error]
    pub enum Error<T> {
        /// 调用者未被授权
        NotAuthorized,
        /// 资产不存在
        AssetNotFound,
        /// 权证不存在
        CertificateNotFound,
        /// 权证已过期或被撤销
        CertificateNotActive,
        /// 调用者不是资产/权证的所有者
        NotOwner,
        /// 资产处于锁定状态
        AssetLocked,
        /// 订单不存在
        OrderNotFound,
        /// 订单状态非 Listed
        OrderNotListed,
        /// 订单状态无效
        OrderStatusInvalid,
        /// 订单已被同一资产的其他订单占用
        AssetAlreadyListed,
        /// 订单摘要不匹配
        OrderDigestMismatch,
        /// 订单已结算
        AlreadySettled,
        /// 活跃权证阻止操作
        OutstandingCertificates,
        /// 调用者不是订单买方
        CallerNotBuyer,
        /// 支付金额不足
        InsufficientPayment,
        /// 价格无效
        InvalidPrice,
        /// 自我转移
        SelfTransfer,
    }

    // ========================================================================
    // Pallet 结构
    // ========================================================================

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    // ========================================================================
    // Hooks
    // ========================================================================

    #[pallet::hooks]
    impl<T: Config> Hooks<BlockNumberFor<T>> for Pallet<T> {
        fn on_initialize(_n: BlockNumberFor<T>) -> Weight {
            Weight::zero()
        }
    }

    // ========================================================================
    // Extrinsics — 上架
    // ========================================================================

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// 上架 MetaToken。
        ///
        /// 调用者必须是资产所有者（通过 dataassets pallet 验证）。
        /// 每个资产仅允许一个活跃 MetaToken 订单。
        #[pallet::call_index(0)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 2))]
        pub fn list_asset(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let seller = ensure_signed(origin)?;

            // 验证所有权（调用 dataassets pallet 内部 API）
            let asset = T::DataAssets::get_asset(&asset_id)
                .ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.owner == seller, Error::<T>::NotOwner);
            ensure!(!asset.is_locked(), Error::<T>::AssetLocked);
            ensure!(!price.is_zero(), Error::<T>::InvalidPrice);

            // 检查已有活跃订单
            if AssetOrderIndex::<T>::contains_key(&asset_id) {
                let existing_id = AssetOrderIndex::<T>::get(&asset_id).unwrap();
                if let Some(existing) = Orders::<T>::get(&existing_id) {
                    ensure!(
                        !matches!(existing.status, OrderStatus::Listed | OrderStatus::Locked),
                        Error::<T>::AssetAlreadyListed,
                    );
                }
            }

            let block = frame_system::Pallet::<T>::block_number();
            let order_id = Self::generate_order_id(&asset_id, &seller, block);

            // 计算 order_digest
            let order_digest = Self::compute_order_digest(
                &order_id, &seller, price, 0u8, &asset_id,
            );

            let order = RuntimeOrder {
                order_id,
                order_digest,
                seller: seller.clone(),
                buyer: None,
                object_type: TradeAssetType::DataAsset,
                object_id: asset_id,
                parent_asset_id: [0u8; 32],
                right_type: 0,
                valid_until: None,
                price,
                status: OrderStatus::Listed,
                created_at: block,
                settled_at: None,
            };

            Orders::<T>::insert(&order_id, &order);
            AssetOrderIndex::<T>::insert(&asset_id, &order_id);

            Self::deposit_event(Event::AssetListed {
                order_id,
                asset_id,
                seller,
                price,
            });
            Ok(())
        }

        /// 上架 RightToken（转让已有权证）。
        ///
        /// 调用者必须是权证持有者（通过 dataassets pallet 验证）。
        #[pallet::call_index(1)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 2))]
        pub fn list_certificate(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: BalanceOf<T>,
            right_type: u8,
        ) -> DispatchResult {
            let seller = ensure_signed(origin)?;

            // 验证权证持有者
            let cert = T::DataAssets::get_certificate(&asset_id, &certificate_id)
                .ok_or(Error::<T>::CertificateNotFound)?;
            ensure!(cert.owner == seller, Error::<T>::NotOwner);
            ensure!(cert.is_active(), Error::<T>::CertificateNotActive);

            // 检查有效期
            if let Some(expiry) = cert.valid_until {
                let now = T::DataAssets::current_timestamp();
                ensure!(now <= expiry, Error::<T>::CertificateNotActive);
            }
            ensure!(!price.is_zero(), Error::<T>::InvalidPrice);

            // 检查已有活跃订单
            let cert_key = (asset_id, certificate_id);
            if CertOrderIndex::<T>::contains_key(&cert_key) {
                let existing_id = CertOrderIndex::<T>::get(&cert_key).unwrap();
                if let Some(existing) = Orders::<T>::get(&existing_id) {
                    ensure!(
                        !matches!(existing.status, OrderStatus::Listed | OrderStatus::Locked),
                        Error::<T>::AssetAlreadyListed,
                    );
                }
            }

            let block = frame_system::Pallet::<T>::block_number();
            let order_id = Self::generate_order_id(&certificate_id, &seller, block);

            let order_digest = Self::compute_order_digest(
                &order_id, &seller, price, 1u8, &certificate_id,
            );

            let order = RuntimeOrder {
                order_id,
                order_digest,
                seller: seller.clone(),
                buyer: None,
                object_type: TradeAssetType::Certificate,
                object_id: certificate_id,
                parent_asset_id: asset_id,
                right_type,
                valid_until: cert.valid_until,
                price,
                status: OrderStatus::Listed,
                created_at: block,
                settled_at: None,
            };

            Orders::<T>::insert(&order_id, &order);
            CertOrderIndex::<T>::insert(&cert_key, &order_id);

            Self::deposit_event(Event::CertificateListed {
                order_id,
                asset_id,
                certificate_id,
                seller,
                price,
                right_type,
            });
            Ok(())
        }

        /// 上架权证发行。
        ///
        /// 调用者必须是资产所有者。
        #[pallet::call_index(2)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 2))]
        pub fn list_certificate_issue(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            right_type: u8,
            price: BalanceOf<T>,
            valid_until: Option<u64>,
        ) -> DispatchResult {
            let seller = ensure_signed(origin)?;

            let asset = T::DataAssets::get_asset(&asset_id)
                .ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.owner == seller, Error::<T>::NotOwner);
            ensure!(!price.is_zero(), Error::<T>::InvalidPrice);

            // 检查已有发行订单
            let issue_key = (asset_id, right_type);
            if IssueOrderIndex::<T>::contains_key(&issue_key) {
                let existing_id = IssueOrderIndex::<T>::get(&issue_key).unwrap();
                if let Some(existing) = Orders::<T>::get(&existing_id) {
                    ensure!(
                        !matches!(existing.status, OrderStatus::Listed | OrderStatus::Locked),
                        Error::<T>::AssetAlreadyListed,
                    );
                }
            }

            let block = frame_system::Pallet::<T>::block_number();

            // 为发行订单生成 certificate_id
            let nonce = OrderNonce::<T>::get();
            OrderNonce::<T>::put(nonce.saturating_add(1));
            let cert_id = {
                Self::hash_bytes(&[
                    asset_id.as_slice(),
                    &seller.encode(),
                    &block.encode(),
                    &nonce.encode(),
                ].concat())
            };

            let order_id = Self::generate_order_id(&cert_id, &seller, block);

            let order_digest = Self::compute_order_digest(
                &order_id, &seller, price, 1u8, &cert_id,
            );

            let order = RuntimeOrder {
                order_id,
                order_digest,
                seller: seller.clone(),
                buyer: None,
                object_type: TradeAssetType::Certificate,
                object_id: cert_id,  // 发行订单使用推算的 cert_id
                parent_asset_id: asset_id,
                right_type,
                valid_until,
                price,
                status: OrderStatus::Listed,
                created_at: block,
                settled_at: None,
            };

            Orders::<T>::insert(&order_id, &order);
            IssueOrderIndex::<T>::insert(&issue_key, &order_id);

            Self::deposit_event(Event::CertificateListed {
                order_id,
                asset_id,
                certificate_id: cert_id,
                seller,
                price,
                right_type,
            });
            Ok(())
        }

        // ====================================================================
        // Extrinsics — 购买与结算
        // ====================================================================

        /// 购买 MetaToken。
        ///
        /// G2 的 settle 完全在 Pallet 内完成：
        /// Lock (try_mutate CAS) → 验证 (dataassets pallet 读) → 执行 (dataassets pallet 写) → 记录
        #[pallet::call_index(3)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 4))]
        pub fn buy_asset(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;

            let order_id = AssetOrderIndex::<T>::get(&asset_id)
                .ok_or(Error::<T>::OrderNotFound)?;

            Self::settle_asset_impl(&order_id, asset_id, &buyer, price)
        }

        /// 购买 MetaToken — 显式传入 order_id（推荐实验脚本使用）
        #[pallet::call_index(4)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 4))]
        pub fn buy_asset_by_order_id(
            origin: OriginFor<T>,
            order_id: [u8; 32],
            asset_id: [u8; 32],
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;
            Self::settle_asset_impl(&order_id, asset_id, &buyer, price)
        }

        /// 购买 RightToken（转让已有权证）
        #[pallet::call_index(5)]
        #[pallet::weight(T::DbWeight::get().reads_writes(5, 4))]
        pub fn buy_certificate(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;

            let cert_key = (asset_id, certificate_id);
            let order_id = CertOrderIndex::<T>::get(&cert_key)
                .ok_or(Error::<T>::OrderNotFound)?;

            Self::settle_certificate_impl(
                &order_id, asset_id, certificate_id, &buyer, price, RightAction::TransferRight,
            )
        }

        /// 购买 RightToken — 显式传入 order_id
        #[pallet::call_index(6)]
        #[pallet::weight(T::DbWeight::get().reads_writes(5, 4))]
        pub fn buy_certificate_by_order_id(
            origin: OriginFor<T>,
            order_id: [u8; 32],
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;
            Self::settle_certificate_impl(
                &order_id, asset_id, certificate_id, &buyer, price, RightAction::TransferRight,
            )
        }

        /// 购买权证发行
        #[pallet::call_index(7)]
        #[pallet::weight(T::DbWeight::get().reads_writes(5, 5))]
        pub fn buy_certificate_issue(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            right_type: u8,
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;

            let issue_key = (asset_id, right_type);
            let order_id = IssueOrderIndex::<T>::get(&issue_key)
                .ok_or(Error::<T>::OrderNotFound)?;

            Self::settle_certificate_issue_impl(&order_id, asset_id, &buyer, price)
        }

        /// 购买权证发行 — 显式传入 order_id
        #[pallet::call_index(8)]
        #[pallet::weight(T::DbWeight::get().reads_writes(5, 5))]
        pub fn buy_certificate_issue_by_order_id(
            origin: OriginFor<T>,
            order_id: [u8; 32],
            asset_id: [u8; 32],
            _right_type: u8,
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;
            Self::settle_certificate_issue_impl(&order_id, asset_id, &buyer, price)
        }

        // ====================================================================
        // Extrinsics — 订单管理
        // ====================================================================

        /// 取消订单
        #[pallet::call_index(9)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn cancel_order(
            origin: OriginFor<T>,
            order_id: [u8; 32],
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;

            Orders::<T>::try_mutate(&order_id, |maybe_order| -> Result<(), DispatchError> {
                let order = maybe_order.as_mut().ok_or(Error::<T>::OrderNotFound)?;

                ensure!(
                    order.seller == caller,
                    Error::<T>::NotAuthorized,
                );
                ensure!(
                    matches!(order.status, OrderStatus::Listed | OrderStatus::Locked),
                    Error::<T>::OrderStatusInvalid,
                );

                order.status = OrderStatus::Cancelled;

                // 清除索引
                Self::remove_order_from_index(&order_id, order);

                Self::deposit_event(Event::OrderCancelled { order_id });
                Ok(())
            })?;

            Ok(())
        }
    }

    // ========================================================================
    // 实现 — 内部方法
    // ========================================================================

    impl<T: Config> Pallet<T> {
        // --------------------------------------------------------------------
        // 哈希工具
        // --------------------------------------------------------------------

        fn hash_bytes(input: &[u8]) -> [u8; 32] {
            sp_io::hashing::blake2_256(input)
        }

        fn generate_order_id(
            seed: &[u8; 32],
            seller: &T::AccountId,
            block: BlockNumberFor<T>,
        ) -> [u8; 32] {
            let nonce = OrderNonce::<T>::get();
            OrderNonce::<T>::put(nonce.saturating_add(1));

            let mut input = Vec::new();
            input.extend_from_slice(seed);
            input.extend_from_slice(&seller.encode());
            input.extend_from_slice(&block.encode());
            input.extend_from_slice(&nonce.encode());
            Self::hash_bytes(&input)
        }

        fn compute_order_digest(
            order_id: &[u8; 32],
            seller: &T::AccountId,
            price: BalanceOf<T>,
            object_type: u8,
            object_id: &[u8; 32],
        ) -> [u8; 32] {
            let mut input = Vec::new();
            input.extend_from_slice(order_id);
            input.extend_from_slice(&seller.encode());
            input.extend_from_slice(&price.encode());
            input.push(object_type);
            input.extend_from_slice(object_id);
            Self::hash_bytes(&input)
        }

        fn generate_trade_id(
            order_id: &[u8; 32],
            buyer: &T::AccountId,
            block: BlockNumberFor<T>,
        ) -> [u8; 32] {
            let nonce = OrderNonce::<T>::get();
            OrderNonce::<T>::put(nonce.saturating_add(1));
            let mut input = Vec::new();
            input.extend_from_slice(order_id);
            input.extend_from_slice(&buyer.encode());
            input.extend_from_slice(&block.encode());
            input.extend_from_slice(&nonce.encode());
            Self::hash_bytes(&input)
        }

        // --------------------------------------------------------------------
        // 索引管理
        // --------------------------------------------------------------------

        fn remove_order_from_index(
            order_id: &[u8; 32],
            order: &RuntimeOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        ) {
            match order.object_type {
                TradeAssetType::DataAsset => {
                    if let Some(indexed_id) = AssetOrderIndex::<T>::get(&order.object_id) {
                        if indexed_id == *order_id {
                            AssetOrderIndex::<T>::remove(&order.object_id);
                        }
                    }
                }
                TradeAssetType::Certificate => {
                    let cert_key = (order.parent_asset_id, order.object_id);
                    if let Some(indexed_id) = CertOrderIndex::<T>::get(&cert_key) {
                        if indexed_id == *order_id {
                            CertOrderIndex::<T>::remove(&cert_key);
                        }
                    }
                    let issue_key = (order.parent_asset_id, order.right_type);
                    if let Some(indexed_id) = IssueOrderIndex::<T>::get(&issue_key) {
                        if indexed_id == *order_id {
                            IssueOrderIndex::<T>::remove(&issue_key);
                        }
                    }
                }
            }
        }

        // --------------------------------------------------------------------
        // Lock（CAS 原子）
        // --------------------------------------------------------------------

        fn lock_order(
            order_id: &[u8; 32],
            buyer: &T::AccountId,
        ) -> Result<(), DispatchError> {
            Orders::<T>::try_mutate(order_id, |maybe_order| -> Result<(), DispatchError> {
                let order = maybe_order.as_mut().ok_or(Error::<T>::OrderNotFound)?;
                ensure!(
                    order.status == OrderStatus::Listed,
                    Error::<T>::OrderNotListed,
                );
                order.status = OrderStatus::Locked;
                order.buyer = Some(buyer.clone());
                Ok(())
            })?;

            Self::deposit_event(Event::OrderLocked {
                order_id: *order_id,
                buyer: buyer.clone(),
            });
            Ok(())
        }

        // --------------------------------------------------------------------
        // 验证（对应 G3 的 verify_settle_prerequisites）
        // --------------------------------------------------------------------

        fn verify_settle_prerequisites_asset(
            order_id: &[u8; 32],
            asset_id: &[u8; 32],
            _buyer: &T::AccountId,
            price: BalanceOf<T>,
        ) -> Result<RuntimeOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>, DispatchError> {
            // 条件 1: NoPriorSettlement
            ensure!(
                !Settlements::<T>::contains_key(order_id),
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"AlreadySettled".to_vec(),
                    });
                    Error::<T>::AlreadySettled
                },
            );

            // 条件 2: OrderOpenOrLocked
            let order = Orders::<T>::get(order_id).ok_or({
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderNotFound".to_vec(),
                });
                Error::<T>::OrderNotFound
            })?;

            ensure!(
                matches!(order.status, OrderStatus::Listed | OrderStatus::Locked),
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"OrderStatusInvalid".to_vec(),
                    });
                    Error::<T>::OrderStatusInvalid
                },
            );

            // 条件 3: MatchOrderDigest
            let expected_digest = Self::compute_order_digest(
                order_id, &order.seller, price, 0u8, asset_id,
            );
            ensure!(
                order.order_digest == expected_digest,
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"OrderDigestMismatch".to_vec(),
                    });
                    Error::<T>::OrderDigestMismatch
                },
            );

            // 条件 4: MatchOwnership — 通过 dataassets pallet 验证实际链上所有权
            let asset = T::DataAssets::get_asset(asset_id).ok_or({
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"AssetNotFound".to_vec(),
                });
                Error::<T>::AssetNotFound
            })?;

            ensure!(
                asset.owner == order.seller,
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"OwnerMismatch".to_vec(),
                    });
                    Error::<T>::NotOwner
                },
            );

            ensure!(
                !asset.is_locked(),
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"AssetLocked".to_vec(),
                    });
                    Error::<T>::AssetLocked
                },
            );

            // 条件 5: MatchAuthorization — 通过 dataassets pallet 验证实际授权
            // NOTE: G2 中 Pallet 本身就是唯一的"市场"，因此授权模型简化：
            // Pallet 直接检查 AssetApprovals 中是否存在对 Pallet 本身的授权。
            // 实验脚本中需先调用 dataassets.authorize_market(asset_id, pallet_account_id)
            let _approved = T::DataAssets::get_approval(asset_id)
                .ok_or({
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"UnauthorizedMarket".to_vec(),
                    });
                    Error::<T>::NotAuthorized
                })?;

            // 条件 6: ObjectExistsAndValid — 条件 4 已检查资产存在
            // 条件 7: VerifySignature — Substrate 框架保证

            Ok(order)
        }

        fn verify_settle_prerequisites_certificate(
            order_id: &[u8; 32],
            asset_id: &[u8; 32],
            certificate_id: &[u8; 32],
            _buyer: &T::AccountId,
            price: BalanceOf<T>,
        ) -> Result<RuntimeOrder<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>, DispatchError> {
            // 条件 1: NoPriorSettlement
            ensure!(
                !Settlements::<T>::contains_key(order_id),
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"AlreadySettled".to_vec(),
                    });
                    Error::<T>::AlreadySettled
                },
            );

            // 条件 2: OrderOpenOrLocked
            let order = Orders::<T>::get(order_id).ok_or({
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderNotFound".to_vec(),
                });
                Error::<T>::OrderNotFound
            })?;

            ensure!(
                matches!(order.status, OrderStatus::Listed | OrderStatus::Locked),
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"OrderStatusInvalid".to_vec(),
                    });
                    Error::<T>::OrderStatusInvalid
                },
            );

            // 条件 3: MatchOrderDigest
            let expected_digest = Self::compute_order_digest(
                order_id, &order.seller, price, 1u8, certificate_id,
            );
            ensure!(
                order.order_digest == expected_digest,
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"OrderDigestMismatch".to_vec(),
                    });
                    Error::<T>::OrderDigestMismatch
                },
            );

            // 条件 4: 权证存在且有效
            let cert = T::DataAssets::get_certificate(asset_id, certificate_id).ok_or({
                Self::deposit_event(Event::SettlementRejected {
                    order_id: *order_id,
                    reason: b"CertificateNotFound".to_vec(),
                });
                Error::<T>::CertificateNotFound
            })?;

            ensure!(
                cert.is_active(),
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"CertificateNotActive".to_vec(),
                    });
                    Error::<T>::CertificateNotActive
                },
            );

            // 条件 5: 卖方是权证持有者
            ensure!(
                cert.owner == order.seller,
                {
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"HolderMismatch".to_vec(),
                    });
                    Error::<T>::NotOwner
                },
            );

            // 条件 6: MatchAuthorization — 权证结算的授权检查
            // 与 G3 F 项修复对齐：追溯父资产检查 AssetApprovals
            let _approved = T::DataAssets::get_approval(asset_id)
                .ok_or({
                    Self::deposit_event(Event::SettlementRejected {
                        order_id: *order_id,
                        reason: b"UnauthorizedMarket".to_vec(),
                    });
                    Error::<T>::NotAuthorized
                })?;

            Ok(order)
        }

        // --------------------------------------------------------------------
        // 结算执行
        // --------------------------------------------------------------------

        fn settle_asset_impl(
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            buyer: &T::AccountId,
            price: BalanceOf<T>,
        ) -> DispatchResult {
            // Step 1: Lock（CAS 原子）
            Self::lock_order(order_id, buyer)?;

            // Step 2: 验证
            let _order = Self::verify_settle_prerequisites_asset(
                order_id, &asset_id, buyer, price,
            )?;

            // Step 3: 执行权属转移（调用 dataassets pallet 内部 API）
            // market_account: G2 中 Pallet 本身充当"市场"，传入 Pallet AccountId
            let market = T::PalletId::get().into_account_truncating();
            let seller = T::DataAssets::do_transfer_asset(&asset_id, &market, buyer)?;

            // Step 4: 更新订单状态
            let block = frame_system::Pallet::<T>::block_number();
            Orders::<T>::try_mutate(order_id, |maybe_order| -> Result<(), DispatchError> {
                let order = maybe_order.as_mut().ok_or(Error::<T>::OrderNotFound)?;
                order.status = OrderStatus::Settled;
                order.settled_at = Some(block);
                Ok(())
            })?;

            // 清除索引
            if let Some(order) = Orders::<T>::get(order_id) {
                Self::remove_order_from_index(order_id, &order);
            }

            // Step 5: 生成结算记录（Double-check 通过 try_mutate）
            let trade_id = Self::record_settlement(
                order_id,
                &seller,
                buyer,
                asset_id,
                [0u8; 32],
                TradeAssetType::DataAsset,
                RightAction::TransferAsset,
                price,
            )?;

            Self::deposit_event(Event::TradeSettled {
                order_id: *order_id,
                trade_id,
                seller,
                buyer: buyer.clone(),
                asset_id,
                certificate_id: [0u8; 32],
                object_type: TradeAssetType::DataAsset,
                right_action: RightAction::TransferAsset,
                price,
                settled_at: block,
            });
            Ok(())
        }

        fn settle_certificate_impl(
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            buyer: &T::AccountId,
            price: BalanceOf<T>,
            right_action: RightAction,
        ) -> DispatchResult {
            Self::lock_order(order_id, buyer)?;

            let _order = Self::verify_settle_prerequisites_certificate(
                order_id, &asset_id, &certificate_id, buyer, price,
            )?;

            // 需要 order.seller 作为 current_owner；先拷贝出来避免借用冲突
            let current_owner = _order.seller.clone();
            let seller = T::DataAssets::do_transfer_certificate(
                &asset_id, &certificate_id, &current_owner, buyer,
            )?;

            let block = frame_system::Pallet::<T>::block_number();
            Orders::<T>::try_mutate(order_id, |maybe_order| -> Result<(), DispatchError> {
                let order = maybe_order.as_mut().ok_or(Error::<T>::OrderNotFound)?;
                order.status = OrderStatus::Settled;
                order.settled_at = Some(block);
                Ok(())
            })?;

            if let Some(order) = Orders::<T>::get(order_id) {
                Self::remove_order_from_index(order_id, &order);
            }

            let trade_id = Self::record_settlement(
                order_id,
                &seller,
                buyer,
                asset_id,
                certificate_id,
                TradeAssetType::Certificate,
                right_action,
                price,
            )?;

            Self::deposit_event(Event::TradeSettled {
                order_id: *order_id,
                trade_id,
                seller,
                buyer: buyer.clone(),
                asset_id,
                certificate_id,
                object_type: TradeAssetType::Certificate,
                right_action,
                price,
                settled_at: block,
            });
            Ok(())
        }

        fn settle_certificate_issue_impl(
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            buyer: &T::AccountId,
            price: BalanceOf<T>,
        ) -> DispatchResult {
            Self::lock_order(order_id, buyer)?;

            // 发行路径的验证不检查已有权证（权证尚未存在）
            ensure!(
                !Settlements::<T>::contains_key(order_id),
                Error::<T>::AlreadySettled,
            );

            let order = Orders::<T>::get(order_id)
                .ok_or(Error::<T>::OrderNotFound)?;

            ensure!(
                matches!(order.status, OrderStatus::Listed | OrderStatus::Locked),
                Error::<T>::OrderStatusInvalid,
            );

            ensure!(
                order.buyer.as_ref() == Some(buyer),
                Error::<T>::CallerNotBuyer,
            );

            // 验证资产存在且卖方是所有者
            let asset = T::DataAssets::get_asset(&asset_id)
                .ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.owner == order.seller, Error::<T>::NotOwner);

            // 发行权证
            let cert_id = order.object_id;
            // 构造轻量 asset_info 传给适配器
            let asset_info = AssetInfo {
                owner: asset.owner,
                is_locked: asset.is_locked,
                created_at: asset.created_at,
            };
            T::DataAssets::issue_certificate_internal(
                &asset_id,
                &cert_id,
                &asset_info,
                buyer,
                order.right_type,
                order.valid_until,
            )?;

            let seller = order.seller.clone();
            let block = frame_system::Pallet::<T>::block_number();
            Orders::<T>::try_mutate(order_id, |maybe_order| -> Result<(), DispatchError> {
                let order = maybe_order.as_mut().ok_or(Error::<T>::OrderNotFound)?;
                order.status = OrderStatus::Settled;
                order.settled_at = Some(block);
                Ok(())
            })?;

            if let Some(order) = Orders::<T>::get(order_id) {
                Self::remove_order_from_index(order_id, &order);
            }

            let trade_id = Self::record_settlement(
                order_id,
                &seller,
                buyer,
                asset_id,
                cert_id,
                TradeAssetType::Certificate,
                RightAction::IssueRight,
                price,
            )?;

            Self::deposit_event(Event::TradeSettled {
                order_id: *order_id,
                trade_id,
                seller,
                buyer: buyer.clone(),
                asset_id,
                certificate_id: cert_id,
                object_type: TradeAssetType::Certificate,
                right_action: RightAction::IssueRight,
                price,
                settled_at: block,
            });
            Ok(())
        }

        // --------------------------------------------------------------------
        // 结算记录（CAS Double-check）
        // --------------------------------------------------------------------

        fn record_settlement(
            order_id: &[u8; 32],
            seller: &T::AccountId,
            buyer: &T::AccountId,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            object_type: TradeAssetType,
            right_action: RightAction,
            price: BalanceOf<T>,
        ) -> Result<[u8; 32], DispatchError> {
            let block = frame_system::Pallet::<T>::block_number();
            let trade_id = Self::generate_trade_id(order_id, buyer, block);

            let object_id = match object_type {
                TradeAssetType::DataAsset => asset_id,
                TradeAssetType::Certificate => certificate_id,
            };
            let type_byte = match object_type {
                TradeAssetType::DataAsset => 0u8,
                TradeAssetType::Certificate => 1u8,
            };
            let order_digest = Self::compute_order_digest(
                order_id, seller, price, type_byte, &object_id,
            );

            let record = RuntimeSettlement {
                trade_id,
                order_id: *order_id,
                seller: seller.clone(),
                buyer: buyer.clone(),
                asset_id,
                certificate_id,
                object_type,
                right_action,
                price,
                order_digest,
                settled_at: block,
                state_root_at_settlement: T::DataAssets::compute_state_root(),
            };

            // Double-check: CAS 写入
            Settlements::<T>::try_mutate(order_id, |existing| -> Result<(), DispatchError> {
                ensure!(existing.is_none(), Error::<T>::AlreadySettled);
                *existing = Some(record);
                Ok(())
            })?;

            Ok(trade_id)
        }
    }
}  // end of #[frame_support::pallet] mod

// ============================================================================
// dataassets Pallet 内部 API 访问 trait
// ============================================================================

/// G2 Pallet 需要调用 dataassets pallet 的内部函数执行资产/权证操作。
/// 这个 trait 抽象了 dataassets pallet 的内部 API。
pub trait DataAssetsAccess<AccountId, Balance, BlockNumber> {
    /// 获取资产信息
    fn get_asset(asset_id: &[u8; 32]) -> Option<AssetInfo<AccountId, BlockNumber>>;

    /// 获取权证信息
    fn get_certificate(
        asset_id: &[u8; 32],
        certificate_id: &[u8; 32],
    ) -> Option<CertificateInfo<AccountId, BlockNumber>>;

    /// 获取资产的市场授权
    fn get_approval(asset_id: &[u8; 32]) -> Option<AccountId>;

    /// 执行资产所有权转移（纯写入，前置验证已完成）
    fn do_transfer_asset(
        asset_id: &[u8; 32],
        market_account: &AccountId,
        new_owner: &AccountId,
    ) -> Result<AccountId, DispatchError>;  // returns old owner

    /// 执行权证持有者转移（纯写入，前置验证已完成）
    fn do_transfer_certificate(
        asset_id: &[u8; 32],
        certificate_id: &[u8; 32],
        current_owner: &AccountId,
        new_holder: &AccountId,
    ) -> Result<AccountId, DispatchError>;  // returns old holder

    /// 发行权证
    fn issue_certificate_internal(
        asset_id: &[u8; 32],
        certificate_id: &[u8; 32],
        asset_data: &AssetInfo<AccountId, BlockNumber>,
        holder: &AccountId,
        right_type: u8,
        valid_until: Option<u64>,
    ) -> Result<(), DispatchError>;

    /// 计算资产状态根（用于 SettlementRecord 锚定）
    fn compute_state_root() -> H256;

    /// 获取当前时间戳
    fn current_timestamp() -> u64;
}

/// 资产信息 — dataassets pallet 对外暴露的最小接口
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct AssetInfo<AccountId, BlockNumber> {
    pub owner: AccountId,
    pub is_locked: bool,
    pub created_at: BlockNumber,
}

impl<AccountId, BlockNumber> AssetInfo<AccountId, BlockNumber> {
    pub fn is_locked(&self) -> bool {
        self.is_locked
    }
}

/// 权证信息 — dataassets pallet 对外暴露的最小接口
/// 字段名对齐 dataassets::RightToken（owner 而非 holder）
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct CertificateInfo<AccountId, BlockNumber> {
    pub owner: AccountId,
    pub issuer: AccountId,
    pub right_type: u8,
    pub issued_at: BlockNumber,
    pub valid_until: Option<u64>,
    pub active: bool,
}

impl<AccountId, BlockNumber> CertificateInfo<AccountId, BlockNumber> {
    pub fn is_active(&self) -> bool {
        self.active
    }
    /// 检查权证是否在给定时戳下有效（对齐 RightToken::is_valid）
    pub fn is_valid(&self, current_time: u64) -> bool {
        self.active && self.valid_until.map_or(true, |expiry| current_time <= expiry)
    }
}
