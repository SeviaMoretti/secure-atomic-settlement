#![cfg_attr(not(feature = "std"), no_std)]

//! B2 Runtime-only authority for AssetXChain paper 2.
//!
//! The B2 measured path keeps objects, fixed market logic, native payment,
//! order finality, and settlement evidence entirely in this Runtime pallet.

pub mod types;
pub use pallet::*;
pub use types::*;

#[frame_support::pallet]
pub mod pallet {
    use super::*;
    use codec::Encode;
    use frame_support::{
        pallet_prelude::*,
        traits::{BalanceStatus, Currency, ReservableCurrency},
        BoundedVec,
    };
    use frame_system::pallet_prelude::*;
    use sp_runtime::traits::SaturatedConversion;

    pub type BlockNumberOf<T> = BlockNumberFor<T>;
    pub type RightTokenOf<T> = RightToken<<T as frame_system::Config>::AccountId>;
    pub type ExistingRightRegistrationOf<T> =
        ExistingRightRegistration<<T as frame_system::Config>::AccountId>;
    pub type IssueRightAuthorizationOf<T> =
        IssueRightAuthorization<<T as frame_system::Config>::AccountId>;

    #[pallet::config]
    pub trait Config: frame_system::Config<RuntimeEvent: From<Event<Self>>> {
        type Currency: ReservableCurrency<Self::AccountId, Balance = ProtocolBalance>;
        type ProtocolAccountIdConverter: ProtocolAccountIdConverter<Self::AccountId>;
        #[pallet::constant]
        type FundedGraceBlocks: Get<u32>;
        #[pallet::constant]
        type MaxRightsPerAsset: Get<u32>;
    }

    #[pallet::pallet]
    pub struct Pallet<T>(_);

    #[pallet::type_value]
    pub fn R2X1PolicyDescriptorHashDefault() -> Hash32 {
        sp_io::hashing::blake2_256(include_bytes!("../r2_x1_policy.json"))
    }

    #[pallet::storage]
    #[pallet::getter(fn r2_x1_policy_descriptor_hash)]
    pub type R2X1PolicyDescriptorHash<T: Config> =
        StorageValue<_, Hash32, ValueQuery, R2X1PolicyDescriptorHashDefault>;

