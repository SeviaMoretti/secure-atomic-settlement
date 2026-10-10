#![cfg_attr(not(feature = "std"), no_std)]

///数据物理层存储
///数据存储、验证、激励、惩罚
pub use pallet::*;
pub mod types;

use frame_support::{dispatch::DispatchResult, weights::Weight};
use sp_core::H256;

pub trait WeightInfo {
    fn register_provider() -> Weight;
    fn create_storage_order() -> Weight;
    fn bind_asset_storage() -> Weight;
    fn submit_storage_proof() -> Weight;
}

impl WeightInfo for () {
    fn register_provider() -> Weight {
        Weight::zero()
    }

    fn create_storage_order() -> Weight {
        Weight::zero()
    }

    fn bind_asset_storage() -> Weight {
        Weight::zero()
    }

    fn submit_storage_proof() -> Weight {
        Weight::zero()
    }
}

pub trait IpfsAvailabilityVerifier {
    fn ensure_available(_cid: &[u8], _size: u64) -> DispatchResult {
        Ok(())
    }
}

impl IpfsAvailabilityVerifier for () {}

pub trait XcmAvailabilityVerifier<AccountId> {
    fn ensure_available(
        _asset_id: &[u8; 32],
        _provider: &AccountId,
        _proof_hash: Option<&H256>,
    ) -> DispatchResult {
        Ok(())
    }
}

impl<AccountId> XcmAvailabilityVerifier<AccountId> for () {}

#[cfg(test)]
mod mock;

