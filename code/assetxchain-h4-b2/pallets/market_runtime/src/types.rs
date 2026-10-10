use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{traits::ConstU32, BoundedVec};
use scale_info::TypeInfo;
use sp_core::sr25519;
use sp_std::vec::Vec;

pub type Hash32 = [u8; 32];
pub type ProtocolBalance = u128;
pub type ProtocolAccountId = [u8; 32];
pub type ProtocolSignature = [u8; 64];
pub type AllowedMarkets = BoundedVec<Hash32, ConstU32<16>>;

pub trait ProtocolAccountIdConverter<AccountId> {
    fn to_protocol_account_id(account: &AccountId) -> Option<ProtocolAccountId>;
    fn from_protocol_account_id(account: &ProtocolAccountId) -> Option<AccountId>;
}

pub struct AccountId32Converter;

impl ProtocolAccountIdConverter<sp_runtime::AccountId32> for AccountId32Converter {
    fn to_protocol_account_id(account: &sp_runtime::AccountId32) -> Option<ProtocolAccountId> {
        let mut raw = [0u8; 32];
        raw.copy_from_slice(account.as_ref());
        Some(raw)
    }

    fn from_protocol_account_id(account: &ProtocolAccountId) -> Option<sp_runtime::AccountId32> {
        Some(sp_runtime::AccountId32::new(*account))
    }
}

