#![cfg_attr(not(feature = "std"), no_std, no_main)]

// `market_standard`被打包成了一个crate
use market_standard::{DataAssetsExtError, MarketStandard};

#[ink::contract(env = market_standard::CustomEnvironment)]
mod market_orderbook {
    use super::*;
    use codec::Encode;
    use ink::env::hash::Blake2x256;
    use ink::storage::Mapping;

    /// 订单资产类型
    #[derive(codec::Decode, codec::Encode, Debug, Clone, Copy, PartialEq, Eq)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum AssetType {
        DataAsset,
        Certificate,
    }

    /// 订单状态
    #[derive(codec::Decode, codec::Encode, Debug, Clone, Copy, PartialEq, Eq)]
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

    /// 订单信息
    #[derive(codec::Decode, codec::Encode, Debug, Clone)]
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
        pub order_id: Option<[u8; 32]>,
    }

    #[ink(storage)]
    pub struct MarketOrderbook {
        /// 资产ID -> 订单详情
        orders: Mapping<[u8; 32], Order>,
        /// (资产ID, 权证ID) -> 订单详情
        certificate_orders: Mapping<([u8; 32], [u8; 32]), Order>,
        /// 市场费率 (Basis Points)
        fee_ratio: u32,
        /// 管理员
        admin: AccountId,
    }

    /// 定义事件
    #[ink(event)]
    pub struct AssetListed {
        #[ink(topic)]
        asset_id: [u8; 32],
        seller: AccountId,
        price: Balance,
    }

    #[ink(event)]
    pub struct AssetSold {
        #[ink(topic)]
        asset_id: [u8; 32],
        buyer: AccountId,
        price: Balance,
    }

    #[ink(event)]
    pub struct AssetWithdrawn {
        #[ink(topic)]
        asset_id: [u8; 32],
        owner: AccountId,
    }

    #[ink(event)]
    pub struct CertificateListed {
        #[ink(topic)]
        asset_id: [u8; 32],
        #[ink(topic)]
        certificate_id: [u8; 32],
        seller: AccountId,
        price: Balance,
    }

    #[ink(event)]
    pub struct CertificateSold {
        #[ink(topic)]
        asset_id: [u8; 32],
        #[ink(topic)]
        certificate_id: [u8; 32],
        buyer: AccountId,
        price: Balance,
    }

    #[derive(Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    #[allow(clippy::cast_possible_truncation)]
    pub enum Error {
        /// 调用链扩展失败
        ChainExtension(DataAssetsExtError),
        /// 资产已存在
        AssetAlreadyListed,
        /// 资产不存在
        AssetNotFound,
        /// 权限不足
        NotOwner,
        /// 资金不足
        InsufficientPayment,
        /// 转账失败
        TransferFailed,
        /// 订单状态不允许当前操作
        InvalidOrderStatus,
    }

    // 将链扩展错误转换为合约错误
    impl From<DataAssetsExtError> for Error {
        fn from(e: DataAssetsExtError) -> Self {
            Error::ChainExtension(e)
        }
    }

    impl MarketOrderbook {
        #[ink(constructor)]
        pub fn new(fee_ratio: u32) -> Self {
            Self {
                orders: Mapping::default(),
                certificate_orders: Mapping::default(),
                fee_ratio,
                admin: Self::env().caller(),
            }
        }

        /// 生成订单投影所需的 order_id 和 order_digest，
        /// 并通过 Chain Extension 写入运行时侧 MarketOrders 存储。
        fn create_order_projection(
            &self,
            object_type: u8,
            object_id: [u8; 32],
            parent_asset_id: Option<[u8; 32]>,
            seller: AccountId,
            price: Balance,
        ) -> Result<([u8; 32], [u8; 32]), Error> {
            let block_number = self.env().block_number();

            // order_id = blake2_256(asset_id/cert_id || seller || block_number)
            let mut order_id_input = object_id.to_vec();
            order_id_input.extend_from_slice(&seller.encode());
            order_id_input.extend_from_slice(&block_number.to_le_bytes());
            let mut order_id = [0u8; 32];
            ink::env::hash_bytes::<Blake2x256>(&order_id_input, &mut order_id);

            // order_digest = blake2_256(order_id || seller || price || object_type || object_id)
            let mut digest_input = order_id.to_vec();
            digest_input.extend_from_slice(&seller.encode());
            digest_input.extend_from_slice(&price.to_le_bytes());
            digest_input.push(object_type);
            digest_input.extend_from_slice(&object_id);
            let mut order_digest = [0u8; 32];
            ink::env::hash_bytes::<Blake2x256>(&digest_input, &mut order_digest);

            self.env()
                .extension()
                .create_order_projection(
                    order_id,
                    order_digest,
                    object_type,
                    object_id,
                    parent_asset_id,
                    seller,
                    price,
                )
                .map_err(|e| Error::ChainExtension(e))?;

            Ok((order_id, order_digest))
        }

        /// 从订单字段重新计算 order_digest，供结算时传入运行时验证。
        fn compute_order_digest(&self, order: &Order) -> [u8; 32] {
            let oid = order.order_id.unwrap_or([0u8; 32]);
            let object_type = match order.asset_type {
                AssetType::DataAsset => 0u8,
                AssetType::Certificate => 1u8,
            };
            let object_id = if order.asset_type == AssetType::Certificate && order.certificate_id != [0u8; 32] {
                order.certificate_id
            } else {
                order.asset_id
            };
            let mut digest_input = oid.to_vec();
            digest_input.extend_from_slice(&order.seller.encode());
            digest_input.extend_from_slice(&order.price.to_le_bytes());
            digest_input.push(object_type);
            digest_input.extend_from_slice(&object_id);
            let mut order_digest = [0u8; 32];
            ink::env::hash_bytes::<Blake2x256>(&digest_input, &mut order_digest);
            order_digest
        }

        /// 【非标准接口】用户必须先调用此方法设置价格，然后调用 standard 的 asset_enter
        /// 或者在此方法内部调用 asset_enter 逻辑
        #[ink(message)]
        pub fn list_asset(&mut self, asset_id: [u8; 32], price: Balance) -> Result<(), Error> {
            if self.orders.contains(asset_id) {
                return Err(Error::AssetAlreadyListed);
            }

            // 注意：在实际场景中，Runtime 应该先确保资产已经转入合约账户 (escrow)
            // 只有资产在合约名下，合约才能在未来调用 transfer_asset 转出它

            let caller = self.env().caller();

            // 记录订单
            let mut order = Order {
                seller: caller,
                buyer: None,
                price,
                asset_type: AssetType::DataAsset,
                asset_id,
                certificate_id: [0u8; 32],
                right_type: 0,
                valid_until: None,
                status: OrderStatus::Listed,
                created_at: self.env().block_number(),
                settled_at: None,
                order_id: None,
            };
            self.orders.insert(asset_id, &order);

            // 写入运行时侧订单投影
            let (order_id, _order_digest) = self.create_order_projection(
                0u8,         // DataAsset
                asset_id,
                None,        // no parent for DataAsset
                caller,
                price,
            )?;

            // 保存 order_id 到合约端订单
            order.order_id = Some(order_id);
            self.orders.insert(asset_id, &order);

            // 触发标准中的进入逻辑（如果需要额外的状态变更是写在这里）
            self.asset_enter(asset_id);

            self.env().emit_event(AssetListed {
                asset_id,
                seller: caller,
                price,
            });

            Ok(())
        }

        /// 买家支付 native token，合约将 Asset 通过 Extension 转给买家
        #[ink(message, payable)]
        pub fn buy_asset(&mut self, asset_id: [u8; 32]) -> Result<(), Error> {
            let mut order = self.orders.get(asset_id).ok_or(Error::AssetNotFound)?;
            let caller = self.env().caller();
            let transferred_val = self.env().transferred_value();

            if order.status != OrderStatus::Listed {
                return Err(Error::InvalidOrderStatus);
            }

            if transferred_val < order.price {
                return Err(Error::InsufficientPayment);
            }

            order.status = OrderStatus::Locked;
            order.buyer = Some(caller);
            self.orders.insert(asset_id, &order);

            // 运行时侧 Lock（使用合约端存储的 order_id）
            if let Some(oid) = order.order_id {
                if let Err(e) = self.env().extension().lock_order(oid) {
                    order.status = OrderStatus::Listed;
                    order.buyer = None;
                    self.orders.insert(asset_id, &order);
                    return Err(Error::ChainExtension(e));
                }
            }

            // 1. 先调用 Chain Extension 转移资产给买家，避免资产转移失败后仍给卖家付款。
            // 合约 (Self) -> 买家 (Caller)
            let order_id = order.order_id.unwrap_or([0u8; 32]);
            let order_digest = self.compute_order_digest(&order);
            if let Err(e) = self
                .env()
                .extension()
                .settle_asset_trade(asset_id, caller, order.price, order_id, order_digest)
            {
                order.status = OrderStatus::Failed;
                order.settled_at = None;
                self.orders.insert(asset_id, &order);
                return Err(Error::ChainExtension(e));
            }

            // 2. 给卖家转钱 (Native Token)
            if self.env().transfer(order.seller, order.price).is_err() {
                order.status = OrderStatus::Failed;
                order.settled_at = None;
                self.orders.insert(asset_id, &order);
                return Err(Error::TransferFailed);
            }

            // 3. 保留已结算订单用于后续统计和审计。
            order.status = OrderStatus::Settled;
            order.settled_at = Some(self.env().block_number());
            self.orders.insert(asset_id, &order);

            // 运行时侧更新订单状态为 Settled
            if let Some(oid) = order.order_id {
                let _ = self.env().extension().update_order_status(oid, 2u8); // 2=Settled
            }

            // 4. 报告交易结果 (Standard Trait)
            // 现在生成一个假的 trade_id 用于演示
            let trade_id = [1u8; 32];
            self.report_trade_result(trade_id, true);

            self.env().emit_event(AssetSold {
                asset_id,
                buyer: caller,
                price: order.price,
            });

            Ok(())
        }

        /// 上架权证订单。权证应先由卖方转入市场合约账户托管。
        #[ink(message)]
        pub fn list_certificate(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
            price: Balance,
        ) -> Result<(), Error> {
            let order_key = (asset_id, certificate_id);
            if self.certificate_orders.contains(order_key) {
                return Err(Error::AssetAlreadyListed);
            }

            let seller = self.env().caller();
            let mut order = Order {
                seller,
                buyer: None,
                price,
                asset_type: AssetType::Certificate,
                asset_id,
                certificate_id,
                right_type: 0,
                valid_until: None,
                status: OrderStatus::Listed,
                created_at: self.env().block_number(),
                settled_at: None,
                order_id: None,
            };
            self.certificate_orders.insert(order_key, &order);

            // 写入运行时侧订单投影
            let (order_id, _order_digest) = self.create_order_projection(
                1u8,                // Certificate
                certificate_id,
                Some(asset_id),     // parent asset
                seller,
                price,
            )?;
            order.order_id = Some(order_id);
            self.certificate_orders.insert(order_key, &order);

            self.env().emit_event(CertificateListed {
                asset_id,
                certificate_id,
                seller,
                price,
            });

            Ok(())
        }

        /// 上架发行型权证订单。购买时由市场合约触发 Runtime 直接给买方发行权证。
        #[ink(message)]
        pub fn list_certificate_issue(
            &mut self,
            asset_id: [u8; 32],
            right_type: u8,
            valid_until: Option<u64>,
            price: Balance,
        ) -> Result<(), Error> {
            if self.orders.contains(asset_id) {
                return Err(Error::AssetAlreadyListed);
            }

            let seller = self.env().caller();
            let mut order = Order {
                seller,
                buyer: None,
                price,
                asset_type: AssetType::Certificate,
                asset_id,
                certificate_id: [0u8; 32],
                right_type,
                valid_until,
                status: OrderStatus::Listed,
                created_at: self.env().block_number(),
                settled_at: None,
                order_id: None,
            };
            self.orders.insert(asset_id, &order);

            // 写入运行时侧订单投影
            let (order_id, _order_digest) = self.create_order_projection(
                1u8,                // Certificate
                asset_id,           // 发行型订单以 asset_id 为 object_id（权证尚未创建）
                Some(asset_id),
                seller,
                price,
            )?;
            order.order_id = Some(order_id);
            self.orders.insert(asset_id, &order);

            self.env().emit_event(CertificateListed {
                asset_id,
                certificate_id: [0u8; 32],
                seller,
                price,
            });

            Ok(())
        }

        /// 购买发行型权证，Runtime 在结算时直接为买方发行权证。
        #[ink(message, payable)]
        pub fn buy_certificate_issue(&mut self, asset_id: [u8; 32]) -> Result<(), Error> {
            let mut order = self.orders.get(asset_id).ok_or(Error::AssetNotFound)?;
            let buyer = self.env().caller();
            let transferred_val = self.env().transferred_value();

            if order.status != OrderStatus::Listed || order.asset_type != AssetType::Certificate {
                return Err(Error::InvalidOrderStatus);
            }

            if transferred_val < order.price {
                return Err(Error::InsufficientPayment);
            }

            order.status = OrderStatus::Locked;
            order.buyer = Some(buyer);
            self.orders.insert(asset_id, &order);

            // 运行时侧 Lock
            if let Some(oid) = order.order_id {
                if let Err(e) = self.env().extension().lock_order(oid) {
                    order.status = OrderStatus::Listed;
                    order.buyer = None;
                    self.orders.insert(asset_id, &order);
                    return Err(Error::ChainExtension(e));
                }
            }

            if let Err(e) = self.env().extension().issue_certificate(
                asset_id,
                order.seller,
                buyer,
                order.right_type,
                order.valid_until,
            ) {
                order.status = OrderStatus::Failed;
                order.settled_at = None;
                self.orders.insert(asset_id, &order);
                return Err(Error::ChainExtension(e));
            }

            if self.env().transfer(order.seller, order.price).is_err() {
                order.status = OrderStatus::Failed;
                order.settled_at = None;
                self.orders.insert(asset_id, &order);
                return Err(Error::TransferFailed);
            }

            order.status = OrderStatus::Settled;
            order.settled_at = Some(self.env().block_number());
            self.orders.insert(asset_id, &order);
            if let Some(oid) = order.order_id {
                let _ = self.env().extension().update_order_status(oid, 2u8);
            }
            self.report_trade_result([3u8; 32], true);

            self.env().emit_event(CertificateSold {
                asset_id,
                certificate_id: [0u8; 32],
                buyer,
                price: order.price,
            });

            Ok(())
        }

        /// 买家支付 native token，合约将权证通过 Extension 转给买家。
        #[ink(message, payable)]
        pub fn buy_certificate(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> Result<(), Error> {
            let order_key = (asset_id, certificate_id);
            let mut order = self
                .certificate_orders
                .get(order_key)
                .ok_or(Error::AssetNotFound)?;
            let buyer = self.env().caller();
            let transferred_val = self.env().transferred_value();

            if order.status != OrderStatus::Listed {
                return Err(Error::InvalidOrderStatus);
            }

            if transferred_val < order.price {
                return Err(Error::InsufficientPayment);
            }

            order.status = OrderStatus::Locked;
            order.buyer = Some(buyer);
            self.certificate_orders.insert(order_key, &order);

            // 运行时侧 Lock
            if let Some(oid) = order.order_id {
                if let Err(e) = self.env().extension().lock_order(oid) {
                    order.status = OrderStatus::Listed;
                    order.buyer = None;
                    self.certificate_orders.insert(order_key, &order);
                    return Err(Error::ChainExtension(e));
                }
            }

            let order_id = order.order_id.unwrap_or([0u8; 32]);
            let order_digest = self.compute_order_digest(&order);
            if let Err(e) = self.env().extension().settle_certificate_trade(
                asset_id,
                certificate_id,
                buyer,
                order.price,
                order_id,
                order_digest,
            ) {
                order.status = OrderStatus::Failed;
                order.settled_at = None;
                self.certificate_orders.insert(order_key, &order);
                return Err(Error::ChainExtension(e));
            }

            if self.env().transfer(order.seller, order.price).is_err() {
                order.status = OrderStatus::Failed;
                order.settled_at = None;
                self.certificate_orders.insert(order_key, &order);
                return Err(Error::TransferFailed);
            }

            order.status = OrderStatus::Settled;
            order.settled_at = Some(self.env().block_number());
            self.certificate_orders.insert(order_key, &order);
            if let Some(oid) = order.order_id {
                let _ = self.env().extension().update_order_status(oid, 2u8);
            }
            self.report_trade_result([2u8; 32], true);

            self.env().emit_event(CertificateSold {
                asset_id,
                certificate_id,
                buyer,
                price: order.price,
            });

            Ok(())
        }

        /// 卖方撤销资产订单并通过链扩展取回托管资产。
        #[ink(message)]
        pub fn cancel_asset_order(&mut self, asset_id: [u8; 32]) -> Result<(), Error> {
            let mut order = self.orders.get(asset_id).ok_or(Error::AssetNotFound)?;
            let caller = self.env().caller();

            if order.seller != caller {
                return Err(Error::NotOwner);
            }

            if order.status != OrderStatus::Listed {
                return Err(Error::InvalidOrderStatus);
            }

            self.env().extension().transfer_asset(asset_id, caller)?;

            order.status = OrderStatus::Cancelled;
            self.orders.insert(asset_id, &order);

            self.env().emit_event(AssetWithdrawn {
                asset_id,
                owner: caller,
            });

            Ok(())
        }

        /// 卖方撤销权证订单并通过链扩展取回托管权证。
        #[ink(message)]
        pub fn cancel_certificate_order(
            &mut self,
            asset_id: [u8; 32],
            certificate_id: [u8; 32],
        ) -> Result<(), Error> {
            let order_key = (asset_id, certificate_id);
            let mut order = self
                .certificate_orders
                .get(order_key)
                .ok_or(Error::AssetNotFound)?;
            let caller = self.env().caller();

            if order.seller != caller {
                return Err(Error::NotOwner);
            }

            if order.status != OrderStatus::Listed {
                return Err(Error::InvalidOrderStatus);
            }

            self.env()
                .extension()
                .transfer_certificate(asset_id, certificate_id, caller)?;

            order.status = OrderStatus::Cancelled;
            self.certificate_orders.insert(order_key, &order);

            Ok(())
        }
    }

    /// 实现 MarketStandard Trait
    impl MarketStandard for MarketOrderbook {
        #[ink(message)]
        fn is_assetx_market(&self) -> bool {
            true
        }

        #[ink(message)]
        fn get_market_type(&self) -> u8 {
            0 // 0 代表 OrderBook
        }

        #[ink(message)]
        fn get_fee_ratio(&self) -> u32 {
            self.fee_ratio
        }

        #[ink(message)]
        fn check_admission(&self, _asset_id: [u8; 32]) -> bool {
            // 简单实现：允许所有资产
            true
        }

        #[ink(message)]
        fn can_list_asset(&self, asset_id: [u8; 32], _owner: AccountId) -> bool {
            // 如果订单表中不存在，则可以上架
            !self.orders.contains(asset_id)
        }

        #[ink(message)]
        fn asset_enter(&mut self, asset_id: [u8; 32]) {
            // 在 list_asset 中处理了主要逻辑
            // 这里可以做一些额外的统计或状态标记
            ink::env::debug_println!("Asset {:?} entered the market", asset_id);
        }

        #[ink(message)]
        fn asset_leave(&mut self, asset_id: [u8; 32]) {
            if let Err(e) = self.cancel_asset_order(asset_id) {
                ink::env::debug_println!("Asset leave failed: {:?}", e);
            }
        }

        #[ink(message)]
        fn report_trade_result(&mut self, trade_id: [u8; 32], success: bool) {
            ink::env::debug_println!("Trade {:?} finished. Success: {}", trade_id, success);
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use codec::Decode;
        use std::cell::RefCell;

        thread_local! {
            static RECORDED_ASSET_TRANSFER: RefCell<Option<(u16, [u8; 32], AccountId)>> =
                RefCell::new(None);
            static RECORDED_CERTIFICATE_TRANSFER: RefCell<Option<(u16, [u8; 32], [u8; 32], AccountId)>> =
                RefCell::new(None);
            static RECORDED_ASSET_SETTLEMENT: RefCell<Option<(u16, [u8; 32], AccountId, Balance)>> =
                RefCell::new(None);
            static RECORDED_CERTIFICATE_SETTLEMENT: RefCell<Option<(u16, [u8; 32], [u8; 32], AccountId, Balance)>> =
                RefCell::new(None);
            static RECORDED_CERTIFICATE_ISSUE: RefCell<
                Option<(u16, [u8; 32], AccountId, AccountId, u8, Option<u64>)>,
            > = RefCell::new(None);
        }

        struct FailingTransferExtension;

        impl ink::env::test::ChainExtension for FailingTransferExtension {
            fn ext_id(&self) -> u16 {
                market_standard::DATA_ASSETS_EXT_ID as u16
            }

            fn call(&mut self, func_id: u16, _input: &[u8], _output: &mut Vec<u8>) -> u32 {
                assert!(
                    func_id == market_standard::TRANSFER_ASSET_FUNC_ID as u16
                        || func_id == market_standard::TRANSFER_CERT_FUNC_ID as u16
                        || func_id == market_standard::ISSUE_CERT_FUNC_ID as u16
                        || func_id == market_standard::SETTLE_ASSET_TRADE_FUNC_ID as u16
                        || func_id == market_standard::SETTLE_CERT_TRADE_FUNC_ID as u16
                        || func_id == market_standard::CREATE_ORDER_PROJECTION_FUNC_ID as u16
                        || func_id == market_standard::LOCK_ORDER_FUNC_ID as u16
                        || func_id == market_standard::UPDATE_ORDER_STATUS_FUNC_ID as u16
                );
                DataAssetsExtError::TransferFailed as u32
            }
        }

        struct RecordingTransferExtension;

        impl ink::env::test::ChainExtension for RecordingTransferExtension {
            fn ext_id(&self) -> u16 {
                market_standard::DATA_ASSETS_EXT_ID as u16
            }

            fn call(&mut self, func_id: u16, input: &[u8], _output: &mut Vec<u8>) -> u32 {
                let payload = Vec::<u8>::decode(&mut &input[..]).expect("encoded input payload");
                if func_id == market_standard::TRANSFER_ASSET_FUNC_ID as u16 {
                    let (asset_id, buyer) = <([u8; 32], AccountId)>::decode(&mut &payload[..])
                        .expect("valid asset transfer input");
                    RECORDED_ASSET_TRANSFER.with(|recorded| {
                        *recorded.borrow_mut() = Some((func_id, asset_id, buyer));
                    });
                } else if func_id == market_standard::SETTLE_ASSET_TRADE_FUNC_ID as u16 {
                    let (asset_id, buyer, price, order_id, order_digest) =
                        <([u8; 32], AccountId, Balance, [u8; 32], [u8; 32])>::decode(&mut &payload[..])
                            .expect("valid asset settlement input");
                    RECORDED_ASSET_SETTLEMENT.with(|recorded| {
                        *recorded.borrow_mut() = Some((func_id, asset_id, buyer, price));
                    });
                } else if func_id == market_standard::TRANSFER_CERT_FUNC_ID as u16 {
                    let (asset_id, certificate_id, buyer) =
                        <([u8; 32], [u8; 32], AccountId)>::decode(&mut &payload[..])
                            .expect("valid certificate transfer input");
                    RECORDED_CERTIFICATE_TRANSFER.with(|recorded| {
                        *recorded.borrow_mut() = Some((func_id, asset_id, certificate_id, buyer));
                    });
                } else if func_id == market_standard::SETTLE_CERT_TRADE_FUNC_ID as u16 {
                    let (asset_id, certificate_id, buyer, price, order_id, order_digest) =
                        <([u8; 32], [u8; 32], AccountId, Balance, [u8; 32], [u8; 32])>::decode(&mut &payload[..])
                            .expect("valid certificate settlement input");
                    RECORDED_CERTIFICATE_SETTLEMENT.with(|recorded| {
                        *recorded.borrow_mut() =
                            Some((func_id, asset_id, certificate_id, buyer, price));
                    });
                } else if func_id == market_standard::ISSUE_CERT_FUNC_ID as u16 {
                    let (asset_id, issuer, buyer, right_type, valid_until) =
                        <([u8; 32], AccountId, AccountId, u8, Option<u64>)>::decode(
                            &mut &payload[..],
                        )
                        .expect("valid certificate issue input");
                    RECORDED_CERTIFICATE_ISSUE.with(|recorded| {
                        *recorded.borrow_mut() =
                            Some((func_id, asset_id, issuer, buyer, right_type, valid_until));
                    });
                } else if func_id == market_standard::CREATE_ORDER_PROJECTION_FUNC_ID as u16
                    || func_id == market_standard::LOCK_ORDER_FUNC_ID as u16
                    || func_id == market_standard::UPDATE_ORDER_STATUS_FUNC_ID as u16
                {
                    // 新增投影/锁/更新方法，直接成功
                } else {
                    panic!("unexpected function id {func_id}");
                }
                0
            }
        }

        #[ink::test]
        fn failed_asset_transfer_keeps_payment_and_marks_order_failed() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(FailingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [7u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            let seller_balance_before =
                ink::env::test::get_account_balance::<market_standard::CustomEnvironment>(seller)
                    .unwrap();

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(
                market.buy_asset(asset_id),
                Err(Error::ChainExtension(DataAssetsExtError::TransferFailed))
            );

            let seller_balance_after =
                ink::env::test::get_account_balance::<market_standard::CustomEnvironment>(seller)
                    .unwrap();
            assert_eq!(seller_balance_after, seller_balance_before);
            let order = market
                .orders
                .get(asset_id)
                .expect("failed asset order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Failed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn buy_asset_calls_transfer_asset_extension_with_asset_and_buyer() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            RECORDED_ASSET_SETTLEMENT.with(|recorded| *recorded.borrow_mut() = None);
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [8u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(market.buy_asset(asset_id), Ok(()));

            RECORDED_ASSET_SETTLEMENT.with(|recorded| {
                assert_eq!(
                    *recorded.borrow(),
                    Some((
                        market_standard::SETTLE_ASSET_TRADE_FUNC_ID as u16,
                        asset_id,
                        buyer,
                        price
                    ))
                );
            });
            let order = market
                .orders
                .get(asset_id)
                .expect("settled asset order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Settled);
            assert!(order.settled_at.is_some());
        }

        #[ink::test]
        fn buy_certificate_calls_transfer_certificate_extension_with_certificate_and_buyer() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            RECORDED_CERTIFICATE_SETTLEMENT.with(|recorded| *recorded.borrow_mut() = None);
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [9u8; 32];
            let certificate_id = [10u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate(asset_id, certificate_id, price),
                Ok(())
            );

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(market.buy_certificate(asset_id, certificate_id), Ok(()));

            RECORDED_CERTIFICATE_SETTLEMENT.with(|recorded| {
                assert_eq!(
                    *recorded.borrow(),
                    Some((
                        market_standard::SETTLE_CERT_TRADE_FUNC_ID as u16,
                        asset_id,
                        certificate_id,
                        buyer,
                        price
                    ))
                );
            });
            let order = market
                .certificate_orders
                .get((asset_id, certificate_id))
                .expect("settled certificate order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Settled);
            assert!(order.settled_at.is_some());
        }

        #[ink::test]
        fn failed_certificate_transfer_keeps_payment_and_marks_order_failed() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(FailingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [11u8; 32];
            let certificate_id = [12u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate(asset_id, certificate_id, price),
                Ok(())
            );

            let seller_balance_before =
                ink::env::test::get_account_balance::<market_standard::CustomEnvironment>(seller)
                    .unwrap();

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(
                market.buy_certificate(asset_id, certificate_id),
                Err(Error::ChainExtension(DataAssetsExtError::TransferFailed))
            );

            let seller_balance_after =
                ink::env::test::get_account_balance::<market_standard::CustomEnvironment>(seller)
                    .unwrap();
            assert_eq!(seller_balance_after, seller_balance_before);
            let order = market
                .certificate_orders
                .get((asset_id, certificate_id))
                .expect("failed certificate order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Failed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn listed_asset_order_records_full_state() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            let seller = accounts.bob;
            let asset_id = [13u8; 32];
            let price = 100;

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            let order = market.orders.get(asset_id).expect("asset order exists");
            assert_eq!(order.seller, seller);
            assert_eq!(order.buyer, None);
            assert_eq!(order.price, price);
            assert_eq!(order.asset_type, AssetType::DataAsset);
            assert_eq!(order.asset_id, asset_id);
            assert_eq!(order.certificate_id, [0u8; 32]);
            assert_eq!(order.right_type, 0);
            assert_eq!(order.valid_until, None);
            assert_eq!(order.status, OrderStatus::Listed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn listed_certificate_order_records_full_state() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            let seller = accounts.bob;
            let asset_id = [14u8; 32];
            let certificate_id = [15u8; 32];
            let price = 100;

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate(asset_id, certificate_id, price),
                Ok(())
            );

            let order = market
                .certificate_orders
                .get((asset_id, certificate_id))
                .expect("certificate order exists");
            assert_eq!(order.seller, seller);
            assert_eq!(order.buyer, None);
            assert_eq!(order.price, price);
            assert_eq!(order.asset_type, AssetType::Certificate);
            assert_eq!(order.asset_id, asset_id);
            assert_eq!(order.certificate_id, certificate_id);
            assert_eq!(order.valid_until, None);
            assert_eq!(order.status, OrderStatus::Listed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn listed_certificate_issue_order_records_right_terms() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            let seller = accounts.bob;
            let asset_id = [29u8; 32];
            let price = 100;
            let right_type = 2;
            let valid_until = Some(1_000u64);

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate_issue(asset_id, right_type, valid_until, price),
                Ok(())
            );

            let order = market.orders.get(asset_id).expect("issue order exists");
            assert_eq!(order.seller, seller);
            assert_eq!(order.buyer, None);
            assert_eq!(order.asset_type, AssetType::Certificate);
            assert_eq!(order.asset_id, asset_id);
            assert_eq!(order.certificate_id, [0u8; 32]);
            assert_eq!(order.right_type, right_type);
            assert_eq!(order.valid_until, valid_until);
            assert_eq!(order.status, OrderStatus::Listed);
        }

        #[ink::test]
        fn insufficient_payment_leaves_asset_order_listed() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [16u8; 32];
            let price = 100;

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price - 1);

            assert_eq!(market.buy_asset(asset_id), Err(Error::InsufficientPayment));

            let order = market.orders.get(asset_id).expect("asset order remains");
            assert_eq!(order.buyer, None);
            assert_eq!(order.status, OrderStatus::Listed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn failed_asset_settlement_marks_order_failed_without_removing_it() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(FailingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [17u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(
                market.buy_asset(asset_id),
                Err(Error::ChainExtension(DataAssetsExtError::TransferFailed))
            );

            let order = market.orders.get(asset_id).expect("asset order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Failed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn seller_payment_failure_marks_asset_order_failed() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = AccountId::from([99u8; 32]);
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [28u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_value_transferred::<market_standard::CustomEnvironment>(price);

            assert_eq!(market.buy_asset(asset_id), Err(Error::TransferFailed));

            let order = market.orders.get(asset_id).expect("asset order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Failed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn successful_certificate_purchase_records_settled_status() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [18u8; 32];
            let certificate_id = [19u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate(asset_id, certificate_id, price),
                Ok(())
            );

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(market.buy_certificate(asset_id, certificate_id), Ok(()));

            let order = market
                .certificate_orders
                .get((asset_id, certificate_id))
                .expect("settled certificate order remains for audit");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Settled);
            assert!(order.settled_at.is_some());
        }

        #[ink::test]
        fn buy_certificate_issue_calls_issue_extension_with_buyer_and_right_terms() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            RECORDED_CERTIFICATE_ISSUE.with(|recorded| *recorded.borrow_mut() = None);
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [30u8; 32];
            let price = 100;
            let right_type = 1;
            let valid_until = Some(2_000u64);

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate_issue(asset_id, right_type, valid_until, price),
                Ok(())
            );

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(market.buy_certificate_issue(asset_id), Ok(()));

            RECORDED_CERTIFICATE_ISSUE.with(|recorded| {
                assert_eq!(
                    *recorded.borrow(),
                    Some((
                        market_standard::ISSUE_CERT_FUNC_ID as u16,
                        asset_id,
                        seller,
                        buyer,
                        right_type,
                        valid_until
                    ))
                );
            });

            let order = market
                .orders
                .get(asset_id)
                .expect("settled issue order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Settled);
            assert!(order.settled_at.is_some());
        }

        #[ink::test]
        fn insufficient_payment_leaves_certificate_issue_order_listed() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [31u8; 32];
            let price = 100;

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate_issue(asset_id, 1, None, price),
                Ok(())
            );

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price - 1);

            assert_eq!(
                market.buy_certificate_issue(asset_id),
                Err(Error::InsufficientPayment)
            );

            let order = market.orders.get(asset_id).expect("issue order remains");
            assert_eq!(order.buyer, None);
            assert_eq!(order.status, OrderStatus::Listed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn failed_certificate_issue_marks_order_failed_without_payment() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(FailingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [32u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate_issue(asset_id, 1, None, price),
                Ok(())
            );

            let seller_balance_before =
                ink::env::test::get_account_balance::<market_standard::CustomEnvironment>(seller)
                    .unwrap();

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);

            assert_eq!(
                market.buy_certificate_issue(asset_id),
                Err(Error::ChainExtension(DataAssetsExtError::TransferFailed))
            );

            let seller_balance_after =
                ink::env::test::get_account_balance::<market_standard::CustomEnvironment>(seller)
                    .unwrap();
            assert_eq!(seller_balance_after, seller_balance_before);

            let order = market.orders.get(asset_id).expect("issue order remains");
            assert_eq!(order.buyer, Some(buyer));
            assert_eq!(order.status, OrderStatus::Failed);
            assert_eq!(order.settled_at, None);
        }

        #[ink::test]
        fn settled_asset_order_cannot_be_listed_again_and_overwritten() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let buyer = accounts.charlie;
            let asset_id = [27u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(buyer);
            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::transfer_in::<market_standard::CustomEnvironment>(price);
            assert_eq!(market.buy_asset(asset_id), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);
            assert_eq!(
                market.list_asset(asset_id, price + 1),
                Err(Error::AssetAlreadyListed)
            );

            let order = market
                .orders
                .get(asset_id)
                .expect("settled asset order remains");
            assert_eq!(order.status, OrderStatus::Settled);
            assert_eq!(order.price, price);
            assert_eq!(order.buyer, Some(buyer));
        }

        #[ink::test]
        fn failed_asset_cancel_returns_error_without_panic_or_removal() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(FailingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let asset_id = [20u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            assert_eq!(
                market.cancel_asset_order(asset_id),
                Err(Error::ChainExtension(DataAssetsExtError::TransferFailed))
            );

            let order = market.orders.get(asset_id).expect("asset order remains");
            assert_eq!(order.status, OrderStatus::Listed);
        }

        #[ink::test]
        fn asset_cancel_by_non_seller_returns_error_without_status_change() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            let seller = accounts.bob;
            let other = accounts.charlie;
            let asset_id = [21u8; 32];
            let price = 100;

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            ink::env::test::set_caller::<market_standard::CustomEnvironment>(other);

            assert_eq!(market.cancel_asset_order(asset_id), Err(Error::NotOwner));

            let order = market.orders.get(asset_id).expect("asset order remains");
            assert_eq!(order.status, OrderStatus::Listed);
        }

        #[ink::test]
        fn successful_asset_cancel_marks_order_cancelled() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            RECORDED_ASSET_TRANSFER.with(|recorded| *recorded.borrow_mut() = None);
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let asset_id = [22u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(market.list_asset(asset_id, price), Ok(()));

            assert_eq!(market.cancel_asset_order(asset_id), Ok(()));

            RECORDED_ASSET_TRANSFER.with(|recorded| {
                assert_eq!(
                    *recorded.borrow(),
                    Some((
                        market_standard::TRANSFER_ASSET_FUNC_ID as u16,
                        asset_id,
                        seller
                    ))
                );
            });

            let order = market
                .orders
                .get(asset_id)
                .expect("cancelled asset order remains");
            assert_eq!(order.status, OrderStatus::Cancelled);
            assert_eq!(order.buyer, None);
            assert!(order.settled_at.is_none());
        }

        #[ink::test]
        fn successful_certificate_cancel_marks_order_cancelled() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            RECORDED_CERTIFICATE_TRANSFER.with(|recorded| *recorded.borrow_mut() = None);
            ink::env::test::register_chain_extension(RecordingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let asset_id = [23u8; 32];
            let certificate_id = [24u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate(asset_id, certificate_id, price),
                Ok(())
            );

            assert_eq!(
                market.cancel_certificate_order(asset_id, certificate_id),
                Ok(())
            );

            RECORDED_CERTIFICATE_TRANSFER.with(|recorded| {
                assert_eq!(
                    *recorded.borrow(),
                    Some((
                        market_standard::TRANSFER_CERT_FUNC_ID as u16,
                        asset_id,
                        certificate_id,
                        seller
                    ))
                );
            });

            let order = market
                .certificate_orders
                .get((asset_id, certificate_id))
                .expect("cancelled certificate order remains");
            assert_eq!(order.status, OrderStatus::Cancelled);
            assert_eq!(order.buyer, None);
            assert!(order.settled_at.is_none());
        }

        #[ink::test]
        fn failed_certificate_cancel_returns_error_without_status_change() {
            let accounts = ink::env::test::default_accounts::<market_standard::CustomEnvironment>();
            ink::env::test::register_chain_extension(FailingTransferExtension);

            let contract = accounts.alice;
            let seller = accounts.bob;
            let asset_id = [25u8; 32];
            let certificate_id = [26u8; 32];
            let price = 100;

            ink::env::test::set_callee::<market_standard::CustomEnvironment>(contract);
            ink::env::test::set_caller::<market_standard::CustomEnvironment>(seller);

            let mut market = MarketOrderbook::new(0);
            assert_eq!(
                market.list_certificate(asset_id, certificate_id, price),
                Ok(())
            );

            assert_eq!(
                market.cancel_certificate_order(asset_id, certificate_id),
                Err(Error::ChainExtension(DataAssetsExtError::TransferFailed))
            );

            let order = market
                .certificate_orders
                .get((asset_id, certificate_id))
                .expect("certificate order remains");
            assert_eq!(order.status, OrderStatus::Listed);
        }
    }
}
