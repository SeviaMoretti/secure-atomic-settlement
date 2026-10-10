/*
// 最初实现的lib.rs
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::{
        AccountIdConversion, CheckedAdd, CheckedDiv, CheckedMul, CheckedSub, One, Zero
    };
    use frame_support::traits::{Currency, ExistenceRequirement, Get};
    use frame_support::PalletId;
    use sp_runtime::Perbill;
    use scale_info::TypeInfo;
    use sp_runtime::RuntimeDebug;
    use codec::{Encode, Decode, MaxEncodedLen, DecodeWithMemTracking};

    pub type AssetId = [u8; 32];
    type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

    // ===================================================================
    // 1. 数据结构
    // ===================================================================

    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub enum MarketMechanism {
        OrderBook,
        // AMM 在 NFT/Unique Asset 场景下较复杂，这里简化为不支持或仅支持整单兑换
        // 如果需要 AMM 支持，资产本身要支持 Shared/Fractionalized
        AMM, 
    }

    #[derive(Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub enum AccessRule {
        Public,
        QualityGate {
            min_score: u8, // 数据元证没有这个字段
            must_be_verified: bool,
        },
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct MarketInfo<AccountId> {
        pub creator: AccountId,
        // 
        pub market_account: AccountId,
        pub mechanism: MarketMechanism,
        pub access_rule: AccessRule,
        pub fee_ratio: u32,
        pub active: bool,
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct ListingInfo<AccountId, Balance, MarketId> {
        pub market_id: MarketId,
        pub seller: AccountId,
        pub asset_id: AssetId,
        pub price: Balance,
    }

    // ===================================================================
    // 2. Config
    // ===================================================================
    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_dataassets::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        type Currency: Currency<Self::AccountId>;

        #[pallet::constant]
        type PalletId: Get<PalletId>;

        type MarketId: Parameter + Member + Copy + Default + MaxEncodedLen + One + CheckedAdd + PartialEq;
        type ListingId: Parameter + Member + Copy + Default + MaxEncodedLen + One + CheckedAdd;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    // ===================================================================
    // 3. Storage
    // ===================================================================
    
    #[pallet::storage]
    pub type MarketCount<T: Config> = StorageValue<_, T::MarketId, ValueQuery>;
    
    #[pallet::storage]
    pub type ListingCount<T: Config> = StorageValue<_, T::ListingId, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn markets)]
    pub type Markets<T: Config> = StorageMap<_, Blake2_128Concat, T::MarketId, MarketInfo<T::AccountId>>;

    #[pallet::storage]
    #[pallet::getter(fn listings)]
    pub type Listings<T: Config> = StorageMap<_, Blake2_128Concat, T::ListingId, ListingInfo<T::AccountId, BalanceOf<T>, T::MarketId>>;

    // ===================================================================
    // 4. Events & Errors
    // ===================================================================
    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        MarketCreated { market_id: T::MarketId, creator: T::AccountId, mechanism: MarketMechanism },
        AssetListed { market_id: T::MarketId, listing_id: T::ListingId, seller: T::AccountId, asset_id: AssetId, price: BalanceOf<T> },
        AssetSold { market_id: T::MarketId, listing_id: T::ListingId, buyer: T::AccountId, price: BalanceOf<T> },
        ListingCancelled { market_id: T::MarketId, listing_id: T::ListingId },
    }

    #[pallet::error]
    pub enum Error<T> {
        MarketNotFound,
        MarketNotActive,
        ListingNotFound,
        WrongMarketMechanism,
        ListingNotInMarket,
        QualityTooLow,
        NotVerified,
        AssetNotFound,
        NotOwner,
        MathOverflow,
        /// 市场未获得资产授权，请先在 DataAssets 模块调用 authorize_market
        MarketNotAuthorized,
        /// 资产已被其他人授权给别的市场
        AssetAuthorizedToOthers,
    }

    // ===================================================================
    // 5. Extrinsics
    // ===================================================================
    #[pallet::call]
    impl<T: Config> Pallet<T> {
        
        #[pallet::call_index(0)]
        #[pallet::weight(10_000)]
        pub fn create_market(
            origin: OriginFor<T>,
            mechanism: MarketMechanism, 
            access_rule: AccessRule,
            fee_ratio: u32,
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;
            ensure!(fee_ratio <= 10000, Error::<T>::MathOverflow);

            let market_id = MarketCount::<T>::get();
            let next_id = market_id.checked_add(&One::one()).ok_or(Error::<T>::MathOverflow)?;
            
            let mechanism_clone = mechanism.clone();
            
            let info = MarketInfo {
                creator: creator.clone(),
                market_account: Self::market_account_id(market_id),
                mechanism,
                access_rule,
                fee_ratio,
                active: true,
            };

            Markets::<T>::insert(market_id, info);
            Self::deposit_event(Event::MarketCreated { market_id, creator, mechanism: mechanism_clone });
            MarketCount::<T>::put(next_id);
            Ok(())
        }

        /// 卖家挂单 (OrderBook)
        /// 前提：卖家必须先在 pallet-dataassets 调用 `authorize_market(asset_id, market_account_id)`
        #[pallet::call_index(1)]
        #[pallet::weight(10_000)]
        pub fn list_asset(
            origin: OriginFor<T>,
            market_id: T::MarketId,
            asset_id: AssetId,
            price: BalanceOf<T>,
        ) -> DispatchResult {
            let seller = ensure_signed(origin)?;
            let market = Markets::<T>::get(market_id).ok_or(Error::<T>::MarketNotFound)?;
            ensure!(market.active, Error::<T>::MarketNotActive);
            ensure!(market.mechanism == MarketMechanism::OrderBook, Error::<T>::WrongMarketMechanism);

            // 1. 验证资产所有权
            let asset = pallet_dataassets::Pallet::<T>::get_asset(&asset_id)
                .ok_or(Error::<T>::AssetNotFound)?;
            ensure!(asset.core.owner == seller, Error::<T>::NotOwner);

            // 2. 验证市场准入规则
            Self::check_access_rule(&market, &asset_id)?;

            // 3. 验证授权
            // 需要确保assets模块中，该asset已经 approve 给了当前市场的 account_id
            let market_account = Self::market_account_id(market_id);
            let approved_account = pallet_dataassets::Pallet::<T>::asset_approvals(&asset_id)
                .ok_or(Error::<T>::MarketNotAuthorized)?;
            
            ensure!(approved_account == market_account, Error::<T>::AssetAuthorizedToOthers);

            // 4. 创建挂单 (不再转移资产，只记录挂单)
            let listing_id = ListingCount::<T>::get();
            let next_lid = listing_id.checked_add(&One::one()).ok_or(Error::<T>::MathOverflow)?;

            let listing = ListingInfo {
                market_id,
                seller: seller.clone(),
                asset_id,
                price,
            };
            Listings::<T>::insert(listing_id, listing);
            ListingCount::<T>::put(next_lid);

            Self::deposit_event(Event::AssetListed { market_id, listing_id, seller, asset_id, price });
            Ok(())
        }

        /// 买家购买
        #[pallet::call_index(2)]
        #[pallet::weight(10_000)]
        pub fn buy_asset(
            origin: OriginFor<T>,
            market_id: T::MarketId,
            listing_id: T::ListingId,
        ) -> DispatchResult {
            let buyer = ensure_signed(origin)?;
            let listing = Listings::<T>::get(listing_id).ok_or(Error::<T>::ListingNotFound)?;
            
            ensure!(listing.market_id == market_id, Error::<T>::ListingNotInMarket);
            let market = Markets::<T>::get(market_id).ok_or(Error::<T>::MarketNotFound)?;
            ensure!(market.active, Error::<T>::MarketNotActive);

            // 1. 计算费用
            let (fee, seller_receive) = Self::calc_fee(listing.price, market.fee_ratio)?;

            // 这儿嗯啊的转了两次帐，那手续费是卖家单独付款不是从交易中扣除的
            // 2. 执行资金划转 (原子性由 Substrate 保证，若后续步骤失败会回滚)
            // 买家 -> 市场创建者 (Fee)
            if !fee.is_zero() {
                <T as Config>::Currency::transfer(&buyer, &market.creator, fee, ExistenceRequirement::KeepAlive)?;
            }
            // 买家 -> 卖家 (货款)
            <T as Config>::Currency::transfer(&buyer, &listing.seller, seller_receive, ExistenceRequirement::KeepAlive)?;

            // 3.执行资产交割
            // 调用dataassets的内部方法，以市场身份转移资产
            // 这里假设在 pallet-dataassets 中添加了 `transfer_by_market_internal`
            // 如果没有添加，则无法通过编译
            let market_account = Self::market_account_id(market_id);
            pallet_dataassets::Pallet::<T>::transfer_by_market_internal(
                &listing.asset_id,
                &market_account,
                &buyer
            )?;

            // 4. 移除挂单
            Listings::<T>::remove(listing_id);

            Self::deposit_event(Event::AssetSold { market_id, listing_id, buyer, price: listing.price });
            Ok(())
        }

        /// 取消挂单
        #[pallet::call_index(3)]
        #[pallet::weight(10_000)]
        pub fn cancel_listing(
            origin: OriginFor<T>,
            market_id: T::MarketId,
            listing_id: T::ListingId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            let listing = Listings::<T>::get(listing_id).ok_or(Error::<T>::ListingNotFound)?;
            ensure!(listing.market_id == market_id, Error::<T>::ListingNotInMarket);
            ensure!(listing.seller == who, Error::<T>::NotOwner);

            Listings::<T>::remove(listing_id);
            // 注意：取消挂单不自动撤销授权，用户需要手动去 dataassets 撤销，或者这里可以跨模块调用撤销
            
            Self::deposit_event(Event::ListingCancelled { market_id, listing_id });
            Ok(())
        }

        // 从市场对应的账户提前手续费到市场创建者账户
    }

    impl<T: Config> Pallet<T> {
        /// 获取市场的托管账户/操作账户
        /// ！！！！！！！！市场应该一个市场一个账户
        /// ！！！！！！！！MarketInfo不存储市场账户、market_id
        pub fn account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }
        /// 为每个市场生成独立的账户
        pub fn market_account_id(market_id: T::MarketId) -> T::AccountId {
            T::PalletId::get().into_sub_account_truncating(market_id)
        }
        
        /// 获取pallet-markets的主账户
        pub fn pallet_account_id() -> T::AccountId {
            T::PalletId::get().into_account_truncating()
        }

        fn check_access_rule(market: &MarketInfo<T::AccountId>, _asset_id: &AssetId) -> DispatchResult {
            match market.access_rule {
                AccessRule::Public => Ok(()),
                AccessRule::QualityGate { .. } => {
                    // 由于DataAsset 结构体定义中没有quality字段
                    // 先暂时跳过具体的质量检查，或者需要确保 DataAsset有相关字段。
                    // 如果dataassets提供了获取质量的方法：
                    // let score = pallet_dataassets::Pallet::<T>::get_asset_quality(asset_id);
                    Ok(())
                }
            }
        }

        fn calc_fee(amount: BalanceOf<T>, ratio: u32) -> Result<(BalanceOf<T>, BalanceOf<T>), Error<T>> {
            let fee_part = Perbill::from_rational(ratio, 10000);
            let fee = fee_part * amount;
            let remaining = amount.checked_sub(&fee).ok_or(Error::<T>::MathOverflow)?;
            Ok((fee, remaining))
        }
    }
}
*/