pub const SETTLEMENT_PROTOCOL_VERSION: u16 = 1;
pub const SETTLEMENT_DOMAIN_ID: [u8; 13] = *b"axc/p2/shared";
pub const AUTHORIZATION_DOMAIN_ID: &[u8] = b"axc/p2/authorization";
pub const CAP_CREATE_META_ORDER: u8 = 1 << 0;
pub const CAP_CREATE_RIGHT_ISSUE_ORDER: u8 = 1 << 1;
pub const CAP_CREATE_RIGHT_TRANSFER_ORDER: u8 = 1 << 2;
pub const CAP_RELAY_FUNDING: u8 = 1 << 3;
pub const CAP_RELAY_SETTLEMENT: u8 = 1 << 4;
pub const ALL_CAPABILITIES: u8 = CAP_CREATE_META_ORDER
    | CAP_CREATE_RIGHT_ISSUE_ORDER
    | CAP_CREATE_RIGHT_TRANSFER_ORDER
    | CAP_RELAY_FUNDING
    | CAP_RELAY_SETTLEMENT;

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
pub enum MarketOperation {
    #[codec(index = 0)]
    TransferMeta,
    #[codec(index = 1)]
    IssueRight,
    #[codec(index = 2)]
    TransferRight,
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
pub enum AssetStatus {
    #[codec(index = 3)]
    PendingValidation,
    #[codec(index = 4)]
    Tradable,
    #[codec(index = 5)]
    Retiring,
    #[codec(index = 6)]
    Retired,
    #[codec(index = 7)]
    Rejected,
    #[codec(index = 8)]
    Challenged,
    #[codec(index = 9)]
    Invalidated,
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
pub enum RightStatus {
    #[codec(index = 1)]
    Active,
    #[codec(index = 2)]
    Expired,
    #[codec(index = 3)]
    Exhausted,
    #[codec(index = 4)]
    Revoked,
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
pub enum RightType {
    #[codec(index = 1)]
    LegacyUsage,
    #[codec(index = 2)]
    LegacyAccess,
    #[codec(index = 3)]
    View,
    #[codec(index = 4)]
    Compute,
    #[codec(index = 5)]
    Train,
    #[codec(index = 6)]
    Derive,
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
pub enum DataScope {
    #[codec(index = 0)]
    FullAsset,
    #[codec(index = 1)]
    FieldSetHash(Hash32),
    #[codec(index = 2)]
    PartitionHash(Hash32),
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
pub struct RightValidity {
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub max_uses: Option<u32>,
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
pub struct PricePolicy {
    pub reference_price: ProtocolBalance,
    pub floor_ratio_bps: u16,
    pub signed_market_fee_cap_bps: u16,
    pub safety_fee_bps: u16,
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
    Copy,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    MaxEncodedLen,
)]
pub enum AuthorizationStatus {
    Active,
    Revoked,
    Expired,
    Consumed,
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
pub enum IssueQuotaReservationStatus {
    Reserved,
    Issued,
    Released,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct MetaToken<AccountId> {
    pub asset_id: Hash32,
    pub owner: AccountId,
    pub registration_provider: AccountId,
    pub service_provider: AccountId,
    pub metadata_digest: Hash32,
    pub data_commitment: Hash32,
    pub availability_descriptor_version: u32,
    pub validation_program_hash: Hash32,
    pub state_version: u64,
    pub status: AssetStatus,
    pub parent_asset_id: Option<Hash32>,
    pub supersedes_asset_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct MetaRegistration<AccountId> {
    pub asset_id: Hash32,
    pub owner: AccountId,
    pub registration_provider: AccountId,
    pub service_provider: AccountId,
    pub metadata_digest: Hash32,
    pub data_commitment: Hash32,
    pub validation_program_hash: Hash32,
    pub status: AssetStatus,
    pub parent_asset_id: Option<Hash32>,
    pub supersedes_asset_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct RightToken<AccountId> {
    pub right_id: Hash32,
    pub asset_id: Hash32,
    pub holder: AccountId,
    pub issuer: AccountId,
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity: RightValidity,
    pub remaining_uses: Option<u32>,
    pub reserved_uses: u32,
    pub transferable: bool,
    pub allowed_transfer_markets: AllowedMarkets,
    pub royalty_bps: u16,
    pub state_version: u64,
    pub status: RightStatus,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct ExistingRightRegistration<AccountId> {
    pub right_id: Hash32,
    pub asset_id: Hash32,
    pub holder: AccountId,
    pub issuer: AccountId,
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity: RightValidity,
    pub transferable: bool,
    pub allowed_transfer_markets: AllowedMarkets,
    pub royalty_bps: u16,
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
pub struct MarketCapacityTier {
    pub tier_id: u8,
    pub max_active_orders: u32,
    pub max_funded_orders: u32,
    pub max_active_exposure_value: ProtocolBalance,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct MarketRegistration<AccountId, BlockNumber> {
    pub market_id: Hash32,
    pub operator: AccountId,
    pub protocol_version: u16,
    pub capabilities: u8,
    pub market_fee_bps: u16,
    pub capacity_tier: MarketCapacityTier,
    pub stake: ProtocolBalance,
    pub active_orders: u32,
    pub funded_orders: u32,
    pub active_exposure_value: ProtocolBalance,
    pub active_authorizations: u32,
    pub status: MarketStatus,
    pub registered_at: BlockNumber,
    pub exit_requested_at: Option<BlockNumber>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct IssueRightAuthorizationTerms {
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity_template: RightValidity,
    pub max_issue_count: u32,
    pub max_total_use_units: u64,
    pub price_policy: PricePolicy,
    pub royalty_bps: u16,
    pub transferable: bool,
    pub allowed_transfer_markets: AllowedMarkets,
    pub expiry: u32,
    pub nonce: u64,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct TransferMetaAuthorizationTerms<AccountId> {
    pub price_policy: PricePolicy,
    pub allowed_buyer: Option<AccountId>,
    pub expiry: u32,
    pub nonce: u64,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct IssueRightAuthorization<AccountId> {
    pub auth_id: Hash32,
    pub asset_id: Hash32,
    pub owner: AccountId,
    pub market_id: Hash32,
    pub operation: MarketOperation,
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity_template: RightValidity,
    pub max_issue_count: u32,
    pub max_total_use_units: u64,
    pub price_policy: PricePolicy,
    pub royalty_bps: u16,
    pub transferable: bool,
    pub allowed_transfer_markets: AllowedMarkets,
    pub reserved_count: u32,
    pub reserved_use_units: u64,
    pub issued_count: u32,
    pub issued_use_units: u64,
    pub expiry: u32,
    pub nonce: u64,
    pub status: AuthorizationStatus,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct TransferMetaAuthorization<AccountId> {
    pub auth_id: Hash32,
    pub asset_id: Hash32,
    pub owner: AccountId,
    pub market_id: Hash32,
    pub operation: MarketOperation,
    pub price_policy: PricePolicy,
    pub allowed_buyer: Option<AccountId>,
    pub expiry: u32,
    pub nonce: u64,
    pub status: AuthorizationStatus,
    pub consumed_order_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct IssueQuotaReservation {
    pub order_id: Hash32,
    pub auth_id: Hash32,
    pub reserved_count: u32,
    pub reserved_use_units: u64,
    pub status: IssueQuotaReservationStatus,
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

/// Frozen 16-field SCALE order commitment.
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
    pub price: ProtocolBalance,
    pub currency: CurrencyId,
    pub policy_hash: Hash32,
    pub right_parameters_hash: Option<Hash32>,
    pub seller_state_version: u64,
    pub order_nonce: u64,
    pub expiry: u32,
}

/// Frozen nine-field SCALE seller commitment.
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

/// Frozen six-field SCALE buyer commitment.
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct BuyerCommitment {
    pub order_id: Hash32,
    pub buyer: ProtocolAccountId,
    pub price: ProtocolBalance,
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
pub struct AmountSplit {
    pub gross_price: ProtocolBalance,
    pub seller_net: ProtocolBalance,
    pub royalty: ProtocolBalance,
    pub market_fee: ProtocolBalance,
    pub safety_fee: ProtocolBalance,
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

/// Frozen authoritative 24-field settlement record.
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
    pub gross_price: ProtocolBalance,
    pub seller_amount: ProtocolBalance,
    pub royalty_recipient: Option<ProtocolAccountId>,
    pub royalty_amount: ProtocolBalance,
    pub market_fee_recipient: ProtocolAccountId,
    pub market_fee_amount: ProtocolBalance,
    pub safety_fee_amount: ProtocolBalance,
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
    pub active_exposure_value: ProtocolBalance,
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
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QMetaSnapshot<AccountId> {
    pub asset_id: Hash32,
    pub meta: Option<MetaToken<AccountId>>,
    pub owner_index_matches: Option<bool>,
    pub active_right_count: u32,
    pub active_order_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QRightSnapshot<AccountId> {
    pub right_id: Hash32,
    pub right: Option<RightToken<AccountId>>,
    pub valid: Option<bool>,
    pub holder_index_matches: Option<bool>,
    pub parent_index_matches: Option<bool>,
    pub active_order_id: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QOrderSnapshot {
    pub order_id: Hash32,
    pub order: Option<RuntimeOrder>,
    pub active_capacity_occupied: bool,
    pub funded_capacity_occupied: bool,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QPaymentSnapshot {
    pub order_id: Hash32,
    pub state: PaymentState,
    pub escrow: Option<RuntimeEscrow>,
    pub escrow_amount: ProtocolBalance,
    pub buyer_reserved_balance: ProtocolBalance,
    pub reserved_balance_reconciles: bool,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QEvidenceSnapshot {
    pub order_id: Hash32,
    pub settlement_record_count: u8,
    pub settlement: Option<SettlementRecord>,
    pub record_fields_hash: Option<Hash32>,
    pub pre_state_commitment: Option<Hash32>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QIdentitySnapshot {
    pub settlement_protocol_version: u16,
    pub settlement_domain_id: [u8; 13],
    pub order_commitment_hash: Option<Hash32>,
    pub seller_nonce: Option<u64>,
    pub buyer_nonce: Option<u64>,
    pub authorization_nonce: Option<u64>,
    pub object_state_version: Option<u64>,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct QAllSnapshot<AccountId> {
    pub observed_at: u32,
    pub meta: QMetaSnapshot<AccountId>,
    pub right: Option<QRightSnapshot<AccountId>>,
    pub order: QOrderSnapshot,
    pub authorization: NormalizedAuthorizationSnapshot,
    pub payment: QPaymentSnapshot,
    pub evidence: QEvidenceSnapshot,
    pub market: NormalizedMarketSnapshot,
    pub identity: QIdentitySnapshot,
    pub state_commitment: Hash32,
}

pub fn normalized_state_commitment(snapshot: &NormalizedSettlementSnapshot) -> Hash32 {
    canonical_hash(snapshot)
}

fn canonical_hash<T: Encode>(value: &T) -> Hash32 {
    sp_io::hashing::blake2_256(&value.encode())
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
    canonical_hash(&(
        domain_separator(genesis_hash, order.operation, MessageType::Order),
        order,
    ))
}

pub fn order_commitment_hash(order: &OrderCommitment) -> Hash32 {
    canonical_hash(order)
}

pub fn settlement_id(genesis_hash: Hash32, operation: MarketOperation, order_id: Hash32) -> Hash32 {
    canonical_hash(&(
        domain_separator(genesis_hash, operation, MessageType::Settlement),
        order_id,
        *b"settlement",
    ))
}

pub fn right_id(genesis_hash: Hash32, operation: MarketOperation, order_id: Hash32) -> Hash32 {
    canonical_hash(&(
        domain_separator(genesis_hash, operation, MessageType::Right),
        order_id,
        *b"right",
    ))
}

pub fn escrow_id(genesis_hash: Hash32, operation: MarketOperation, order_id: Hash32) -> Hash32 {
    canonical_hash(&(
        domain_separator(genesis_hash, operation, MessageType::Settlement),
        order_id,
        *b"escrow",
    ))
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

pub fn verify_signature(
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

fn mul_bps_floor(value: ProtocolBalance, bps: u16) -> Option<ProtocolBalance> {
    let bps = ProtocolBalance::from(bps);
    (value / 10_000)
        .checked_mul(bps)?
        .checked_add((value % 10_000).checked_mul(bps)? / 10_000)
}

pub fn amount_split(
    gross_price: ProtocolBalance,
    royalty_bps: u16,
    market_fee_bps: u16,
    safety_fee_bps: u16,
) -> Option<AmountSplit> {
    let total_bps = u32::from(royalty_bps)
        .checked_add(u32::from(market_fee_bps))?
        .checked_add(u32::from(safety_fee_bps))?;
    if total_bps > 10_000 {
        return None;
    }
    let royalty = mul_bps_floor(gross_price, royalty_bps)?;
    let market_fee = mul_bps_floor(gross_price, market_fee_bps)?;
    let safety_fee = mul_bps_floor(gross_price, safety_fee_bps)?;
    let deductions = royalty.checked_add(market_fee)?.checked_add(safety_fee)?;
    Some(AmountSplit {
        gross_price,
        seller_net: gross_price.checked_sub(deductions)?,
        royalty,
        market_fee,
        safety_fee,
    })
}

pub fn price_policy_hash(policy: &PricePolicy) -> Hash32 {
    canonical_hash(policy)
}

pub fn right_parameters_hash(
    right_type: RightType,
    data_scope: DataScope,
    validity: RightValidity,
    transferable: bool,
    allowed_transfer_markets: &AllowedMarkets,
    royalty_bps: u16,
) -> Hash32 {
    canonical_hash(&(
        right_type,
        data_scope,
        validity,
        transferable,
        allowed_transfer_markets,
        royalty_bps,
    ))
}
