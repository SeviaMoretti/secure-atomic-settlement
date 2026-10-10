use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use pallet_shared_traits::MarketOperation;
use scale_info::TypeInfo;
use sp_core::sr25519;
use sp_io::hashing::blake2_256;
use sp_std::vec::Vec;

use crate::types::PricePolicy;

pub type Hash32 = [u8; 32];
pub type ProtocolAccountId = [u8; 32];
pub type ProtocolSignature = [u8; 64];

pub const SETTLEMENT_PROTOCOL_VERSION: u16 = 1;
pub const SETTLEMENT_DOMAIN_ID: [u8; 13] = *b"axc/p2/shared";

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum ObjectType {
    #[codec(index = 0)]
    MetaToken,
    #[codec(index = 1)]
    RightToken,
}

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
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

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
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

/// Paper-minimum v1 only accepts the Runtime native fungible balance.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum CurrencyId {
    #[codec(index = 0)]
    Native,
}

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum MessageType {
    #[codec(index = 0)]
    Order,
    #[codec(index = 1)]
    Seller,
    #[codec(index = 2)]
    Buyer,
    #[codec(index = 3)]
    Settlement,
    #[codec(index = 4)]
    Right,
}

/// The field declaration order is the frozen SCALE order from shared-protocol-v1.
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct OrderCommitment {
    pub settlement_protocol_version: u16,
    pub market_id: Hash32,
    pub operation: MarketOperation,
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

/// The field declaration order is the frozen SCALE order from shared-protocol-v1.
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct SellerCommitment {
    pub order_id: Hash32,
    pub object_id: Hash32,
    pub operation: MarketOperation,
    pub seller: ProtocolAccountId,
    pub state_version: u64,
    pub price_policy_hash: Hash32,
    pub right_parameters_hash: Option<Hash32>,
    pub expires_at: u32,
    pub seller_nonce: u64,
}

/// Non-canonical policy preimage supplied alongside create_order.
///
/// The witness is deliberately outside OrderCommitment and SellerCommitment:
/// both signed messages bind its hash, while their frozen SCALE layouts and
/// deterministic identifiers remain unchanged.
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum PricePolicyWitness {
    #[codec(index = 0)]
    Authorization,
    #[codec(index = 1)]
    SellerSnapshot(PricePolicy),
}

/// The field declaration order is the frozen SCALE order from shared-protocol-v1.
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct BuyerCommitment {
    pub order_id: Hash32,
    pub buyer: ProtocolAccountId,
    pub price: u128,
    pub currency: CurrencyId,
    pub expires_at: u32,
    pub buyer_nonce: u64,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
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

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct AmountSplit {
    pub gross_price: u128,
    pub seller_net: u128,
    pub royalty: u128,
    pub market_fee: u128,
    pub safety_fee: u128,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct RuntimeEscrow {
    pub escrow_id: Hash32,
    pub order_id: Hash32,
    pub payer: ProtocolAccountId,
    pub buyer: ProtocolAccountId,
    pub currency: CurrencyId,
    pub amounts: AmountSplit,
    pub royalty_recipient: Option<ProtocolAccountId>,
    pub market_fee_recipient: ProtocolAccountId,
    pub state: PaymentState,
}

/// Authoritative settlement-v2 record. Declaration order is the frozen
/// 24-field SCALE order from shared-protocol-v1.
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct SettlementRecord {
    pub settlement_id: Hash32,
    pub order_id: Hash32,
    pub operation: MarketOperation,
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

#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum NormalizedAuthorizationKind {
    #[codec(index = 0)]
    TransferMeta,
    #[codec(index = 1)]
    IssueRight,
    #[codec(index = 2)]
    TransferRightPolicy,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct NormalizedObjectSnapshot {
    pub asset_id: Hash32,
    pub asset_owner: ProtocolAccountId,
    pub asset_status: u8,
    pub asset_state_version: u64,
    pub right_id: Option<Hash32>,
    pub right_exists: bool,
    pub right_holder: Option<ProtocolAccountId>,
    pub right_status: Option<u8>,
    pub right_state_version: Option<u64>,
    pub right_parameters_hash: Option<Hash32>,
    pub active_order_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct NormalizedAuthorizationSnapshot {
    pub authorization_id: Hash32,
    pub kind: NormalizedAuthorizationKind,
    pub asset_id: Hash32,
    pub owner: ProtocolAccountId,
    pub market_id: Hash32,
    pub policy_hash: Hash32,
    pub right_parameters_hash: Option<Hash32>,
    pub nonce: u64,
    pub expiry: u32,
    pub status: u8,
    pub reserved_count: u32,
    pub reserved_use_units: u64,
    pub issued_count: u32,
    pub issued_use_units: u64,
    pub consumed_order_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct NormalizedMarketSnapshot {
    pub market_id: Hash32,
    pub operator: ProtocolAccountId,
    pub protocol_version: u16,
    pub capabilities: u8,
    pub capacity_tier: u8,
    pub active_orders: u32,
    pub funded_orders: u32,
    pub active_exposure_value: u128,
    pub active_authorizations: u32,
    pub status: u8,
    pub market_fee_bps: u16,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct NormalizedSettlementSnapshot {
    pub object: NormalizedObjectSnapshot,
    pub order: RuntimeOrder,
    pub authorization: NormalizedAuthorizationSnapshot,
    pub payment: RuntimeEscrow,
    pub settlement: Option<SettlementRecord>,
    pub market: NormalizedMarketSnapshot,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProtocolError {
    UnsupportedProtocolVersion,
    MalformedCommitment,
    InvalidSignature,
    NonceUsed,
    StateVersionMismatch,
    OrderExpired,
    NonceOverflow,
    FeeOutOfBounds,
    ArithmeticOverflow,
}

/// Stable common errors exposed by the paper-minimum settlement protocol.
///
/// Zero is reserved for Chain Extension success. The explicit values are part
/// of the CE v2 wire contract and must not be renumbered.
#[repr(u32)]
#[derive(
    Encode,
    Decode,
    DecodeWithMemTracking,
    Clone,
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
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

fn canonical_hash<T: Encode>(value: &T) -> Hash32 {
    blake2_256(&value.encode())
}

pub fn domain_separator(
    genesis_hash: Hash32,
    operation: MarketOperation,
    message_type: MessageType,
) -> Hash32 {
    canonical_hash(&(
        genesis_hash,
        SETTLEMENT_DOMAIN_ID,
        SETTLEMENT_PROTOCOL_VERSION,
        message_type,
        operation,
    ))
}

pub fn order_id(genesis_hash: Hash32, order: &OrderCommitment) -> Hash32 {
    let domain = domain_separator(genesis_hash, order.operation, MessageType::Order);
    canonical_hash(&(domain, order))
}

pub fn order_commitment_hash(order: &OrderCommitment) -> Hash32 {
    canonical_hash(order)
}

pub fn normalized_state_commitment(snapshot: &NormalizedSettlementSnapshot) -> Hash32 {
    canonical_hash(snapshot)
}

pub fn settlement_id(genesis_hash: Hash32, operation: MarketOperation, order_id: Hash32) -> Hash32 {
    let domain = domain_separator(genesis_hash, operation, MessageType::Settlement);
    canonical_hash(&(domain, order_id, *b"settlement"))
}

pub fn right_id(genesis_hash: Hash32, operation: MarketOperation, order_id: Hash32) -> Hash32 {
    let domain = domain_separator(genesis_hash, operation, MessageType::Right);
    canonical_hash(&(domain, order_id, *b"right"))
}

pub fn escrow_id(genesis_hash: Hash32, operation: MarketOperation, order_id: Hash32) -> Hash32 {
    let domain = domain_separator(genesis_hash, operation, MessageType::Settlement);
    canonical_hash(&(domain, order_id, *b"escrow"))
}

fn mul_bps_floor(value: u128, bps: u16) -> Result<u128, ProtocolError> {
    if bps > 10_000 {
        return Err(ProtocolError::FeeOutOfBounds);
    }
    let bps = u128::from(bps);
    let whole = (value / 10_000)
        .checked_mul(bps)
        .ok_or(ProtocolError::ArithmeticOverflow)?;
    let remainder = (value % 10_000)
        .checked_mul(bps)
        .ok_or(ProtocolError::ArithmeticOverflow)?
        / 10_000;
    whole
        .checked_add(remainder)
        .ok_or(ProtocolError::ArithmeticOverflow)
}

pub fn amount_split(
    gross_price: u128,
    royalty_bps: u16,
    market_fee_bps: u16,
    safety_fee_bps: u16,
) -> Result<AmountSplit, ProtocolError> {
    let total_bps = u32::from(royalty_bps)
        .checked_add(u32::from(market_fee_bps))
        .and_then(|value| value.checked_add(u32::from(safety_fee_bps)))
        .ok_or(ProtocolError::ArithmeticOverflow)?;
    if total_bps > 10_000 {
        return Err(ProtocolError::FeeOutOfBounds);
    }

    let royalty = mul_bps_floor(gross_price, royalty_bps)?;
    let market_fee = mul_bps_floor(gross_price, market_fee_bps)?;
    let safety_fee = mul_bps_floor(gross_price, safety_fee_bps)?;
    let deductions = royalty
        .checked_add(market_fee)
        .and_then(|value| value.checked_add(safety_fee))
        .ok_or(ProtocolError::ArithmeticOverflow)?;
    let seller_net = gross_price
        .checked_sub(deductions)
        .ok_or(ProtocolError::FeeOutOfBounds)?;

    Ok(AmountSplit {
        gross_price,
        seller_net,
        royalty,
        market_fee,
        safety_fee,
    })
}

pub fn seller_signing_payload(
    genesis_hash: Hash32,
    operation: MarketOperation,
    commitment: &SellerCommitment,
) -> Vec<u8> {
    (
        domain_separator(genesis_hash, operation, MessageType::Seller),
        commitment,
    )
        .encode()
}

pub fn buyer_signing_payload(
    genesis_hash: Hash32,
    operation: MarketOperation,
    commitment: &BuyerCommitment,
) -> Vec<u8> {
    (
        domain_separator(genesis_hash, operation, MessageType::Buyer),
        commitment,
    )
        .encode()
}

fn verify_signature(
    signer: &ProtocolAccountId,
    signature: &ProtocolSignature,
    payload: &[u8],
) -> bool {
    sp_io::crypto::sr25519_verify(
        &sr25519::Signature::from_raw(*signature),
        payload,
        &sr25519::Public::from_raw(*signer),
    )
}

pub fn validate_order_commitment(
    genesis_hash: Hash32,
    order: &OrderCommitment,
    current_block: u32,
) -> Result<Hash32, ProtocolError> {
    if order.settlement_protocol_version != SETTLEMENT_PROTOCOL_VERSION {
        return Err(ProtocolError::UnsupportedProtocolVersion);
    }
    if current_block > order.expiry {
        return Err(ProtocolError::OrderExpired);
    }
    if order.seller_state_version == 0 {
        return Err(ProtocolError::StateVersionMismatch);
    }

    let shape_is_valid = match order.operation {
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
    if !shape_is_valid {
        return Err(ProtocolError::MalformedCommitment);
    }

    Ok(order_id(genesis_hash, order))
}

pub fn verify_seller_commitment(
    genesis_hash: Hash32,
    order: &OrderCommitment,
    commitment: &SellerCommitment,
    signature: &ProtocolSignature,
    current_nonce: u64,
    current_state_version: u64,
    current_block: u32,
) -> Result<Hash32, ProtocolError> {
    let recomputed_order_id = validate_order_commitment(genesis_hash, order, current_block)?;
    if commitment.order_id != recomputed_order_id
        || commitment.object_id != order.object_id
        || commitment.operation != order.operation
        || commitment.seller != order.seller
        || commitment.state_version != order.seller_state_version
        || commitment.price_policy_hash != order.policy_hash
        || commitment.right_parameters_hash != order.right_parameters_hash
        || commitment.expires_at != order.expiry
        || commitment.seller_nonce != order.order_nonce
    {
        return Err(ProtocolError::MalformedCommitment);
    }

    let payload = seller_signing_payload(genesis_hash, order.operation, commitment);
    if !verify_signature(&commitment.seller, signature, &payload) {
        return Err(ProtocolError::InvalidSignature);
    }
    if commitment.seller_nonce != current_nonce {
        return Err(ProtocolError::NonceUsed);
    }
    if commitment.state_version != current_state_version {
        return Err(ProtocolError::StateVersionMismatch);
    }
    Ok(recomputed_order_id)
}

pub fn verify_buyer_commitment(
    genesis_hash: Hash32,
    order: &OrderCommitment,
    commitment: &BuyerCommitment,
    signature: &ProtocolSignature,
    current_nonce: u64,
    current_block: u32,
) -> Result<Hash32, ProtocolError> {
    let recomputed_order_id = validate_order_commitment(genesis_hash, order, current_block)?;
    if commitment.order_id != recomputed_order_id
        || commitment.price != order.price
        || commitment.currency != order.currency
        || commitment.expires_at != order.expiry
        || order
            .allowed_buyer
            .map_or(false, |allowed| allowed != commitment.buyer)
    {
        return Err(ProtocolError::MalformedCommitment);
    }

    let payload = buyer_signing_payload(genesis_hash, order.operation, commitment);
    if !verify_signature(&commitment.buyer, signature, &payload) {
        return Err(ProtocolError::InvalidSignature);
    }
    if commitment.buyer_nonce != current_nonce {
        return Err(ProtocolError::NonceUsed);
    }
    Ok(recomputed_order_id)
}

pub fn next_nonce(current_nonce: u64) -> Result<u64, ProtocolError> {
    current_nonce
        .checked_add(1)
        .ok_or(ProtocolError::NonceOverflow)
}
