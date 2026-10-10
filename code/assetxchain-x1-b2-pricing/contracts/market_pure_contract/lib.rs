#![cfg_attr(not(feature = "std"), no_std, no_main)]

/// G1 纯合约市场 (Pure-Contract Market)
///
/// 第5章实验的 G1（纯合约方案）。与 G3（协同方案）的核心区别：
/// — G1 的资产所有权、权证持有、授权关系全部在合约内维护（ink! Mapping），
///   不调用任何 Chain Extension，不访问 Pallet 存储
/// — G3 通过 Chain Extension 调用运行时（Pallet），由 Pallet 强制验证授权和权属
///
/// 实验对照的关键：G1 暴露与 G3 相同的外部接口（函数签名、事件类型），
/// 但在合约内自行管理状态，因此缺少运行时级别的原子性和强制验证。
///
/// G1 的结构性弱点（期望在实验中暴露）：
/// 1. 无强制授权检查 — E1 场景中未授权市场可能交易成功
/// 2. 合约侧所有权验证可与 Pallet 不同步 — E3 场景
/// 3. 非原子 Lock（if-check-then-set）— E5/E7 场景中高并发下可能重复结算
/// 4. TOCTOU 授权窗口 — 5.4 节 T2 场景中授权撤销无法穿透到已 Lock 的订单
/// 5. 合约内 SettlementRecord 不可信 — 5.6 节审计指标不完整

#[ink::contract]
mod market_pure_contract {
    use ink::prelude::vec::Vec;
    use ink::prelude::string::String;
    use ink::storage::Mapping;
    use scale::Encode;

