#![cfg_attr(not(feature = "std"), no_std)]

pub use pallet::*;

use codec::Decode;
use core::marker::PhantomData;
use frame_support::traits::Currency;
use pallet_collaterals::CollateralRole;
use pallet_contracts::{chain_extension::ReturnFlags, CollectEvents, DebugInfo, Determinism};
use pallet_shared_traits::{MarketContractVerifier, MarketStakeHandler};

#[cfg(feature = "runtime-benchmarks")]
pub mod benchmarking;

pub mod weights;

// `original_lib.rs` is retained only as legacy characterization source. It is
// intentionally not compiled because it calls pre-v2 direct asset mutation.

const SELECTOR_IS_MARKET: [u8; 4] = [0x26, 0x3e, 0x53, 0x34];

pub struct ContractsMarketVerifier<T>(PhantomData<T>);

impl<T> MarketContractVerifier<T::AccountId> for ContractsMarketVerifier<T>
where
    T: frame_system::Config + pallet_contracts::Config,
{
    fn is_market(operator: &T::AccountId, contract_address: &T::AccountId) -> bool {
        let result = pallet_contracts::Pallet::<T>::bare_call(
            operator.clone(),
            contract_address.clone(),
            0u32.into(),
            frame_support::weights::Weight::from_parts(5_000_000_000, 256 * 1024),
            None,
            SELECTOR_IS_MARKET.to_vec(),
            DebugInfo::Skip,
            CollectEvents::Skip,
            Determinism::Enforced,
        );

        match result.result {
            Ok(retval) if !retval.flags.contains(ReturnFlags::REVERT) => {
                matches!(
                    Result::<bool, u8>::decode(&mut &retval.data[..]),
                    Ok(Ok(true))
                )
            }
            _ => false,
        }
    }
}

type CollateralBalanceOf<T> = <<T as pallet_collaterals::Config>::Currency as Currency<
    <T as frame_system::Config>::AccountId,
>>::Balance;

pub struct CollateralMarketStake<T>(PhantomData<T>);

impl<T> MarketStakeHandler<T::AccountId, CollateralBalanceOf<T>> for CollateralMarketStake<T>
where
    T: frame_system::Config + pallet_collaterals::Config,
{
    fn pledge(
        operator: &T::AccountId,
        amount: CollateralBalanceOf<T>,
    ) -> frame_support::dispatch::DispatchResult {
        pallet_collaterals::Pallet::<T>::internal_pledge(
            operator,
            CollateralRole::MarketOperator,
            amount,
        )
    }

    fn unbond(operator: &T::AccountId) -> frame_support::dispatch::DispatchResult {
        pallet_collaterals::Pallet::<T>::internal_unbond(operator, CollateralRole::MarketOperator)
    }
}

#[frame_support::pallet]
pub mod pallet {
    use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
    use frame_support::{pallet_prelude::*, traits::EnsureOrigin};
    use frame_system::pallet_prelude::*;
    use pallet_shared_traits::{
        MarketAccessError, MarketContractVerifier, MarketOperation, MarketPaymentTerms,
        MarketRegistryProvider, MarketStakeHandler, MarketStateSnapshot,
    };
    use scale_info::TypeInfo;
    use sp_io::hashing::blake2_256;
    use sp_runtime::traits::{AtLeast32BitUnsigned, SaturatedConversion, Zero};

    pub type MarketId = [u8; 32];
    pub type BalanceOf<T> = <T as Config>::Balance;

    pub trait WeightInfo {
        fn set_capacity_tier() -> Weight;
        fn register_market() -> Weight;
        fn restrict_market() -> Weight;
        fn activate_market() -> Weight;
        fn request_exit() -> Weight;
        fn deregister_market() -> Weight;
    }

