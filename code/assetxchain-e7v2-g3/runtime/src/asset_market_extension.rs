// 数据资产扩展模块
// 与pallet-contracts交互，实现数据资产扩展，市场交易成功后，更新数据资产的状态

use codec::{Decode, DecodeAll, Encode, MaxEncodedLen};
use frame_support::weights::Weight;
use pallet_contracts::chain_extension::{
    BufInBufOutState, ChainExtension, Environment, Ext, InitState, RetVal,
};
use pallet_dataassets::protocol::{
    self, BuyerCommitment, CommonErrorCode, OrderCommitment, OrderState, PaymentState,
    PricePolicyWitness, ProtocolSignature, RuntimeOrder, SellerCommitment, SettlementRecord,
};
use scale_info::TypeInfo;
use sp_runtime::{traits::Zero, DispatchError};

pub const LEGACY_EXTENSION_ID: u16 = 1;

pub const SETTLEMENT_V2_EXTENSION_ID: u16 = 2;
pub const CREATE_ORDER_V2_FUNC_ID: u16 = 1;
pub const FUND_ORDER_V2_FUNC_ID: u16 = 2;
pub const SETTLE_V2_FUNC_ID: u16 = 3;
pub const CANCEL_OPEN_V2_FUNC_ID: u16 = 4;
pub const REFUND_FUNDED_V2_FUNC_ID: u16 = 5;
pub const QUERY_ORDER_V2_FUNC_ID: u16 = 6;
pub const QUERY_SETTLEMENT_V2_FUNC_ID: u16 = 7;

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct CreateOrderV2Request {
    pub order: OrderCommitment,
    pub price_policy_witness: PricePolicyWitness,
    pub seller_commitment: SellerCommitment,
    pub seller_signature: ProtocolSignature,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct FundOrderV2Request {
    pub order_id: protocol::Hash32,
    pub buyer_commitment: BuyerCommitment,
    pub buyer_signature: ProtocolSignature,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct FundedV2Receipt {
    pub order_id: protocol::Hash32,
    pub escrow_id: protocol::Hash32,
    pub funded_at: u32,
    pub grace_deadline: u32,
    pub order_state: OrderState,
    pub payment_state: PaymentState,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct RefundedV2Receipt {
    pub order_id: protocol::Hash32,
    pub buyer: protocol::ProtocolAccountId,
    pub amount: u128,
    pub order_state: OrderState,
    pub payment_state: PaymentState,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum V2Call {
    CreateOrder,
    FundOrder,
    Settle,
    CancelOpen,
    RefundFunded,
}

fn read_v2_input<E, D>(env: &mut Environment<E, BufInBufOutState>) -> Result<D, CommonErrorCode>
where
    E: Ext,
    D: Decode + MaxEncodedLen,
{
    if env.in_len() as usize > D::max_encoded_len() {
        return Err(CommonErrorCode::MalformedCommitment);
    }
    let input = env
        .read(env.in_len())
        .map_err(|_| CommonErrorCode::MalformedCommitment)?;
    decode_v2_bytes(&input)
}

fn decode_v2_bytes<D: Decode + MaxEncodedLen>(input: &[u8]) -> Result<D, CommonErrorCode> {
    if input.len() > D::max_encoded_len() {
        return Err(CommonErrorCode::MalformedCommitment);
    }
    D::decode_all(&mut &input[..]).map_err(|_| CommonErrorCode::MalformedCommitment)
}

fn write_v2_output<E: Ext, D: Encode>(
    env: &mut Environment<E, BufInBufOutState>,
    output: &D,
) -> Result<(), DispatchError> {
    env.write(&output.encode(), false, Some(Weight::from_parts(1_000, 0)))
}

fn v2_signed_caller<T: pallet_contracts::Config>(
    caller: &pallet_contracts::Origin<T>,
) -> Result<T::AccountId, DispatchError> {
    caller.account_id().cloned()
}

fn v2_weight<T: pallet_dataassets::Config>(func_id: u16) -> Weight {
    type WeightInfoOf<T> = <T as pallet_dataassets::Config>::WeightInfo;
    match func_id {
        CREATE_ORDER_V2_FUNC_ID => {
            <WeightInfoOf<T> as pallet_dataassets::pallet::WeightInfo>::create_order()
        }
        FUND_ORDER_V2_FUNC_ID => {
            <WeightInfoOf<T> as pallet_dataassets::pallet::WeightInfo>::fund_order()
        }
        SETTLE_V2_FUNC_ID => <WeightInfoOf<T> as pallet_dataassets::pallet::WeightInfo>::settle(),
        CANCEL_OPEN_V2_FUNC_ID => {
            <WeightInfoOf<T> as pallet_dataassets::pallet::WeightInfo>::cancel_open()
        }
        REFUND_FUNDED_V2_FUNC_ID => {
            <WeightInfoOf<T> as pallet_dataassets::pallet::WeightInfo>::refund_funded()
        }
        QUERY_ORDER_V2_FUNC_ID | QUERY_SETTLEMENT_V2_FUNC_ID => {
            Weight::from_parts(25_000_000, 8_192)
        }
        _ => Weight::zero(),
    }
}

fn map_v2_error<T: pallet_dataassets::Config>(
    call: V2Call,
    error: &DispatchError,
) -> Option<CommonErrorCode> {
    use pallet_dataassets::Error as E;

    let is = |candidate: E<T>| *error == candidate.into();
    let code = if is(E::<T>::UnsupportedProtocolVersion) {
        CommonErrorCode::UnsupportedProtocolVersion
    } else if is(E::<T>::MalformedCommitment) || is(E::<T>::InvalidInput) {
        CommonErrorCode::MalformedCommitment
    } else if is(E::<T>::MarketNotRegistered) {
        CommonErrorCode::MarketNotRegistered
    } else if is(E::<T>::MarketRestricted) {
        CommonErrorCode::MarketRestricted
    } else if is(E::<T>::CapabilityDenied) {
        CommonErrorCode::CapabilityDenied
    } else if is(E::<T>::MarketActiveOrderCapacityExceeded) || is(E::<T>::MarketCapacityExceeded) {
        CommonErrorCode::CapacityExceeded
    } else if is(E::<T>::AuthorizationMissing)
        || is(E::<T>::NotAuthorized)
        || is(E::<T>::AuthorizationPolicyMismatch)
    {
        CommonErrorCode::AuthorizationMissing
    } else if is(E::<T>::AuthorizationExpired) {
        CommonErrorCode::AuthorizationExpired
    } else if is(E::<T>::AuthorizationRevoked)
        || is(E::<T>::AuthorizationNotActive)
        || is(E::<T>::AuthorizationConsumed)
    {
        CommonErrorCode::AuthorizationRevoked
    } else if is(E::<T>::QuotaExceeded)
        || is(E::<T>::QuotaReservationAlreadyExists)
        || is(E::<T>::QuotaReservationMissing)
        || is(E::<T>::QuotaReservationStateInvalid)
        || is(E::<T>::QuotaReservationAuthorizationMismatch)
    {
        CommonErrorCode::QuotaExceeded
    } else if is(E::<T>::InvalidSignature) {
        CommonErrorCode::InvalidSignature
    } else if is(E::<T>::NonceUsed)
        || is(E::<T>::AuthorizationNonceMismatch)
        || is(E::<T>::OrderAlreadyExists)
        || is(E::<T>::ProtocolNonceOverflow)
    {
        CommonErrorCode::NonceUsed
    } else if is(E::<T>::StateVersionMismatch) {
        CommonErrorCode::StateVersionMismatch
    } else if is(E::<T>::OrderNotFound) {
        CommonErrorCode::OrderNotFound
    } else if is(E::<T>::OrderStatusInvalid) {
        match call {
            V2Call::FundOrder | V2Call::CancelOpen => CommonErrorCode::OrderNotOpen,
            V2Call::Settle => CommonErrorCode::OrderNotFunded,
            V2Call::CreateOrder | V2Call::RefundFunded => CommonErrorCode::TerminalOrder,
        }
    } else if is(E::<T>::OrderNotFunded) {
        CommonErrorCode::OrderNotFunded
    } else if is(E::<T>::OrderExpired) {
        CommonErrorCode::OrderExpired
    } else if is(E::<T>::AlreadySettled) {
        CommonErrorCode::AlreadySettled
    } else if is(E::<T>::TerminalOrder) {
        CommonErrorCode::TerminalOrder
    } else if is(E::<T>::AssetNotFound) || is(E::<T>::AssetNotActive) {
        CommonErrorCode::AssetNotTradable
    } else if is(E::<T>::CertificateNotFound)
        || is(E::<T>::CertificateNotActive)
        || is(E::<T>::RightInvalid)
        || is(E::<T>::InvalidRightParameters)
        || is(E::<T>::RightAlreadyExists)
    {
        CommonErrorCode::RightInvalid
    } else if is(E::<T>::NotOwner) || is(E::<T>::OwnershipMismatch) {
        CommonErrorCode::OwnershipMismatch
    } else if is(E::<T>::ParentMismatch) {
        CommonErrorCode::ParentMismatch
    } else if is(E::<T>::AssetLocked)
        || is(E::<T>::OutstandingCertificates)
        || is(E::<T>::MutualExclusionConflict)
        || is(E::<T>::ActiveObjectOrderExists)
        || is(E::<T>::ActiveObjectOrderMismatch)
    {
        CommonErrorCode::MutualExclusionConflict
    } else if is(E::<T>::BelowPriceFloor) {
        CommonErrorCode::BelowPriceFloor
    } else if is(E::<T>::FeeOutOfBounds) || is(E::<T>::InvalidPricePolicy) {
        CommonErrorCode::FeeOutOfBounds
    } else if is(E::<T>::InsufficientBalance) {
        CommonErrorCode::InsufficientBalance
    } else if is(E::<T>::EscrowMismatch) || is(E::<T>::PaymentAmountInvalid) {
        CommonErrorCode::EscrowMismatch
    } else if is(E::<T>::PayerBuyerMismatch) || is(E::<T>::ProtocolAccountIdInvalid) {
        CommonErrorCode::PayerBuyerMismatch
    } else if is(E::<T>::GracePeriodActive) {
        CommonErrorCode::GracePeriodActive
    } else if is(E::<T>::OrderNotExpired) {
        CommonErrorCode::RefundNotAllowed
    } else {
        return None;
    };
    Some(code)
}

fn v2_genesis_hash<T: frame_system::Config>() -> Result<protocol::Hash32, DispatchError>
where
    T::Hash: AsRef<[u8]>,
{
    let zero: frame_system::pallet_prelude::BlockNumberFor<T> = Zero::zero();
    let hash = frame_system::Pallet::<T>::block_hash(zero);
    let bytes = hash.as_ref();
    if bytes.len() != 32 {
        return Err(DispatchError::Other("Invalid genesis hash length"));
    }
    let mut result = [0u8; 32];
    result.copy_from_slice(bytes);
    Ok(result)
}

fn mapped_v2_error<T: pallet_dataassets::Config>(
    call: V2Call,
    error: DispatchError,
) -> Result<RetVal, DispatchError> {
    match map_v2_error::<T>(call, &error) {
        Some(code) => Ok(RetVal::Converging(code.status_code())),
        None => Err(error),
    }
}

fn call_v2<T, E>(mut env: Environment<E, InitState>) -> Result<RetVal, DispatchError>
where
    T: pallet_contracts::Config + pallet_dataassets::Config,
    T::Hash: AsRef<[u8]>,
    <T as frame_system::Config>::RuntimeOrigin:
        From<frame_system::RawOrigin<<T as frame_system::Config>::AccountId>>,
    E: Ext<T = T>,
{
    let func_id = env.func_id();
    if !matches!(
        func_id,
        CREATE_ORDER_V2_FUNC_ID
            | FUND_ORDER_V2_FUNC_ID
            | SETTLE_V2_FUNC_ID
            | CANCEL_OPEN_V2_FUNC_ID
            | REFUND_FUNDED_V2_FUNC_ID
            | QUERY_ORDER_V2_FUNC_ID
            | QUERY_SETTLEMENT_V2_FUNC_ID
    ) {
        return Err(DispatchError::Other("Unregistered CE v2 function"));
    }

    env.charge_weight(v2_weight::<T>(func_id))?;
    let caller = if func_id <= REFUND_FUNDED_V2_FUNC_ID {
        let caller = env.ext().caller();
        Some(v2_signed_caller::<T>(&caller)?)
    } else {
        None
    };
    let mut env = env.buf_in_buf_out();

    match func_id {
        CREATE_ORDER_V2_FUNC_ID => {
            let request: CreateOrderV2Request = match read_v2_input(&mut env) {
                Ok(request) => request,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let order_id = protocol::order_id(v2_genesis_hash::<T>()?, &request.order);
            let origin =
                frame_system::RawOrigin::Signed(caller.expect("mutation has caller")).into();
            match pallet_dataassets::Pallet::<T>::create_order(
                origin,
                request.order,
                request.price_policy_witness,
                request.seller_commitment,
                request.seller_signature,
            ) {
                Ok(()) => {
                    write_v2_output(&mut env, &order_id)?;
                    Ok(RetVal::Converging(0))
                }
                Err(error) => mapped_v2_error::<T>(V2Call::CreateOrder, error),
            }
        }
        FUND_ORDER_V2_FUNC_ID => {
            let request: FundOrderV2Request = match read_v2_input(&mut env) {
                Ok(request) => request,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let origin =
                frame_system::RawOrigin::Signed(caller.expect("mutation has caller")).into();
            match pallet_dataassets::Pallet::<T>::fund_order(
                origin,
                request.order_id,
                request.buyer_commitment,
                request.buyer_signature,
            ) {
                Ok(()) => {
                    let order = pallet_dataassets::Pallet::<T>::query_order(&request.order_id)
                        .ok_or(DispatchError::Other("Funded order missing"))?;
                    let escrow = pallet_dataassets::Pallet::<T>::query_payment(&request.order_id)
                        .ok_or(DispatchError::Other("Funded escrow missing"))?;
                    let receipt = FundedV2Receipt {
                        order_id: request.order_id,
                        escrow_id: escrow.escrow_id,
                        funded_at: order
                            .funded_at
                            .ok_or(DispatchError::Other("Funded block missing"))?,
                        grace_deadline: order
                            .grace_deadline
                            .ok_or(DispatchError::Other("Grace deadline missing"))?,
                        order_state: order.state,
                        payment_state: escrow.state,
                    };
                    write_v2_output(&mut env, &receipt)?;
                    Ok(RetVal::Converging(0))
                }
                Err(error) => mapped_v2_error::<T>(V2Call::FundOrder, error),
            }
        }
        SETTLE_V2_FUNC_ID => {
            let order_id: protocol::Hash32 = match read_v2_input(&mut env) {
                Ok(order_id) => order_id,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let origin =
                frame_system::RawOrigin::Signed(caller.expect("mutation has caller")).into();
            match pallet_dataassets::Pallet::<T>::settle(origin, order_id) {
                Ok(()) => {
                    let settlement = pallet_dataassets::Pallet::<T>::query_settlement(&order_id)
                        .ok_or(DispatchError::Other("Settlement record missing"))?;
                    write_v2_output(&mut env, &settlement.settlement_id)?;
                    Ok(RetVal::Converging(0))
                }
                Err(error) => mapped_v2_error::<T>(V2Call::Settle, error),
            }
        }
        CANCEL_OPEN_V2_FUNC_ID => {
            let order_id: protocol::Hash32 = match read_v2_input(&mut env) {
                Ok(order_id) => order_id,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let origin =
                frame_system::RawOrigin::Signed(caller.expect("mutation has caller")).into();
            match pallet_dataassets::Pallet::<T>::cancel_open(origin, order_id) {
                Ok(()) => {
                    let state = pallet_dataassets::Pallet::<T>::query_order(&order_id)
                        .ok_or(DispatchError::Other("Cancelled order missing"))?
                        .state;
                    write_v2_output(&mut env, &state)?;
                    Ok(RetVal::Converging(0))
                }
                Err(error) => mapped_v2_error::<T>(V2Call::CancelOpen, error),
            }
        }
        REFUND_FUNDED_V2_FUNC_ID => {
            let order_id: protocol::Hash32 = match read_v2_input(&mut env) {
                Ok(order_id) => order_id,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let origin =
                frame_system::RawOrigin::Signed(caller.expect("mutation has caller")).into();
            match pallet_dataassets::Pallet::<T>::refund_funded(origin, order_id) {
                Ok(()) => {
                    let order = pallet_dataassets::Pallet::<T>::query_order(&order_id)
                        .ok_or(DispatchError::Other("Refunded order missing"))?;
                    let payment = pallet_dataassets::Pallet::<T>::query_payment(&order_id)
                        .ok_or(DispatchError::Other("Refunded payment missing"))?;
                    let buyer = order
                        .buyer_commitment
                        .ok_or(DispatchError::Other("Refund buyer missing"))?
                        .buyer;
                    let receipt = RefundedV2Receipt {
                        order_id,
                        buyer,
                        amount: payment.amounts.gross_price,
                        order_state: order.state,
                        payment_state: payment.state,
                    };
                    write_v2_output(&mut env, &receipt)?;
                    Ok(RetVal::Converging(0))
                }
                Err(error) => mapped_v2_error::<T>(V2Call::RefundFunded, error),
            }
        }
        QUERY_ORDER_V2_FUNC_ID => {
            let order_id: protocol::Hash32 = match read_v2_input(&mut env) {
                Ok(order_id) => order_id,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let order: RuntimeOrder = match pallet_dataassets::Pallet::<T>::query_order(&order_id) {
                Some(order) => order,
                None => {
                    return Ok(RetVal::Converging(
                        CommonErrorCode::OrderNotFound.status_code(),
                    ))
                }
            };
            write_v2_output(&mut env, &order)?;
            Ok(RetVal::Converging(0))
        }
        QUERY_SETTLEMENT_V2_FUNC_ID => {
            let order_id: protocol::Hash32 = match read_v2_input(&mut env) {
                Ok(order_id) => order_id,
                Err(error) => return Ok(RetVal::Converging(error.status_code())),
            };
            let settlement: Option<SettlementRecord> =
                pallet_dataassets::Pallet::<T>::query_settlement(&order_id);
            write_v2_output(&mut env, &settlement)?;
            Ok(RetVal::Converging(0))
        }
        _ => Err(DispatchError::Other("Unregistered CE v2 function")),
    }
}

fn extension_version_enabled(extension_id: u16) -> bool {
    extension_id == SETTLEMENT_V2_EXTENSION_ID
}

#[derive(Default)]
pub struct DataAssetsExtension;

impl<T> ChainExtension<T> for DataAssetsExtension
where
    T: pallet_contracts::Config + pallet_dataassets::Config,
    <T as frame_system::Config>::Hash: AsRef<[u8]>,
    <T as frame_system::Config>::RuntimeOrigin:
        From<frame_system::RawOrigin<<T as frame_system::Config>::AccountId>>,
{
    fn call<E: Ext>(&mut self, env: Environment<E, InitState>) -> Result<RetVal, DispatchError>
    where
        E: Ext<T = T>,
    {
        let extension_id = env.ext_id();
        if !extension_version_enabled(extension_id) {
            return if extension_id == LEGACY_EXTENSION_ID {
                Err(DispatchError::Other("Legacy chain extension disabled"))
            } else {
                Err(DispatchError::Other("Unregistered chain extension"))
            };
        }
        call_v2::<T, E>(env)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use frame_support::traits::Get;
    use pallet_dataassets::protocol::{CurrencyId, ObjectType};
    use pallet_shared_traits::MarketOperation;

    fn sample_order() -> OrderCommitment {
        OrderCommitment {
            settlement_protocol_version: protocol::SETTLEMENT_PROTOCOL_VERSION,
            market_id: [1; 32],
            operation: MarketOperation::TransferMeta,
            object_type: ObjectType::MetaToken,
            object_id: [2; 32],
            parent_asset_id: None,
            authorization_id: [3; 32],
            seller: [4; 32],
            allowed_buyer: Some([5; 32]),
            price: 10_000,
            currency: CurrencyId::Native,
            policy_hash: [6; 32],
            right_parameters_hash: None,
            seller_state_version: 1,
            order_nonce: 7,
            expiry: 20,
        }
    }

    fn sample_seller(order_id: protocol::Hash32) -> SellerCommitment {
        SellerCommitment {
            order_id,
            object_id: [2; 32],
            operation: MarketOperation::TransferMeta,
            seller: [4; 32],
            state_version: 1,
            price_policy_hash: [6; 32],
            right_parameters_hash: None,
            expires_at: 20,
            seller_nonce: 7,
        }
    }

    fn sample_buyer(order_id: protocol::Hash32) -> BuyerCommitment {
        BuyerCommitment {
            order_id,
            buyer: [5; 32],
            price: 10_000,
            currency: CurrencyId::Native,
            expires_at: 20,
            buyer_nonce: 9,
        }
    }

    fn sample_runtime_order(order_id: protocol::Hash32) -> RuntimeOrder {
        RuntimeOrder {
            order_id,
            order_commitment: sample_order(),
            authorization_id: [3; 32],
            seller_commitment: sample_seller(order_id),
            buyer_commitment: Some(sample_buyer(order_id)),
            payment_asset: CurrencyId::Native,
            escrow_id: Some([10; 32]),
            created_at: 10,
            funded_at: Some(11),
            expiry: 20,
            grace_deadline: Some(21),
            state: OrderState::Funded,
        }
    }

    fn sample_settlement(order_id: protocol::Hash32) -> SettlementRecord {
        SettlementRecord {
            settlement_id: [11; 32],
            order_id,
            operation: MarketOperation::TransferMeta,
            asset_id: [2; 32],
            right_id: None,
            authorization_id: [3; 32],
            market_id: [1; 32],
            policy_hash: [6; 32],
            order_commitment_hash: [12; 32],
            seller: [4; 32],
            buyer: [5; 32],
            currency: CurrencyId::Native,
            gross_price: 10_000,
            seller_amount: 9_750,
            royalty_recipient: None,
            royalty_amount: 0,
            market_fee_recipient: [13; 32],
            market_fee_amount: 250,
            safety_fee_amount: 0,
            pre_state_version: 1,
            post_state_version: 2,
            pre_state_commitment: [14; 32],
            block_number: 12,
            settlement_protocol_version: protocol::SETTLEMENT_PROTOCOL_VERSION,
        }
    }

    #[test]
    fn legacy_extension_is_disabled_and_only_v2_is_enabled() {
        assert!(!extension_version_enabled(LEGACY_EXTENSION_ID));
        assert!(extension_version_enabled(SETTLEMENT_V2_EXTENSION_ID));
        assert!(!extension_version_enabled(3));
    }

    #[test]
    fn production_runtime_rejects_all_legacy_direct_mutations() {
        type RuntimeError = pallet_dataassets::Error<crate::Runtime>;
        type LegacyMutationEnabled =
            <crate::Runtime as pallet_dataassets::Config>::LegacyMutationEnabled;

        assert!(
            !<LegacyMutationEnabled as Get<bool>>::get(),
            "production Runtime must keep legacy mutations disabled"
        );

        let user = crate::AccountId::from([7u8; 32]);
        let market = crate::AccountId::from([8u8; 32]);
        let asset_id = [1u8; 32];
        let right_id = [2u8; 32];
        frame_support::__private::TestExternalities::default().execute_with(|| {
            let expected: DispatchError = RuntimeError::LegacyMutationDisabled.into();
            assert_eq!(
                crate::DataAssets::issue_certificate(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                    user.clone(),
                    0,
                    None,
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::transfer_asset(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                    market.clone(),
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::revoke_certificate(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                    right_id,
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::lock_asset(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::unlock_asset(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::authorize_market(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                    market.clone(),
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::revoke_authorization(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::transfer_asset_by_market(
                    crate::RuntimeOrigin::signed(market),
                    asset_id,
                    user.clone(),
                ),
                Err(expected.clone())
            );
            assert_eq!(
                crate::DataAssets::transfer_certificate(
                    crate::RuntimeOrigin::signed(user.clone()),
                    asset_id,
                    right_id,
                    user,
                ),
                Err(expected)
            );
        });
    }

    #[test]
    fn v2_extension_and_function_ids_are_versioned_and_unique() {
        assert_eq!(SETTLEMENT_V2_EXTENSION_ID, 2);
        assert_ne!(SETTLEMENT_V2_EXTENSION_ID, LEGACY_EXTENSION_ID);
        let ids = [
            CREATE_ORDER_V2_FUNC_ID,
            FUND_ORDER_V2_FUNC_ID,
            SETTLE_V2_FUNC_ID,
            CANCEL_OPEN_V2_FUNC_ID,
            REFUND_FUNDED_V2_FUNC_ID,
            QUERY_ORDER_V2_FUNC_ID,
            QUERY_SETTLEMENT_V2_FUNC_ID,
        ];
        assert_eq!(ids, [1, 2, 3, 4, 5, 6, 7]);
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id));
        }
    }

    #[test]
    fn common_error_statuses_are_nonzero_unique_and_scale_stable() {
        assert_eq!(CommonErrorCode::ALL.len(), 33);
        assert!(CommonErrorCode::from_status_code(0).is_none());
        for (index, error) in CommonErrorCode::ALL.iter().copied().enumerate() {
            let status = error.status_code();
            assert_ne!(status, 0);
            assert_eq!(status as usize, index + 1);
            assert_eq!(error.encode(), vec![status as u8]);
            assert_eq!(CommonErrorCode::from_status_code(status), Some(error));
        }
    }

    #[test]
    fn all_five_mutations_and_two_queries_use_strict_runtime_scale_types() {
        let order = sample_order();
        let order_id = protocol::order_id([9; 32], &order);
        let create = CreateOrderV2Request {
            price_policy_witness: PricePolicyWitness::Authorization,
            seller_commitment: sample_seller(order_id),
            seller_signature: [8; 64],
            order: order.clone(),
        };
        let create_encoded = create.encode();
        assert_eq!(
            decode_v2_bytes::<CreateOrderV2Request>(&create_encoded),
            Ok(create)
        );
        let mut trailing = create_encoded;
        trailing.push(0);
        assert_eq!(
            decode_v2_bytes::<CreateOrderV2Request>(&trailing),
            Err(CommonErrorCode::MalformedCommitment)
        );

        let fund = FundOrderV2Request {
            order_id,
            buyer_commitment: sample_buyer(order_id),
            buyer_signature: [9; 64],
        };
        assert_eq!(
            decode_v2_bytes::<FundOrderV2Request>(&fund.encode()),
            Ok(fund)
        );

        for function_id in [
            SETTLE_V2_FUNC_ID,
            CANCEL_OPEN_V2_FUNC_ID,
            REFUND_FUNDED_V2_FUNC_ID,
            QUERY_ORDER_V2_FUNC_ID,
            QUERY_SETTLEMENT_V2_FUNC_ID,
        ] {
            assert_eq!(
                decode_v2_bytes::<protocol::Hash32>(&order_id.encode()),
                Ok(order_id),
                "function {function_id} must accept the exact Runtime Hash32 input"
            );
        }

        let funded = FundedV2Receipt {
            order_id,
            escrow_id: [10; 32],
            funded_at: 11,
            grace_deadline: 21,
            order_state: OrderState::Funded,
            payment_state: PaymentState::Escrowed,
        };
        assert_eq!(
            decode_v2_bytes::<FundedV2Receipt>(&funded.encode()),
            Ok(funded)
        );

        let settlement_id = [11; 32];
        assert_eq!(
            decode_v2_bytes::<protocol::Hash32>(&settlement_id.encode()),
            Ok(settlement_id)
        );
        assert_eq!(
            decode_v2_bytes::<OrderState>(&OrderState::Cancelled.encode()),
            Ok(OrderState::Cancelled)
        );

        let refunded = RefundedV2Receipt {
            order_id,
            buyer: [5; 32],
            amount: 10_000,
            order_state: OrderState::Refunded,
            payment_state: PaymentState::Refunded,
        };
        assert_eq!(
            decode_v2_bytes::<RefundedV2Receipt>(&refunded.encode()),
            Ok(refunded)
        );

        let runtime_order = sample_runtime_order(order_id);
        assert_eq!(
            decode_v2_bytes::<RuntimeOrder>(&runtime_order.encode()),
            Ok(runtime_order)
        );
        let settlement = Some(sample_settlement(order_id));
        assert_eq!(
            decode_v2_bytes::<Option<SettlementRecord>>(&settlement.encode()),
            Ok(settlement)
        );
    }

    #[test]
    fn v2_error_mapping_covers_s213_negative_clean_duplicate_and_race_outcomes() {
        type RuntimeError = pallet_dataassets::Error<crate::Runtime>;
        let cases = [
            // NEG and CLEAN outcomes.
            (
                V2Call::CreateOrder,
                RuntimeError::UnsupportedProtocolVersion.into(),
                CommonErrorCode::UnsupportedProtocolVersion,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::MarketNotRegistered.into(),
                CommonErrorCode::MarketNotRegistered,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::MarketRestricted.into(),
                CommonErrorCode::MarketRestricted,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::CapabilityDenied.into(),
                CommonErrorCode::CapabilityDenied,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::MarketCapacityExceeded.into(),
                CommonErrorCode::CapacityExceeded,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::AuthorizationMissing.into(),
                CommonErrorCode::AuthorizationMissing,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::AuthorizationExpired.into(),
                CommonErrorCode::AuthorizationExpired,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::AuthorizationRevoked.into(),
                CommonErrorCode::AuthorizationRevoked,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::QuotaExceeded.into(),
                CommonErrorCode::QuotaExceeded,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::InvalidSignature.into(),
                CommonErrorCode::InvalidSignature,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::StateVersionMismatch.into(),
                CommonErrorCode::StateVersionMismatch,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::OwnershipMismatch.into(),
                CommonErrorCode::OwnershipMismatch,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::ParentMismatch.into(),
                CommonErrorCode::ParentMismatch,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::MutualExclusionConflict.into(),
                CommonErrorCode::MutualExclusionConflict,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::RightInvalid.into(),
                CommonErrorCode::RightInvalid,
            ),
            (
                V2Call::CancelOpen,
                RuntimeError::NotOwner.into(),
                CommonErrorCode::OwnershipMismatch,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::BelowPriceFloor.into(),
                CommonErrorCode::BelowPriceFloor,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::FeeOutOfBounds.into(),
                CommonErrorCode::FeeOutOfBounds,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::PayerBuyerMismatch.into(),
                CommonErrorCode::PayerBuyerMismatch,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::InsufficientBalance.into(),
                CommonErrorCode::InsufficientBalance,
            ),
            (
                V2Call::Settle,
                RuntimeError::OrderNotFunded.into(),
                CommonErrorCode::OrderNotFunded,
            ),
            (
                V2Call::Settle,
                RuntimeError::OrderExpired.into(),
                CommonErrorCode::OrderExpired,
            ),
            (
                V2Call::Settle,
                RuntimeError::EscrowMismatch.into(),
                CommonErrorCode::EscrowMismatch,
            ),
            (
                V2Call::RefundFunded,
                RuntimeError::GracePeriodActive.into(),
                CommonErrorCode::GracePeriodActive,
            ),
            (
                V2Call::RefundFunded,
                RuntimeError::OrderNotExpired.into(),
                CommonErrorCode::RefundNotAllowed,
            ),
            // DUP and RACE outcomes.
            (
                V2Call::CreateOrder,
                RuntimeError::OrderAlreadyExists.into(),
                CommonErrorCode::NonceUsed,
            ),
            (
                V2Call::FundOrder,
                RuntimeError::OrderStatusInvalid.into(),
                CommonErrorCode::OrderNotOpen,
            ),
            (
                V2Call::Settle,
                RuntimeError::OrderStatusInvalid.into(),
                CommonErrorCode::OrderNotFunded,
            ),
            (
                V2Call::Settle,
                RuntimeError::AlreadySettled.into(),
                CommonErrorCode::AlreadySettled,
            ),
            (
                V2Call::RefundFunded,
                RuntimeError::TerminalOrder.into(),
                CommonErrorCode::TerminalOrder,
            ),
            (
                V2Call::CreateOrder,
                RuntimeError::ActiveObjectOrderExists.into(),
                CommonErrorCode::MutualExclusionConflict,
            ),
        ];
        for (call, native, common) in cases {
            assert_eq!(map_v2_error::<crate::Runtime>(call, &native), Some(common));
        }
        let internal: DispatchError = RuntimeError::RightIndexInconsistent.into();
        assert_eq!(
            map_v2_error::<crate::Runtime>(V2Call::Settle, &internal),
            None
        );
    }

    #[test]
    fn v2_permissions_use_the_external_signed_caller() {
        let user = crate::AccountId::from([7u8; 32]);
        let contract = crate::AccountId::from([8u8; 32]);
        let caller = pallet_contracts::Origin::<crate::Runtime>::Signed(user.clone());
        assert_eq!(v2_signed_caller::<crate::Runtime>(&caller), Ok(user));
        assert_ne!(v2_signed_caller::<crate::Runtime>(&caller), Ok(contract));
        assert!(v2_signed_caller::<crate::Runtime>(&pallet_contracts::Origin::Root).is_err());
    }
}
