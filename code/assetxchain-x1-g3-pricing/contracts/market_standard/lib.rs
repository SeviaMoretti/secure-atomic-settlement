#![cfg_attr(not(feature = "std"), no_std)]

use codec::{Decode, Encode};
use ink::env::Environment;
use scale_info::TypeInfo;

pub type Hash32 = [u8; 32];
pub type ProtocolAccountId = [u8; 32];
pub type ProtocolSignature = [u8; 64];
pub type Balance = <ink::env::DefaultEnvironment as Environment>::Balance;

pub const SETTLEMENT_PROTOCOL_VERSION: u16 = 1;
pub const SETTLEMENT_V2_EXT_ID: u32 = 2;
pub const CREATE_ORDER_V2_FUNC_ID: u32 = 1;
pub const FUND_ORDER_V2_FUNC_ID: u32 = 2;
pub const SETTLE_V2_FUNC_ID: u32 = 3;
pub const CANCEL_OPEN_V2_FUNC_ID: u32 = 4;
pub const REFUND_FUNDED_V2_FUNC_ID: u32 = 5;
pub const QUERY_ORDER_V2_FUNC_ID: u32 = 6;
pub const QUERY_SETTLEMENT_V2_FUNC_ID: u32 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub enum Operation {
    #[codec(index = 0)]
    TransferMeta,
    #[codec(index = 1)]
    IssueRight,
    #[codec(index = 2)]
    TransferRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub enum ObjectType {
    #[codec(index = 0)]
    MetaToken,
    #[codec(index = 1)]
    RightToken,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub enum OrderState {
    #[codec(index = 0)]
    Open,
    #[codec(index = 1)]
    Funded,
    #[codec(index = 2)]
    Settled,
    #[codec(index = 3)]
    Cancelled,
    #[codec(index = 4)]
    Expired,
    #[codec(index = 5)]
    Refunded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub enum PaymentState {
    #[codec(index = 0)]
    Unfunded,
    #[codec(index = 1)]
    Escrowed,
    #[codec(index = 2)]
    Released,
    #[codec(index = 3)]
    Refunded,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub enum CurrencyId {
    #[codec(index = 0)]
    Native,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct PricePolicy {
    pub reference_price: u128,
    pub floor_ratio_bps: u16,
    pub signed_market_fee_cap_bps: u16,
    pub safety_fee_bps: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub enum PricePolicyWitness {
    #[codec(index = 0)]
    Authorization,
    #[codec(index = 1)]
    SellerSnapshot(PricePolicy),
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct OrderCommitment {
    pub settlement_protocol_version: u16,
    pub market_id: Hash32,
    pub operation: Operation,
    pub object_type: ObjectType,
    pub object_id: Hash32,
    pub parent_asset_id: Option<Hash32>,
    pub authorization_id: Hash32,
    pub seller: ProtocolAccountId,
    pub allowed_buyer: Option<ProtocolAccountId>,
    pub price: u128,
    pub currency: CurrencyId,
    pub policy_hash: Hash32,
    pub right_parameters_hash: Option<Hash32>,
    pub seller_state_version: u64,
    pub order_nonce: u64,
    pub expiry: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct SellerCommitment {
    pub order_id: Hash32,
    pub object_id: Hash32,
    pub operation: Operation,
    pub seller: ProtocolAccountId,
    pub state_version: u64,
    pub price_policy_hash: Hash32,
    pub right_parameters_hash: Option<Hash32>,
    pub expires_at: u32,
    pub seller_nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct BuyerCommitment {
    pub order_id: Hash32,
    pub buyer: ProtocolAccountId,
    pub price: u128,
    pub currency: CurrencyId,
    pub expires_at: u32,
    pub buyer_nonce: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct RuntimeOrder {
    pub order_id: Hash32,
    pub order_commitment: OrderCommitment,
    pub authorization_id: Hash32,
    pub seller_commitment: SellerCommitment,
    pub buyer_commitment: Option<BuyerCommitment>,
    pub payment_asset: CurrencyId,
    pub escrow_id: Option<Hash32>,
    pub created_at: u32,
    pub funded_at: Option<u32>,
    pub expiry: u32,
    pub grace_deadline: Option<u32>,
    pub state: OrderState,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct SettlementRecord {
    pub settlement_id: Hash32,
    pub order_id: Hash32,
    pub operation: Operation,
    pub asset_id: Hash32,
    pub right_id: Option<Hash32>,
    pub authorization_id: Hash32,
    pub market_id: Hash32,
    pub policy_hash: Hash32,
    pub order_commitment_hash: Hash32,
    pub seller: ProtocolAccountId,
    pub buyer: ProtocolAccountId,
    pub currency: CurrencyId,
    pub gross_price: u128,
    pub seller_amount: u128,
    pub royalty_recipient: Option<ProtocolAccountId>,
    pub royalty_amount: u128,
    pub market_fee_recipient: ProtocolAccountId,
    pub market_fee_amount: u128,
    pub safety_fee_amount: u128,
    pub pre_state_version: u64,
    pub post_state_version: u64,
    pub pre_state_commitment: Hash32,
    pub block_number: u32,
    pub settlement_protocol_version: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct FundedV2Receipt {
    pub order_id: Hash32,
    pub escrow_id: Hash32,
    pub funded_at: u32,
    pub grace_deadline: u32,
    pub order_state: OrderState,
    pub payment_state: PaymentState,
}

#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, TypeInfo)]
#[cfg_attr(feature = "std", derive(ink::storage::traits::StorageLayout))]
pub struct RefundedV2Receipt {
    pub order_id: Hash32,
    pub buyer: ProtocolAccountId,
    pub amount: u128,
    pub order_state: OrderState,
    pub payment_state: PaymentState,
}

#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
pub enum CommonErrorCode {
    #[codec(index = 1)]
    UnsupportedProtocolVersion = 1,
    #[codec(index = 2)]
    MalformedCommitment = 2,
    #[codec(index = 3)]
    DomainMismatch = 3,
    #[codec(index = 4)]
    MarketNotRegistered = 4,
    #[codec(index = 5)]
    MarketRestricted = 5,
    #[codec(index = 6)]
    CapabilityDenied = 6,
    #[codec(index = 7)]
    CapacityExceeded = 7,
    #[codec(index = 8)]
    AuthorizationMissing = 8,
    #[codec(index = 9)]
    AuthorizationExpired = 9,
    #[codec(index = 10)]
    AuthorizationRevoked = 10,
    #[codec(index = 11)]
    QuotaExceeded = 11,
    #[codec(index = 12)]
    InvalidSignature = 12,
    #[codec(index = 13)]
    NonceUsed = 13,
    #[codec(index = 14)]
    StateVersionMismatch = 14,
    #[codec(index = 15)]
    OrderNotFound = 15,
    #[codec(index = 16)]
    OrderNotOpen = 16,
    #[codec(index = 17)]
    OrderNotFunded = 17,
    #[codec(index = 18)]
    OrderExpired = 18,
    #[codec(index = 19)]
    AlreadySettled = 19,
    #[codec(index = 20)]
    TerminalOrder = 20,
    #[codec(index = 21)]
    AssetNotTradable = 21,
    #[codec(index = 22)]
    RightInvalid = 22,
    #[codec(index = 23)]
    OwnershipMismatch = 23,
    #[codec(index = 24)]
    ParentMismatch = 24,
    #[codec(index = 25)]
    MutualExclusionConflict = 25,
    #[codec(index = 26)]
    BelowPriceFloor = 26,
    #[codec(index = 27)]
    FeeOutOfBounds = 27,
    #[codec(index = 28)]
    InsufficientBalance = 28,
    #[codec(index = 29)]
    EscrowMismatch = 29,
    #[codec(index = 30)]
    PayerBuyerMismatch = 30,
    #[codec(index = 31)]
    RefundNotAllowed = 31,
    #[codec(index = 32)]
    GracePeriodActive = 32,
    #[codec(index = 33)]
    RefundBeneficiaryMismatch = 33,
}

impl CommonErrorCode {
    pub const ALL: [Self; 33] = [
        Self::UnsupportedProtocolVersion,
        Self::MalformedCommitment,
        Self::DomainMismatch,
        Self::MarketNotRegistered,
        Self::MarketRestricted,
        Self::CapabilityDenied,
        Self::CapacityExceeded,
        Self::AuthorizationMissing,
        Self::AuthorizationExpired,
        Self::AuthorizationRevoked,
        Self::QuotaExceeded,
        Self::InvalidSignature,
        Self::NonceUsed,
        Self::StateVersionMismatch,
        Self::OrderNotFound,
        Self::OrderNotOpen,
        Self::OrderNotFunded,
        Self::OrderExpired,
        Self::AlreadySettled,
        Self::TerminalOrder,
        Self::AssetNotTradable,
        Self::RightInvalid,
        Self::OwnershipMismatch,
        Self::ParentMismatch,
        Self::MutualExclusionConflict,
        Self::BelowPriceFloor,
        Self::FeeOutOfBounds,
        Self::InsufficientBalance,
        Self::EscrowMismatch,
        Self::PayerBuyerMismatch,
        Self::RefundNotAllowed,
        Self::GracePeriodActive,
        Self::RefundBeneficiaryMismatch,
    ];

    pub const fn status_code(self) -> u32 {
        self as u32
    }

    pub fn from_status_code(status: u32) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|error| error.status_code() == status)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, TypeInfo)]
pub enum SettlementV2ExtError {
    Common(CommonErrorCode),
    UnknownStatus(u32),
    Decode,
}

impl ink::env::chain_extension::FromStatusCode for SettlementV2ExtError {
    fn from_status_code(status_code: u32) -> Result<(), Self> {
        if status_code == 0 {
            return Ok(());
        }
        match CommonErrorCode::from_status_code(status_code) {
            Some(error) => Err(Self::Common(error)),
            None => Err(Self::UnknownStatus(status_code)),
        }
    }
}

impl From<codec::Error> for SettlementV2ExtError {
    fn from(_: codec::Error) -> Self {
        Self::Decode
    }
}

#[ink::chain_extension(extension = 2)]
pub trait SettlementV2Ext {
    type ErrorCode = SettlementV2ExtError;

    #[ink(function = 1)]
    fn create_order(
        order: OrderCommitment,
        price_policy_witness: PricePolicyWitness,
        seller_commitment: SellerCommitment,
        seller_signature: ProtocolSignature,
    ) -> Result<Hash32, SettlementV2ExtError>;

    #[ink(function = 2)]
    fn fund_order(
        order_id: Hash32,
        buyer_commitment: BuyerCommitment,
        buyer_signature: ProtocolSignature,
    ) -> Result<FundedV2Receipt, SettlementV2ExtError>;

    #[ink(function = 3)]
    fn settle(order_id: Hash32) -> Result<Hash32, SettlementV2ExtError>;

    #[ink(function = 4)]
    fn cancel_open(order_id: Hash32) -> Result<OrderState, SettlementV2ExtError>;

    #[ink(function = 5)]
    fn refund_funded(order_id: Hash32) -> Result<RefundedV2Receipt, SettlementV2ExtError>;

    #[ink(function = 6)]
    fn query_order(order_id: Hash32) -> Result<RuntimeOrder, SettlementV2ExtError>;

    #[ink(function = 7)]
    fn query_settlement(order_id: Hash32)
        -> Result<Option<SettlementRecord>, SettlementV2ExtError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(TypeInfo))]
pub enum CustomEnvironment {}

impl Environment for CustomEnvironment {
    const MAX_EVENT_TOPICS: usize = <ink::env::DefaultEnvironment as Environment>::MAX_EVENT_TOPICS;
    type AccountId = <ink::env::DefaultEnvironment as Environment>::AccountId;
    type Balance = <ink::env::DefaultEnvironment as Environment>::Balance;
    type Hash = <ink::env::DefaultEnvironment as Environment>::Hash;
    type BlockNumber = <ink::env::DefaultEnvironment as Environment>::BlockNumber;
    type Timestamp = <ink::env::DefaultEnvironment as Environment>::Timestamp;
    type ChainExtension = SettlementV2Ext;
}

#[ink::trait_definition]
pub trait MarketStandard {
    #[ink(message)]
    fn is_assetx_market(&self) -> bool;

    #[ink(message)]
    fn get_market_type(&self) -> u8;

    #[ink(message)]
    fn get_fee_ratio(&self) -> u32;

    #[ink(message)]
    fn check_admission(&self, asset_id: Hash32) -> bool;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ink::env::chain_extension::FromStatusCode;

    #[test]
    fn v2_ids_are_fixed_and_v1_is_not_exposed() {
        assert_eq!(SETTLEMENT_V2_EXT_ID, 2);
        assert_eq!(
            [
                CREATE_ORDER_V2_FUNC_ID,
                FUND_ORDER_V2_FUNC_ID,
                SETTLE_V2_FUNC_ID,
                CANCEL_OPEN_V2_FUNC_ID,
                REFUND_FUNDED_V2_FUNC_ID,
                QUERY_ORDER_V2_FUNC_ID,
                QUERY_SETTLEMENT_V2_FUNC_ID,
            ],
            [1, 2, 3, 4, 5, 6, 7]
        );
    }

    #[test]
    fn all_common_statuses_and_unknown_nonzero_statuses_are_errors() {
        assert_eq!(SettlementV2ExtError::from_status_code(0), Ok(()));
        for error in CommonErrorCode::ALL {
            assert_eq!(
                SettlementV2ExtError::from_status_code(error.status_code()),
                Err(SettlementV2ExtError::Common(error))
            );
        }
        assert_eq!(
            SettlementV2ExtError::from_status_code(99),
            Err(SettlementV2ExtError::UnknownStatus(99))
        );
    }

    #[test]
    fn frozen_discriminants_and_error_scale_values_are_stable() {
        assert_eq!(Operation::TransferMeta.encode(), vec![0]);
        assert_eq!(Operation::IssueRight.encode(), vec![1]);
        assert_eq!(Operation::TransferRight.encode(), vec![2]);
        assert_eq!(OrderState::Refunded.encode(), vec![5]);
        for error in CommonErrorCode::ALL {
            assert_eq!(error.encode(), vec![error.status_code() as u8]);
        }
    }
}