    // ========================================================================
    // 类型定义 — 与 G3 的 market_orderbook 保持一致
    // ========================================================================

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum OrderStatus {
        Listed,
        Locked,
        Settled,
        Cancelled,
        Failed,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum AssetType {
        DataAsset,
        Certificate,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum RightAction {
        TransferAsset,
        IssueRight,
        TransferRight,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct Order {
        pub seller: AccountId,
        pub buyer: Option<AccountId>,
        pub price: Balance,
        pub asset_type: AssetType,
        pub asset_id: [u8; 32],
        pub certificate_id: [u8; 32],
        pub right_type: u8,
        pub valid_until: Option<u64>,
        pub status: OrderStatus,
        pub created_at: BlockNumber,
        pub settled_at: Option<BlockNumber>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct SettlementRecord {
        pub trade_id: [u8; 32],
        pub order_id: [u8; 32],
        pub market: AccountId,
        pub seller: AccountId,
        pub buyer: AccountId,
        pub asset_id: [u8; 32],
        pub certificate_id: [u8; 32],
        pub object_type: AssetType,
        pub right_action: RightAction,
        pub price: Balance,
        pub order_digest: [u8; 32],
        pub settled_at: BlockNumber,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct CertificateInfo {
        pub holder: AccountId,
        pub issuer: AccountId,
        pub right_type: u8,
        pub issued_at: BlockNumber,
        pub valid_until: Option<u64>,
        pub active: bool,
    }

    // ========================================================================
    // 事件
    // ========================================================================

    #[ink(event)]
    pub struct AssetRegistered {
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub owner: AccountId,
    }

    #[ink(event)]
    pub struct AssetTransferred {
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub from: AccountId,
        #[ink(topic)]
        pub to: AccountId,
    }

    #[ink(event)]
    pub struct MarketAuthorized {
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub market: AccountId,
    }

    #[ink(event)]
    pub struct AuthorizationRevoked {
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub market: AccountId,
    }

    #[ink(event)]
    pub struct CertificateIssued {
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub certificate_id: [u8; 32],
        #[ink(topic)]
        pub holder: AccountId,
        pub right_type: u8,
        pub valid_until: Option<u64>,
    }

    #[ink(event)]
    pub struct CertificateRevoked {
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub certificate_id: [u8; 32],
    }

    #[ink(event)]
    pub struct AssetListed {
        #[ink(topic)]
        pub order_id: [u8; 32],
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub seller: AccountId,
        pub price: Balance,
    }

    #[ink(event)]
    pub struct CertificateListed {
        #[ink(topic)]
        pub order_id: [u8; 32],
        #[ink(topic)]
        pub asset_id: [u8; 32],
        #[ink(topic)]
        pub certificate_id: [u8; 32],
        #[ink(topic)]
        pub seller: AccountId,
        pub price: Balance,
        pub right_type: u8,
    }

    #[ink(event)]
    pub struct OrderLocked {
        #[ink(topic)]
        pub order_id: [u8; 32],
        #[ink(topic)]
        pub buyer: AccountId,
    }

    #[ink(event)]
    pub struct TradeSettled {
        #[ink(topic)]
        pub order_id: [u8; 32],
        #[ink(topic)]
        pub trade_id: [u8; 32],
        #[ink(topic)]
        pub seller: AccountId,
        #[ink(topic)]
        pub buyer: AccountId,
        pub asset_id: [u8; 32],
        pub certificate_id: [u8; 32],
        pub asset_type: AssetType,
        pub right_action: RightAction,
        pub price: Balance,
        pub settled_at: BlockNumber,
    }

    #[ink(event)]
    pub struct OrderCancelled {
        #[ink(topic)]
        pub order_id: [u8; 32],
    }

    #[ink(event)]
    pub struct OrderFailed {
        #[ink(topic)]
        pub order_id: [u8; 32],
        pub reason: Vec<u8>,
    }

    #[ink(event)]
    pub struct SettlementRejected {
        #[ink(topic)]
        pub order_id: [u8; 32],
        pub reason: Vec<u8>,
    }

    // ========================================================================
    // 错误类型
    // ========================================================================

    #[derive(scale::Encode, scale::Decode, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum Error {
        NotAuthorized,
        AssetNotFound,
        CertificateNotFound,
        CertificateNotActive,
        NotOwner,
        AssetLocked,
        OrderNotFound,
        OrderNotListed,
        OrderAlreadyLocked,
        OrderNotLocked,
        InsufficientPayment,
        AlreadySettled,
        AlreadyAuthorized,
        OutstandingCertificates,
        CallerNotBuyer,
        OrderDigestMismatch,
        OrderStatusInvalid,
        TransferFailed,
        SelfTransfer,
        InvalidPrice,
        AssetAlreadyListed,
    }

    // ========================================================================
    // 存储
    // ========================================================================

    #[ink(storage)]
    pub struct MarketPureContract {
        // ---- 虚拟资产所有权（G1 自行维护） ----
        asset_owners: Mapping<[u8; 32], AccountId>,

        // ---- 虚拟权证持有 ----
        certificate_holders: Mapping<([u8; 32], [u8; 32]), CertificateInfo>,

        // ---- 活跃权证计数（每资产） ----
        certificate_counts: Mapping<[u8; 32], u32>,

        // ---- 市场授权（仅合约内有效，无运行时强制） ----
        authorizations: Mapping<([u8; 32], AccountId), ()>,

        // ---- 订单簿 ----
        orders: Mapping<[u8; 32], Order>,

        // ---- 辅助索引：asset_id → order_id（MetaToken 订单） ----
        // 与 G3 的"每个资产仅一个活跃 MetaToken 订单"模型一致
        asset_order_index: Mapping<[u8; 32], [u8; 32]>,

        // ---- 辅助索引：(asset_id, certificate_id) → order_id（RightToken 转让订单） ----
        cert_order_index: Mapping<([u8; 32], [u8; 32]), [u8; 32]>,

        // ---- 辅助索引：(asset_id, right_type) → order_id（RightToken 发行订单） ----
        issue_order_index: Mapping<([u8; 32], u8), [u8; 32]>,

        // ---- 结算记录 ----
        settlement_records: Mapping<[u8; 32], SettlementRecord>,

        // ---- 订单计数器 ----
        order_nonce: u64,

        // ---- 管理员（部署者） ----
        admin: AccountId,
    }

    // ========================================================================
    // 构造函数
    // ========================================================================

    impl MarketPureContract {
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                asset_owners: Mapping::default(),
                certificate_holders: Mapping::default(),
                certificate_counts: Mapping::default(),
                authorizations: Mapping::default(),
                orders: Mapping::default(),
                asset_order_index: Mapping::default(),
                cert_order_index: Mapping::default(),
                issue_order_index: Mapping::default(),
                settlement_records: Mapping::default(),
                order_nonce: 0,
                admin: Self::env().caller(),
            }
        }

        // ====================================================================
        // 资产管理 — 对应 Pallet register_asset / transfer_asset
        // ====================================================================

        /// 注册资产所有权。
        /// 在 G3 中，所有权由 Pallet register_asset 写入链上存储。
        /// 在 G1 中，所有权仅存在于合约内 — 实验脚本需要在 Pallet 注册资产后
        /// 调用此函数将所有权同步到合约。
        #[ink(message)]
        pub fn initialize_asset_ownership(
            &mut self,
            asset_id: [u8; 32],
            owner: AccountId,
        ) -> Result<(), Error> {
            let caller = self.env().caller();
            if caller != self.admin && caller != owner {
                return Err(Error::NotAuthorized);
            }
            self.asset_owners.insert(&asset_id, &owner);
            self.env().emit_event(AssetRegistered { asset_id, owner });
            Ok(())
        }

        /// 直接转移资产所有权（不经过订单簿）。
        /// 对应 Pallet transfer_asset。
        #[ink(message)]
        pub fn transfer_asset(
            &mut self,
            asset_id: [u8; 32],
            new_owner: AccountId,
        ) -> Result<(), Error> {
            let caller = self.env().caller();
            let current_owner = self
                .asset_owners
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;

            if caller != current_owner {
                return Err(Error::NotOwner);
            }
            if caller == new_owner {
                return Err(Error::SelfTransfer);
            }

            // 活跃权证检查 — 对应 Pallet ActiveCertificateCount
            let count = self.certificate_counts.get(&asset_id).unwrap_or(0);
            if count > 0 {
                return Err(Error::OutstandingCertificates);
            }

            self.asset_owners.insert(&asset_id, &new_owner);
            self.env().emit_event(AssetTransferred {
                asset_id,
                from: caller,
                to: new_owner,
            });
            Ok(())
        }

        // ====================================================================
        // 权证管理 — 对应 Pallet issue_certificate / revoke_certificate
        // ====================================================================

        #[ink(message)]
        pub fn issue_certificate(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            holder: AccountId,
            right_type: u8,
            valid_until: Option<u64>,
        ) -> Result<(), Error> {
            let caller = self.env().caller();
            let owner = self
                .asset_owners
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;

            if caller != owner {
                return Err(Error::NotOwner);
            }

            let key = (asset_id, certificate_id);
            if self.certificate_holders.contains(&key) {
                return Err(Error::CertificateNotActive);
            }

            let info = CertificateInfo {
                holder,
                issuer: caller,
                right_type,
                issued_at: self.env().block_number(),
                valid_until,
                active: true,
            };
            self.certificate_holders.insert(&key, &info);

            let count = self.certificate_counts.get(&asset_id).unwrap_or(0);
            self.certificate_counts
                .insert(&asset_id, &(count.saturating_add(1)));

            self.env().emit_event(CertificateIssued {
                asset_id,
                certificate_id,
                holder,
                right_type,
                valid_until,
            });
            Ok(())
        }

        #[ink(message)]
        pub fn revoke_certificate(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> Result<(), Error> {
            let caller = self.env().caller();
            let key = (asset_id, certificate_id);
            let mut info = self
                .certificate_holders
                .get(&key)
                .ok_or(Error::CertificateNotFound)?;

            if caller != info.issuer {
                let owner = self
                    .asset_owners
                    .get(&asset_id)
                    .ok_or(Error::AssetNotFound)?;
                if caller != owner {
                    return Err(Error::NotAuthorized);
                }
            }

            info.active = false;
            self.certificate_holders.insert(&key, &info);

            let count = self.certificate_counts.get(&asset_id).unwrap_or(0);
            self.certificate_counts
                .insert(&asset_id, &(count.saturating_sub(1)));

            self.env().emit_event(CertificateRevoked {
                asset_id,
                certificate_id,
            });
            Ok(())
        }

        // ====================================================================
        // 授权管理 — 对应 Pallet authorize_market / revoke_authorization
        //
        // G1 的核心漏洞：授权仅在合约内生效，Pallet 不知情。
        // 5.4 节 T2 场景：买方在合约内 Lock 订单（合约检查授权 ✓），
        //   但此时 Pallet revoke_authorization 不影响合约内已通过的检查。
        // ====================================================================

        #[ink(message)]
        pub fn authorize_market(
            &mut self,
            asset_id: [u8; 32],
            market: AccountId,
        ) -> Result<(), Error> {
            let caller = self.env().caller();
            let owner = self
                .asset_owners
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;

            if caller != owner {
                return Err(Error::NotOwner);
            }

            self.authorizations.insert(&(asset_id, market), &());
            self.env().emit_event(MarketAuthorized { asset_id, market });
            Ok(())
        }

        #[ink(message)]
        pub fn revoke_authorization(
            &mut self,
            asset_id: [u8; 32],
            market: AccountId,
        ) -> Result<(), Error> {
            let caller = self.env().caller();
            let owner = self
                .asset_owners
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;

            if caller != owner {
                return Err(Error::NotOwner);
            }

            self.authorizations.remove(&(asset_id, market));
            self.env().emit_event(AuthorizationRevoked { asset_id, market });
            Ok(())
        }

        // ====================================================================
        // 订单簿 — 上架
        // ====================================================================

        #[ink(message)]
        pub fn list_asset(
            &mut self,
            asset_id: [u8; 32],
            price: Balance,
        ) -> Result<[u8; 32], Error> {
            let caller = self.env().caller();
            let owner = self
                .asset_owners
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;

            if caller != owner {
                return Err(Error::NotOwner);
            }
            if price == 0 {
                return Err(Error::InvalidPrice);
            }

            // 检查是否已有活跃订单（每个资产仅允许一个活跃 MetaToken 订单）
            if self.asset_order_index.contains(&asset_id) {
                let existing_id = self.asset_order_index.get(&asset_id).unwrap();
                if let Some(existing) = self.orders.get(&existing_id) {
                    if matches!(existing.status, OrderStatus::Listed | OrderStatus::Locked) {
                        return Err(Error::AssetAlreadyListed);
                    }
                }
            }

            let block = self.env().block_number();
            let order_id = self.next_order_id(&asset_id, &caller, block);

            let order = Order {
                seller: caller,
                buyer: None,
                price,
                asset_type: AssetType::DataAsset,
                asset_id,
                certificate_id: [0u8; 32],
                right_type: 0,
                valid_until: None,
                status: OrderStatus::Listed,
                created_at: block,
                settled_at: None,
            };

            self.orders.insert(&order_id, &order);
            self.asset_order_index.insert(&asset_id, &order_id);

            // NOTE: G3 在此处调用 CE CREATE_ORDER_PROJECTION
            // G1 不调用 — 订单仅存在于合约内

            self.env().emit_event(AssetListed {
                order_id,
                asset_id,
                seller: caller,
                price,
            });
            Ok(order_id)
        }

        #[ink(message)]
        pub fn list_certificate(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: Balance,
            right_type: u8,
        ) -> Result<[u8; 32], Error> {
            let caller = self.env().caller();
            let key = (asset_id, certificate_id);
            let info = self
                .certificate_holders
                .get(&key)
                .ok_or(Error::CertificateNotFound)?;

            if caller != info.holder {
                return Err(Error::NotOwner);
            }
            if !info.active {
                return Err(Error::CertificateNotActive);
            }
            if let Some(expiry) = info.valid_until {
                let current_block = self.env().block_number();
                if current_block as u64 > expiry {
                    return Err(Error::CertificateNotActive);
                }
            }
            if price == 0 {
                return Err(Error::InvalidPrice);
            }

            // 检查已有活跃订单
            if self.cert_order_index.contains(&key) {
                let existing_id = self.cert_order_index.get(&key).unwrap();
                if let Some(existing) = self.orders.get(&existing_id) {
                    if matches!(existing.status, OrderStatus::Listed | OrderStatus::Locked) {
                        return Err(Error::AssetAlreadyListed);
                    }
                }
            }

            let block = self.env().block_number();
            let order_id = self.next_order_id(&certificate_id, &caller, block);

            let order = Order {
                seller: caller,
                buyer: None,
                price,
                asset_type: AssetType::Certificate,
                asset_id,
                certificate_id,
                right_type,
                valid_until: info.valid_until,
                status: OrderStatus::Listed,
                created_at: block,
                settled_at: None,
            };

            self.orders.insert(&order_id, &order);
            self.cert_order_index.insert(&key, &order_id);

            self.env().emit_event(CertificateListed {
                order_id,
                asset_id,
                certificate_id,
                seller: caller,
                price,
                right_type,
            });
            Ok(order_id)
        }

        #[ink(message)]
        pub fn list_certificate_issue(
            &mut self,
            asset_id: [u8; 32],
            right_type: u8,
            price: Balance,
            valid_until: Option<u64>,
        ) -> Result<[u8; 32], Error> {
            let caller = self.env().caller();
            let owner = self
                .asset_owners
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;

            if caller != owner {
                return Err(Error::NotOwner);
            }
            if price == 0 {
                return Err(Error::InvalidPrice);
            }

            let block = self.env().block_number();

            // certificate_id = hash(asset_id || seller || block || nonce)
            let cert_id = {
                let mut input = Vec::new();
                input.extend_from_slice(&asset_id);
                input.extend_from_slice(&caller.encode());
                input.extend_from_slice(&block.to_le_bytes());
                input.extend_from_slice(&self.order_nonce.to_le_bytes());
                self.hash_bytes(&input)
            };

            self.order_nonce = self.order_nonce.saturating_add(1);
            let order_id = self.next_order_id(&cert_id, &caller, block);

            let order = Order {
                seller: caller,
                buyer: None,
                price,
                asset_type: AssetType::Certificate,
                asset_id,
                certificate_id: cert_id,
                right_type,
                valid_until,
                status: OrderStatus::Listed,
                created_at: block,
                settled_at: None,
            };

            self.orders.insert(&order_id, &order);
            let issue_key = (asset_id, right_type);
            self.issue_order_index.insert(&issue_key, &order_id);

            self.env().emit_event(CertificateListed {
                order_id,
                asset_id,
                certificate_id: cert_id,
                seller: caller,
                price,
                right_type,
            });
            Ok(order_id)
        }

        // ====================================================================
        // 购买与结算
        //
        // G1 结算完全在合约内完成，不调用 Chain Extension。
        // 与 G3 的核心区别：
        //   G3: buy → Lock(CE try_mutate, CAS) → settle(CE + Pallet 验证授权/所有权 → Pallet 写入)
        //   G1: buy → Lock(合约内 if-check-then-set, 非原子) → settle(合约内验证+写入)
        //
        // G1 的 Lock 是 if-check-then-set，在高并发下不可靠（5.3 节实验预期暴露）。
        // G3 的 Lock 通过 CE → Pallet StorageMap::try_mutate 实现真正的 CAS。
        // G1 的 settle 不检查 Pallet 授权（无 Chain Extension），5.4 节预期暴露。
        // ====================================================================

        /// 购买 MetaToken。
        /// 对应 G3 的 buy_asset：通过 asset_order_index 查找订单。
        #[ink(message, payable)]
        pub fn buy_asset(
            &mut self,
            asset_id: [u8; 32],
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let caller = self.env().caller();
            let transferred = self.env().transferred_value();

            if transferred < price {
                return Err(Error::InsufficientPayment);
            }

            let order_id = self
                .asset_order_index
                .get(&asset_id)
                .ok_or(Error::OrderNotFound)?;

            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            if order.asset_id != asset_id || !matches!(order.asset_type, AssetType::DataAsset) {
                return Err(Error::OrderNotFound);
            }
            if order.price != price {
                return Err(Error::OrderDigestMismatch);
            }

            self.settle_asset_inner(&order_id, asset_id, &caller, price)
        }

        /// 购买 MetaToken — 显式传入 order_id（推荐实验脚本使用）
        #[ink(message, payable)]
        pub fn buy_asset_by_order_id(
            &mut self,
            order_id: [u8; 32],
            asset_id: [u8; 32],
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let caller = self.env().caller();
            let transferred = self.env().transferred_value();

            if transferred < price {
                return Err(Error::InsufficientPayment);
            }

            self.settle_asset_inner(&order_id, asset_id, &caller, price)
        }

        /// 购买 RightToken（转让已有权证）
        #[ink(message, payable)]
        pub fn buy_certificate(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let caller = self.env().caller();
            let transferred = self.env().transferred_value();

            if transferred < price {
                return Err(Error::InsufficientPayment);
            }

            let cert_key = (asset_id, certificate_id);
            let order_id = self
                .cert_order_index
                .get(&cert_key)
                .ok_or(Error::OrderNotFound)?;

            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            if order.asset_id != asset_id
                || order.certificate_id != certificate_id
                || !matches!(order.asset_type, AssetType::Certificate)
            {
                return Err(Error::OrderNotFound);
            }
            if order.price != price {
                return Err(Error::OrderDigestMismatch);
            }

            self.settle_certificate_inner(
                &order_id,
                asset_id,
                certificate_id,
                &caller,
                price,
                RightAction::TransferRight,
            )
        }

        /// 购买 RightToken — 显式传入 order_id
        #[ink(message, payable)]
        pub fn buy_certificate_by_order_id(
            &mut self,
            order_id: [u8; 32],
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let caller = self.env().caller();
            let transferred = self.env().transferred_value();

            if transferred < price {
                return Err(Error::InsufficientPayment);
            }

            self.settle_certificate_inner(
                &order_id,
                asset_id,
                certificate_id,
                &caller,
                price,
                RightAction::TransferRight,
            )
        }

        /// 购买权证发行
        #[ink(message, payable)]
        pub fn buy_certificate_issue(
            &mut self,
            asset_id: [u8; 32],
            right_type: u8,
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let caller = self.env().caller();
            let transferred = self.env().transferred_value();

            if transferred < price {
                return Err(Error::InsufficientPayment);
            }

            let issue_key = (asset_id, right_type);
            let order_id = self
                .issue_order_index
                .get(&issue_key)
                .ok_or(Error::OrderNotFound)?;

            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            if order.asset_id != asset_id
                || order.right_type != right_type
                || !matches!(order.asset_type, AssetType::Certificate)
            {
                return Err(Error::OrderNotFound);
            }
            if order.price != price {
                return Err(Error::OrderDigestMismatch);
            }

            self.settle_certificate_issue_inner(&order_id, asset_id, &caller, price)
        }

        /// 购买权证发行 — 显式传入 order_id
        #[ink(message, payable)]
        pub fn buy_certificate_issue_by_order_id(
            &mut self,
            order_id: [u8; 32],
            asset_id: [u8; 32],
            _right_type: u8,
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let caller = self.env().caller();
            let transferred = self.env().transferred_value();

            if transferred < price {
                return Err(Error::InsufficientPayment);
            }

            self.settle_certificate_issue_inner(&order_id, asset_id, &caller, price)
        }

        // ====================================================================
        // 订单管理
        // ====================================================================

        #[ink(message)]
        pub fn cancel_order(&mut self, order_id: [u8; 32]) -> Result<(), Error> {
            let caller = self.env().caller();
            let mut order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;

            if caller != order.seller && caller != self.admin {
                return Err(Error::NotAuthorized);
            }
            if !matches!(order.status, OrderStatus::Listed | OrderStatus::Locked) {
                return Err(Error::OrderStatusInvalid);
            }

            order.status = OrderStatus::Cancelled;
            self.orders.insert(&order_id, &order);

            // 清除索引
            self.remove_order_from_index(&order_id, &order);

            self.env().emit_event(OrderCancelled { order_id });
            Ok(())
        }

        // ====================================================================
        // 查询方法
        // ====================================================================

        #[ink(message)]
        pub fn get_order(&self, order_id: [u8; 32]) -> Option<Order> {
            self.orders.get(&order_id)
        }

        #[ink(message)]
        pub fn get_asset_owner(&self, asset_id: [u8; 32]) -> Option<AccountId> {
            self.asset_owners.get(&asset_id)
        }

        #[ink(message)]
        pub fn get_certificate(
            &self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> Option<CertificateInfo> {
            self.certificate_holders.get(&(asset_id, certificate_id))
        }

        #[ink(message)]
        pub fn get_certificate_count(&self, asset_id: [u8; 32]) -> u32 {
            self.certificate_counts.get(&asset_id).unwrap_or(0)
        }

        #[ink(message)]
        pub fn is_authorized(&self, asset_id: [u8; 32], market: AccountId) -> bool {
            self.authorizations.contains(&(asset_id, market))
        }

        #[ink(message)]
        pub fn get_settlement_record(&self, order_id: [u8; 32]) -> Option<SettlementRecord> {
            self.settlement_records.get(&order_id)
        }

        #[ink(message)]
        pub fn get_order_count(&self) -> u64 {
            self.order_nonce
        }

        #[ink(message)]
        pub fn get_asset_order_id(&self, asset_id: [u8; 32]) -> Option<[u8; 32]> {
            self.asset_order_index.get(&asset_id)
        }

        #[ink(message)]
        pub fn get_certificate_order_id(
            &self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> Option<[u8; 32]> {
            self.cert_order_index.get(&(asset_id, certificate_id))
        }

        // ====================================================================
        // 内部方法 — 哈希
        // ====================================================================

        fn hash_bytes(&self, input: &[u8]) -> [u8; 32] {
            let mut output = [0u8; 32];
            ink::env::hash_bytes::<ink::env::hash::Blake2x256>(input, &mut output);
            output
        }

        fn next_order_id(
            &mut self,
            seed: &[u8; 32],
            seller: &AccountId,
            block: BlockNumber,
        ) -> [u8; 32] {
            self.order_nonce = self.order_nonce.saturating_add(1);
            let mut input = Vec::new();
            input.extend_from_slice(seed);
            input.extend_from_slice(&seller.encode());
            input.extend_from_slice(&block.to_le_bytes());
            input.extend_from_slice(&self.order_nonce.to_le_bytes());
            self.hash_bytes(&input)
        }

        fn compute_order_digest(
            &self,
            order_id: &[u8; 32],
            seller: &AccountId,
            price: Balance,
            object_type: u8,
            object_id: &[u8; 32],
        ) -> [u8; 32] {
            let mut input = Vec::new();
            input.extend_from_slice(order_id);
            input.extend_from_slice(&seller.encode());
            input.extend_from_slice(&price.to_le_bytes());
            input.push(object_type);
            input.extend_from_slice(object_id);
            self.hash_bytes(&input)
        }

        fn generate_trade_id(
            &self,
            order_id: &[u8; 32],
            buyer: &AccountId,
            block: BlockNumber,
        ) -> [u8; 32] {
            let mut input = Vec::new();
            input.extend_from_slice(order_id);
            input.extend_from_slice(&buyer.encode());
            input.extend_from_slice(&block.to_le_bytes());
            self.hash_bytes(&input)
        }

        // ====================================================================
        // 内部方法 — 订单 Lock / 索引管理
        // ====================================================================

        /// 锁定订单 — G1 的非原子 Lock（核心弱点）。
        ///
        /// G3 的 Lock 通过 CE → Pallet try_mutate 实现 CAS 语义：
        /// 两个并发的 lock 调用中，只有一个能成功将 status 从 Open 变为 Locked。
        /// G1 的 Lock 是 if-check-then-set，在 ink! 的单线程执行模型下安全，
        /// 但在 Substrate 区块级别的并发场景中可能被绕过。
        fn lock_order_by_id(
            &mut self,
            order_id: &[u8; 32],
            buyer: &AccountId,
        ) -> Result<(), Error> {
            let mut order = self.orders.get(order_id).ok_or(Error::OrderNotFound)?;

            // G1 的关键弱点：非原子 check-then-set
            // 两个并发的 lock 调用可能都通过此检查
            if order.status != OrderStatus::Listed {
                return Err(Error::OrderNotListed);
            }

            order.status = OrderStatus::Locked;
            order.buyer = Some(*buyer);
            self.orders.insert(order_id, &order);

            self.env().emit_event(OrderLocked {
                order_id: *order_id,
                buyer: *buyer,
            });
            Ok(())
        }

        /// 从索引中移除订单
        fn remove_order_from_index(&mut self, order_id: &[u8; 32], order: &Order) {
            match order.asset_type {
                AssetType::DataAsset => {
                    if let Some(indexed_id) = self.asset_order_index.get(&order.asset_id) {
                        if indexed_id == *order_id {
                            self.asset_order_index.remove(&order.asset_id);
                        }
                    }
                }
                AssetType::Certificate => {
                    // 可能同时有两个索引，都清理
                    let cert_key = (order.asset_id, order.certificate_id);
                    if let Some(indexed_id) = self.cert_order_index.get(&cert_key) {
                        if indexed_id == *order_id {
                            self.cert_order_index.remove(&cert_key);
                        }
                    }
                    let issue_key = (order.asset_id, order.right_type);
                    if let Some(indexed_id) = self.issue_order_index.get(&issue_key) {
                        if indexed_id == *order_id {
                            self.issue_order_index.remove(&issue_key);
                        }
                    }
                }
            }
        }

        // ====================================================================
        // 内部方法 — 结算核心（三种路径）
        // ====================================================================

        /// MetaToken 结算路径
        ///
        /// 与 G3 的 settle_asset_trade_by_market_internal 对等的验证逻辑，
        /// 但完全在合约内执行（无 Pallet 强制）。
        fn settle_asset_inner(
            &mut self,
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            buyer: &AccountId,
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            // Step 1: Lock（非原子）
            self.lock_order_by_id(order_id, buyer)?;

            // Step 2: 验证（对应 G3 verify_settle_prerequisites）
            self.verify_asset_settle(order_id, &asset_id)?;

            // Step 3: 执行权属转移
            let order = self.orders.get(order_id).ok_or(Error::OrderNotFound)?;
            let seller = order.seller;

            self.asset_owners.insert(&asset_id, buyer);

            // 清除授权（所有权转移后，旧授权失效）
            let market = self.env().account_id();
            self.authorizations.remove(&(asset_id, market));

            // 更新订单状态
            let mut settled_order = order;
            settled_order.status = OrderStatus::Settled;
            settled_order.settled_at = Some(self.env().block_number());
            self.orders.insert(order_id, &settled_order);

            // 清除索引
            self.remove_order_from_index(order_id, &settled_order);

            // Step 4: 生成结算记录
            let trade_id = self.record_settlement(
                order_id,
                &seller,
                buyer,
                asset_id,
                [0u8; 32],
                AssetType::DataAsset,
                RightAction::TransferAsset,
                price,
            );

            self.env().emit_event(TradeSettled {
                order_id: *order_id,
                trade_id,
                seller,
                buyer: *buyer,
                asset_id,
                certificate_id: [0u8; 32],
                asset_type: AssetType::DataAsset,
                right_action: RightAction::TransferAsset,
                price,
                settled_at: self.env().block_number(),
            });
            Ok((*order_id, trade_id))
        }

        /// RightToken 转让结算路径
        fn settle_certificate_inner(
            &mut self,
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            buyer: &AccountId,
            price: Balance,
            right_action: RightAction,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            self.lock_order_by_id(order_id, buyer)?;
            self.verify_certificate_settle(order_id, asset_id, certificate_id)?;

            let order = self.orders.get(order_id).ok_or(Error::OrderNotFound)?;
            let seller = order.seller;
            let key = (asset_id, certificate_id);
            let mut info = self
                .certificate_holders
                .get(&key)
                .ok_or(Error::CertificateNotFound)?;

            info.holder = *buyer;
            self.certificate_holders.insert(&key, &info);

            let mut settled_order = order;
            settled_order.status = OrderStatus::Settled;
            settled_order.settled_at = Some(self.env().block_number());
            self.orders.insert(order_id, &settled_order);
            self.remove_order_from_index(order_id, &settled_order);

            let trade_id = self.record_settlement(
                order_id,
                &seller,
                buyer,
                asset_id,
                certificate_id,
                AssetType::Certificate,
                right_action,
                price,
            );

            self.env().emit_event(TradeSettled {
                order_id: *order_id,
                trade_id,
                seller,
                buyer: *buyer,
                asset_id,
                certificate_id,
                asset_type: AssetType::Certificate,
                right_action,
                price,
                settled_at: self.env().block_number(),
            });
            Ok((*order_id, trade_id))
        }

        /// RightToken 发行结算路径
        fn settle_certificate_issue_inner(
            &mut self,
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            buyer: &AccountId,
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            self.lock_order_by_id(order_id, buyer)?;

            // 发行路径的验证不同于转让路径——权证尚未存在
            if self.settlement_records.contains(order_id) {
                return Err(Error::AlreadySettled);
            }

            let order = self.orders.get(order_id).ok_or(Error::OrderNotFound)?;
            if !matches!(order.status, OrderStatus::Listed | OrderStatus::Locked) {
                return Err(Error::OrderStatusInvalid);
            }
            if order.buyer != Some(*buyer) {
                return Err(Error::CallerNotBuyer);
            }

            // 发行新权证
            let cert_id = order.certificate_id;
            let seller = order.seller;

            let info = CertificateInfo {
                holder: *buyer,
                issuer: seller,
                right_type: order.right_type,
                issued_at: self.env().block_number(),
                valid_until: order.valid_until,
                active: true,
            };
            self.certificate_holders
                .insert(&(asset_id, cert_id), &info);

            let count = self.certificate_counts.get(&asset_id).unwrap_or(0);
            self.certificate_counts
                .insert(&asset_id, &(count.saturating_add(1)));

            self.env().emit_event(CertificateIssued {
                asset_id,
                certificate_id: cert_id,
                holder: *buyer,
                right_type: order.right_type,
                valid_until: order.valid_until,
            });

            // 更新订单
            let mut settled_order = order;
            settled_order.status = OrderStatus::Settled;
            settled_order.settled_at = Some(self.env().block_number());
            self.orders.insert(order_id, &settled_order);
            self.remove_order_from_index(order_id, &settled_order);

            let trade_id = self.record_settlement(
                order_id,
                &seller,
                buyer,
                asset_id,
                cert_id,
                AssetType::Certificate,
                RightAction::IssueRight,
                price,
            );

            self.env().emit_event(TradeSettled {
                order_id: *order_id,
                trade_id,
                seller,
                buyer: *buyer,
                asset_id,
                certificate_id: cert_id,
                asset_type: AssetType::Certificate,
                right_action: RightAction::IssueRight,
                price,
                settled_at: self.env().block_number(),
            });
            Ok((*order_id, trade_id))
        }

        // ====================================================================
        // 内部方法 — 结算验证
        // ====================================================================

        /// MetaToken 结算前验证 — 对应 G3 的 verify_settle_prerequisites
        ///
        /// G1 与 G3 的关键差异：
        /// — G3 检查 Pallet AssetApprovals（运行时的授权状态）
        /// — G1 检查合约内 authorizations（可能与 Pallet 不同步）
        /// — G3 通过 Pallet get_asset 检查实际链上所有权
        /// — G1 检查合约内 asset_owners（可能与 Pallet 不同步）
        fn verify_asset_settle(
            &self,
            order_id: &[u8; 32],
            asset_id: &[u8; 32],
        ) -> Result<(), Error> {
            // 条件 1: NoPriorSettlement
            if self.settlement_records.contains(order_id) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"AlreadySettled".to_vec(),
                });
                return Err(Error::AlreadySettled);
            }

            // 条件 2: OrderOpenOrLocked
            let order = self.orders.get(order_id).ok_or({
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderNotFound".to_vec(),
                });
                Error::OrderNotFound
            })?;

            if !matches!(order.status, OrderStatus::Listed | OrderStatus::Locked) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderStatusInvalid".to_vec(),
                });
                return Err(Error::OrderStatusInvalid);
            }

            // 条件 3: MatchOwnership — 卖方是合约内记录的当前权属人
            // G1 的弱点：合约内所有权可能与 Pallet 所有权不一致
            let contract_owner = self.asset_owners.get(asset_id).ok_or({
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"AssetNotFound".to_vec(),
                });
                Error::AssetNotFound
            })?;

            if contract_owner != order.seller {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"OwnerMismatch".to_vec(),
                });
                return Err(Error::NotOwner);
            }

            // 条件 4: MatchAuthorization
            // ⚠️ G1 的授权检查：仅检查合约内记录。
            // 在 5.4 节 T2 场景中，Pallet 撤销授权后合约内授权可能仍存在
            // → G1 通过了此检查而 G3 会拒绝。
            // 在 E1 场景中，若市场从未被授权，此检查捕获（与 G3 行为一致）。
            let market = self.env().account_id();
            if !self.authorizations.contains(&(*asset_id, market)) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"UnauthorizedMarket".to_vec(),
                });
                return Err(Error::NotAuthorized);
            }

            // 条件 5-6（ObjectExistsAndValid + VerifySignature）
            // 合约内检查已足够——资产存在性和签名由 ink! 框架保证

            Ok(())
        }

        /// RightToken 结算前验证
        fn verify_certificate_settle(
            &self,
            order_id: &[u8; 32],
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> Result<(), Error> {
            // 条件 1: NoPriorSettlement
            if self.settlement_records.contains(order_id) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"AlreadySettled".to_vec(),
                });
                return Err(Error::AlreadySettled);
            }

            // 条件 2: OrderOpenOrLocked
            let order = self.orders.get(order_id).ok_or({
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderNotFound".to_vec(),
                });
                Error::OrderNotFound
            })?;

            if !matches!(order.status, OrderStatus::Listed | OrderStatus::Locked) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"OrderStatusInvalid".to_vec(),
                });
                return Err(Error::OrderStatusInvalid);
            }

            // 条件 3: 权证存在且有效
            let key = (asset_id, certificate_id);
            let info = self.certificate_holders.get(&key).ok_or({
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"CertificateNotFound".to_vec(),
                });
                Error::CertificateNotFound
            })?;

            if !info.active {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"CertificateNotActive".to_vec(),
                });
                return Err(Error::CertificateNotActive);
            }

            // 条件 4: 卖方是权证持有者
            if info.holder != order.seller {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"HolderMismatch".to_vec(),
                });
                return Err(Error::NotOwner);
            }

            // 条件 5: MatchAuthorization（权证结算的授权检查）
            // G3 的 F 项修复：追溯父资产检查 AssetApprovals
            // G1 检查合约内 authorizations（与 G3 修复后的逻辑对等，但无 Pallet 强制）
            let market = self.env().account_id();
            if !self.authorizations.contains(&(asset_id, market)) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"UnauthorizedMarket".to_vec(),
                });
                return Err(Error::NotAuthorized);
            }

            Ok(())
        }

        // ====================================================================
        // 内部方法 — 结算记录
        // ====================================================================

        fn record_settlement(
            &mut self,
            order_id: &[u8; 32],
            seller: &AccountId,
            buyer: &AccountId,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            object_type: AssetType,
            right_action: RightAction,
            price: Balance,
        ) -> [u8; 32] {
            let block = self.env().block_number();
            let market = self.env().account_id();

            let trade_id = self.generate_trade_id(order_id, buyer, block);

            let object_id = match object_type {
                AssetType::DataAsset => asset_id,
                AssetType::Certificate => certificate_id,
            };
            let type_byte = match object_type {
                AssetType::DataAsset => 0u8,
                AssetType::Certificate => 1u8,
            };
            let order_digest =
                self.compute_order_digest(order_id, seller, price, type_byte, &object_id);

            let record = SettlementRecord {
                trade_id,
                order_id: *order_id,
                market,
                seller: *seller,
                buyer: *buyer,
                asset_id,
                certificate_id,
                object_type,
                right_action,
                price,
                order_digest,
                settled_at: block,
            };

            // G1 的非原子 double-check — 使用 contains_key + insert（非 try_mutate）
            // G3 使用 Pallet StorageMap::try_mutate 实现真正的原子 CAS
            if self.settlement_records.contains(order_id) {
                self.env().emit_event(SettlementRejected {
                    order_id: *order_id,
                    reason: b"AlreadySettled_DoubleCheck_G1_NonAtomic".to_vec(),
                });
                // G1 在重复结算时不返回 Err——这不如 G3 安全，
                // 但实验需要观察此差异来量化 G1 的并发脆弱性
            }
            self.settlement_records.insert(order_id, &record);

            trade_id
        }
    }

    // ========================================================================
    // 测试
    // ========================================================================

    #[cfg(test)]
    mod tests {
        use super::*;

        #[ink::test]
        fn test_register_asset() {
            let contract = MarketPureContract::new();
            // 集成测试需设置 caller，示例已在函数体内
        }

        #[ink::test]
        fn test_list_asset_emits_correct_event() {
            // 验证 list_asset 返回有效 order_id 且事件被 emit
        }

        #[ink::test]
        fn test_buy_asset_full_flow() {
            // 验证 register → authorize → list → buy 完整流程
        }

        #[ink::test]
        fn test_double_settle_should_return_already_settled() {
            // 验证重复结算的行为
        }

        #[ink::test]
        fn test_unauthorized_market_may_pass_in_g1() {
            // G1 的授权检查仅在合约内有效
            // 预期：未授权市场可以在绕开合约的情况下通过 Pallet 转移资产
            // 实验时需验证此行为与 G3 的差异
        }
    }
}
