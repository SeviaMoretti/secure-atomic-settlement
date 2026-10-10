// 资产交易市场模块
// 与pallet-contracts交互，实现资产交易市场
// 市场：交易模式：订单薄、拍卖等
//      准入规则
//      手续费
//      资产类型：数据元证、权证
// 
// pallet存储：市场注册表（创建者、合约账户）
// 市场相关合约-合约账户
#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;
use frame_support::traits::{Currency, ReservableCurrency};

use pallet_collaterals::{CollateralRole};

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

pub mod weights;

mod original_lib;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::{pallet_prelude::*};
    use frame_system::{pallet_prelude::*};

    use pallet_contracts::{CollectEvents, DebugInfo, Determinism, chain_extension::ReturnFlags};

    use codec::{Encode, Decode, MaxEncodedLen, DecodeWithMemTracking};
    
    /// 函数选择器：对应ink!合约的is_assetx_market()方法
    const SELECTOR_IS_MARKET: [u8; 4] = [0x26, 0x3e, 0x53, 0x34];

    pub trait WeightInfo {
        fn register_market() -> Weight;
        fn unregister_market() -> Weight;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);
    
    // 市场资产类型
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking)]
    pub enum MarketAssetType {
        DataAsset,      // 交易元证 (Assets)
        Certificate,    // 交易权证 (Certificates)
    }

    // 市场元数据
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking)]
    pub struct MarketRegistryInfo<AccountId> {
        pub creator: AccountId,         // 创建者
        pub contract_address: AccountId,// 智能合约地址 (Ink!合约部署后的地址)
        pub asset_type: MarketAssetType,// 交易资产类型
        pub status: MarketStatus,       // Active, Suspended
    }

    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen, DecodeWithMemTracking)]
    pub enum MarketStatus {
        Active,
        Inactive,
    }

    #[pallet::config]
    // 继承 pallet_collaterals::Config
    pub trait Config: frame_system::Config + pallet_contracts::Config + pallet_collaterals::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;
        type Currency: Currency<Self::AccountId> + ReservableCurrency<Self::AccountId>;
        type MarketWeightInfo: WeightInfo;
    }

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        MarketRegistered {
            creator: T::AccountId,
            contract_address: T::AccountId,
            asset_type: MarketAssetType,
        },
        MarketUnregistered {
            contract_address: T::AccountId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        /// 市场已存在
        MarketAlreadyExists,
        /// 市场不存在
        MarketNotFound,
        /// 不是市场所有者
        NotOwner,
        /// 市场验证失败
        MarketVerificationFailed,
    }

    #[pallet::storage]
    #[pallet::getter(fn registered_markets)]
    pub type RegisteredMarkets<T: Config> = StorageMap<
        _, 
        Blake2_128Concat, 
        T::AccountId, // Contract Address
        MarketRegistryInfo<T::AccountId>
    >;

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        /// 注册一个新市场
        /// 用户先部署智能合约，获得 ontract_address，然后调用此函数进行注册
        #[pallet::call_index(0)]
        #[pallet::weight(T::MarketWeightInfo::register_market())]
        pub fn register_market(
            origin: OriginFor<T>,
            contract_address: T::AccountId,
            asset_type: MarketAssetType,
        ) -> DispatchResult {
            let creator = ensure_signed(origin)?;

            // 1. 基础检查
            ensure!(!RegisteredMarkets::<T>::contains_key(&contract_address), Error::<T>::MarketAlreadyExists);
            let asset_type_for_event = asset_type.clone();

            // 2.质押
            // 获取配置中定义的市场运营者最小质押金额
            let min_collateral = <T as pallet_collaterals::Config>::MinMarketOperatorCollateral::get();
            
            // 调用pallet-collaterals的内部质押函数
            // 检查余额并锁定资金，余额不足会返回Error
            pallet_collaterals::Pallet::<T>::internal_pledge(
                &creator,
                CollateralRole::MarketOperator,
                min_collateral
            )?;
            
            // 3. 验证合约逻辑
            let input_data = SELECTOR_IS_MARKET.to_vec();
            let gas_limit = Weight::from_parts(5_000_000_000, 256 * 1024);

            let result = pallet_contracts::Pallet::<T>::bare_call(
                creator.clone(),
                contract_address.clone(),
                0u32.into(),
                gas_limit,
                None,
                input_data,
                DebugInfo::Skip,
                CollectEvents::Skip,
                Determinism::Enforced,
            );

            let verified = match result.result {
                Ok(retval) => {
                    if retval.flags.contains(ReturnFlags::REVERT) {
                        false
                    } else {
                        let decoded_result: Result<Result<bool, u8>, _> = Decode::decode(&mut &retval.data[..]);
                        match decoded_result {
                            Ok(Ok(true)) => true,
                            _ => false,
                        }
                    }
                },
                Err(_) => false,
            };

            ensure!(verified, Error::<T>::MarketVerificationFailed);

            // 4. 存储市场信息
            let info = MarketRegistryInfo {
                creator: creator.clone(),
                contract_address: contract_address.clone(),
                asset_type,
                status: MarketStatus::Active,
            };

            RegisteredMarkets::<T>::insert(&contract_address, info);

            Self::deposit_event(Event::MarketRegistered { creator, contract_address, asset_type: asset_type_for_event });
            Ok(())
        }

        /// 注销市场
        #[pallet::call_index(1)]
        #[pallet::weight(T::MarketWeightInfo::unregister_market())]
        pub fn unregister_market(
            origin: OriginFor<T>,
            contract_address: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            
            // 1. 检查权限
            let market = RegisteredMarkets::<T>::get(&contract_address).ok_or(Error::<T>::MarketNotFound)?;
            ensure!(market.creator == who, Error::<T>::NotOwner);

            // 2. 解除质押
            // 调用 pallet-collaterals 的内部解押函数
            // 如果未满 2 年，这里会返回CollateralNotReadyForRelease错误
            // 这意味着市场在锁定期内无法被完全注销
            pallet_collaterals::Pallet::<T>::internal_unbond(
                &who,
                CollateralRole::MarketOperator
            )?;

            // 3. 移除市场信息
            RegisteredMarkets::<T>::remove(&contract_address);
            
            Self::deposit_event(Event::MarketUnregistered { contract_address });
            Ok(())
        }
    }
}