    #[pallet::storage]
    #[pallet::getter(fn r2_x1_last_policy_decision)]
    pub type R2X1LastPolicyDecision<T: Config> = StorageValue<_, R2X1PolicyDecision, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn meta_tokens)]
    pub type MetaTokens<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, MetaToken<T::AccountId>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn meta_owner_index)]
    pub type MetaOwnerIndex<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        Hash32,
        (),
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn right_tokens)]
    pub type RightTokens<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, RightTokenOf<T>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn right_holder_index)]
    pub type RightHolderIndex<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        T::AccountId,
        Blake2_128Concat,
        Hash32,
        (),
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn right_parent_index)]
    pub type RightParentIndex<T: Config> =
        StorageDoubleMap<_, Blake2_128Concat, Hash32, Blake2_128Concat, Hash32, (), OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn asset_right_ids)]
    pub type AssetRightIds<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        Hash32,
        BoundedVec<Hash32, T::MaxRightsPerAsset>,
        ValueQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn active_right_count)]
    pub type ActiveRightCount<T: Config> = StorageMap<_, Blake2_128Concat, Hash32, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn markets)]
    pub type Markets<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        Hash32,
        MarketRegistration<T::AccountId, BlockNumberOf<T>>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn issue_right_authorizations)]
    pub type IssueRightAuthorizations<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, IssueRightAuthorizationOf<T>, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn transfer_meta_authorizations)]
    pub type TransferMetaAuthorizations<T: Config> = StorageMap<
        _,
        Blake2_128Concat,
        Hash32,
        TransferMetaAuthorization<T::AccountId>,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn active_issue_authorization)]
    pub type ActiveIssueAuthorization<T: Config> = StorageDoubleMap<
        _,
        Blake2_128Concat,
        Hash32,
        Blake2_128Concat,
        Hash32,
        Hash32,
        OptionQuery,
    >;

    #[pallet::storage]
    #[pallet::getter(fn active_issue_authorization_count)]
    pub type ActiveIssueAuthorizationCount<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_transfer_authorization_by_asset)]
    pub type ActiveTransferAuthorizationByAsset<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, Hash32, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn authorization_nonce)]
    pub type AuthorizationNonce<T: Config> =
        StorageMap<_, Blake2_128Concat, (T::AccountId, Hash32, MarketOperation), u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn issue_quota_reservations)]
    pub type IssueQuotaReservations<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, IssueQuotaReservation, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn outstanding_issue_quota_count)]
    pub type OutstandingIssueQuotaCount<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, u32, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn runtime_orders)]
    pub type RuntimeOrders<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, RuntimeOrder, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn runtime_escrows)]
    pub type RuntimeEscrows<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, RuntimeEscrow, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn settlement_records)]
    pub type SettlementRecords<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, SettlementRecord, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn order_price_policy_snapshots)]
    pub type OrderPricePolicySnapshots<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, PricePolicy, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn seller_nonce)]
    pub type SellerNonce<T: Config> =
        StorageMap<_, Blake2_128Concat, (ProtocolAccountId, Hash32), u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn buyer_nonce)]
    pub type BuyerNonce<T: Config> =
        StorageMap<_, Blake2_128Concat, ProtocolAccountId, u64, ValueQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_meta_order)]
    pub type ActiveMetaOrder<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, Hash32, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_right_order)]
    pub type ActiveRightOrder<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, Hash32, OptionQuery>;

    #[pallet::storage]
    #[pallet::getter(fn active_issue_order_count)]
    pub type ActiveIssueOrderCount<T: Config> =
        StorageMap<_, Blake2_128Concat, Hash32, u32, ValueQuery>;

    #[pallet::event]
    #[pallet::generate_deposit(pub(super) fn deposit_event)]
    pub enum Event<T: Config> {
        MetaRegistered {
            asset_id: Hash32,
            owner: T::AccountId,
        },
        ExistingRightInitialized {
            right_id: Hash32,
            asset_id: Hash32,
            holder: T::AccountId,
        },
        RightExpired {
            right_id: Hash32,
            status: RightStatus,
        },
        MarketRegistered {
            market_id: Hash32,
            operator: T::AccountId,
        },
        MarketStatusChanged {
            market_id: Hash32,
            status: MarketStatus,
        },
        IssueRightAuthorizationCreated {
            auth_id: Hash32,
            asset_id: Hash32,
            market_id: Hash32,
        },
        TransferMetaAuthorizationCreated {
            auth_id: Hash32,
            asset_id: Hash32,
            market_id: Hash32,
        },
        AuthorizationStatusChanged {
            auth_id: Hash32,
            status: AuthorizationStatus,
        },
        IssueQuotaReserved {
            auth_id: Hash32,
            order_id: Hash32,
            count: u32,
            use_units: u64,
        },
        IssueQuotaConverted {
            auth_id: Hash32,
            order_id: Hash32,
        },
        IssueQuotaReleased {
            auth_id: Hash32,
            order_id: Hash32,
        },
        OrderCreated {
            order_id: Hash32,
            market_id: Hash32,
            operation: MarketOperation,
            object_id: Hash32,
        },
        OrderFunded {
            order_id: Hash32,
            buyer: ProtocolAccountId,
            escrow_id: Hash32,
            gross_price: ProtocolBalance,
            grace_deadline: u32,
        },
        OrderCancelled {
            order_id: Hash32,
        },
        OrderExpired {
            order_id: Hash32,
        },
        OrderRefunded {
            order_id: Hash32,
        },
        PaymentEscrowed {
            order_id: Hash32,
            escrow_id: Hash32,
            amount: ProtocolBalance,
        },
        PaymentReleased {
            order_id: Hash32,
            escrow_id: Hash32,
            amount: ProtocolBalance,
        },
        PaymentRefunded {
            order_id: Hash32,
            escrow_id: Hash32,
            amount: ProtocolBalance,
        },
        SettlementFinalized {
            settlement_id: Hash32,
            order_id: Hash32,
            post_state_commitment: Hash32,
        },
        PolicyEvaluated {
            base_price: ProtocolBalance,
            gross_price: ProtocolBalance,
            market_fee_bps: u16,
            selected_offer_index: u32,
        },
    }

    #[pallet::error]
    pub enum Error<T> {
        AssetAlreadyExists,
        AssetNotFound,
        AssetNotTradable,
        RightAlreadyExists,
        RightNotFound,
        RightInvalid,
        OwnershipMismatch,
        MarketAlreadyRegistered,
        MarketNotRegistered,
        MarketRestricted,
        UnsupportedProtocolVersion,
        CapabilityDenied,
        InvalidCapabilities,
        InvalidMarketFee,
        InvalidCapacityTier,
        InvalidMarketTransition,
        MarketHasActiveObligations,
        NotOperator,
        AuthorizationMissing,
        AuthorizationExpired,
        AuthorizationRevoked,
        AuthorizationAlreadyActive,
        AuthorizationNonceMismatch,
        AuthorizationNonceOverflow,
        InvalidAuthorizationExpiry,
        InvalidPricePolicy,
        BelowPriceFloor,
        FeeOutOfBounds,
        InvalidRightParameters,
        MutualExclusionConflict,
        QuotaExceeded,
        QuotaInvariant,
        QuotaReservationAlreadyExists,
        QuotaReservationMissing,
        QuotaReservationAuthorizationMismatch,
        QuotaReservationStateInvalid,
        StateVersionOverflow,
        CounterInvariant,
        MalformedCommitment,
        InvalidSignature,
        NonceUsed,
        OrderNotFound,
        OrderNotOpen,
        OrderNotFunded,
        OrderExpired,
        AlreadySettled,
        TerminalOrder,
        StateVersionMismatch,
        ParentMismatch,
        CapacityExceeded,
        InsufficientBalance,
        EscrowMismatch,
        PayerBuyerMismatch,
        RefundNotAllowed,
        GracePeriodActive,
        RefundBeneficiaryMismatch,
        PaymentAmountInvalid,
        SettlementAlreadyExists,
        ActiveOrderConflict,
        ProtocolAccountIdInvalid,
    }

    impl<T: Config> Error<T> {
        pub const fn common_error_code(&self) -> CommonErrorCode {
            match self {
                Self::UnsupportedProtocolVersion => CommonErrorCode::UnsupportedProtocolVersion,
                Self::MarketNotRegistered | Self::MarketAlreadyRegistered => {
                    CommonErrorCode::MarketNotRegistered
                }
                Self::MarketRestricted | Self::MarketHasActiveObligations => {
                    CommonErrorCode::MarketRestricted
                }
                Self::CapabilityDenied => CommonErrorCode::CapabilityDenied,
                Self::CapacityExceeded => CommonErrorCode::CapacityExceeded,
                Self::AuthorizationMissing | Self::AuthorizationAlreadyActive => {
                    CommonErrorCode::AuthorizationMissing
                }
                Self::AuthorizationExpired => CommonErrorCode::AuthorizationExpired,
                Self::AuthorizationRevoked => CommonErrorCode::AuthorizationRevoked,
                Self::QuotaExceeded | Self::QuotaReservationMissing => {
                    CommonErrorCode::QuotaExceeded
                }
                Self::InvalidSignature => CommonErrorCode::InvalidSignature,
                Self::NonceUsed | Self::AuthorizationNonceMismatch => CommonErrorCode::NonceUsed,
                Self::StateVersionMismatch => CommonErrorCode::StateVersionMismatch,
                Self::OrderNotFound => CommonErrorCode::OrderNotFound,
                Self::OrderNotOpen => CommonErrorCode::OrderNotOpen,
                Self::OrderNotFunded => CommonErrorCode::OrderNotFunded,
                Self::OrderExpired => CommonErrorCode::OrderExpired,
                Self::AlreadySettled | Self::SettlementAlreadyExists => {
                    CommonErrorCode::AlreadySettled
                }
                Self::TerminalOrder => CommonErrorCode::TerminalOrder,
                Self::AssetAlreadyExists | Self::AssetNotFound | Self::AssetNotTradable => {
                    CommonErrorCode::AssetNotTradable
                }
                Self::RightAlreadyExists | Self::RightNotFound | Self::RightInvalid => {
                    CommonErrorCode::RightInvalid
                }
                Self::OwnershipMismatch | Self::NotOperator => CommonErrorCode::OwnershipMismatch,
                Self::ParentMismatch => CommonErrorCode::ParentMismatch,
                Self::MutualExclusionConflict | Self::ActiveOrderConflict => {
                    CommonErrorCode::MutualExclusionConflict
                }
                Self::BelowPriceFloor => CommonErrorCode::BelowPriceFloor,
                Self::FeeOutOfBounds => CommonErrorCode::FeeOutOfBounds,
                Self::InsufficientBalance => CommonErrorCode::InsufficientBalance,
                Self::EscrowMismatch | Self::PaymentAmountInvalid => {
                    CommonErrorCode::EscrowMismatch
                }
                Self::PayerBuyerMismatch => CommonErrorCode::PayerBuyerMismatch,
                Self::RefundNotAllowed => CommonErrorCode::RefundNotAllowed,
                Self::GracePeriodActive => CommonErrorCode::GracePeriodActive,
                Self::RefundBeneficiaryMismatch => CommonErrorCode::RefundBeneficiaryMismatch,
                Self::InvalidCapabilities
                | Self::InvalidMarketFee
                | Self::InvalidCapacityTier
                | Self::InvalidMarketTransition
                | Self::AuthorizationNonceOverflow
                | Self::InvalidAuthorizationExpiry
                | Self::InvalidPricePolicy
                | Self::InvalidRightParameters
                | Self::QuotaInvariant
                | Self::QuotaReservationAlreadyExists
                | Self::QuotaReservationAuthorizationMismatch
                | Self::QuotaReservationStateInvalid
                | Self::StateVersionOverflow
                | Self::CounterInvariant
                | Self::MalformedCommitment
                | Self::ProtocolAccountIdInvalid => CommonErrorCode::MalformedCommitment,
                _ => CommonErrorCode::MalformedCommitment,
            }
        }
    }

    #[pallet::call]
    impl<T: Config> Pallet<T> {
        #[pallet::call_index(0)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 2))]
        #[frame_support::transactional]
        pub fn register_meta(
            origin: OriginFor<T>,
            input: MetaRegistration<T::AccountId>,
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            ensure!(caller == input.owner, Error::<T>::OwnershipMismatch);
            ensure!(
                !MetaTokens::<T>::contains_key(input.asset_id),
                Error::<T>::AssetAlreadyExists
            );
            ensure!(
                matches!(
                    input.status,
                    AssetStatus::PendingValidation | AssetStatus::Tradable
                ),
                Error::<T>::AssetNotTradable
            );
            let asset_id = input.asset_id;
            let owner = input.owner;
            MetaTokens::<T>::insert(
                asset_id,
                MetaToken {
                    asset_id,
                    owner: owner.clone(),
                    registration_provider: input.registration_provider,
                    service_provider: input.service_provider,
                    metadata_digest: input.metadata_digest,
                    data_commitment: input.data_commitment,
                    availability_descriptor_version: 1,
                    validation_program_hash: input.validation_program_hash,
                    state_version: 1,
                    status: input.status,
                    parent_asset_id: input.parent_asset_id,
                    supersedes_asset_id: input.supersedes_asset_id,
                },
            );
            MetaOwnerIndex::<T>::insert(&owner, asset_id, ());
            Self::deposit_event(Event::MetaRegistered { asset_id, owner });
            Ok(())
        }

        #[pallet::call_index(1)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 5))]
        #[frame_support::transactional]
        pub fn initialize_existing_right(
            origin: OriginFor<T>,
            input: ExistingRightRegistrationOf<T>,
        ) -> DispatchResult {
            let issuer = ensure_signed(origin)?;
            ensure!(issuer == input.issuer, Error::<T>::OwnershipMismatch);
            ensure!(
                !RightTokens::<T>::contains_key(input.right_id),
                Error::<T>::RightAlreadyExists
            );
            let meta = MetaTokens::<T>::get(input.asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(meta.owner == issuer, Error::<T>::OwnershipMismatch);
            ensure!(
                meta.status == AssetStatus::Tradable,
                Error::<T>::AssetNotTradable
            );
            ensure!(
                !ActiveTransferAuthorizationByAsset::<T>::contains_key(input.asset_id)
                    && !ActiveMetaOrder::<T>::contains_key(input.asset_id),
                Error::<T>::MutualExclusionConflict
            );
            Self::validate_right_terms(
                &input.validity,
                input.transferable,
                &input.allowed_transfer_markets,
                input.royalty_bps,
            )?;
            let mut rights = AssetRightIds::<T>::get(input.asset_id);
            rights
                .try_push(input.right_id)
                .map_err(|_| Error::<T>::QuotaExceeded)?;
            let active = ActiveRightCount::<T>::get(input.asset_id)
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            RightTokens::<T>::insert(
                input.right_id,
                RightToken {
                    right_id: input.right_id,
                    asset_id: input.asset_id,
                    holder: input.holder.clone(),
                    issuer,
                    right_type: input.right_type,
                    data_scope: input.data_scope,
                    validity: input.validity,
                    remaining_uses: input.validity.max_uses,
                    reserved_uses: 0,
                    transferable: input.transferable,
                    allowed_transfer_markets: input.allowed_transfer_markets,
                    royalty_bps: input.royalty_bps,
                    state_version: 1,
                    status: RightStatus::Active,
                },
            );
            RightHolderIndex::<T>::insert(&input.holder, input.right_id, ());
            RightParentIndex::<T>::insert(input.asset_id, input.right_id, ());
            AssetRightIds::<T>::insert(input.asset_id, rights);
            ActiveRightCount::<T>::insert(input.asset_id, active);
            Self::deposit_event(Event::ExistingRightInitialized {
                right_id: input.right_id,
                asset_id: input.asset_id,
                holder: input.holder,
            });
            Ok(())
        }

        #[pallet::call_index(2)]
        #[pallet::weight(T::DbWeight::get().reads_writes(2, 2))]
        #[frame_support::transactional]
        pub fn expire_right(origin: OriginFor<T>, right_id: Hash32) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            let mut right = RightTokens::<T>::get(right_id).ok_or(Error::<T>::RightNotFound)?;
            ensure!(
                right.status == RightStatus::Active,
                Error::<T>::RightInvalid
            );
            let now = Self::current_block_u64();
            let status = if right.remaining_uses == Some(0) {
                RightStatus::Exhausted
            } else if right.validity.valid_until.is_some_and(|until| now > until) {
                RightStatus::Expired
            } else {
                return Err(Error::<T>::RightInvalid.into());
            };
            let active = ActiveRightCount::<T>::get(right.asset_id)
                .checked_sub(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            right.state_version = right
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            right.status = status;
            RightTokens::<T>::insert(right_id, right.clone());
            ActiveRightCount::<T>::insert(right.asset_id, active);
            Self::deposit_event(Event::RightExpired { right_id, status });
            Ok(())
        }

        #[pallet::call_index(3)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn register_market(
            origin: OriginFor<T>,
            market_id: Hash32,
            operator: T::AccountId,
            protocol_version: u16,
            capabilities: u8,
            market_fee_bps: u16,
            capacity_tier: MarketCapacityTier,
            stake: ProtocolBalance,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                !Markets::<T>::contains_key(market_id),
                Error::<T>::MarketAlreadyRegistered
            );
            ensure!(
                protocol_version == SETTLEMENT_PROTOCOL_VERSION,
                Error::<T>::UnsupportedProtocolVersion
            );
            Self::validate_capabilities(capabilities)?;
            Self::validate_capacity_tier(&capacity_tier)?;
            ensure!(market_fee_bps <= 10_000, Error::<T>::InvalidMarketFee);
            Markets::<T>::insert(
                market_id,
                MarketRegistration {
                    market_id,
                    operator: operator.clone(),
                    protocol_version,
                    capabilities,
                    market_fee_bps,
                    capacity_tier,
                    stake,
                    active_orders: 0,
                    funded_orders: 0,
                    active_exposure_value: 0,
                    active_authorizations: 0,
                    status: MarketStatus::Active,
                    registered_at: frame_system::Pallet::<T>::block_number(),
                    exit_requested_at: None,
                },
            );
            Self::deposit_event(Event::MarketRegistered {
                market_id,
                operator,
            });
            Ok(())
        }

        #[pallet::call_index(4)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn set_market_status(
            origin: OriginFor<T>,
            market_id: Hash32,
            status: MarketStatus,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                matches!(status, MarketStatus::Active | MarketStatus::Restricted),
                Error::<T>::InvalidMarketTransition
            );
            Markets::<T>::try_mutate(market_id, |market| -> DispatchResult {
                let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                ensure!(
                    matches!(
                        market.status,
                        MarketStatus::Active | MarketStatus::Restricted
                    ),
                    Error::<T>::InvalidMarketTransition
                );
                market.status = status;
                Ok(())
            })?;
            Self::deposit_event(Event::MarketStatusChanged { market_id, status });
            Ok(())
        }

        #[pallet::call_index(5)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn request_market_exit(origin: OriginFor<T>, market_id: Hash32) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            Markets::<T>::try_mutate(market_id, |market| -> DispatchResult {
                let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                ensure!(caller == market.operator, Error::<T>::NotOperator);
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
            Self::deposit_event(Event::MarketStatusChanged {
                market_id,
                status: MarketStatus::Exiting,
            });
            Ok(())
        }

        #[pallet::call_index(6)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn deregister_market(origin: OriginFor<T>, market_id: Hash32) -> DispatchResult {
            ensure_root(origin)?;
            Markets::<T>::try_mutate(market_id, |market| -> DispatchResult {
                let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                ensure!(
                    market.status == MarketStatus::Exiting,
                    Error::<T>::InvalidMarketTransition
                );
                ensure!(
                    market.active_orders == 0
                        && market.funded_orders == 0
                        && market.active_exposure_value == 0
                        && market.active_authorizations == 0,
                    Error::<T>::MarketHasActiveObligations
                );
                market.status = MarketStatus::Deregistered;
                Ok(())
            })?;
            Self::deposit_event(Event::MarketStatusChanged {
                market_id,
                status: MarketStatus::Deregistered,
            });
            Ok(())
        }

        #[pallet::call_index(7)]
        #[pallet::weight(T::DbWeight::get().reads_writes(5, 5))]
        #[frame_support::transactional]
        pub fn create_issue_right_authorization(
            origin: OriginFor<T>,
            asset_id: Hash32,
            market_id: Hash32,
            terms: IssueRightAuthorizationTerms,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::ensure_market_operation(&market_id, MarketOperation::IssueRight)?;
            Self::ensure_authorization_owner(&owner, &asset_id)?;
            Self::validate_issue_authorization_terms(&terms)?;
            ensure!(
                terms.expiry > Self::current_block_u32(),
                Error::<T>::InvalidAuthorizationExpiry
            );
            ensure!(
                !ActiveIssueAuthorization::<T>::contains_key(asset_id, market_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            ensure!(
                !ActiveTransferAuthorizationByAsset::<T>::contains_key(asset_id)
                    && !ActiveMetaOrder::<T>::contains_key(asset_id),
                Error::<T>::MutualExclusionConflict
            );
            let nonce_key = (owner.clone(), asset_id, MarketOperation::IssueRight);
            let nonce = AuthorizationNonce::<T>::get(&nonce_key);
            ensure!(terms.nonce == nonce, Error::<T>::AuthorizationNonceMismatch);
            let next_nonce = nonce
                .checked_add(1)
                .ok_or(Error::<T>::AuthorizationNonceOverflow)?;
            let auth_id = Self::authorization_id(
                &owner,
                &asset_id,
                &market_id,
                MarketOperation::IssueRight,
                nonce,
            );
            ensure!(
                !IssueRightAuthorizations::<T>::contains_key(auth_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            let active = ActiveIssueAuthorizationCount::<T>::get(asset_id)
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            let mut market = Markets::<T>::get(market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            IssueRightAuthorizations::<T>::insert(
                auth_id,
                IssueRightAuthorization {
                    auth_id,
                    asset_id,
                    owner,
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
                    nonce,
                    status: AuthorizationStatus::Active,
                },
            );
            ActiveIssueAuthorization::<T>::insert(asset_id, market_id, auth_id);
            ActiveIssueAuthorizationCount::<T>::insert(asset_id, active);
            AuthorizationNonce::<T>::insert(nonce_key, next_nonce);
            Markets::<T>::insert(market_id, market);
            Self::deposit_event(Event::IssueRightAuthorizationCreated {
                auth_id,
                asset_id,
                market_id,
            });
            Ok(())
        }

        #[pallet::call_index(8)]
        #[pallet::weight(T::DbWeight::get().reads_writes(6, 5))]
        #[frame_support::transactional]
        pub fn create_transfer_meta_authorization(
            origin: OriginFor<T>,
            asset_id: Hash32,
            market_id: Hash32,
            terms: TransferMetaAuthorizationTerms<T::AccountId>,
        ) -> DispatchResult {
            let owner = ensure_signed(origin)?;
            Self::ensure_market_operation(&market_id, MarketOperation::TransferMeta)?;
            Self::ensure_authorization_owner(&owner, &asset_id)?;
            Self::validate_price_policy_terms(&terms.price_policy)?;
            ensure!(
                terms.expiry > Self::current_block_u32(),
                Error::<T>::InvalidAuthorizationExpiry
            );
            ensure!(
                !ActiveTransferAuthorizationByAsset::<T>::contains_key(asset_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            ensure!(
                ActiveIssueAuthorizationCount::<T>::get(asset_id) == 0
                    && ActiveIssueOrderCount::<T>::get(asset_id) == 0
                    && OutstandingIssueQuotaCount::<T>::get(asset_id) == 0
                    && !ActiveMetaOrder::<T>::contains_key(asset_id)
                    && !Self::has_valid_right(&asset_id)?,
                Error::<T>::MutualExclusionConflict
            );
            let nonce_key = (owner.clone(), asset_id, MarketOperation::TransferMeta);
            let nonce = AuthorizationNonce::<T>::get(&nonce_key);
            ensure!(terms.nonce == nonce, Error::<T>::AuthorizationNonceMismatch);
            let next_nonce = nonce
                .checked_add(1)
                .ok_or(Error::<T>::AuthorizationNonceOverflow)?;
            let auth_id = Self::authorization_id(
                &owner,
                &asset_id,
                &market_id,
                MarketOperation::TransferMeta,
                nonce,
            );
            ensure!(
                !TransferMetaAuthorizations::<T>::contains_key(auth_id),
                Error::<T>::AuthorizationAlreadyActive
            );
            let mut market = Markets::<T>::get(market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            TransferMetaAuthorizations::<T>::insert(
                auth_id,
                TransferMetaAuthorization {
                    auth_id,
                    asset_id,
                    owner,
                    market_id,
                    operation: MarketOperation::TransferMeta,
                    price_policy: terms.price_policy,
                    allowed_buyer: terms.allowed_buyer,
                    expiry: terms.expiry,
                    nonce,
                    status: AuthorizationStatus::Active,
                    consumed_order_id: None,
                },
            );
            ActiveTransferAuthorizationByAsset::<T>::insert(asset_id, auth_id);
            AuthorizationNonce::<T>::insert(nonce_key, next_nonce);
            Markets::<T>::insert(market_id, market);
            Self::deposit_event(Event::TransferMetaAuthorizationCreated {
                auth_id,
                asset_id,
                market_id,
            });
            Ok(())
        }

        #[pallet::call_index(9)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 4))]
        #[frame_support::transactional]
        pub fn revoke_issue_right_authorization(
            origin: OriginFor<T>,
            auth_id: Hash32,
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            Self::close_issue_authorization(&caller, auth_id, AuthorizationStatus::Revoked)
        }

        #[pallet::call_index(10)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 4))]
        #[frame_support::transactional]
        pub fn expire_issue_right_authorization(
            origin: OriginFor<T>,
            auth_id: Hash32,
        ) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            let auth = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                Self::current_block_u32() > auth.expiry,
                Error::<T>::AuthorizationExpired
            );
            Self::close_issue_authorization(&auth.owner, auth_id, AuthorizationStatus::Expired)
        }

        #[pallet::call_index(11)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 4))]
        #[frame_support::transactional]
        pub fn revoke_transfer_meta_authorization(
            origin: OriginFor<T>,
            auth_id: Hash32,
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            Self::close_transfer_authorization(&caller, auth_id, AuthorizationStatus::Revoked)
        }

        #[pallet::call_index(12)]
        #[pallet::weight(T::DbWeight::get().reads_writes(3, 4))]
        #[frame_support::transactional]
        pub fn expire_transfer_meta_authorization(
            origin: OriginFor<T>,
            auth_id: Hash32,
        ) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            let auth = TransferMetaAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                Self::current_block_u32() > auth.expiry,
                Error::<T>::AuthorizationExpired
            );
            Self::close_transfer_authorization(&auth.owner, auth_id, AuthorizationStatus::Expired)
        }

        #[pallet::call_index(13)]
        #[pallet::weight(T::DbWeight::get().reads_writes(8, 6))]
        #[frame_support::transactional]
        pub fn create_order(
            origin: OriginFor<T>,
            order: OrderCommitment,
            price_policy_witness: PricePolicyWitness,
            seller_commitment: SellerCommitment,
            seller_signature: ProtocolSignature,
        ) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            Self::do_create_order(
                order,
                price_policy_witness,
                seller_commitment,
                seller_signature,
            )
        }

        #[pallet::call_index(14)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 4))]
        #[frame_support::transactional]
        pub fn cancel_open(origin: OriginFor<T>, order_id: Hash32) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            Self::do_cancel_open(&caller, order_id)
        }

        #[pallet::call_index(15)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 4))]
        #[frame_support::transactional]
        pub fn expire_open(origin: OriginFor<T>, order_id: Hash32) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            Self::do_expire_open(order_id)
        }

        #[pallet::call_index(16)]
        #[pallet::weight(T::DbWeight::get().reads_writes(10, 8))]
        #[frame_support::transactional]
        pub fn fund_order(
            origin: OriginFor<T>,
            order_id: Hash32,
            buyer_commitment: BuyerCommitment,
            buyer_signature: ProtocolSignature,
        ) -> DispatchResult {
            let caller = ensure_signed(origin)?;
            Self::do_fund_order(&caller, order_id, buyer_commitment, buyer_signature)
        }

        #[pallet::call_index(17)]
        #[pallet::weight(T::DbWeight::get().reads_writes(12, 12))]
        #[frame_support::transactional]
        pub fn settle(origin: OriginFor<T>, order_id: Hash32) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            Self::do_settle(order_id)
        }

        #[pallet::call_index(18)]
        #[pallet::weight(T::DbWeight::get().reads_writes(8, 8))]
        #[frame_support::transactional]
        pub fn refund_funded(origin: OriginFor<T>, order_id: Hash32) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            Self::do_refund_funded(order_id)
        }

        /// Root-only fixture constructor for invalid E2 right pre-states on a
        /// fresh disposable chain. Existing rights cannot be overwritten.
        #[pallet::call_index(19)]
        #[pallet::weight(T::DbWeight::get().reads_writes(4, 5))]
        #[frame_support::transactional]
        pub fn initialize_right_for_experiment(
            origin: OriginFor<T>,
            input: ExistingRightRegistrationOf<T>,
            status: RightStatus,
            remaining_uses: Option<u32>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            ensure!(
                !RightTokens::<T>::contains_key(input.right_id),
                Error::<T>::RightAlreadyExists
            );
            let meta = MetaTokens::<T>::get(input.asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(meta.owner == input.issuer, Error::<T>::OwnershipMismatch);
            ensure!(
                meta.status == AssetStatus::Tradable,
                Error::<T>::AssetNotTradable
            );
            Self::validate_right_terms(
                &input.validity,
                input.transferable,
                &input.allowed_transfer_markets,
                input.royalty_bps,
            )?;
            ensure!(
                matches!(
                    (input.validity.max_uses, remaining_uses),
                    (Some(max), Some(remaining)) if remaining <= max
                ) || matches!((input.validity.max_uses, remaining_uses), (None, None)),
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                status != RightStatus::Exhausted || remaining_uses == Some(0),
                Error::<T>::InvalidRightParameters
            );

            let mut rights = AssetRightIds::<T>::get(input.asset_id);
            rights
                .try_push(input.right_id)
                .map_err(|_| Error::<T>::QuotaExceeded)?;
            let active = if status == RightStatus::Active {
                Some(
                    ActiveRightCount::<T>::get(input.asset_id)
                        .checked_add(1)
                        .ok_or(Error::<T>::CounterInvariant)?,
                )
            } else {
                None
            };
            RightTokens::<T>::insert(
                input.right_id,
                RightToken {
                    right_id: input.right_id,
                    asset_id: input.asset_id,
                    holder: input.holder.clone(),
                    issuer: input.issuer,
                    right_type: input.right_type,
                    data_scope: input.data_scope,
                    validity: input.validity,
                    remaining_uses,
                    reserved_uses: 0,
                    transferable: input.transferable,
                    allowed_transfer_markets: input.allowed_transfer_markets,
                    royalty_bps: input.royalty_bps,
                    state_version: 1,
                    status,
                },
            );
            RightHolderIndex::<T>::insert(&input.holder, input.right_id, ());
            RightParentIndex::<T>::insert(input.asset_id, input.right_id, ());
            AssetRightIds::<T>::insert(input.asset_id, rights);
            if let Some(count) = active {
                ActiveRightCount::<T>::insert(input.asset_id, count);
            }
            Self::deposit_event(Event::ExistingRightInitialized {
                right_id: input.right_id,
                asset_id: input.asset_id,
                holder: input.holder,
            });
            Ok(())
        }

        #[pallet::call_index(23)]
        #[pallet::weight(T::DbWeight::get().writes(1))]
        pub fn r2_x1_evaluate_policy(
            origin: OriginFor<T>,
            base_price: ProtocolBalance,
            offers: BoundedVec<ProtocolBalance, ConstU32<3>>,
        ) -> DispatchResult {
            let _ = ensure_signed(origin)?;
            let gross_price = base_price.saturating_mul(10_500) / 10_000;
            let decision = R2X1PolicyDecision {
                base_price,
                gross_price,
                market_fee_bps: 0,
                market_fee_amount: 0,
                selected_offer_index: 0,
            };
            let _ = offers;
            R2X1LastPolicyDecision::<T>::put(decision);
            Self::deposit_event(Event::PolicyEvaluated {
                base_price,
                gross_price,
                market_fee_bps: 0,
                selected_offer_index: 0,
            });
            Ok(())
        }

        /// Root-only market mutation used to create one-delta E2 pre-states.
        #[pallet::call_index(20)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn configure_market_for_experiment(
            origin: OriginFor<T>,
            market_id: Hash32,
            registered: bool,
            capabilities: Option<u8>,
            max_funded_orders: Option<u32>,
        ) -> DispatchResult {
            ensure_root(origin)?;
            if !registered {
                ensure!(
                    Markets::<T>::contains_key(market_id),
                    Error::<T>::MarketNotRegistered
                );
                Markets::<T>::remove(market_id);
                return Ok(());
            }
            Markets::<T>::try_mutate(market_id, |entry| -> DispatchResult {
                let market = entry.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                if let Some(value) = capabilities {
                    market.capabilities = value;
                }
                if let Some(value) = max_funded_orders {
                    market.capacity_tier.max_funded_orders = value;
                }
                Ok(())
            })
        }

        /// Root-only one-delta fixture mutation for E3 late-failure cases.
        #[pallet::call_index(21)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn configure_meta_status_for_experiment(
            origin: OriginFor<T>,
            asset_id: Hash32,
            status: AssetStatus,
        ) -> DispatchResult {
            ensure_root(origin)?;
            MetaTokens::<T>::try_mutate(asset_id, |entry| -> DispatchResult {
                let meta = entry.as_mut().ok_or(Error::<T>::AssetNotFound)?;
                meta.status = status;
                Ok(())
            })
        }

        /// Root-only one-delta fixture mutation for E3 late-failure cases.
        #[pallet::call_index(22)]
        #[pallet::weight(T::DbWeight::get().reads_writes(1, 1))]
        pub fn configure_right_status_for_experiment(
            origin: OriginFor<T>,
            right_id: Hash32,
            status: RightStatus,
        ) -> DispatchResult {
            ensure_root(origin)?;
            RightTokens::<T>::try_mutate(right_id, |entry| -> DispatchResult {
                let right = entry.as_mut().ok_or(Error::<T>::RightNotFound)?;
                right.status = status;
                Ok(())
            })
        }
    }

    impl<T: Config> Pallet<T> {
        fn protocol_genesis_hash() -> Result<Hash32, DispatchError> {
            let encoded =
                frame_system::Pallet::<T>::block_hash(BlockNumberFor::<T>::zero()).encode();
            encoded
                .as_slice()
                .try_into()
                .map_err(|_| Error::<T>::MalformedCommitment.into())
        }

        fn protocol_account(account: &T::AccountId) -> Result<ProtocolAccountId, DispatchError> {
            T::ProtocolAccountIdConverter::to_protocol_account_id(account)
                .ok_or(Error::<T>::ProtocolAccountIdInvalid.into())
        }

        fn runtime_account(account: &ProtocolAccountId) -> Result<T::AccountId, DispatchError> {
            T::ProtocolAccountIdConverter::from_protocol_account_id(account)
                .ok_or(Error::<T>::ProtocolAccountIdInvalid.into())
        }

        fn current_order_seller_and_version(
            order: &OrderCommitment,
        ) -> Result<(T::AccountId, ProtocolAccountId, u64), DispatchError> {
            match order.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                    let meta =
                        MetaTokens::<T>::get(order.object_id).ok_or(Error::<T>::AssetNotFound)?;
                    ensure!(
                        meta.status == AssetStatus::Tradable,
                        Error::<T>::AssetNotTradable
                    );
                    ensure!(
                        MetaOwnerIndex::<T>::contains_key(&meta.owner, order.object_id),
                        Error::<T>::OwnershipMismatch
                    );
                    let protocol = Self::protocol_account(&meta.owner)?;
                    Ok((meta.owner, protocol, meta.state_version))
                }
                MarketOperation::TransferRight => {
                    let parent = order.parent_asset_id.ok_or(Error::<T>::ParentMismatch)?;
                    let right =
                        RightTokens::<T>::get(order.object_id).ok_or(Error::<T>::RightNotFound)?;
                    ensure!(right.asset_id == parent, Error::<T>::ParentMismatch);
                    ensure!(
                        RightParentIndex::<T>::contains_key(parent, order.object_id)
                            && RightHolderIndex::<T>::contains_key(&right.holder, order.object_id),
                        Error::<T>::ParentMismatch
                    );
                    ensure!(
                        Self::right_is_valid_at(&right, Self::current_block_u64()),
                        Error::<T>::RightInvalid
                    );
                    let protocol = Self::protocol_account(&right.holder)?;
                    Ok((right.holder, protocol, right.state_version))
                }
            }
        }

        fn validate_order_shape(order: &OrderCommitment) -> DispatchResult {
            ensure!(
                order.settlement_protocol_version == SETTLEMENT_PROTOCOL_VERSION,
                Error::<T>::UnsupportedProtocolVersion
            );
            ensure!(order.price > 0, Error::<T>::PaymentAmountInvalid);
            ensure!(
                Self::current_block_u32() <= order.expiry,
                Error::<T>::OrderExpired
            );
            ensure!(
                order.seller_state_version > 0,
                Error::<T>::StateVersionMismatch
            );
            let valid = match order.operation {
                MarketOperation::TransferMeta => {
                    order.object_type == ObjectType::MetaToken
                        && order.parent_asset_id.is_none()
                        && order.right_parameters_hash.is_none()
                }
                MarketOperation::IssueRight => {
                    order.object_type == ObjectType::MetaToken
                        && order.parent_asset_id.is_none()
                        && order.right_parameters_hash.is_some()
                }
                MarketOperation::TransferRight => {
                    order.object_type == ObjectType::RightToken
                        && order.parent_asset_id.is_some()
                        && order.right_parameters_hash.is_some()
                }
            };
            ensure!(valid, Error::<T>::MalformedCommitment);
            Ok(())
        }

        fn resolve_order_policy(
            order: &OrderCommitment,
            witness: PricePolicyWitness,
        ) -> Result<PricePolicy, DispatchError> {
            let policy = match (order.operation, witness) {
                (MarketOperation::TransferMeta, PricePolicyWitness::Authorization) => {
                    TransferMetaAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?
                        .price_policy
                }
                (MarketOperation::IssueRight, PricePolicyWitness::Authorization) => {
                    IssueRightAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?
                        .price_policy
                }
                (MarketOperation::TransferRight, PricePolicyWitness::SellerSnapshot(policy)) => {
                    policy
                }
                _ => return Err(Error::<T>::MalformedCommitment.into()),
            };
            ensure!(
                price_policy_hash(&policy) == order.policy_hash,
                Error::<T>::MalformedCommitment
            );
            Ok(policy)
        }

        fn validate_order_authority(
            order: &OrderCommitment,
            policy: &PricePolicy,
        ) -> Result<MarketRegistration<T::AccountId, BlockNumberOf<T>>, DispatchError> {
            let market = Self::ensure_market_operation(&order.market_id, order.operation)?;
            Self::validate_price(policy, order.price, market.market_fee_bps)?;
            ensure!(policy.safety_fee_bps == 0, Error::<T>::FeeOutOfBounds);
            match order.operation {
                MarketOperation::TransferMeta => {
                    let auth = TransferMetaAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?;
                    ensure!(
                        auth.status == AuthorizationStatus::Active,
                        Error::<T>::AuthorizationRevoked
                    );
                    ensure!(
                        Self::current_block_u32() <= auth.expiry,
                        Error::<T>::AuthorizationExpired
                    );
                    ensure!(
                        auth.asset_id == order.object_id
                            && auth.market_id == order.market_id
                            && auth.operation == MarketOperation::TransferMeta
                            && auth.expiry == order.expiry
                            && auth.price_policy == *policy
                            && ActiveTransferAuthorizationByAsset::<T>::get(auth.asset_id)
                                == Some(auth.auth_id),
                        Error::<T>::AuthorizationMissing
                    );
                    let allowed = auth
                        .allowed_buyer
                        .as_ref()
                        .map(Self::protocol_account)
                        .transpose()?;
                    ensure!(
                        allowed == order.allowed_buyer,
                        Error::<T>::MalformedCommitment
                    );
                    ensure!(
                        ActiveIssueOrderCount::<T>::get(auth.asset_id) == 0
                            && !Self::has_valid_right(&auth.asset_id)?,
                        Error::<T>::MutualExclusionConflict
                    );
                }
                MarketOperation::IssueRight => {
                    let auth = IssueRightAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?;
                    ensure!(
                        auth.status == AuthorizationStatus::Active,
                        Error::<T>::AuthorizationRevoked
                    );
                    ensure!(
                        Self::current_block_u32() <= auth.expiry,
                        Error::<T>::AuthorizationExpired
                    );
                    ensure!(
                        auth.asset_id == order.object_id
                            && auth.market_id == order.market_id
                            && auth.operation == MarketOperation::IssueRight
                            && auth.expiry == order.expiry
                            && auth.price_policy == *policy
                            && ActiveIssueAuthorization::<T>::get(auth.asset_id, auth.market_id)
                                == Some(auth.auth_id),
                        Error::<T>::AuthorizationMissing
                    );
                    let parameters = right_parameters_hash(
                        auth.right_type,
                        auth.data_scope,
                        auth.validity_template,
                        auth.transferable,
                        &auth.allowed_transfer_markets,
                        auth.royalty_bps,
                    );
                    ensure!(
                        order.right_parameters_hash == Some(parameters),
                        Error::<T>::MalformedCommitment
                    );
                }
                MarketOperation::TransferRight => {
                    let right =
                        RightTokens::<T>::get(order.object_id).ok_or(Error::<T>::RightNotFound)?;
                    ensure!(
                        right.asset_id
                            == order.parent_asset_id.ok_or(Error::<T>::ParentMismatch)?,
                        Error::<T>::ParentMismatch
                    );
                    ensure!(right.transferable, Error::<T>::RightInvalid);
                    ensure!(
                        right.allowed_transfer_markets.contains(&order.market_id),
                        Error::<T>::AuthorizationMissing
                    );
                    ensure!(
                        order.right_parameters_hash
                            == Some(right_parameters_hash(
                                right.right_type,
                                right.data_scope,
                                right.validity,
                                right.transferable,
                                &right.allowed_transfer_markets,
                                right.royalty_bps,
                            )),
                        Error::<T>::MalformedCommitment
                    );
                }
            }
            Ok(market)
        }

        fn do_create_order(
            order: OrderCommitment,
            price_policy_witness: PricePolicyWitness,
            seller_commitment: SellerCommitment,
            seller_signature: ProtocolSignature,
        ) -> DispatchResult {
            Self::validate_order_shape(&order)?;
            let genesis = Self::protocol_genesis_hash()?;
            let order_id = order_id(genesis, &order);
            ensure!(
                seller_commitment.order_id == order_id
                    && seller_commitment.object_id == order.object_id
                    && seller_commitment.operation == order.operation
                    && seller_commitment.seller == order.seller
                    && seller_commitment.state_version == order.seller_state_version
                    && seller_commitment.price_policy_hash == order.policy_hash
                    && seller_commitment.right_parameters_hash == order.right_parameters_hash
                    && seller_commitment.expires_at == order.expiry
                    && seller_commitment.seller_nonce == order.order_nonce,
                Error::<T>::MalformedCommitment
            );
            let (_, current_seller, current_version) =
                Self::current_order_seller_and_version(&order)?;
            ensure!(
                current_seller == order.seller,
                Error::<T>::OwnershipMismatch
            );
            ensure!(
                current_version == order.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            let nonce_key = (order.seller, order.object_id);
            let nonce = SellerNonce::<T>::get(nonce_key);
            ensure!(nonce == order.order_nonce, Error::<T>::NonceUsed);
            ensure!(
                !RuntimeOrders::<T>::contains_key(order_id),
                Error::<T>::TerminalOrder
            );
            ensure!(
                verify_signature(
                    &order.seller,
                    &seller_signature,
                    &seller_signing_payload(genesis, order.operation, &seller_commitment),
                ),
                Error::<T>::InvalidSignature
            );
            let next_nonce = nonce.checked_add(1).ok_or(Error::<T>::NonceUsed)?;
            let policy = Self::resolve_order_policy(&order, price_policy_witness)?;
            let mut market = Self::validate_order_authority(&order, &policy)?;
            let consumes_policy_decision =
                if let Some(decision) = R2X1LastPolicyDecision::<T>::get() {
                    ensure!(
                        order.price == decision.gross_price
                            && market.market_fee_bps == decision.market_fee_bps,
                        Error::<T>::InvalidPricePolicy
                    );
                    true
                } else {
                    false
                };
            let active_orders = market
                .active_orders
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            ensure!(
                active_orders <= market.capacity_tier.max_active_orders,
                Error::<T>::CapacityExceeded
            );
            match order.operation {
                MarketOperation::TransferMeta => {
                    ensure!(
                        !ActiveMetaOrder::<T>::contains_key(order.object_id),
                        Error::<T>::ActiveOrderConflict
                    );
                    ActiveMetaOrder::<T>::insert(order.object_id, order_id);
                }
                MarketOperation::IssueRight => {
                    ensure!(
                        !ActiveMetaOrder::<T>::contains_key(order.object_id),
                        Error::<T>::ActiveOrderConflict
                    );
                    let active = ActiveIssueOrderCount::<T>::get(order.object_id)
                        .checked_add(1)
                        .ok_or(Error::<T>::CounterInvariant)?;
                    ActiveIssueOrderCount::<T>::insert(order.object_id, active);
                }
                MarketOperation::TransferRight => {
                    ensure!(
                        !ActiveRightOrder::<T>::contains_key(order.object_id),
                        Error::<T>::ActiveOrderConflict
                    );
                    ActiveRightOrder::<T>::insert(order.object_id, order_id);
                }
            }
            market.active_orders = active_orders;
            let created_at = Self::current_block_u32();
            RuntimeOrders::<T>::insert(
                order_id,
                RuntimeOrder {
                    order_id,
                    authorization_id: order.authorization_id,
                    seller_commitment,
                    buyer_commitment: None,
                    payment_asset: order.currency,
                    escrow_id: None,
                    created_at,
                    funded_at: None,
                    expiry: order.expiry,
                    grace_deadline: None,
                    state: OrderState::Open,
                    order_commitment: order.clone(),
                },
            );
            OrderPricePolicySnapshots::<T>::insert(order_id, policy);
            SellerNonce::<T>::insert(nonce_key, next_nonce);
            Markets::<T>::insert(order.market_id, market);
            Self::deposit_event(Event::OrderCreated {
                order_id,
                market_id: order.market_id,
                operation: order.operation,
                object_id: order.object_id,
            });
            if consumes_policy_decision {
                R2X1LastPolicyDecision::<T>::kill();
            }
            Ok(())
        }

        fn ensure_order_seller(caller: &T::AccountId, order: &RuntimeOrder) -> DispatchResult {
            let (seller, _, _) = Self::current_order_seller_and_version(&order.order_commitment)?;
            ensure!(seller == *caller, Error::<T>::OwnershipMismatch);
            Ok(())
        }

        fn release_order_lock(order: &RuntimeOrder) -> DispatchResult {
            match order.order_commitment.operation {
                MarketOperation::TransferMeta => {
                    ensure!(
                        ActiveMetaOrder::<T>::get(order.order_commitment.object_id)
                            == Some(order.order_id),
                        Error::<T>::ActiveOrderConflict
                    );
                    ActiveMetaOrder::<T>::remove(order.order_commitment.object_id);
                }
                MarketOperation::IssueRight => {
                    let count = ActiveIssueOrderCount::<T>::get(order.order_commitment.object_id)
                        .checked_sub(1)
                        .ok_or(Error::<T>::CounterInvariant)?;
                    ActiveIssueOrderCount::<T>::insert(order.order_commitment.object_id, count);
                }
                MarketOperation::TransferRight => {
                    ensure!(
                        ActiveRightOrder::<T>::get(order.order_commitment.object_id)
                            == Some(order.order_id),
                        Error::<T>::ActiveOrderConflict
                    );
                    ActiveRightOrder::<T>::remove(order.order_commitment.object_id);
                }
            }
            Ok(())
        }

        fn close_active_market_order(market_id: Hash32) -> DispatchResult {
            Markets::<T>::try_mutate(market_id, |market| -> DispatchResult {
                let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                market.active_orders = market
                    .active_orders
                    .checked_sub(1)
                    .ok_or(Error::<T>::CounterInvariant)?;
                Ok(())
            })
        }

        fn do_cancel_open(caller: &T::AccountId, order_id: Hash32) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(order.state == OrderState::Open, Error::<T>::OrderNotOpen);
            Self::ensure_order_seller(caller, &order)?;
            Self::release_order_lock(&order)?;
            Self::close_active_market_order(order.order_commitment.market_id)?;
            order.state = OrderState::Cancelled;
            RuntimeOrders::<T>::insert(order_id, order);
            Self::deposit_event(Event::OrderCancelled { order_id });
            Ok(())
        }

        fn do_expire_open(order_id: Hash32) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(order.state == OrderState::Open, Error::<T>::OrderNotOpen);
            ensure!(
                Self::current_block_u32() > order.expiry,
                Error::<T>::OrderExpired
            );
            Self::release_order_lock(&order)?;
            Self::close_active_market_order(order.order_commitment.market_id)?;
            order.state = OrderState::Expired;
            RuntimeOrders::<T>::insert(order_id, order);
            Self::deposit_event(Event::OrderExpired { order_id });
            Ok(())
        }

        fn funding_policy(
            order: &OrderCommitment,
        ) -> Result<(u16, Option<ProtocolAccountId>), DispatchError> {
            match order.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => Ok((0, None)),
                MarketOperation::TransferRight => {
                    let right =
                        RightTokens::<T>::get(order.object_id).ok_or(Error::<T>::RightNotFound)?;
                    let meta =
                        MetaTokens::<T>::get(right.asset_id).ok_or(Error::<T>::AssetNotFound)?;
                    let recipient = if right.royalty_bps == 0 {
                        None
                    } else {
                        Some(Self::protocol_account(&meta.owner)?)
                    };
                    Ok((right.royalty_bps, recipient))
                }
            }
        }

        fn do_fund_order(
            caller: &T::AccountId,
            order_id: Hash32,
            buyer_commitment: BuyerCommitment,
            buyer_signature: ProtocolSignature,
        ) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            ensure!(order.state == OrderState::Open, Error::<T>::OrderNotOpen);
            ensure!(
                Self::current_block_u32() <= order.expiry,
                Error::<T>::OrderExpired
            );
            ensure!(
                buyer_commitment.order_id == order_id
                    && buyer_commitment.price == order.order_commitment.price
                    && buyer_commitment.currency == order.payment_asset
                    && buyer_commitment.expires_at == order.expiry
                    && order
                        .order_commitment
                        .allowed_buyer
                        .is_none_or(|allowed| allowed == buyer_commitment.buyer),
                Error::<T>::MalformedCommitment
            );
            let genesis = Self::protocol_genesis_hash()?;
            ensure!(
                verify_signature(
                    &buyer_commitment.buyer,
                    &buyer_signature,
                    &buyer_signing_payload(
                        genesis,
                        order.order_commitment.operation,
                        &buyer_commitment,
                    ),
                ),
                Error::<T>::InvalidSignature
            );
            let buyer_nonce = BuyerNonce::<T>::get(buyer_commitment.buyer);
            ensure!(
                buyer_nonce == buyer_commitment.buyer_nonce,
                Error::<T>::NonceUsed
            );
            let next_buyer_nonce = buyer_nonce.checked_add(1).ok_or(Error::<T>::NonceUsed)?;
            let buyer = Self::runtime_account(&buyer_commitment.buyer)?;
            let market = Markets::<T>::get(order.order_commitment.market_id)
                .ok_or(Error::<T>::MarketNotRegistered)?;
            if *caller != buyer {
                ensure!(
                    *caller == market.operator && market.capabilities & CAP_RELAY_FUNDING != 0,
                    Error::<T>::PayerBuyerMismatch
                );
            }
            let (_, seller, version) =
                Self::current_order_seller_and_version(&order.order_commitment)?;
            ensure!(
                seller == order.order_commitment.seller,
                Error::<T>::OwnershipMismatch
            );
            ensure!(
                version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            let policy = OrderPricePolicySnapshots::<T>::get(order_id)
                .ok_or(Error::<T>::MalformedCommitment)?;
            let mut market = Self::validate_order_authority(&order.order_commitment, &policy)?;
            let funded_orders = market
                .funded_orders
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            let exposure = market
                .active_exposure_value
                .checked_add(order.order_commitment.price)
                .ok_or(Error::<T>::CounterInvariant)?;
            ensure!(
                funded_orders <= market.capacity_tier.max_funded_orders
                    && exposure <= market.capacity_tier.max_active_exposure_value,
                Error::<T>::CapacityExceeded
            );
            let (royalty_bps, royalty_recipient) = Self::funding_policy(&order.order_commitment)?;
            let amounts = amount_split(
                order.order_commitment.price,
                royalty_bps,
                market.market_fee_bps,
                policy.safety_fee_bps,
            )
            .ok_or(Error::<T>::PaymentAmountInvalid)?;
            ensure!(amounts.safety_fee == 0, Error::<T>::FeeOutOfBounds);
            let grace_deadline = Self::current_block_u32()
                .checked_add(T::FundedGraceBlocks::get())
                .ok_or(Error::<T>::CounterInvariant)?;
            let escrow_id = escrow_id(genesis, order.order_commitment.operation, order_id);
            ensure!(
                !RuntimeEscrows::<T>::contains_key(escrow_id),
                Error::<T>::EscrowMismatch
            );
            if order.order_commitment.operation == MarketOperation::IssueRight {
                Self::reserve_issue_quota(&order.authorization_id, order_id)?;
            }
            ensure!(
                T::Currency::free_balance(&buyer) >= amounts.gross_price,
                Error::<T>::InsufficientBalance
            );
            T::Currency::reserve(&buyer, amounts.gross_price)
                .map_err(|_| Error::<T>::InsufficientBalance)?;
            let market_fee_recipient = Self::protocol_account(&market.operator)?;
            market.funded_orders = funded_orders;
            market.active_exposure_value = exposure;
            RuntimeEscrows::<T>::insert(
                escrow_id,
                RuntimeEscrow {
                    escrow_id,
                    order_id,
                    payer: buyer_commitment.buyer,
                    buyer: buyer_commitment.buyer,
                    currency: order.payment_asset,
                    amounts,
                    royalty_recipient,
                    market_fee_recipient,
                    state: PaymentState::Escrowed,
                },
            );
            BuyerNonce::<T>::insert(buyer_commitment.buyer, next_buyer_nonce);
            order.buyer_commitment = Some(buyer_commitment.clone());
            order.escrow_id = Some(escrow_id);
            order.funded_at = Some(Self::current_block_u32());
            order.grace_deadline = Some(grace_deadline);
            order.state = OrderState::Funded;
            RuntimeOrders::<T>::insert(order_id, &order);
            Markets::<T>::insert(order.order_commitment.market_id, market);
            Self::deposit_event(Event::OrderFunded {
                order_id,
                buyer: buyer_commitment.buyer,
                escrow_id,
                gross_price: amounts.gross_price,
                grace_deadline,
            });
            Self::deposit_event(Event::PaymentEscrowed {
                order_id,
                escrow_id,
                amount: amounts.gross_price,
            });
            Ok(())
        }

        fn refund_allowed(order: &RuntimeOrder) -> bool {
            if order
                .grace_deadline
                .is_some_and(|deadline| Self::current_block_u32() > deadline)
            {
                return true;
            }
            match order.order_commitment.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                    MetaTokens::<T>::get(order.order_commitment.object_id).is_none_or(|meta| {
                        meta.status != AssetStatus::Tradable
                            || meta.state_version != order.order_commitment.seller_state_version
                    })
                }
                MarketOperation::TransferRight => {
                    RightTokens::<T>::get(order.order_commitment.object_id).is_none_or(|right| {
                        !Self::right_is_valid_at(&right, Self::current_block_u64())
                    })
                }
            }
        }

        fn do_refund_funded(order_id: Hash32) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            match order.state {
                OrderState::Funded => {}
                OrderState::Open => return Err(Error::<T>::OrderNotFunded.into()),
                _ => return Err(Error::<T>::TerminalOrder.into()),
            }
            ensure!(Self::refund_allowed(&order), Error::<T>::GracePeriodActive);
            ensure!(
                !SettlementRecords::<T>::contains_key(order_id),
                Error::<T>::AlreadySettled
            );
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let mut escrow =
                RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.order_id == order_id && escrow.state == PaymentState::Escrowed,
                Error::<T>::EscrowMismatch
            );
            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::<T>::RefundBeneficiaryMismatch)?;
            ensure!(
                buyer_commitment.buyer == escrow.buyer && escrow.payer == escrow.buyer,
                Error::<T>::RefundBeneficiaryMismatch
            );
            let buyer = Self::runtime_account(&escrow.buyer)?;
            ensure!(
                T::Currency::unreserve(&buyer, escrow.amounts.gross_price) == 0,
                Error::<T>::EscrowMismatch
            );
            if order.order_commitment.operation == MarketOperation::IssueRight {
                Self::release_issue_quota(&order.authorization_id, &order_id)?;
            }
            Self::release_order_lock(&order)?;
            Markets::<T>::try_mutate(
                order.order_commitment.market_id,
                |market| -> DispatchResult {
                    let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                    market.active_orders = market
                        .active_orders
                        .checked_sub(1)
                        .ok_or(Error::<T>::CounterInvariant)?;
                    market.funded_orders = market
                        .funded_orders
                        .checked_sub(1)
                        .ok_or(Error::<T>::CounterInvariant)?;
                    market.active_exposure_value = market
                        .active_exposure_value
                        .checked_sub(escrow.amounts.gross_price)
                        .ok_or(Error::<T>::CounterInvariant)?;
                    Ok(())
                },
            )?;
            escrow.state = PaymentState::Refunded;
            order.state = OrderState::Refunded;
            RuntimeEscrows::<T>::insert(escrow_id, escrow.clone());
            RuntimeOrders::<T>::insert(order_id, order);
            Self::deposit_event(Event::PaymentRefunded {
                order_id,
                escrow_id,
                amount: escrow.amounts.gross_price,
            });
            Self::deposit_event(Event::OrderRefunded { order_id });
            Ok(())
        }

        fn normalized_object_snapshot(
            order: &RuntimeOrder,
        ) -> Result<NormalizedObjectSnapshot, DispatchError> {
            let commitment = &order.order_commitment;
            let asset_id = commitment.parent_asset_id.unwrap_or(commitment.object_id);
            let meta = MetaTokens::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            let right_id = match commitment.operation {
                MarketOperation::TransferMeta => None,
                MarketOperation::IssueRight => Some(right_id(
                    Self::protocol_genesis_hash()?,
                    MarketOperation::IssueRight,
                    order.order_id,
                )),
                MarketOperation::TransferRight => Some(commitment.object_id),
            };
            let right = right_id.and_then(RightTokens::<T>::get);
            let active_order_id = match commitment.operation {
                MarketOperation::TransferMeta => ActiveMetaOrder::<T>::get(asset_id),
                MarketOperation::IssueRight => None,
                MarketOperation::TransferRight => ActiveRightOrder::<T>::get(commitment.object_id),
            };
            Ok(NormalizedObjectSnapshot {
                asset_id,
                asset_owner: Self::protocol_account(&meta.owner)?,
                asset_status: meta.status.encode()[0],
                asset_state_version: meta.state_version,
                right_id,
                right_exists: right.is_some(),
                right_holder: right
                    .as_ref()
                    .map(|value| Self::protocol_account(&value.holder))
                    .transpose()?,
                right_status: right.as_ref().map(|value| value.status.encode()[0]),
                right_state_version: right.as_ref().map(|value| value.state_version),
                right_parameters_hash: right.as_ref().map(|value| {
                    right_parameters_hash(
                        value.right_type,
                        value.data_scope,
                        value.validity,
                        value.transferable,
                        &value.allowed_transfer_markets,
                        value.royalty_bps,
                    )
                }),
                active_order_id,
            })
        }

        fn normalized_authorization_snapshot(
            order: &RuntimeOrder,
        ) -> Result<NormalizedAuthorizationSnapshot, DispatchError> {
            let commitment = &order.order_commitment;
            match commitment.operation {
                MarketOperation::TransferMeta => {
                    let auth = TransferMetaAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?;
                    Ok(NormalizedAuthorizationSnapshot {
                        authorization_id: auth.auth_id,
                        kind: NormalizedAuthorizationKind::TransferMeta,
                        asset_id: auth.asset_id,
                        owner: Self::protocol_account(&auth.owner)?,
                        market_id: auth.market_id,
                        policy_hash: price_policy_hash(&auth.price_policy),
                        right_parameters_hash: None,
                        nonce: auth.nonce,
                        expiry: auth.expiry,
                        status: auth.status.encode()[0],
                        reserved_count: 0,
                        reserved_use_units: 0,
                        issued_count: 0,
                        issued_use_units: 0,
                        consumed_order_id: auth.consumed_order_id,
                    })
                }
                MarketOperation::IssueRight => {
                    let auth = IssueRightAuthorizations::<T>::get(order.authorization_id)
                        .ok_or(Error::<T>::AuthorizationMissing)?;
                    Ok(NormalizedAuthorizationSnapshot {
                        authorization_id: auth.auth_id,
                        kind: NormalizedAuthorizationKind::IssueRight,
                        asset_id: auth.asset_id,
                        owner: Self::protocol_account(&auth.owner)?,
                        market_id: auth.market_id,
                        policy_hash: price_policy_hash(&auth.price_policy),
                        right_parameters_hash: Some(right_parameters_hash(
                            auth.right_type,
                            auth.data_scope,
                            auth.validity_template,
                            auth.transferable,
                            &auth.allowed_transfer_markets,
                            auth.royalty_bps,
                        )),
                        nonce: auth.nonce,
                        expiry: auth.expiry,
                        status: auth.status.encode()[0],
                        reserved_count: auth.reserved_count,
                        reserved_use_units: auth.reserved_use_units,
                        issued_count: auth.issued_count,
                        issued_use_units: auth.issued_use_units,
                        consumed_order_id: None,
                    })
                }
                MarketOperation::TransferRight => Ok(NormalizedAuthorizationSnapshot {
                    authorization_id: commitment.authorization_id,
                    kind: NormalizedAuthorizationKind::TransferRightPolicy,
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

        fn normalized_market_snapshot(
            market: &MarketRegistration<T::AccountId, BlockNumberOf<T>>,
        ) -> Result<NormalizedMarketSnapshot, DispatchError> {
            Ok(NormalizedMarketSnapshot {
                market_id: market.market_id,
                operator: Self::protocol_account(&market.operator)?,
                protocol_version: market.protocol_version,
                capabilities: market.capabilities,
                capacity_tier: market.capacity_tier.tier_id,
                active_orders: market.active_orders,
                funded_orders: market.funded_orders,
                active_exposure_value: market.active_exposure_value,
                active_authorizations: market.active_authorizations,
                status: market.status.encode()[0],
                market_fee_bps: market.market_fee_bps,
            })
        }

        fn normalized_settlement_snapshot(
            order: &RuntimeOrder,
            escrow: &RuntimeEscrow,
            settlement: Option<SettlementRecord>,
        ) -> Result<NormalizedSettlementSnapshot, DispatchError> {
            let market = Markets::<T>::get(order.order_commitment.market_id)
                .ok_or(Error::<T>::MarketNotRegistered)?;
            Ok(NormalizedSettlementSnapshot {
                object: Self::normalized_object_snapshot(order)?,
                order: order.clone(),
                authorization: Self::normalized_authorization_snapshot(order)?,
                payment: escrow.clone(),
                settlement,
                market: Self::normalized_market_snapshot(&market)?,
            })
        }

        pub fn q_meta(asset_id: Hash32) -> QMetaSnapshot<T::AccountId> {
            let meta = MetaTokens::<T>::get(asset_id);
            QMetaSnapshot {
                asset_id,
                owner_index_matches: meta
                    .as_ref()
                    .map(|value| MetaOwnerIndex::<T>::contains_key(&value.owner, asset_id)),
                active_right_count: ActiveRightCount::<T>::get(asset_id),
                active_order_id: ActiveMetaOrder::<T>::get(asset_id),
                meta,
            }
        }

        pub fn q_right(right_id: Hash32) -> QRightSnapshot<T::AccountId> {
            let right = RightTokens::<T>::get(right_id);
            QRightSnapshot {
                right_id,
                valid: right
                    .as_ref()
                    .map(|value| Self::right_is_valid_at(value, Self::current_block_u64())),
                holder_index_matches: right
                    .as_ref()
                    .map(|value| RightHolderIndex::<T>::contains_key(&value.holder, right_id)),
                parent_index_matches: right
                    .as_ref()
                    .map(|value| RightParentIndex::<T>::contains_key(value.asset_id, right_id)),
                active_order_id: ActiveRightOrder::<T>::get(right_id),
                right,
            }
        }

        pub fn q_order(order_id: Hash32) -> QOrderSnapshot {
            let order = RuntimeOrders::<T>::get(order_id);
            QOrderSnapshot {
                order_id,
                active_capacity_occupied: order.as_ref().is_some_and(|value| {
                    matches!(value.state, OrderState::Open | OrderState::Funded)
                }),
                funded_capacity_occupied: order
                    .as_ref()
                    .is_some_and(|value| value.state == OrderState::Funded),
                order,
            }
        }

        pub fn q_authorization(
            order_id: Hash32,
        ) -> Result<NormalizedAuthorizationSnapshot, DispatchError> {
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            Self::normalized_authorization_snapshot(&order)
        }

        pub fn q_payment(order_id: Hash32) -> Result<QPaymentSnapshot, DispatchError> {
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            let escrow = order.escrow_id.and_then(RuntimeEscrows::<T>::get);
            let state = escrow
                .as_ref()
                .map(|value| value.state)
                .unwrap_or(PaymentState::Unfunded);
            let escrow_amount = escrow
                .as_ref()
                .map(|value| value.amounts.gross_price)
                .unwrap_or(0);
            let buyer_reserved_balance = escrow
                .as_ref()
                .map(|value| Self::runtime_account(&value.buyer))
                .transpose()?
                .map(|buyer| T::Currency::reserved_balance(&buyer))
                .unwrap_or(0);
            Ok(QPaymentSnapshot {
                order_id,
                state,
                escrow,
                escrow_amount,
                buyer_reserved_balance,
                reserved_balance_reconciles: state != PaymentState::Escrowed
                    || buyer_reserved_balance >= escrow_amount,
            })
        }

        pub fn q_evidence(order_id: Hash32) -> QEvidenceSnapshot {
            let settlement = SettlementRecords::<T>::get(order_id);
            QEvidenceSnapshot {
                order_id,
                settlement_record_count: u8::from(settlement.is_some()),
                record_fields_hash: settlement
                    .as_ref()
                    .map(|record| sp_io::hashing::blake2_256(&record.encode())),
                pre_state_commitment: settlement
                    .as_ref()
                    .map(|record| record.pre_state_commitment),
                settlement,
            }
        }

        pub fn q_market(market_id: Hash32) -> Result<NormalizedMarketSnapshot, DispatchError> {
            let market = Markets::<T>::get(market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            Self::normalized_market_snapshot(&market)
        }

        pub fn q_identity(order_id: Hash32) -> Result<QIdentitySnapshot, DispatchError> {
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            let object = Self::normalized_object_snapshot(&order)?;
            let authorization_nonce = match order.order_commitment.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                    Some(AuthorizationNonce::<T>::get((
                        Self::runtime_account(&order.order_commitment.seller)?,
                        object.asset_id,
                        order.order_commitment.operation,
                    )))
                }
                MarketOperation::TransferRight => None,
            };
            Ok(QIdentitySnapshot {
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
                settlement_domain_id: SETTLEMENT_DOMAIN_ID,
                order_commitment_hash: Some(order_commitment_hash(&order.order_commitment)),
                seller_nonce: Some(SellerNonce::<T>::get((
                    order.order_commitment.seller,
                    order.order_commitment.object_id,
                ))),
                buyer_nonce: order
                    .buyer_commitment
                    .as_ref()
                    .map(|buyer| BuyerNonce::<T>::get(buyer.buyer)),
                authorization_nonce,
                object_state_version: match order.order_commitment.operation {
                    MarketOperation::TransferRight => object.right_state_version,
                    MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                        Some(object.asset_state_version)
                    }
                },
            })
        }

        pub fn q_all(order_id: Hash32) -> Result<QAllSnapshot<T::AccountId>, DispatchError> {
            let order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            let asset_id = order
                .order_commitment
                .parent_asset_id
                .unwrap_or(order.order_commitment.object_id);
            let right_id = match order.order_commitment.operation {
                MarketOperation::TransferMeta => None,
                MarketOperation::IssueRight => Some(right_id(
                    Self::protocol_genesis_hash()?,
                    MarketOperation::IssueRight,
                    order_id,
                )),
                MarketOperation::TransferRight => Some(order.order_commitment.object_id),
            };
            let observed_at = Self::current_block_u32();
            let meta = Self::q_meta(asset_id);
            let right = right_id.map(Self::q_right);
            let order_snapshot = Self::q_order(order_id);
            let authorization = Self::normalized_authorization_snapshot(&order)?;
            let payment = Self::q_payment(order_id)?;
            let evidence = Self::q_evidence(order_id);
            let market = Self::q_market(order.order_commitment.market_id)?;
            let identity = Self::q_identity(order_id)?;
            let state_commitment = sp_io::hashing::blake2_256(
                &(
                    observed_at,
                    &meta,
                    &right,
                    &order_snapshot,
                    &authorization,
                    &payment,
                    &evidence,
                    &market,
                    &identity,
                )
                    .encode(),
            );
            Ok(QAllSnapshot {
                observed_at,
                meta,
                right,
                order: order_snapshot,
                authorization,
                payment,
                evidence,
                market,
                identity,
                state_commitment,
            })
        }

        fn release_payment(escrow: &RuntimeEscrow, seller: &T::AccountId) -> DispatchResult {
            let buyer = Self::runtime_account(&escrow.buyer)?;
            let market = Self::runtime_account(&escrow.market_fee_recipient)?;
            let royalty = escrow
                .royalty_recipient
                .as_ref()
                .map(Self::runtime_account)
                .transpose()?;
            let reconciled = escrow
                .amounts
                .seller_net
                .checked_add(escrow.amounts.royalty)
                .and_then(|value| value.checked_add(escrow.amounts.market_fee))
                .and_then(|value| value.checked_add(escrow.amounts.safety_fee))
                .ok_or(Error::<T>::PaymentAmountInvalid)?;
            ensure!(
                reconciled == escrow.amounts.gross_price
                    && escrow.amounts.safety_fee == 0
                    && ((escrow.amounts.royalty == 0 && royalty.is_none())
                        || (escrow.amounts.royalty > 0 && royalty.is_some())),
                Error::<T>::EscrowMismatch
            );
            let transfer = |recipient: &T::AccountId, amount: ProtocolBalance| -> DispatchResult {
                if amount == 0 {
                    return Ok(());
                }
                let remainder = T::Currency::repatriate_reserved(
                    &buyer,
                    recipient,
                    amount,
                    BalanceStatus::Free,
                )?;
                ensure!(remainder == 0, Error::<T>::EscrowMismatch);
                Ok(())
            };
            transfer(seller, escrow.amounts.seller_net)?;
            if let Some(recipient) = royalty.as_ref() {
                transfer(recipient, escrow.amounts.royalty)?;
            }
            transfer(&market, escrow.amounts.market_fee)?;
            Ok(())
        }

        fn close_funded_market_order(
            market_id: Hash32,
            gross_price: ProtocolBalance,
        ) -> DispatchResult {
            Markets::<T>::try_mutate(market_id, |market| -> DispatchResult {
                let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                market.active_orders = market
                    .active_orders
                    .checked_sub(1)
                    .ok_or(Error::<T>::CounterInvariant)?;
                market.funded_orders = market
                    .funded_orders
                    .checked_sub(1)
                    .ok_or(Error::<T>::CounterInvariant)?;
                market.active_exposure_value = market
                    .active_exposure_value
                    .checked_sub(gross_price)
                    .ok_or(Error::<T>::CounterInvariant)?;
                Ok(())
            })
        }

        fn settle_transfer_meta(
            order: &RuntimeOrder,
            buyer: &T::AccountId,
        ) -> Result<(Hash32, Option<Hash32>, u64, u64, T::AccountId), DispatchError> {
            let mut meta = MetaTokens::<T>::get(order.order_commitment.object_id)
                .ok_or(Error::<T>::AssetNotFound)?;
            ensure!(
                meta.status == AssetStatus::Tradable,
                Error::<T>::AssetNotTradable
            );
            ensure!(
                meta.state_version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            ensure!(
                Self::protocol_account(&meta.owner)? == order.order_commitment.seller,
                Error::<T>::OwnershipMismatch
            );
            ensure!(
                ActiveMetaOrder::<T>::get(meta.asset_id) == Some(order.order_id),
                Error::<T>::ActiveOrderConflict
            );
            ensure!(
                ActiveIssueAuthorizationCount::<T>::get(meta.asset_id) == 0
                    && ActiveIssueOrderCount::<T>::get(meta.asset_id) == 0
                    && OutstandingIssueQuotaCount::<T>::get(meta.asset_id) == 0
                    && !Self::has_valid_right(&meta.asset_id)?,
                Error::<T>::MutualExclusionConflict
            );
            let mut auth = TransferMetaAuthorizations::<T>::get(order.authorization_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                auth.status != AuthorizationStatus::Consumed
                    && auth.asset_id == meta.asset_id
                    && auth.market_id == order.order_commitment.market_id
                    && auth.owner == meta.owner,
                Error::<T>::AuthorizationRevoked
            );
            let old_owner = meta.owner.clone();
            let pre = meta.state_version;
            meta.owner = buyer.clone();
            meta.state_version = meta
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            MetaOwnerIndex::<T>::remove(&old_owner, meta.asset_id);
            MetaOwnerIndex::<T>::insert(buyer, meta.asset_id, ());
            MetaTokens::<T>::insert(meta.asset_id, meta.clone());
            if auth.status == AuthorizationStatus::Active {
                Markets::<T>::try_mutate(auth.market_id, |market| -> DispatchResult {
                    let market = market.as_mut().ok_or(Error::<T>::MarketNotRegistered)?;
                    market.active_authorizations = market
                        .active_authorizations
                        .checked_sub(1)
                        .ok_or(Error::<T>::CounterInvariant)?;
                    Ok(())
                })?;
                ActiveTransferAuthorizationByAsset::<T>::remove(auth.asset_id);
            }
            auth.status = AuthorizationStatus::Consumed;
            auth.consumed_order_id = Some(order.order_id);
            TransferMetaAuthorizations::<T>::insert(auth.auth_id, auth);
            Self::deposit_event(Event::AuthorizationStatusChanged {
                auth_id: order.authorization_id,
                status: AuthorizationStatus::Consumed,
            });
            Ok((meta.asset_id, None, pre, meta.state_version, old_owner))
        }

        fn settle_issue_right(
            order: &RuntimeOrder,
            buyer: &T::AccountId,
        ) -> Result<(Hash32, Option<Hash32>, u64, u64, T::AccountId), DispatchError> {
            let meta = MetaTokens::<T>::get(order.order_commitment.object_id)
                .ok_or(Error::<T>::AssetNotFound)?;
            ensure!(
                meta.status == AssetStatus::Tradable,
                Error::<T>::AssetNotTradable
            );
            ensure!(
                meta.state_version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            ensure!(
                Self::protocol_account(&meta.owner)? == order.order_commitment.seller,
                Error::<T>::OwnershipMismatch
            );
            let auth = IssueRightAuthorizations::<T>::get(order.authorization_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                auth.asset_id == meta.asset_id
                    && auth.market_id == order.order_commitment.market_id
                    && auth.owner == meta.owner
                    && auth.status != AuthorizationStatus::Consumed,
                Error::<T>::AuthorizationRevoked
            );
            let reservation = IssueQuotaReservations::<T>::get(order.order_id)
                .ok_or(Error::<T>::QuotaReservationMissing)?;
            ensure!(
                reservation.auth_id == auth.auth_id
                    && reservation.status == IssueQuotaReservationStatus::Reserved,
                Error::<T>::QuotaReservationStateInvalid
            );
            ensure!(
                auth.validity_template
                    .valid_until
                    .is_none_or(|until| Self::current_block_u64() <= until),
                Error::<T>::RightInvalid
            );
            let issued_id = right_id(
                Self::protocol_genesis_hash()?,
                MarketOperation::IssueRight,
                order.order_id,
            );
            ensure!(
                !RightTokens::<T>::contains_key(issued_id),
                Error::<T>::RightAlreadyExists
            );
            let mut rights = AssetRightIds::<T>::get(meta.asset_id);
            rights
                .try_push(issued_id)
                .map_err(|_| Error::<T>::QuotaExceeded)?;
            let active = ActiveRightCount::<T>::get(meta.asset_id)
                .checked_add(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            RightTokens::<T>::insert(
                issued_id,
                RightToken {
                    right_id: issued_id,
                    asset_id: meta.asset_id,
                    holder: buyer.clone(),
                    issuer: meta.owner.clone(),
                    right_type: auth.right_type,
                    data_scope: auth.data_scope,
                    validity: auth.validity_template,
                    remaining_uses: auth.validity_template.max_uses,
                    reserved_uses: 0,
                    transferable: auth.transferable,
                    allowed_transfer_markets: auth.allowed_transfer_markets,
                    royalty_bps: auth.royalty_bps,
                    state_version: 1,
                    status: RightStatus::Active,
                },
            );
            RightHolderIndex::<T>::insert(buyer, issued_id, ());
            RightParentIndex::<T>::insert(meta.asset_id, issued_id, ());
            AssetRightIds::<T>::insert(meta.asset_id, rights);
            ActiveRightCount::<T>::insert(meta.asset_id, active);
            Self::convert_issue_quota_to_issued(&auth.auth_id, &order.order_id)?;
            Ok((meta.asset_id, Some(issued_id), 0, 1, meta.owner))
        }

        fn settle_transfer_right(
            order: &RuntimeOrder,
            buyer: &T::AccountId,
        ) -> Result<(Hash32, Option<Hash32>, u64, u64, T::AccountId), DispatchError> {
            let parent = order
                .order_commitment
                .parent_asset_id
                .ok_or(Error::<T>::ParentMismatch)?;
            let mut right = RightTokens::<T>::get(order.order_commitment.object_id)
                .ok_or(Error::<T>::RightNotFound)?;
            ensure!(right.asset_id == parent, Error::<T>::ParentMismatch);
            ensure!(
                Self::right_is_valid_at(&right, Self::current_block_u64()),
                Error::<T>::RightInvalid
            );
            ensure!(
                right.transferable
                    && right
                        .allowed_transfer_markets
                        .contains(&order.order_commitment.market_id),
                Error::<T>::AuthorizationMissing
            );
            ensure!(
                right.state_version == order.order_commitment.seller_state_version,
                Error::<T>::StateVersionMismatch
            );
            ensure!(
                Self::protocol_account(&right.holder)? == order.order_commitment.seller,
                Error::<T>::OwnershipMismatch
            );
            ensure!(
                ActiveRightOrder::<T>::get(right.right_id) == Some(order.order_id),
                Error::<T>::ActiveOrderConflict
            );
            ensure!(
                order.order_commitment.right_parameters_hash
                    == Some(right_parameters_hash(
                        right.right_type,
                        right.data_scope,
                        right.validity,
                        right.transferable,
                        &right.allowed_transfer_markets,
                        right.royalty_bps,
                    )),
                Error::<T>::MalformedCommitment
            );
            let old_holder = right.holder.clone();
            let pre = right.state_version;
            right.holder = buyer.clone();
            right.state_version = right
                .state_version
                .checked_add(1)
                .ok_or(Error::<T>::StateVersionOverflow)?;
            RightHolderIndex::<T>::remove(&old_holder, right.right_id);
            RightHolderIndex::<T>::insert(buyer, right.right_id, ());
            RightTokens::<T>::insert(right.right_id, right.clone());
            Ok((
                parent,
                Some(right.right_id),
                pre,
                right.state_version,
                old_holder,
            ))
        }

        fn do_settle(order_id: Hash32) -> DispatchResult {
            let mut order = RuntimeOrders::<T>::get(order_id).ok_or(Error::<T>::OrderNotFound)?;
            match order.state {
                OrderState::Funded => {}
                OrderState::Open => return Err(Error::<T>::OrderNotFunded.into()),
                OrderState::Settled => return Err(Error::<T>::AlreadySettled.into()),
                _ => return Err(Error::<T>::TerminalOrder.into()),
            }
            ensure!(
                !SettlementRecords::<T>::contains_key(order_id),
                Error::<T>::AlreadySettled
            );
            ensure!(
                Self::current_block_u32()
                    <= order.grace_deadline.ok_or(Error::<T>::EscrowMismatch)?,
                Error::<T>::OrderExpired
            );
            let escrow_id = order.escrow_id.ok_or(Error::<T>::EscrowMismatch)?;
            let mut escrow =
                RuntimeEscrows::<T>::get(escrow_id).ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                escrow.order_id == order_id
                    && escrow.state == PaymentState::Escrowed
                    && escrow.currency == order.payment_asset
                    && escrow.amounts.gross_price == order.order_commitment.price,
                Error::<T>::EscrowMismatch
            );
            let buyer_commitment = order
                .buyer_commitment
                .clone()
                .ok_or(Error::<T>::EscrowMismatch)?;
            ensure!(
                buyer_commitment.buyer == escrow.buyer && escrow.payer == escrow.buyer,
                Error::<T>::EscrowMismatch
            );
            let buyer = Self::runtime_account(&escrow.buyer)?;
            let pre_snapshot = Self::normalized_settlement_snapshot(&order, &escrow, None)?;
            let pre_state_commitment = normalized_state_commitment(&pre_snapshot);
            let (asset_id, right_id, pre_version, post_version, seller) =
                match order.order_commitment.operation {
                    MarketOperation::TransferMeta => Self::settle_transfer_meta(&order, &buyer)?,
                    MarketOperation::IssueRight => Self::settle_issue_right(&order, &buyer)?,
                    MarketOperation::TransferRight => Self::settle_transfer_right(&order, &buyer)?,
                };
            Self::release_payment(&escrow, &seller)?;
            escrow.state = PaymentState::Released;
            order.state = OrderState::Settled;
            Self::release_order_lock(&order)?;
            Self::close_funded_market_order(
                order.order_commitment.market_id,
                escrow.amounts.gross_price,
            )?;
            RuntimeEscrows::<T>::insert(escrow_id, escrow.clone());
            RuntimeOrders::<T>::insert(order_id, order.clone());
            let settlement_id = settlement_id(
                Self::protocol_genesis_hash()?,
                order.order_commitment.operation,
                order_id,
            );
            let record = SettlementRecord {
                settlement_id,
                order_id,
                operation: order.order_commitment.operation,
                asset_id,
                right_id,
                authorization_id: order.authorization_id,
                market_id: order.order_commitment.market_id,
                policy_hash: order.order_commitment.policy_hash,
                order_commitment_hash: order_commitment_hash(&order.order_commitment),
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
                pre_state_version: pre_version,
                post_state_version: post_version,
                pre_state_commitment,
                block_number: Self::current_block_u32(),
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            };
            SettlementRecords::<T>::insert(order_id, record.clone());
            let post_snapshot =
                Self::normalized_settlement_snapshot(&order, &escrow, Some(record))?;
            let post_state_commitment = normalized_state_commitment(&post_snapshot);
            Self::deposit_event(Event::PaymentReleased {
                order_id,
                escrow_id,
                amount: escrow.amounts.gross_price,
            });
            Self::deposit_event(Event::SettlementFinalized {
                settlement_id,
                order_id,
                post_state_commitment,
            });
            Ok(())
        }

        pub fn ensure_market_operation(
            market_id: &Hash32,
            operation: MarketOperation,
        ) -> Result<MarketRegistration<T::AccountId, BlockNumberOf<T>>, DispatchError> {
            let market = Markets::<T>::get(market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            ensure!(
                market.status == MarketStatus::Active,
                Error::<T>::MarketRestricted
            );
            ensure!(
                market.protocol_version == SETTLEMENT_PROTOCOL_VERSION,
                Error::<T>::MarketRestricted
            );
            ensure!(
                market.capabilities & Self::operation_capability(operation) != 0,
                Error::<T>::CapabilityDenied
            );
            Ok(market)
        }

        pub fn validate_price(
            policy: &PricePolicy,
            price: ProtocolBalance,
            market_fee_bps: u16,
        ) -> Result<ProtocolBalance, DispatchError> {
            Self::validate_price_policy_terms(policy)?;
            ensure!(
                market_fee_bps <= policy.signed_market_fee_cap_bps,
                Error::<T>::FeeOutOfBounds
            );
            let floor = policy
                .reference_price
                .checked_mul(policy.floor_ratio_bps as ProtocolBalance)
                .and_then(|value| value.checked_add(9_999))
                .ok_or(Error::<T>::InvalidPricePolicy)?
                / 10_000;
            ensure!(price >= floor, Error::<T>::BelowPriceFloor);
            Ok(floor)
        }

        pub fn is_right_valid(right_id: &Hash32) -> Result<bool, DispatchError> {
            let right = RightTokens::<T>::get(right_id).ok_or(Error::<T>::RightNotFound)?;
            Ok(Self::right_is_valid_at(&right, Self::current_block_u64()))
        }

        pub fn ensure_right_transfer_authorized(
            right_id: &Hash32,
            market_id: &Hash32,
        ) -> DispatchResult {
            Self::ensure_market_operation(market_id, MarketOperation::TransferRight)?;
            let right = RightTokens::<T>::get(right_id).ok_or(Error::<T>::RightNotFound)?;
            ensure!(
                Self::right_is_valid_at(&right, Self::current_block_u64()),
                Error::<T>::RightInvalid
            );
            ensure!(right.transferable, Error::<T>::RightInvalid);
            ensure!(
                right.allowed_transfer_markets.contains(market_id),
                Error::<T>::AuthorizationMissing
            );
            Ok(())
        }

        #[allow(dead_code)]
        pub(crate) fn reserve_issue_quota(auth_id: &Hash32, order_id: Hash32) -> DispatchResult {
            ensure!(
                !IssueQuotaReservations::<T>::contains_key(order_id),
                Error::<T>::QuotaReservationAlreadyExists
            );
            let mut auth = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(
                auth.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationRevoked
            );
            ensure!(
                Self::current_block_u32() <= auth.expiry,
                Error::<T>::AuthorizationExpired
            );
            let units = auth.validity_template.max_uses.unwrap_or(0) as u64;
            let reserved_count = auth
                .reserved_count
                .checked_add(1)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let reserved_units = auth
                .reserved_use_units
                .checked_add(units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let total_count = auth
                .issued_count
                .checked_add(reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let total_units = auth
                .issued_use_units
                .checked_add(reserved_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            ensure!(
                total_count <= auth.max_issue_count && total_units <= auth.max_total_use_units,
                Error::<T>::QuotaExceeded
            );
            let outstanding = OutstandingIssueQuotaCount::<T>::get(auth.asset_id)
                .checked_add(1)
                .ok_or(Error::<T>::QuotaInvariant)?;
            auth.reserved_count = reserved_count;
            auth.reserved_use_units = reserved_units;
            IssueRightAuthorizations::<T>::insert(auth_id, auth.clone());
            IssueQuotaReservations::<T>::insert(
                order_id,
                IssueQuotaReservation {
                    order_id,
                    auth_id: *auth_id,
                    reserved_count: 1,
                    reserved_use_units: units,
                    status: IssueQuotaReservationStatus::Reserved,
                },
            );
            OutstandingIssueQuotaCount::<T>::insert(auth.asset_id, outstanding);
            Self::deposit_event(Event::IssueQuotaReserved {
                auth_id: *auth_id,
                order_id,
                count: 1,
                use_units: units,
            });
            Ok(())
        }

        #[allow(dead_code)]
        pub(crate) fn convert_issue_quota_to_issued(
            auth_id: &Hash32,
            order_id: &Hash32,
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
            let mut auth = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            auth.reserved_count = auth
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            auth.reserved_use_units = auth
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            auth.issued_count = auth
                .issued_count
                .checked_add(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            auth.issued_use_units = auth
                .issued_use_units
                .checked_add(reservation.reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let outstanding = OutstandingIssueQuotaCount::<T>::get(auth.asset_id)
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            reservation.status = IssueQuotaReservationStatus::Issued;
            IssueRightAuthorizations::<T>::insert(auth_id, auth.clone());
            IssueQuotaReservations::<T>::insert(order_id, reservation);
            OutstandingIssueQuotaCount::<T>::insert(auth.asset_id, outstanding);
            Self::deposit_event(Event::IssueQuotaConverted {
                auth_id: *auth_id,
                order_id: *order_id,
            });
            Ok(())
        }

        #[allow(dead_code)]
        pub(crate) fn release_issue_quota(auth_id: &Hash32, order_id: &Hash32) -> DispatchResult {
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
            let mut auth = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            auth.reserved_count = auth
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            auth.reserved_use_units = auth
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::<T>::QuotaInvariant)?;
            let outstanding = OutstandingIssueQuotaCount::<T>::get(auth.asset_id)
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::<T>::QuotaInvariant)?;
            reservation.status = IssueQuotaReservationStatus::Released;
            IssueRightAuthorizations::<T>::insert(auth_id, auth.clone());
            IssueQuotaReservations::<T>::insert(order_id, reservation);
            OutstandingIssueQuotaCount::<T>::insert(auth.asset_id, outstanding);
            Self::deposit_event(Event::IssueQuotaReleased {
                auth_id: *auth_id,
                order_id: *order_id,
            });
            Ok(())
        }

        fn current_block_u32() -> u32 {
            frame_system::Pallet::<T>::block_number().saturated_into()
        }

        fn current_block_u64() -> u64 {
            frame_system::Pallet::<T>::block_number().saturated_into()
        }

        fn operation_capability(operation: MarketOperation) -> u8 {
            match operation {
                MarketOperation::TransferMeta => CAP_CREATE_META_ORDER,
                MarketOperation::IssueRight => CAP_CREATE_RIGHT_ISSUE_ORDER,
                MarketOperation::TransferRight => CAP_CREATE_RIGHT_TRANSFER_ORDER,
            }
        }

        fn validate_capabilities(capabilities: u8) -> DispatchResult {
            ensure!(
                capabilities != 0 && capabilities & !ALL_CAPABILITIES == 0,
                Error::<T>::InvalidCapabilities
            );
            Ok(())
        }

        fn validate_capacity_tier(tier: &MarketCapacityTier) -> DispatchResult {
            ensure!(
                tier.max_active_orders > 0
                    && tier.max_funded_orders > 0
                    && tier.max_funded_orders <= tier.max_active_orders
                    && tier.max_active_exposure_value > 0,
                Error::<T>::InvalidCapacityTier
            );
            Ok(())
        }

        fn validate_price_policy_terms(policy: &PricePolicy) -> DispatchResult {
            ensure!(
                policy.reference_price > 0
                    && (1..=10_000).contains(&policy.floor_ratio_bps)
                    && policy.signed_market_fee_cap_bps <= 10_000
                    && policy.safety_fee_bps <= 10_000
                    && policy.signed_market_fee_cap_bps as u32 + policy.safety_fee_bps as u32
                        <= 10_000,
                Error::<T>::InvalidPricePolicy
            );
            Ok(())
        }

        fn validate_right_terms(
            validity: &RightValidity,
            transferable: bool,
            markets: &AllowedMarkets,
            royalty_bps: u16,
        ) -> DispatchResult {
            ensure!(royalty_bps <= 10_000, Error::<T>::InvalidRightParameters);
            ensure!(
                validity
                    .valid_until
                    .is_none_or(|until| until > validity.valid_from),
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                validity.max_uses.is_none_or(|uses| uses > 0),
                Error::<T>::InvalidRightParameters
            );
            ensure!(
                transferable == !markets.is_empty(),
                Error::<T>::InvalidRightParameters
            );
            for (index, market_id) in markets.iter().enumerate() {
                ensure!(
                    !markets[..index].contains(market_id),
                    Error::<T>::InvalidRightParameters
                );
                Self::ensure_market_operation(market_id, MarketOperation::TransferRight)?;
            }
            Ok(())
        }

        fn validate_issue_authorization_terms(
            terms: &IssueRightAuthorizationTerms,
        ) -> DispatchResult {
            Self::validate_price_policy_terms(&terms.price_policy)?;
            Self::validate_right_terms(
                &terms.validity_template,
                terms.transferable,
                &terms.allowed_transfer_markets,
                terms.royalty_bps,
            )?;
            ensure!(
                terms.max_issue_count > 0,
                Error::<T>::InvalidRightParameters
            );
            let required_units = (terms.validity_template.max_uses.unwrap_or(0) as u64)
                .checked_mul(terms.max_issue_count as u64)
                .ok_or(Error::<T>::InvalidRightParameters)?;
            ensure!(
                terms.max_total_use_units >= required_units,
                Error::<T>::InvalidRightParameters
            );
            Ok(())
        }

        fn ensure_authorization_owner(owner: &T::AccountId, asset_id: &Hash32) -> DispatchResult {
            let meta = MetaTokens::<T>::get(asset_id).ok_or(Error::<T>::AssetNotFound)?;
            ensure!(meta.owner == *owner, Error::<T>::OwnershipMismatch);
            ensure!(
                meta.status == AssetStatus::Tradable,
                Error::<T>::AssetNotTradable
            );
            Ok(())
        }

        fn authorization_id(
            owner: &T::AccountId,
            asset_id: &Hash32,
            market_id: &Hash32,
            operation: MarketOperation,
            nonce: u64,
        ) -> Hash32 {
            sp_io::hashing::blake2_256(
                &(
                    AUTHORIZATION_DOMAIN_ID,
                    owner,
                    asset_id,
                    market_id,
                    operation,
                    nonce,
                )
                    .encode(),
            )
        }

        fn right_is_valid_at(right: &RightTokenOf<T>, now: u64) -> bool {
            right.status == RightStatus::Active
                && now >= right.validity.valid_from
                && right.validity.valid_until.is_none_or(|until| now <= until)
                && right.remaining_uses.is_none_or(|uses| uses > 0)
        }

        fn has_valid_right(asset_id: &Hash32) -> Result<bool, DispatchError> {
            for right_id in AssetRightIds::<T>::get(asset_id) {
                let right = RightTokens::<T>::get(right_id).ok_or(Error::<T>::RightNotFound)?;
                ensure!(right.asset_id == *asset_id, Error::<T>::QuotaInvariant);
                if Self::right_is_valid_at(&right, Self::current_block_u64()) {
                    return Ok(true);
                }
            }
            Ok(false)
        }

        fn close_issue_authorization(
            owner: &T::AccountId,
            auth_id: Hash32,
            status: AuthorizationStatus,
        ) -> DispatchResult {
            let mut auth = IssueRightAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(auth.owner == *owner, Error::<T>::OwnershipMismatch);
            ensure!(
                auth.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationRevoked
            );
            let active = ActiveIssueAuthorizationCount::<T>::get(auth.asset_id)
                .checked_sub(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            let mut market =
                Markets::<T>::get(auth.market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_sub(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            auth.status = status;
            IssueRightAuthorizations::<T>::insert(auth_id, auth.clone());
            ActiveIssueAuthorization::<T>::remove(auth.asset_id, auth.market_id);
            ActiveIssueAuthorizationCount::<T>::insert(auth.asset_id, active);
            Markets::<T>::insert(auth.market_id, market);
            Self::deposit_event(Event::AuthorizationStatusChanged { auth_id, status });
            Ok(())
        }

        fn close_transfer_authorization(
            owner: &T::AccountId,
            auth_id: Hash32,
            status: AuthorizationStatus,
        ) -> DispatchResult {
            let mut auth = TransferMetaAuthorizations::<T>::get(auth_id)
                .ok_or(Error::<T>::AuthorizationMissing)?;
            ensure!(auth.owner == *owner, Error::<T>::OwnershipMismatch);
            ensure!(
                auth.status == AuthorizationStatus::Active,
                Error::<T>::AuthorizationRevoked
            );
            let mut market =
                Markets::<T>::get(auth.market_id).ok_or(Error::<T>::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_sub(1)
                .ok_or(Error::<T>::CounterInvariant)?;
            auth.status = status;
            TransferMetaAuthorizations::<T>::insert(auth_id, auth.clone());
            ActiveTransferAuthorizationByAsset::<T>::remove(auth.asset_id);
            Markets::<T>::insert(auth.market_id, market);
            Self::deposit_event(Event::AuthorizationStatusChanged { auth_id, status });
            Ok(())
        }
    }
}

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;