#[cfg(test)]
mod tests;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use frame_support::pallet_prelude::*;
    use frame_system::pallet_prelude::*;
    use pallet_collaterals::{CollateralRole, Pallet as Collaterals};
    use pallet_shared_traits::{AssetQueryError, DataAssetProvider};
    use sp_std::vec::Vec;

    use crate::types::StorageOrder;
    use sp_runtime::traits::Zero;

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::config]
    pub trait Config: frame_system::Config + pallet_collaterals::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        /// Asset ownership lookup, backed by pallet-dataassets in the runtime.
        type DataAssetProvider: DataAssetProvider<Self::AccountId, [u8; 32]>;

        /// Reserved extension point for real IPFS availability checks.
        type IpfsAvailabilityVerifier: IpfsAvailabilityVerifier;

        /// Reserved extension point for XCM / storage-chain availability checks.
        type XcmAvailabilityVerifier: XcmAvailabilityVerifier<Self::AccountId>;

        /// 存储证明的有效周期（以区块数为单位）
        #[pallet::constant]
        type ProofPeriod: Get<BlockNumberFor<Self>>;

        type WeightInfo: WeightInfo;
    }

    type BalanceOf<T> =
        <<T as pallet_collaterals::Config>::Currency as frame_support::traits::Currency<
            <T as frame_system::Config>::AccountId,
        >>::Balance;

    /// 存储提供者信息
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct ProviderInfo<BlockNumber, Balance> {
        pub endpoint: BoundedVec<u8, ConstU32<128>>, // IPFS Multiaddr
        pub capacity: u32,                           // 存储容量（单位：GB）
        pub pledged_amount: Balance,                 // 质押金额
        pub registered_at: BlockNumber,
        pub is_active: bool,
    }

    /// 资产的存储绑定信息
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct AssetStorageInfo<AccountId, Balance> {
        pub provider_id: AccountId,     // 绑定的服务商
        pub storage_fund: Balance, // 应该是从账户中扣除的、 IPFS存储费用池（从注册质押金中划扣）
        pub storage_account: AccountId, // 存储专用账户（用于支付存储费用）
        pub is_weak: bool,         // 是否处于余额不足的虚弱状态
    }

    /// 存储证明记录
    #[derive(Encode, Decode, Clone, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
    pub struct StorageProof<BlockNumber> {
        pub last_proof_block: BlockNumber,
        pub proof_hash: H256,
    }

    /// 资产ID -> 跨链存储订单映射
    #[pallet::storage]
    #[pallet::getter(fn storage_orders)]
    pub type StorageOrders<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // asset_id
        StorageOrder<BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn providers)]
    pub type Providers<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        ProviderInfo<BlockNumberFor<T>, BalanceOf<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn storage_proofs)]
    pub type StorageProofs<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        [u8; 32], // asset_id
        Blake2_128Concat,
        T::AccountId, // provider
        StorageProof<BlockNumberFor<T>>,
        OptionQuery,
    >;

    /// 记录资产ID与存储绑定信息的映射
    #[pallet::storage]
    #[pallet::getter(fn asset_storage_binds)]
    pub type AssetStorageBinds<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        [u8; 32], // asset_id
        AssetStorageInfo<T::AccountId, BalanceOf<T>>,
        OptionQuery,
    >;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        ProviderRegistered {
            who: T::AccountId,
            endpoint: Vec<u8>,
        },
        StorageOrderCreated {
            asset_id: [u8; 32],
            owner: T::AccountId,
        },
        AssetStorageBound {
            asset_id: [u8; 32],
            provider: T::AccountId,
        },
        ProofSubmitted {
            asset_id: [u8; 32],
            provider: T::AccountId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        NotAProvider,
        ProviderAlreadyExists,
        InvalidEndpoint,
        InvalidCapacity,
        InvalidCid,
        InvalidSize,
        AssetNotRegistered,
        NotAssetOwner,
        StorageOrderAlreadyExists,
        StorageOrderNotFound,
        StorageBindingAlreadyExists,
        StorageBindingNotFound,
        NotAssetStorageProvider,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(<T as Config>::WeightInfo::register_provider())]
        pub fn register_provider(
            origin: OriginFor<T>,
            endpoint: Vec<u8>,
            capacity: u32,
            pledge_amount: BalanceOf<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            ensure!(
                !Providers::<T>::contains_key(&who),
                Error::<T>::ProviderAlreadyExists
            );
            ensure!(capacity > 0, Error::<T>::InvalidCapacity);
            let bounded_endpoint =
                BoundedVec::try_from(endpoint.clone()).map_err(|_| Error::<T>::InvalidEndpoint)?;
            ensure!(!bounded_endpoint.is_empty(), Error::<T>::InvalidEndpoint);

            Collaterals::<T>::internal_pledge(&who, CollateralRole::IpfsProvider, pledge_amount)?;

            let provider = ProviderInfo {
                endpoint: bounded_endpoint,
                capacity,
                pledged_amount: pledge_amount,
                registered_at: frame_system::Pallet::<T>::block_number(),
                is_active: true,
            };
            Providers::<T>::insert(&who, provider);

            Self::deposit_event(Event::ProviderRegistered { who, endpoint });
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(<T as Config>::WeightInfo::create_storage_order())]
        pub fn create_storage_order(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            cid: Vec<u8>,
            size: u64,
            paid_fee: BalanceOf<T>,
            valid_until: BlockNumberFor<T>,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_asset_owner(&asset_id, &who)?;
            ensure!(
                !StorageOrders::<T>::contains_key(asset_id),
                Error::<T>::StorageOrderAlreadyExists
            );
            ensure!(size > 0, Error::<T>::InvalidSize);

            let bounded_cid =
                BoundedVec::try_from(cid.clone()).map_err(|_| Error::<T>::InvalidCid)?;
            ensure!(!bounded_cid.is_empty(), Error::<T>::InvalidCid);
            T::IpfsAvailabilityVerifier::ensure_available(&bounded_cid, size)?;

            let status = if paid_fee.is_zero() {
                crate::types::StorageStatus::Unfunded
            } else {
                crate::types::StorageStatus::Active
            };

            let order = StorageOrder {
                cid: bounded_cid,
                size,
                status,
                paid_fee,
                ordered_at: frame_system::Pallet::<T>::block_number(),
                valid_until,
            };
            StorageOrders::<T>::insert(asset_id, order);

            Self::deposit_event(Event::StorageOrderCreated {
                asset_id,
                owner: who,
            });
            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(<T as Config>::WeightInfo::bind_asset_storage())]
        pub fn bind_asset_storage(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            provider: T::AccountId,
        ) -> DispatchResult {
            let who = ensure_signed(origin)?;
            Self::ensure_asset_owner(&asset_id, &who)?;
            let provider_info = Providers::<T>::get(&provider).ok_or(Error::<T>::NotAProvider)?;
            ensure!(provider_info.is_active, Error::<T>::NotAProvider);
            let order =
                StorageOrders::<T>::get(asset_id).ok_or(Error::<T>::StorageOrderNotFound)?;
            ensure!(
                !AssetStorageBinds::<T>::contains_key(asset_id),
                Error::<T>::StorageBindingAlreadyExists
            );

            T::XcmAvailabilityVerifier::ensure_available(&asset_id, &provider, None)?;

            let binding = AssetStorageInfo {
                provider_id: provider.clone(),
                storage_fund: order.paid_fee,
                storage_account: provider.clone(),
                is_weak: false,
            };
            AssetStorageBinds::<T>::insert(asset_id, binding);

            Self::deposit_event(Event::AssetStorageBound { asset_id, provider });
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(<T as Config>::WeightInfo::submit_storage_proof())]
        pub fn submit_storage_proof(
            origin: OriginFor<T>,
            asset_id: [u8; 32],
            proof_hash: H256,
        ) -> DispatchResult {
            let provider = ensure_signed(origin)?;
            ensure!(
                Providers::<T>::contains_key(&provider),
                Error::<T>::NotAProvider
            );
            let binding =
                AssetStorageBinds::<T>::get(asset_id).ok_or(Error::<T>::StorageBindingNotFound)?;
            ensure!(
                binding.provider_id == provider,
                Error::<T>::NotAssetStorageProvider
            );

            T::XcmAvailabilityVerifier::ensure_available(&asset_id, &provider, Some(&proof_hash))?;

            let proof = StorageProof {
                last_proof_block: frame_system::Pallet::<T>::block_number(),
                proof_hash,
            };
            StorageProofs::<T>::insert(asset_id, &provider, proof);

            Self::deposit_event(Event::ProofSubmitted { asset_id, provider });
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        fn ensure_asset_owner(asset_id: &[u8; 32], who: &T::AccountId) -> DispatchResult {
            let owner = T::DataAssetProvider::get_asset_owner(asset_id)
                .map_err(Self::map_asset_query_error)?;
            ensure!(owner == *who, Error::<T>::NotAssetOwner);
            Ok(())
        }

        fn map_asset_query_error(error: AssetQueryError) -> Error<T> {
            match error {
                AssetQueryError::AssetNotFound => Error::<T>::AssetNotRegistered,
                AssetQueryError::InvalidOwner | AssetQueryError::OwnerAccountDoesNotExist => {
                    Error::<T>::NotAssetOwner
                }
            }
        }
    }
}