    #[derive(
        Encode,
        Decode,
        DecodeWithMemTracking,
        Clone,
        Copy,
        PartialEq,
        Eq,
        RuntimeDebug,
        TypeInfo,
        MaxEncodedLen,
    )]
    pub struct MarketCapabilities(u8);

    impl MarketCapabilities {
        pub const CREATE_META_ORDER: u8 = 1 << 0;
        pub const CREATE_RIGHT_ISSUE_ORDER: u8 = 1 << 1;
        pub const CREATE_RIGHT_TRANSFER_ORDER: u8 = 1 << 2;
        pub const RELAY_FUNDING: u8 = 1 << 3;
        pub const RELAY_SETTLEMENT: u8 = 1 << 4;
        const ALL: u8 = Self::CREATE_META_ORDER
            | Self::CREATE_RIGHT_ISSUE_ORDER
            | Self::CREATE_RIGHT_TRANSFER_ORDER
            | Self::RELAY_FUNDING
            | Self::RELAY_SETTLEMENT;

        pub fn from_bits(bits: u8) -> Option<Self> {
            let capabilities = Self(bits);
            capabilities.is_valid().then_some(capabilities)
        }

        pub fn transaction_operations() -> Self {
            Self(
                Self::CREATE_META_ORDER
                    | Self::CREATE_RIGHT_ISSUE_ORDER
                    | Self::CREATE_RIGHT_TRANSFER_ORDER,
            )
        }

        pub fn bits(self) -> u8 {
            self.0
        }

        pub fn is_valid(self) -> bool {
            self.0 != 0 && self.0 & !Self::ALL == 0
        }

        pub fn supports_operation(self, operation: MarketOperation) -> bool {
            let capability = match operation {
                MarketOperation::TransferMeta => Self::CREATE_META_ORDER,
                MarketOperation::IssueRight => Self::CREATE_RIGHT_ISSUE_ORDER,
                MarketOperation::TransferRight => Self::CREATE_RIGHT_TRANSFER_ORDER,
            };
            self.0 & capability != 0
        }
    }

    #[derive(
        Encode,
        Decode,
        DecodeWithMemTracking,
        Clone,
        Copy,
        PartialEq,
        Eq,
        RuntimeDebug,
        TypeInfo,
        MaxEncodedLen,
    )]
    pub enum MarketStatus {
        Active,
        Restricted,
        Exiting,
        Deregistered,
    }

    #[derive(
        Encode,
        Decode,
        DecodeWithMemTracking,
        Clone,
        PartialEq,
        Eq,
        RuntimeDebug,
        TypeInfo,
        MaxEncodedLen,
    )]
    pub struct MarketCapacityTier<Balance> {
        pub tier_id: u8,
        pub required_stake: Balance,
        pub max_active_orders: u32,
        pub max_funded_orders: u32,
        pub max_active_exposure_value: Balance,
    }

    #[derive(
        Encode,
        Decode,
        DecodeWithMemTracking,
        Clone,
        PartialEq,
        Eq,
        RuntimeDebug,
        TypeInfo,
        MaxEncodedLen,
    )]
    pub struct MarketRegistration<AccountId, Balance, BlockNumber> {
        pub market_id: MarketId,
        pub operator: AccountId,
        pub contract_address: AccountId,
        pub code_hash: [u8; 32],
        pub protocol_version: u16,
        pub capabilities: MarketCapabilities,
        pub capacity_tier: u8,
        pub stake: Balance,
        pub active_orders: u32,
        pub funded_orders: u32,
        pub active_exposure_value: Balance,
        pub status: MarketStatus,
        pub registered_at: BlockNumber,
        pub exit_requested_at: Option<BlockNumber>,
    }

    #[pallet::pallet]
    #[pallet::storage_version(STORAGE_VERSION)]
    pub struct Pallet<T>(_);

    const STORAGE_VERSION: StorageVersion = StorageVersion::new(2);

    #[pallet::config]
    pub trait Config: frame_system::Config {
        type RuntimeEvent: From<Event<Self>> + IsType<<Self as frame_system::Config>::RuntimeEvent>;

        type Balance: Parameter
            + Member
            + AtLeast32BitUnsigned
            + Default
            + Copy
            + MaxEncodedLen
            + TypeInfo;

        type MarketAdminOrigin: EnsureOrigin<Self::RuntimeOrigin>;
        type ContractVerifier: MarketContractVerifier<Self::AccountId>;
        type StakeHandler: MarketStakeHandler<Self::AccountId, Self::Balance>;

        #[pallet::constant]
        type SettlementProtocolVersion: Get<u16>;

        type MarketWeightInfo: WeightInfo;
    }

    #[pallet::storage]
    #[pallet::getter(fn capacity_tiers)]
    pub type CapacityTiers<T: Config> =
        StorageMap<_, Blake2_128Concat, u8, MarketCapacityTier<BalanceOf<T>>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn market_registrations)]
    pub type MarketRegistrations<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        MarketId,
        MarketRegistration<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn contract_market_id)]
    pub type ContractMarketIds<T: Config> =
        StorageMap<_, Blake2_128Concat, T::AccountId, MarketId, OptionQuery>;

    #[pallet::storage]
    pub type NextMarketNonce<T: Config> = StorageValue<_, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_authorization_count)]
    pub type ActiveAuthorizationCount<T: Config> =
        StorageMap<_, Blake2_128Concat, MarketId, u32, ValueQuery>;

    /// Registration-time fee policy. It is immutable for the lifetime of a
    /// stable market identity and is snapshotted into each funded escrow.
    #[pallet::storage]
    #[pallet::getter(fn market_fee_bps)]
    pub type MarketFeeBps<T: Config> = StorageMap<_, Blake2_128Concat, MarketId, u16, OptionQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        CapacityTierSet {
            tier_id: u8,
        },
        MarketRegistered {
            market_id: MarketId,
            operator: T::AccountId,
            contract_address: T::AccountId,
        },
        MarketRestricted {
            market_id: MarketId,
        },
        MarketActivated {
            market_id: MarketId,
        },
        MarketExitRequested {
            market_id: MarketId,
        },
        MarketDeregistered {
            market_id: MarketId,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        CapacityTierNotFound,
        InvalidCapacityTier,
        InvalidCapabilities,
        InvalidMarketFee,
        MarketAlreadyRegistered,
        MarketNotRegistered,
        NotOperator,
        MarketVerificationFailed,
        MarketRestricted,
        CapabilityDenied,
        InvalidMarketTransition,
        MarketHasActiveObligations,
        ActiveOrderCapacityExceeded,
        ActiveOrderCounterOverflow,
        ActiveOrderCounterUnderflow,
        FundedOrderCapacityExceeded,
        FundedOrderCounterOverflow,
        FundedOrderCounterUnderflow,
        ExposureCapacityExceeded,
        ExposureCounterOverflow,
        ExposureCounterUnderflow,
        AuthorizationCounterOverflow,
        AuthorizationCounterUnderflow,
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(T::MarketWeightInfo::set_capacity_tier())]
        pub fn set_capacity_tier(
            origin: OriginFor<T>,
            tier: MarketCapacityTier<BalanceOf<T>>,
        ) -> DispatchResult {
            T::MarketAdminOrigin::ensure_origin(origin)?;
            ensure!(
                !tier.required_stake.is_zero()
                    && tier.max_active_orders > 0
                    && tier.max_funded_orders > 0
                    && tier.max_funded_orders <= tier.max_active_orders
                    && !tier.max_active_exposure_value.is_zero(),
                Error::<T>::InvalidCapacityTier
            );

            let tier_id = tier.tier_id;
            CapacityTiers::<T>::insert(tier_id, tier);
            Self::deposit_event(Event::CapacityTierSet { tier_id });
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(T::MarketWeightInfo::register_market())]
        pub fn register_market(
            origin: OriginFor<T>,
            contract_address: T::AccountId,
            code_hash: [u8; 32],
            capabilities: MarketCapabilities,
            capacity_tier: u8,
            market_fee_bps: u16,
        ) -> DispatchResult {
            let operator = ensure_signed(origin)?;
            ensure!(capabilities.is_valid(), Error::<T>::InvalidCapabilities);
            ensure!(market_fee_bps <= 10_000, Error::<T>::InvalidMarketFee);
            ensure!(
                !ContractMarketIds::<T>::contains_key(&contract_address),
                Error::<T>::MarketAlreadyRegistered
            );
            let tier =
                CapacityTiers::<T>::get(capacity_tier).ok_or(Error::<T>::CapacityTierNotFound)?;
            ensure!(
                T::ContractVerifier::is_market(&operator, &contract_address),
                Error::<T>::MarketVerificationFailed
            );

            let nonce = NextMarketNonce::<T>::get();
            let market_id = Self::derive_market_id(&operator, &contract_address, nonce);
            ensure!(
                !MarketRegistrations::<T>::contains_key(market_id),
                Error::<T>::MarketAlreadyRegistered
            );

            T::StakeHandler::pledge(&operator, tier.required_stake)?;

            let registration = MarketRegistration {
                market_id,
                operator: operator.clone(),
                contract_address: contract_address.clone(),
                code_hash,
                protocol_version: T::SettlementProtocolVersion::get(),
                capabilities,
                capacity_tier,
                stake: tier.required_stake,
                active_orders: 0,
                funded_orders: 0,
                active_exposure_value: Zero::zero(),
                status: MarketStatus::Active,
                registered_at: frame_system::Pallet::<T>::block_number(),
                exit_requested_at: None,
            };

            NextMarketNonce::<T>::put(nonce.saturating_add(1));
            MarketRegistrations::<T>::insert(market_id, registration);
            MarketFeeBps::<T>::insert(market_id, market_fee_bps);
            ContractMarketIds::<T>::insert(&contract_address, market_id);
            Self::deposit_event(Event::MarketRegistered {
                market_id,
                operator,
                contract_address,
            });
            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(T::MarketWeightInfo::restrict_market())]
        pub fn restrict_market(origin: OriginFor<T>, market_id: MarketId) -> DispatchResult {
            T::MarketAdminOrigin::ensure_origin(origin)?;
            MarketRegistrations::<T>::try_mutate(market_id, |entry| -> DispatchResult {
                let market = entry.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                ensure!(
                    market.status == MarketStatus::Active,
                    Error::<T>::InvalidMarketTransition
                );
                market.status = MarketStatus::Restricted;
                Ok(())
            })?;
            Self::deposit_event(Event::MarketRestricted { market_id });
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(T::MarketWeightInfo::activate_market())]
        pub fn activate_market(origin: OriginFor<T>, market_id: MarketId) -> DispatchResult {
            T::MarketAdminOrigin::ensure_origin(origin)?;
            MarketRegistrations::<T>::try_mutate(market_id, |entry| -> DispatchResult {
                let market = entry.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                ensure!(
                    market.status == MarketStatus::Restricted,
                    Error::<T>::InvalidMarketTransition
                );
                market.status = MarketStatus::Active;
                Ok(())
            })?;
            Self::deposit_event(Event::MarketActivated { market_id });
            Ok(())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(T::MarketWeightInfo::request_exit())]
        pub fn request_exit(origin: OriginFor<T>, market_id: MarketId) -> DispatchResult {
            let operator = ensure_signed(origin)?;
            MarketRegistrations::<T>::try_mutate(market_id, |entry| -> DispatchResult {
                let market = entry.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                ensure!(market.operator == operator, Error::<T>::NotOperator);
                ensure!(
                    matches!(
                        market.status,
                        MarketStatus::Active | MarketStatus::Restricted
                    ),
                    Error::<T>::InvalidMarketTransition
                );
                market.status = MarketStatus::Exiting;
                market.exit_requested_at = Some(frame_system::Pallet::<T>::block_number());
                Ok(())
            })?;
            Self::deposit_event(Event::MarketExitRequested { market_id });
            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(T::MarketWeightInfo::deregister_market())]
        pub fn deregister_market(origin: OriginFor<T>, market_id: MarketId) -> DispatchResult {
            let operator = ensure_signed(origin)?;
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            ensure!(market.operator == operator, Error::<T>::NotOperator);
            ensure!(
                market.status == MarketStatus::Exiting,
                Error::<T>::InvalidMarketTransition
            );
            ensure!(
                market.active_orders == 0
                    && market.funded_orders == 0
                    && market.active_exposure_value.is_zero()
                    && ActiveAuthorizationCount::<T>::get(market_id) == 0,
                Error::<T>::MarketHasActiveObligations
            );

            T::StakeHandler::unbond(&operator)?;

            MarketRegistrations::<T>::mutate(market_id, |entry| {
                if let Some(registration) = entry {
                    registration.status = MarketStatus::Deregistered;
                }
            });
            ContractMarketIds::<T>::remove(&market.contract_address);
            Self::deposit_event(Event::MarketDeregistered { market_id });
            Ok(())
        }

        /// Admin-only fixture transition used to create one-delta E2 market
        /// pre-states on a fresh disposable chain.
        #[pallet::call_index(6)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 3))]
        pub fn configure_market_for_experiment(
            origin: OriginFor<T>,
            market_id: MarketId,
            registered: bool,
            capabilities: Option<MarketCapabilities>,
            max_funded_orders: Option<u32>,
        ) -> DispatchResult {
            T::MarketAdminOrigin::ensure_origin(origin)?;
            if !registered {
                let market = MarketRegistrations::<T>::get(market_id)
                    .ok_or(Error::<T>::MarketNotRegistered)?;
                MarketRegistrations::<T>::remove(market_id);
                MarketFeeBps::<T>::remove(market_id);
                ContractMarketIds::<T>::remove(market.contract_address);
                return Ok(());
            }
            let market = MarketRegistrations::<T>::try_mutate(
                market_id,
                |entry| -> Result<_, DispatchError> {
                    let market = entry.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                    if let Some(value) = capabilities {
                        market.capabilities = value;
                    }
                    Ok(market.clone())
                },
            )?;
            if let Some(value) = max_funded_orders {
                CapacityTiers::<T>::try_mutate(market.capacity_tier, |entry| -> DispatchResult {
                    let tier = entry.as_mut().ok_or(Error::<T>::CapacityTierNotFound)?;
                    tier.max_funded_orders = value;
                    Ok(())
                })?;
            }
            Ok(())
        }
    }

    impl<T: Config> Pallet<T> {
        fn derive_market_id(
            operator: &T::AccountId,
            contract_address: &T::AccountId,
            nonce: u64,
        ) -> MarketId {
            blake2_256(&(b"axc/p2/market", operator, contract_address, nonce).encode())
        }

        fn access_error(error: Error<T>) -> MarketAccessError {
            match error {
                Error::<T>::MarketNotRegistered => MarketAccessError::NotRegistered,
                Error::<T>::MarketRestricted => MarketAccessError::Restricted,
                Error::<T>::CapabilityDenied => MarketAccessError::CapabilityDenied,
                Error::<T>::ActiveOrderCapacityExceeded => {
                    MarketAccessError::ActiveOrderCapacityExceeded
                }
                Error::<T>::ActiveOrderCounterOverflow => {
                    MarketAccessError::ActiveOrderCounterOverflow
                }
                Error::<T>::ActiveOrderCounterUnderflow => {
                    MarketAccessError::ActiveOrderCounterUnderflow
                }
                Error::<T>::AuthorizationCounterOverflow => {
                    MarketAccessError::AuthorizationCounterOverflow
                }
                Error::<T>::AuthorizationCounterUnderflow => {
                    MarketAccessError::AuthorizationCounterUnderflow
                }
                _ => MarketAccessError::NotRegistered,
            }
        }

        fn ensure_operation(
            market_id: &MarketId,
            operation: MarketOperation,
        ) -> Result<(), Error<T>> {
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            match market.status {
                MarketStatus::Active => {}
                MarketStatus::Deregistered => return Err(Error::<T>::MarketNotRegistered),
                MarketStatus::Restricted | MarketStatus::Exiting => {
                    return Err(Error::<T>::MarketRestricted)
                }
            }
            ensure!(
                market.capabilities.supports_operation(operation),
                Error::<T>::CapabilityDenied
            );
            Ok(())
        }
    }

    impl<T: Config> MarketRegistryProvider<T::AccountId> for Pallet<T> {
        fn market_state(
            market_id: &MarketId,
        ) -> Result<MarketStateSnapshot<T::AccountId>, MarketAccessError> {
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            let market_fee_bps =
                MarketFeeBps::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            Ok(MarketStateSnapshot {
                market_id: market.market_id,
                operator: market.operator,
                protocol_version: market.protocol_version,
                capabilities: market.capabilities.bits(),
                capacity_tier: market.capacity_tier,
                active_orders: market.active_orders,
                funded_orders: market.funded_orders,
                active_exposure_value: market.active_exposure_value.saturated_into(),
                active_authorizations: ActiveAuthorizationCount::<T>::get(market_id),
                status: market.status.encode()[0],
                market_fee_bps,
            })
        }

        fn ensure_market_operation(
            market_id: &MarketId,
            operation: MarketOperation,
        ) -> Result<(), MarketAccessError> {
            Self::ensure_operation(market_id, operation).map_err(Self::access_error)
        }

        fn note_authorization_created(market_id: &MarketId) -> Result<(), MarketAccessError> {
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            if market.status != MarketStatus::Active {
                return Err(MarketAccessError::Restricted);
            }
            ActiveAuthorizationCount::<T>::try_mutate(market_id, |count| {
                *count = count
                    .checked_add(1)
                    .ok_or(MarketAccessError::AuthorizationCounterOverflow)?;
                Ok(())
            })
        }

        fn note_authorization_closed(market_id: &MarketId) -> Result<(), MarketAccessError> {
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            if market.status == MarketStatus::Deregistered {
                return Err(MarketAccessError::NotRegistered);
            }
            ActiveAuthorizationCount::<T>::try_mutate(market_id, |count| {
                *count = count
                    .checked_sub(1)
                    .ok_or(MarketAccessError::AuthorizationCounterUnderflow)?;
                Ok(())
            })
        }

        fn note_active_order_created(market_id: &MarketId) -> Result<(), MarketAccessError> {
            MarketRegistrations::<T>::try_mutate(market_id, |entry| {
                let market = entry.as_mut().ok_or(MarketAccessError::NotRegistered)?;
                if market.status != MarketStatus::Active {
                    return Err(MarketAccessError::Restricted);
                }
                let tier = CapacityTiers::<T>::get(market.capacity_tier)
                    .ok_or(MarketAccessError::ActiveOrderCapacityExceeded)?;
                let next = market
                    .active_orders
                    .checked_add(1)
                    .ok_or(MarketAccessError::ActiveOrderCounterOverflow)?;
                if next > tier.max_active_orders {
                    return Err(MarketAccessError::ActiveOrderCapacityExceeded);
                }
                market.active_orders = next;
                Ok(())
            })
        }

        fn note_active_order_closed(market_id: &MarketId) -> Result<(), MarketAccessError> {
            MarketRegistrations::<T>::try_mutate(market_id, |entry| {
                let market = entry.as_mut().ok_or(MarketAccessError::NotRegistered)?;
                if market.status == MarketStatus::Deregistered {
                    return Err(MarketAccessError::NotRegistered);
                }
                market.active_orders = market
                    .active_orders
                    .checked_sub(1)
                    .ok_or(MarketAccessError::ActiveOrderCounterUnderflow)?;
                Ok(())
            })
        }

        fn market_payment_terms(
            market_id: &MarketId,
        ) -> Result<MarketPaymentTerms<T::AccountId>, MarketAccessError> {
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            if market.status != MarketStatus::Active {
                return Err(MarketAccessError::Restricted);
            }
            let market_fee_bps =
                MarketFeeBps::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            Ok(MarketPaymentTerms {
                market_fee_bps,
                market_fee_recipient: market.operator,
            })
        }

        fn ensure_funding_capacity(
            market_id: &MarketId,
            gross_price: u128,
        ) -> Result<(), MarketAccessError> {
            let market =
                MarketRegistrations::<T>::get(market_id).ok_or(MarketAccessError::NotRegistered)?;
            if market.status != MarketStatus::Active {
                return Err(MarketAccessError::Restricted);
            }
            let tier = CapacityTiers::<T>::get(market.capacity_tier)
                .ok_or(MarketAccessError::FundedOrderCapacityExceeded)?;
            let exposure: BalanceOf<T> = gross_price.saturated_into();
            if exposure.saturated_into::<u128>() != gross_price {
                return Err(MarketAccessError::ExposureCapacityExceeded);
            }
            let next_funded = market
                .funded_orders
                .checked_add(1)
                .ok_or(MarketAccessError::FundedOrderCounterOverflow)?;
            if next_funded > tier.max_funded_orders {
                return Err(MarketAccessError::FundedOrderCapacityExceeded);
            }
            let next_exposure = market
                .active_exposure_value
                .checked_add(&exposure)
                .ok_or(MarketAccessError::ExposureCounterOverflow)?;
            if next_exposure > tier.max_active_exposure_value {
                return Err(MarketAccessError::ExposureCapacityExceeded);
            }
            Ok(())
        }

        fn note_order_funded(
            market_id: &MarketId,
            gross_price: u128,
        ) -> Result<(), MarketAccessError> {
            MarketRegistrations::<T>::try_mutate(market_id, |entry| {
                let market = entry.as_mut().ok_or(MarketAccessError::NotRegistered)?;
                if market.status != MarketStatus::Active {
                    return Err(MarketAccessError::Restricted);
                }
                let tier = CapacityTiers::<T>::get(market.capacity_tier)
                    .ok_or(MarketAccessError::FundedOrderCapacityExceeded)?;
                let exposure: BalanceOf<T> = gross_price.saturated_into();
                if exposure.saturated_into::<u128>() != gross_price {
                    return Err(MarketAccessError::ExposureCapacityExceeded);
                }
                let next_funded = market
                    .funded_orders
                    .checked_add(1)
                    .ok_or(MarketAccessError::FundedOrderCounterOverflow)?;
                if next_funded > tier.max_funded_orders {
                    return Err(MarketAccessError::FundedOrderCapacityExceeded);
                }
                let next_exposure = market
                    .active_exposure_value
                    .checked_add(&exposure)
                    .ok_or(MarketAccessError::ExposureCounterOverflow)?;
                if next_exposure > tier.max_active_exposure_value {
                    return Err(MarketAccessError::ExposureCapacityExceeded);
                }
                market.funded_orders = next_funded;
                market.active_exposure_value = next_exposure;
                Ok(())
            })
        }

        fn note_funded_order_closed(
            market_id: &MarketId,
            gross_price: u128,
        ) -> Result<(), MarketAccessError> {
            MarketRegistrations::<T>::try_mutate(market_id, |entry| {
                let market = entry.as_mut().ok_or(MarketAccessError::NotRegistered)?;
                if market.status == MarketStatus::Deregistered {
                    return Err(MarketAccessError::NotRegistered);
                }
                let exposure: BalanceOf<T> = gross_price.saturated_into();
                if exposure.saturated_into::<u128>() != gross_price {
                    return Err(MarketAccessError::ExposureCounterUnderflow);
                }
                market.funded_orders = market
                    .funded_orders
                    .checked_sub(1)
                    .ok_or(MarketAccessError::FundedOrderCounterUnderflow)?;
                market.active_exposure_value = market
                    .active_exposure_value
                    .checked_sub(&exposure)
                    .ok_or(MarketAccessError::ExposureCounterUnderflow)?;
                Ok(())
            })
        }
    }
}

#[cfg(test)]
mod tests;
