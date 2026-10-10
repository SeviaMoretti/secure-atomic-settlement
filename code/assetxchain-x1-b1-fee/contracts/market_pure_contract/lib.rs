#![cfg_attr(not(feature = "std"), no_std, no_main)]
#![allow(clippy::cast_possible_truncation)]

/// B1 Contract-only authority for the paper-minimum shared protocol.
///
/// This S3.4 boundary also owns orders, per-order native payment escrow,
/// settlement state transitions, refunds, and authoritative evidence. It does
/// not call an AssetXChain Runtime mutation or a G3 gateway.
#[ink::contract]
mod market_pure_contract {
    use ink::prelude::vec::Vec;
    use ink::storage::Mapping;
    use scale::Encode;

    #[cfg(test)]
    std::thread_local! {
        static TEST_ABORT_OPERATION: core::cell::Cell<Option<MarketOperation>> =
            const { core::cell::Cell::new(None) };
    }

    pub type Hash32 = [u8; 32];
    pub type ProtocolAccountId = [u8; 32];
    pub type ProtocolSignature = [u8; 64];

    pub const SETTLEMENT_PROTOCOL_VERSION: u16 = 1;
    pub const SETTLEMENT_DOMAIN_ID: [u8; 13] = *b"axc/p2/shared";
    pub const FUNDED_GRACE_BLOCKS: BlockNumber = 10;
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
    pub const MAX_ALLOWED_TRANSFER_MARKETS: usize = 16;
    pub const MAX_RIGHTS_PER_ASSET: usize = 1024;

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum MarketOperation {
        #[codec(index = 0)]
        TransferMeta,
        #[codec(index = 1)]
        IssueRight,
        #[codec(index = 2)]
        TransferRight,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum ObjectType {
        #[codec(index = 0)]
        MetaToken,
        #[codec(index = 1)]
        RightToken,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum CurrencyId {
        #[codec(index = 0)]
        Native,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum DataScope {
        #[codec(index = 0)]
        FullAsset,
        #[codec(index = 1)]
        FieldSetHash(Hash32),
        #[codec(index = 2)]
        PartitionHash(Hash32),
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct RightValidity {
        pub valid_from: u64,
        pub valid_until: Option<u64>,
        pub max_uses: Option<u32>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct PricePolicy {
        pub reference_price: Balance,
        pub floor_ratio_bps: u16,
        pub signed_market_fee_cap_bps: u16,
        pub safety_fee_bps: u16,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct R2X1PolicyDecision {
        pub base_price: Balance,
        pub gross_price: Balance,
        pub market_fee_bps: u16,
        pub market_fee_amount: Balance,
        pub selected_offer_index: u32,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum MarketStatus {
        Active,
        Restricted,
        Exiting,
        Deregistered,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum AuthorizationStatus {
        Active,
        Revoked,
        Expired,
        Consumed,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum IssueQuotaReservationStatus {
        Reserved,
        Issued,
        Released,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct MetaToken {
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

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct MetaRegistration {
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

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct RightToken {
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
        pub allowed_transfer_markets: Vec<Hash32>,
        pub royalty_bps: u16,
        pub state_version: u64,
        pub status: RightStatus,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct ExistingRightRegistration {
        pub right_id: Hash32,
        pub asset_id: Hash32,
        pub holder: AccountId,
        pub issuer: AccountId,
        pub right_type: RightType,
        pub data_scope: DataScope,
        pub validity: RightValidity,
        pub transferable: bool,
        pub allowed_transfer_markets: Vec<Hash32>,
        pub royalty_bps: u16,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct MarketCapacityTier {
        pub tier_id: u8,
        pub max_active_orders: u32,
        pub max_funded_orders: u32,
        pub max_active_exposure_value: Balance,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct MarketRegistration {
        pub market_id: Hash32,
        pub operator: AccountId,
        pub contract_address: AccountId,
        pub code_hash: Hash32,
        pub protocol_version: u16,
        pub capabilities: u8,
        pub market_fee_bps: u16,
        pub capacity_tier: MarketCapacityTier,
        pub stake: Balance,
        pub active_orders: u32,
        pub funded_orders: u32,
        pub active_exposure_value: Balance,
        pub active_authorizations: u32,
        pub status: MarketStatus,
        pub registered_at: BlockNumber,
        pub exit_requested_at: Option<BlockNumber>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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
        pub allowed_transfer_markets: Vec<Hash32>,
        pub expiry: BlockNumber,
        pub nonce: u64,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct TransferMetaAuthorizationTerms {
        pub price_policy: PricePolicy,
        pub allowed_buyer: Option<AccountId>,
        pub expiry: BlockNumber,
        pub nonce: u64,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct IssueRightAuthorization {
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
        pub allowed_transfer_markets: Vec<Hash32>,
        pub reserved_count: u32,
        pub reserved_use_units: u64,
        pub issued_count: u32,
        pub issued_use_units: u64,
        pub expiry: BlockNumber,
        pub nonce: u64,
        pub status: AuthorizationStatus,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct TransferMetaAuthorization {
        pub auth_id: Hash32,
        pub asset_id: Hash32,
        pub owner: AccountId,
        pub market_id: Hash32,
        pub operation: MarketOperation,
        pub price_policy: PricePolicy,
        pub allowed_buyer: Option<AccountId>,
        pub expiry: BlockNumber,
        pub nonce: u64,
        pub status: AuthorizationStatus,
        pub consumed_order_id: Option<Hash32>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct IssueQuotaReservation {
        pub order_id: Hash32,
        pub auth_id: Hash32,
        pub reserved_count: u32,
        pub reserved_use_units: u64,
        pub status: IssueQuotaReservationStatus,
    }

    /// Frozen shared-protocol-v1 SCALE field order.
    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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
        pub price: Balance,
        pub currency: CurrencyId,
        pub policy_hash: Hash32,
        pub right_parameters_hash: Option<Hash32>,
        pub seller_state_version: u64,
        pub order_nonce: u64,
        pub expiry: BlockNumber,
    }

    /// Frozen shared-protocol-v1 SCALE field order.
    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct SellerCommitment {
        pub order_id: Hash32,
        pub object_id: Hash32,
        pub operation: MarketOperation,
        pub seller: ProtocolAccountId,
        pub state_version: u64,
        pub price_policy_hash: Hash32,
        pub right_parameters_hash: Option<Hash32>,
        pub expires_at: BlockNumber,
        pub seller_nonce: u64,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum PricePolicyWitness {
        #[codec(index = 0)]
        Authorization,
        #[codec(index = 1)]
        SellerSnapshot(PricePolicy),
    }

    /// Frozen shared-protocol-v1 SCALE field order.
    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct BuyerCommitment {
        pub order_id: Hash32,
        pub buyer: ProtocolAccountId,
        pub price: Balance,
        pub currency: CurrencyId,
        pub expires_at: BlockNumber,
        pub buyer_nonce: u64,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct ContractOrder {
        pub order_id: Hash32,
        pub order_commitment: OrderCommitment,
        pub authorization_id: Hash32,
        pub seller_commitment: SellerCommitment,
        pub buyer_commitment: Option<BuyerCommitment>,
        pub payment_asset: CurrencyId,
        pub escrow_id: Option<Hash32>,
        pub created_at: BlockNumber,
        pub funded_at: Option<BlockNumber>,
        pub expiry: BlockNumber,
        pub grace_deadline: Option<BlockNumber>,
        pub state: OrderState,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct AmountSplit {
        pub gross_price: Balance,
        pub seller_net: Balance,
        pub royalty: Balance,
        pub market_fee: Balance,
        pub safety_fee: Balance,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct ContractEscrow {
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

    /// Authoritative settlement record in the frozen 24-field SCALE order.
    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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
        pub gross_price: Balance,
        pub seller_amount: Balance,
        pub royalty_recipient: Option<ProtocolAccountId>,
        pub royalty_amount: Balance,
        pub market_fee_recipient: ProtocolAccountId,
        pub market_fee_amount: Balance,
        pub safety_fee_amount: Balance,
        pub pre_state_version: u64,
        pub post_state_version: u64,
        pub pre_state_commitment: Hash32,
        pub block_number: BlockNumber,
        pub settlement_protocol_version: u16,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum NormalizedAuthorizationKind {
        #[codec(index = 0)]
        TransferMeta,
        #[codec(index = 1)]
        IssueRight,
        #[codec(index = 2)]
        TransferRightPolicy,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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
        pub expiry: BlockNumber,
        pub status: u8,
        pub reserved_count: u32,
        pub reserved_use_units: u64,
        pub issued_count: u32,
        pub issued_use_units: u64,
        pub consumed_order_id: Option<Hash32>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct NormalizedMarketSnapshot {
        pub market_id: Hash32,
        pub operator: ProtocolAccountId,
        pub protocol_version: u16,
        pub capabilities: u8,
        pub capacity_tier: u8,
        pub active_orders: u32,
        pub funded_orders: u32,
        pub active_exposure_value: Balance,
        pub active_authorizations: u32,
        pub status: u8,
        pub market_fee_bps: u16,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct NormalizedSettlementSnapshot {
        pub object: NormalizedObjectSnapshot,
        pub order: ContractOrder,
        pub authorization: NormalizedAuthorizationSnapshot,
        pub payment: ContractEscrow,
        pub settlement: Option<SettlementRecord>,
        pub market: NormalizedMarketSnapshot,
    }

    #[repr(u32)]
    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
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

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QMetaSnapshot {
        pub asset_id: Hash32,
        pub meta: Option<MetaToken>,
        pub owner_index_matches: Option<bool>,
        pub active_right_count: u32,
        pub active_order_id: Option<Hash32>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QRightSnapshot {
        pub right_id: Hash32,
        pub right: Option<RightToken>,
        pub valid: Option<bool>,
        pub holder_index_matches: Option<bool>,
        pub parent_index_matches: Option<bool>,
        pub active_order_id: Option<Hash32>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QOrderSnapshot {
        pub order_id: Hash32,
        pub order: Option<ContractOrder>,
        pub active_capacity_occupied: bool,
        pub funded_capacity_occupied: bool,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QPaymentSnapshot {
        pub order_id: Hash32,
        pub state: PaymentState,
        pub escrow: Option<ContractEscrow>,
        pub escrow_amount: Balance,
        pub total_escrowed: Balance,
        pub contract_balance_reconciles: bool,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QEvidenceSnapshot {
        pub order_id: Hash32,
        pub settlement_record_count: u8,
        pub settlement: Option<SettlementRecord>,
        pub record_fields_hash: Option<Hash32>,
        pub pre_state_commitment: Option<Hash32>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QIdentitySnapshot {
        pub settlement_protocol_version: u16,
        pub settlement_domain_id: [u8; 13],
        pub order_commitment_hash: Option<Hash32>,
        pub seller_nonce: Option<u64>,
        pub buyer_nonce: Option<u64>,
        pub authorization_nonce: Option<u64>,
        pub object_state_version: Option<u64>,
    }

    #[derive(scale::Encode, scale::Decode, Clone, PartialEq, Eq, Debug)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub struct QAllSnapshot {
        pub observed_at: BlockNumber,
        pub meta: QMetaSnapshot,
        pub right: Option<QRightSnapshot>,
        pub order: QOrderSnapshot,
        pub authorization: NormalizedAuthorizationSnapshot,
        pub payment: QPaymentSnapshot,
        pub evidence: QEvidenceSnapshot,
        pub market: NormalizedMarketSnapshot,
        pub identity: QIdentitySnapshot,
        pub state_commitment: Hash32,
    }

    #[derive(scale::Encode, scale::Decode, Clone, Copy, PartialEq, Eq, Debug)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub enum Error {
        NotAdmin,
        NotOperator,
        AssetAlreadyExists,
        AssetNotFound,
        AssetNotTradable,
        RightAlreadyExists,
        RightNotFound,
        RightInvalid,
        RightIndexInconsistent,
        OwnershipMismatch,
        ParentMismatch,
        StateVersionOverflow,
        MarketAlreadyRegistered,
        MarketNotRegistered,
        MarketRestricted,
        CapabilityDenied,
        InvalidCapabilities,
        InvalidCapacityTier,
        InvalidMarketFee,
        InvalidMarketTransition,
        MarketHasActiveObligations,
        UnsupportedProtocolVersion,
        AuthorizationMissing,
        AuthorizationRevoked,
        AuthorizationExpired,
        AuthorizationConsumed,
        AuthorizationAlreadyActive,
        AuthorizationNotActive,
        AuthorizationNonceMismatch,
        AuthorizationNonceOverflow,
        AuthorizationCounterInvariant,
        InvalidAuthorizationExpiry,
        InvalidPricePolicy,
        BelowPriceFloor,
        FeeOutOfBounds,
        InvalidRightParameters,
        InvalidIssueQuota,
        MutualExclusionConflict,
        QuotaExceeded,
        QuotaInvariant,
        QuotaReservationAlreadyExists,
        QuotaReservationMissing,
        QuotaReservationAuthorizationMismatch,
        QuotaReservationStateInvalid,
        MalformedCommitment,
        InvalidSignature,
        NonceUsed,
        StateVersionMismatch,
        CapacityExceeded,
        OrderNotFound,
        OrderNotOpen,
        OrderNotFunded,
        OrderExpired,
        AlreadySettled,
        TerminalOrder,
        InsufficientBalance,
        EscrowMismatch,
        PayerBuyerMismatch,
        RefundNotAllowed,
        GracePeriodActive,
        RefundBeneficiaryMismatch,
        ActiveOrderConflict,
        ActiveOrderInvariant,
        ArithmeticOverflow,
        PaymentTransferFailed,
    }

    impl Error {
        pub const fn common_error_code(self) -> CommonErrorCode {
            match self {
                Self::UnsupportedProtocolVersion => CommonErrorCode::UnsupportedProtocolVersion,
                Self::MalformedCommitment
                | Self::InvalidCapabilities
                | Self::InvalidCapacityTier
                | Self::InvalidMarketFee
                | Self::InvalidMarketTransition
                | Self::StateVersionOverflow
                | Self::AuthorizationCounterInvariant
                | Self::AuthorizationNonceOverflow
                | Self::InvalidAuthorizationExpiry
                | Self::InvalidPricePolicy
                | Self::InvalidRightParameters
                | Self::InvalidIssueQuota
                | Self::QuotaInvariant
                | Self::QuotaReservationAlreadyExists
                | Self::QuotaReservationAuthorizationMismatch
                | Self::QuotaReservationStateInvalid
                | Self::ActiveOrderInvariant
                | Self::ArithmeticOverflow => CommonErrorCode::MalformedCommitment,
                Self::MarketNotRegistered | Self::MarketAlreadyRegistered => {
                    CommonErrorCode::MarketNotRegistered
                }
                Self::MarketRestricted | Self::MarketHasActiveObligations => {
                    CommonErrorCode::MarketRestricted
                }
                Self::CapabilityDenied => CommonErrorCode::CapabilityDenied,
                Self::CapacityExceeded => CommonErrorCode::CapacityExceeded,
                Self::AuthorizationMissing
                | Self::AuthorizationConsumed
                | Self::AuthorizationAlreadyActive
                | Self::AuthorizationNotActive => CommonErrorCode::AuthorizationMissing,
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
                Self::AlreadySettled => CommonErrorCode::AlreadySettled,
                Self::TerminalOrder => CommonErrorCode::TerminalOrder,
                Self::AssetNotTradable | Self::AssetNotFound | Self::AssetAlreadyExists => {
                    CommonErrorCode::AssetNotTradable
                }
                Self::RightInvalid
                | Self::RightNotFound
                | Self::RightAlreadyExists
                | Self::RightIndexInconsistent => CommonErrorCode::RightInvalid,
                Self::NotAdmin | Self::NotOperator | Self::OwnershipMismatch => {
                    CommonErrorCode::OwnershipMismatch
                }
                Self::ParentMismatch => CommonErrorCode::ParentMismatch,
                Self::MutualExclusionConflict | Self::ActiveOrderConflict => {
                    CommonErrorCode::MutualExclusionConflict
                }
                Self::BelowPriceFloor => CommonErrorCode::BelowPriceFloor,
                Self::FeeOutOfBounds => CommonErrorCode::FeeOutOfBounds,
                Self::InsufficientBalance => CommonErrorCode::InsufficientBalance,
                Self::EscrowMismatch | Self::PaymentTransferFailed => {
                    CommonErrorCode::EscrowMismatch
                }
                Self::PayerBuyerMismatch => CommonErrorCode::PayerBuyerMismatch,
                Self::RefundNotAllowed => CommonErrorCode::RefundNotAllowed,
                Self::GracePeriodActive => CommonErrorCode::GracePeriodActive,
                Self::RefundBeneficiaryMismatch => CommonErrorCode::RefundBeneficiaryMismatch,
            }
        }
    }

    #[ink(event)]
    pub struct MetaRegistered {
        #[ink(topic)]
        pub asset_id: Hash32,
        #[ink(topic)]
        pub owner: AccountId,
    }

    #[ink(event)]
    pub struct ExistingRightInitialized {
        #[ink(topic)]
        pub right_id: Hash32,
        #[ink(topic)]
        pub asset_id: Hash32,
        #[ink(topic)]
        pub holder: AccountId,
    }

    #[ink(event)]
    pub struct RightExpired {
        #[ink(topic)]
        pub right_id: Hash32,
        pub status: RightStatus,
    }

    #[ink(event)]
    pub struct MarketRegistered {
        #[ink(topic)]
        pub market_id: Hash32,
        #[ink(topic)]
        pub operator: AccountId,
    }

    #[ink(event)]
    pub struct MarketStatusChanged {
        #[ink(topic)]
        pub market_id: Hash32,
        pub status: MarketStatus,
    }

    #[ink(event)]
    pub struct IssueRightAuthorizationCreated {
        #[ink(topic)]
        pub auth_id: Hash32,
        #[ink(topic)]
        pub asset_id: Hash32,
        #[ink(topic)]
        pub market_id: Hash32,
    }

    #[ink(event)]
    pub struct TransferMetaAuthorizationCreated {
        #[ink(topic)]
        pub auth_id: Hash32,
        #[ink(topic)]
        pub asset_id: Hash32,
        #[ink(topic)]
        pub market_id: Hash32,
    }

    #[ink(event)]
    pub struct AuthorizationStatusChanged {
        #[ink(topic)]
        pub auth_id: Hash32,
        pub status: AuthorizationStatus,
    }

    #[ink(event)]
    pub struct IssueQuotaReserved {
        #[ink(topic)]
        pub auth_id: Hash32,
        #[ink(topic)]
        pub order_id: Hash32,
        pub use_units: u64,
    }

    #[ink(event)]
    pub struct IssueQuotaConverted {
        #[ink(topic)]
        pub auth_id: Hash32,
        #[ink(topic)]
        pub order_id: Hash32,
    }

    #[ink(event)]
    pub struct IssueQuotaReleased {
        #[ink(topic)]
        pub auth_id: Hash32,
        #[ink(topic)]
        pub order_id: Hash32,
    }

    #[ink(event)]
    pub struct OrderCreated {
        #[ink(topic)]
        pub order_id: Hash32,
        #[ink(topic)]
        pub market_id: Hash32,
        pub operation: MarketOperation,
        pub object_id: Hash32,
    }

    #[ink(event)]
    pub struct OrderFunded {
        #[ink(topic)]
        pub order_id: Hash32,
        #[ink(topic)]
        pub buyer: ProtocolAccountId,
        pub escrow_id: Hash32,
        pub gross_price: Balance,
        pub grace_deadline: BlockNumber,
    }

    #[ink(event)]
    pub struct PaymentEscrowed {
        #[ink(topic)]
        pub order_id: Hash32,
        #[ink(topic)]
        pub escrow_id: Hash32,
        pub amount: Balance,
    }

    #[ink(event)]
    pub struct OrderCancelled {
        #[ink(topic)]
        pub order_id: Hash32,
    }

    #[ink(event)]
    pub struct OrderExpired {
        #[ink(topic)]
        pub order_id: Hash32,
    }

    #[ink(event)]
    pub struct OrderRefunded {
        #[ink(topic)]
        pub order_id: Hash32,
        #[ink(topic)]
        pub buyer: ProtocolAccountId,
        pub amount: Balance,
    }

    #[ink(event)]
    pub struct PaymentRefunded {
        #[ink(topic)]
        pub order_id: Hash32,
        #[ink(topic)]
        pub escrow_id: Hash32,
        pub amount: Balance,
    }

    #[ink(event)]
    pub struct PaymentReleased {
        #[ink(topic)]
        pub order_id: Hash32,
        #[ink(topic)]
        pub escrow_id: Hash32,
        pub amount: Balance,
    }

    #[ink(event)]
    pub struct OrderSettled {
        #[ink(topic)]
        pub order_id: Hash32,
    }

    #[ink(event)]
    pub struct SettlementFinalized {
        #[ink(topic)]
        pub settlement_id: Hash32,
        #[ink(topic)]
        pub order_id: Hash32,
        pub post_state_commitment: Hash32,
    }

    #[ink(event)]
    pub struct PolicyEvaluated {
        pub base_price: Balance,
        pub gross_price: Balance,
        pub market_fee_bps: u16,
        pub selected_offer_index: u32,
    }

    #[ink(storage)]
    pub struct MarketPureContract {
        admin: AccountId,
        genesis_hash: Hash32,
        meta_tokens: Mapping<Hash32, MetaToken>,
        meta_owner_index: Mapping<(AccountId, Hash32), ()>,
        right_tokens: Mapping<Hash32, RightToken>,
        right_holder_index: Mapping<(AccountId, Hash32), ()>,
        right_parent_index: Mapping<(Hash32, Hash32), ()>,
        asset_right_ids: Mapping<Hash32, Vec<Hash32>>,
        active_right_count: Mapping<Hash32, u32>,
        markets: Mapping<Hash32, MarketRegistration>,
        next_market_nonce: u64,
        issue_authorizations: Mapping<Hash32, IssueRightAuthorization>,
        transfer_authorizations: Mapping<Hash32, TransferMetaAuthorization>,
        active_issue_authorization: Mapping<(Hash32, Hash32), Hash32>,
        active_issue_authorization_count: Mapping<Hash32, u32>,
        active_transfer_authorization: Mapping<(Hash32, Hash32), Hash32>,
        active_transfer_authorization_by_asset: Mapping<Hash32, Hash32>,
        authorization_nonce: Mapping<(AccountId, Hash32, MarketOperation), u64>,
        issue_quota_reservations: Mapping<Hash32, IssueQuotaReservation>,
        outstanding_issue_quota_count: Mapping<Hash32, u32>,
        orders: Mapping<Hash32, ContractOrder>,
        order_price_policies: Mapping<Hash32, PricePolicy>,
        escrows: Mapping<Hash32, ContractEscrow>,
        settlement_records: Mapping<Hash32, SettlementRecord>,
        settlement_id_index: Mapping<Hash32, Hash32>,
        active_meta_order: Mapping<Hash32, Hash32>,
        active_right_order: Mapping<Hash32, Hash32>,
        seller_order_nonce: Mapping<(ProtocolAccountId, Hash32), u64>,
        buyer_order_nonce: Mapping<ProtocolAccountId, u64>,
        total_escrowed: Balance,
        last_policy_decision: Option<R2X1PolicyDecision>,
    }

    impl MarketPureContract {
        #[ink(constructor)]
        pub fn new() -> Self {
            Self::new_with_genesis_hash([0u8; 32])
        }

        #[ink(constructor)]
        pub fn new_with_genesis_hash(genesis_hash: Hash32) -> Self {
            Self {
                admin: Self::env().caller(),
                genesis_hash,
                meta_tokens: Mapping::default(),
                meta_owner_index: Mapping::default(),
                right_tokens: Mapping::default(),
                right_holder_index: Mapping::default(),
                right_parent_index: Mapping::default(),
                asset_right_ids: Mapping::default(),
                active_right_count: Mapping::default(),
                markets: Mapping::default(),
                next_market_nonce: 0,
                issue_authorizations: Mapping::default(),
                transfer_authorizations: Mapping::default(),
                active_issue_authorization: Mapping::default(),
                active_issue_authorization_count: Mapping::default(),
                active_transfer_authorization: Mapping::default(),
                active_transfer_authorization_by_asset: Mapping::default(),
                authorization_nonce: Mapping::default(),
                issue_quota_reservations: Mapping::default(),
                outstanding_issue_quota_count: Mapping::default(),
                orders: Mapping::default(),
                order_price_policies: Mapping::default(),
                escrows: Mapping::default(),
                settlement_records: Mapping::default(),
                settlement_id_index: Mapping::default(),
                active_meta_order: Mapping::default(),
                active_right_order: Mapping::default(),
                seller_order_nonce: Mapping::default(),
                buyer_order_nonce: Mapping::default(),
                total_escrowed: 0,
                last_policy_decision: None,
            }
        }

        #[ink(message)]
        pub fn register_meta(&mut self, input: MetaRegistration) -> Result<(), Error> {
            let caller = self.env().caller();
            if caller != self.admin && caller != input.owner {
                return Err(Error::OwnershipMismatch);
            }
            if self.meta_tokens.contains(&input.asset_id) {
                return Err(Error::AssetAlreadyExists);
            }
            if !matches!(
                input.status,
                AssetStatus::PendingValidation | AssetStatus::Tradable
            ) {
                return Err(Error::AssetNotTradable);
            }

            let asset_id = input.asset_id;
            let owner = input.owner;
            let meta = MetaToken {
                asset_id,
                owner,
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
            };
            self.meta_tokens.insert(&asset_id, &meta);
            self.meta_owner_index.insert(&(owner, asset_id), &());
            self.env().emit_event(MetaRegistered { asset_id, owner });
            Ok(())
        }

        #[ink(message)]
        pub fn initialize_existing_right(
            &mut self,
            input: ExistingRightRegistration,
        ) -> Result<(), Error> {
            self.ensure_admin()?;
            if self.right_tokens.contains(&input.right_id) {
                return Err(Error::RightAlreadyExists);
            }
            let meta = self
                .meta_tokens
                .get(&input.asset_id)
                .ok_or(Error::AssetNotFound)?;
            if meta.owner != input.issuer {
                return Err(Error::OwnershipMismatch);
            }
            if meta.status != AssetStatus::Tradable {
                return Err(Error::AssetNotTradable);
            }
            if self
                .active_transfer_authorization_by_asset
                .contains(&input.asset_id)
            {
                return Err(Error::MutualExclusionConflict);
            }
            self.validate_right_terms(
                &input.validity,
                input.transferable,
                &input.allowed_transfer_markets,
                input.royalty_bps,
                true,
            )?;

            let mut right_ids = self
                .asset_right_ids
                .get(&input.asset_id)
                .unwrap_or_default();
            if right_ids.len() >= MAX_RIGHTS_PER_ASSET {
                return Err(Error::QuotaExceeded);
            }
            let next_active_count = self
                .active_right_count
                .get(&input.asset_id)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::QuotaInvariant)?;

            let right_id = input.right_id;
            let asset_id = input.asset_id;
            let holder = input.holder;
            let right = RightToken {
                right_id,
                asset_id,
                holder,
                issuer: input.issuer,
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
            };
            right_ids.push(right_id);
            self.right_tokens.insert(&right_id, &right);
            self.right_holder_index.insert(&(holder, right_id), &());
            self.right_parent_index.insert(&(asset_id, right_id), &());
            self.asset_right_ids.insert(&asset_id, &right_ids);
            self.active_right_count
                .insert(&asset_id, &next_active_count);
            self.env().emit_event(ExistingRightInitialized {
                right_id,
                asset_id,
                holder,
            });
            Ok(())
        }

        /// Rooted fixture-only constructor for invalid E2 pre-states. It is
        /// intentionally limited to creating a previously absent right and
        /// cannot mutate a right after an experimental attempt begins.
        #[ink(message)]
        pub fn initialize_right_for_experiment(
            &mut self,
            input: ExistingRightRegistration,
            status: RightStatus,
            remaining_uses: Option<u32>,
        ) -> Result<(), Error> {
            self.ensure_admin()?;
            if self.right_tokens.contains(&input.right_id) {
                return Err(Error::RightAlreadyExists);
            }
            let meta = self
                .meta_tokens
                .get(&input.asset_id)
                .ok_or(Error::AssetNotFound)?;
            if meta.owner != input.issuer {
                return Err(Error::OwnershipMismatch);
            }
            if meta.status != AssetStatus::Tradable {
                return Err(Error::AssetNotTradable);
            }
            self.validate_right_terms(
                &input.validity,
                input.transferable,
                &input.allowed_transfer_markets,
                input.royalty_bps,
                true,
            )?;
            match (input.validity.max_uses, remaining_uses) {
                (Some(max), Some(remaining)) if remaining <= max => {}
                (None, None) => {}
                _ => return Err(Error::InvalidRightParameters),
            }
            if status == RightStatus::Exhausted && remaining_uses != Some(0) {
                return Err(Error::InvalidRightParameters);
            }

            let mut right_ids = self
                .asset_right_ids
                .get(&input.asset_id)
                .unwrap_or_default();
            if right_ids.len() >= MAX_RIGHTS_PER_ASSET {
                return Err(Error::QuotaExceeded);
            }
            let next_active_count = if status == RightStatus::Active {
                Some(
                    self.active_right_count
                        .get(&input.asset_id)
                        .unwrap_or(0)
                        .checked_add(1)
                        .ok_or(Error::QuotaInvariant)?,
                )
            } else {
                None
            };
            let right_id = input.right_id;
            let asset_id = input.asset_id;
            let holder = input.holder;
            let right = RightToken {
                right_id,
                asset_id,
                holder,
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
            };
            right_ids.push(right_id);
            self.right_tokens.insert(&right_id, &right);
            self.right_holder_index.insert(&(holder, right_id), &());
            self.right_parent_index.insert(&(asset_id, right_id), &());
            self.asset_right_ids.insert(&asset_id, &right_ids);
            if let Some(count) = next_active_count {
                self.active_right_count.insert(&asset_id, &count);
            }
            self.env().emit_event(ExistingRightInitialized {
                right_id,
                asset_id,
                holder,
            });
            Ok(())
        }

        #[ink(message)]
        pub fn expire_right(&mut self, right_id: Hash32) -> Result<(), Error> {
            let mut right = self
                .right_tokens
                .get(&right_id)
                .ok_or(Error::RightNotFound)?;
            if right.status != RightStatus::Active {
                return Err(Error::RightInvalid);
            }
            let now = u64::from(self.env().block_number());
            let next_status = if right.remaining_uses == Some(0) {
                RightStatus::Exhausted
            } else if right
                .validity
                .valid_until
                .map_or(false, |until| now > until)
            {
                RightStatus::Expired
            } else {
                return Err(Error::RightInvalid);
            };
            let next_count = self
                .active_right_count
                .get(&right.asset_id)
                .unwrap_or(0)
                .checked_sub(1)
                .ok_or(Error::QuotaInvariant)?;
            right.state_version = right
                .state_version
                .checked_add(1)
                .ok_or(Error::StateVersionOverflow)?;
            right.status = next_status;
            self.right_tokens.insert(&right_id, &right);
            self.active_right_count.insert(&right.asset_id, &next_count);
            self.env().emit_event(RightExpired {
                right_id,
                status: next_status,
            });
            Ok(())
        }

        #[ink(message)]
        pub fn register_market(
            &mut self,
            operator: AccountId,
            contract_address: AccountId,
            code_hash: Hash32,
            protocol_version: u16,
            capabilities: u8,
            market_fee_bps: u16,
            capacity_tier: MarketCapacityTier,
            stake: Balance,
        ) -> Result<Hash32, Error> {
            self.ensure_admin()?;
            if protocol_version != SETTLEMENT_PROTOCOL_VERSION {
                return Err(Error::UnsupportedProtocolVersion);
            }
            self.validate_capabilities(capabilities)?;
            self.validate_capacity_tier(&capacity_tier)?;
            if market_fee_bps > 10_000 {
                return Err(Error::InvalidMarketFee);
            }
            let nonce = self.next_market_nonce;
            let next_nonce = nonce
                .checked_add(1)
                .ok_or(Error::AuthorizationNonceOverflow)?;
            let market_id = self.hash_encoded(&(
                b"axc/p2/contract-market",
                &operator,
                &contract_address,
                code_hash,
                nonce,
            ));
            if self.markets.contains(&market_id) {
                return Err(Error::MarketAlreadyRegistered);
            }
            let registration = MarketRegistration {
                market_id,
                operator,
                contract_address,
                code_hash,
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
                registered_at: self.env().block_number(),
                exit_requested_at: None,
            };
            self.markets.insert(&market_id, &registration);
            self.next_market_nonce = next_nonce;
            self.env().emit_event(MarketRegistered {
                market_id,
                operator,
            });
            Ok(market_id)
        }

        /// Admin-only fixture transition used to construct one-delta E2
        /// market pre-states on a fresh disposable chain.
        #[ink(message)]
        pub fn configure_market_for_experiment(
            &mut self,
            market_id: Hash32,
            registered: bool,
            capabilities: Option<u8>,
            max_funded_orders: Option<u32>,
        ) -> Result<(), Error> {
            self.ensure_admin()?;
            if !registered {
                if !self.markets.contains(&market_id) {
                    return Err(Error::MarketNotRegistered);
                }
                self.markets.remove(&market_id);
                return Ok(());
            }
            let mut market = self
                .markets
                .get(&market_id)
                .ok_or(Error::MarketNotRegistered)?;
            if let Some(value) = capabilities {
                market.capabilities = value;
            }
            if let Some(value) = max_funded_orders {
                market.capacity_tier.max_funded_orders = value;
            }
            self.markets.insert(&market_id, &market);
            Ok(())
        }

        /// Admin-only one-delta fixture mutation for E3 late-failure cases.
        #[ink(message)]
        pub fn configure_meta_status_for_experiment(
            &mut self,
            asset_id: Hash32,
            status: AssetStatus,
        ) -> Result<(), Error> {
            self.ensure_admin()?;
            let mut meta = self
                .meta_tokens
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;
            meta.status = status;
            self.meta_tokens.insert(&asset_id, &meta);
            Ok(())
        }

        /// Admin-only one-delta fixture mutation for E3 late-failure cases.
        #[ink(message)]
        pub fn configure_right_status_for_experiment(
            &mut self,
            right_id: Hash32,
            status: RightStatus,
        ) -> Result<(), Error> {
            self.ensure_admin()?;
            let mut right = self
                .right_tokens
                .get(&right_id)
                .ok_or(Error::RightNotFound)?;
            right.status = status;
            self.right_tokens.insert(&right_id, &right);
            Ok(())
        }

        #[ink(message)]
        pub fn restrict_market(&mut self, market_id: Hash32) -> Result<(), Error> {
            self.ensure_admin()?;
            self.transition_market(market_id, MarketStatus::Restricted)
        }

        #[ink(message)]
        pub fn activate_market(&mut self, market_id: Hash32) -> Result<(), Error> {
            self.ensure_admin()?;
            self.transition_market(market_id, MarketStatus::Active)
        }

        #[ink(message)]
        pub fn request_market_exit(&mut self, market_id: Hash32) -> Result<(), Error> {
            let mut market = self
                .markets
                .get(&market_id)
                .ok_or(Error::MarketNotRegistered)?;
            let caller = self.env().caller();
            if caller != self.admin && caller != market.operator {
                return Err(Error::NotOperator);
            }
            if !matches!(
                market.status,
                MarketStatus::Active | MarketStatus::Restricted
            ) {
                return Err(Error::InvalidMarketTransition);
            }
            market.status = MarketStatus::Exiting;
            market.exit_requested_at = Some(self.env().block_number());
            self.markets.insert(&market_id, &market);
            self.env().emit_event(MarketStatusChanged {
                market_id,
                status: MarketStatus::Exiting,
            });
            Ok(())
        }

        #[ink(message)]
        pub fn deregister_market(&mut self, market_id: Hash32) -> Result<(), Error> {
            self.ensure_admin()?;
            let mut market = self
                .markets
                .get(&market_id)
                .ok_or(Error::MarketNotRegistered)?;
            if market.status != MarketStatus::Exiting {
                return Err(Error::InvalidMarketTransition);
            }
            if market.active_orders != 0
                || market.funded_orders != 0
                || market.active_exposure_value != 0
                || market.active_authorizations != 0
            {
                return Err(Error::MarketHasActiveObligations);
            }
            market.status = MarketStatus::Deregistered;
            self.markets.insert(&market_id, &market);
            self.env().emit_event(MarketStatusChanged {
                market_id,
                status: MarketStatus::Deregistered,
            });
            Ok(())
        }

        #[ink(message)]
        pub fn create_issue_right_authorization(
            &mut self,
            asset_id: Hash32,
            market_id: Hash32,
            terms: IssueRightAuthorizationTerms,
        ) -> Result<Hash32, Error> {
            self.ensure_market_operation(&market_id, MarketOperation::IssueRight)?;
            let owner = self.env().caller();
            self.ensure_authorization_owner(&owner, &asset_id)?;
            self.validate_issue_authorization_terms(&terms)?;
            if terms.expiry <= self.env().block_number() {
                return Err(Error::InvalidAuthorizationExpiry);
            }
            if self
                .active_issue_authorization
                .contains(&(asset_id, market_id))
            {
                return Err(Error::AuthorizationAlreadyActive);
            }
            if self
                .active_transfer_authorization_by_asset
                .contains(&asset_id)
            {
                return Err(Error::MutualExclusionConflict);
            }

            let current_nonce =
                self.current_authorization_nonce(&owner, &asset_id, MarketOperation::IssueRight);
            if terms.nonce != current_nonce {
                return Err(Error::AuthorizationNonceMismatch);
            }
            let next_nonce = current_nonce
                .checked_add(1)
                .ok_or(Error::AuthorizationNonceOverflow)?;
            let auth_id = self.authorization_id(
                &owner,
                &asset_id,
                &market_id,
                MarketOperation::IssueRight,
                terms.nonce,
            );
            if self.issue_authorizations.contains(&auth_id) {
                return Err(Error::AuthorizationAlreadyActive);
            }
            let next_active_count = self
                .active_issue_authorization_count
                .get(&asset_id)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::AuthorizationCounterInvariant)?;
            let mut market = self
                .markets
                .get(&market_id)
                .ok_or(Error::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_add(1)
                .ok_or(Error::AuthorizationCounterInvariant)?;

            let authorization = IssueRightAuthorization {
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
                nonce: terms.nonce,
                status: AuthorizationStatus::Active,
            };
            self.issue_authorizations.insert(&auth_id, &authorization);
            self.active_issue_authorization
                .insert(&(asset_id, market_id), &auth_id);
            self.active_issue_authorization_count
                .insert(&asset_id, &next_active_count);
            self.authorization_nonce
                .insert(&(owner, asset_id, MarketOperation::IssueRight), &next_nonce);
            self.markets.insert(&market_id, &market);
            self.env().emit_event(IssueRightAuthorizationCreated {
                auth_id,
                asset_id,
                market_id,
            });
            Ok(auth_id)
        }

        #[ink(message)]
        pub fn create_transfer_meta_authorization(
            &mut self,
            asset_id: Hash32,
            market_id: Hash32,
            terms: TransferMetaAuthorizationTerms,
        ) -> Result<Hash32, Error> {
            self.ensure_market_operation(&market_id, MarketOperation::TransferMeta)?;
            let owner = self.env().caller();
            self.ensure_authorization_owner(&owner, &asset_id)?;
            self.validate_price_policy_terms(&terms.price_policy)?;
            if terms.expiry <= self.env().block_number() {
                return Err(Error::InvalidAuthorizationExpiry);
            }
            if self
                .active_transfer_authorization_by_asset
                .contains(&asset_id)
            {
                return Err(Error::AuthorizationAlreadyActive);
            }
            if self
                .active_issue_authorization_count
                .get(&asset_id)
                .unwrap_or(0)
                != 0
                || self
                    .outstanding_issue_quota_count
                    .get(&asset_id)
                    .unwrap_or(0)
                    != 0
                || self.has_valid_right(&asset_id)?
            {
                return Err(Error::MutualExclusionConflict);
            }

            let current_nonce =
                self.current_authorization_nonce(&owner, &asset_id, MarketOperation::TransferMeta);
            if terms.nonce != current_nonce {
                return Err(Error::AuthorizationNonceMismatch);
            }
            let next_nonce = current_nonce
                .checked_add(1)
                .ok_or(Error::AuthorizationNonceOverflow)?;
            let auth_id = self.authorization_id(
                &owner,
                &asset_id,
                &market_id,
                MarketOperation::TransferMeta,
                terms.nonce,
            );
            if self.transfer_authorizations.contains(&auth_id) {
                return Err(Error::AuthorizationAlreadyActive);
            }
            let mut market = self
                .markets
                .get(&market_id)
                .ok_or(Error::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_add(1)
                .ok_or(Error::AuthorizationCounterInvariant)?;
            let authorization = TransferMetaAuthorization {
                auth_id,
                asset_id,
                owner,
                market_id,
                operation: MarketOperation::TransferMeta,
                price_policy: terms.price_policy,
                allowed_buyer: terms.allowed_buyer,
                expiry: terms.expiry,
                nonce: terms.nonce,
                status: AuthorizationStatus::Active,
                consumed_order_id: None,
            };
            self.transfer_authorizations
                .insert(&auth_id, &authorization);
            self.active_transfer_authorization
                .insert(&(asset_id, market_id), &auth_id);
            self.active_transfer_authorization_by_asset
                .insert(&asset_id, &auth_id);
            self.authorization_nonce.insert(
                &(owner, asset_id, MarketOperation::TransferMeta),
                &next_nonce,
            );
            self.markets.insert(&market_id, &market);
            self.env().emit_event(TransferMetaAuthorizationCreated {
                auth_id,
                asset_id,
                market_id,
            });
            Ok(auth_id)
        }

        #[ink(message)]
        pub fn revoke_issue_right_authorization(&mut self, auth_id: Hash32) -> Result<(), Error> {
            let authorization = self
                .issue_authorizations
                .get(&auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            if authorization.owner != self.env().caller() {
                return Err(Error::OwnershipMismatch);
            }
            self.close_issue_authorization(authorization, AuthorizationStatus::Revoked)
        }

        #[ink(message)]
        pub fn revoke_transfer_meta_authorization(&mut self, auth_id: Hash32) -> Result<(), Error> {
            let authorization = self
                .transfer_authorizations
                .get(&auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            if authorization.owner != self.env().caller() {
                return Err(Error::OwnershipMismatch);
            }
            self.close_transfer_authorization(authorization, AuthorizationStatus::Revoked)
        }

        #[ink(message)]
        pub fn expire_issue_right_authorization(&mut self, auth_id: Hash32) -> Result<(), Error> {
            let authorization = self
                .issue_authorizations
                .get(&auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            if self.env().block_number() <= authorization.expiry {
                return Err(Error::AuthorizationNotActive);
            }
            self.close_issue_authorization(authorization, AuthorizationStatus::Expired)
        }

        #[ink(message)]
        pub fn expire_transfer_meta_authorization(&mut self, auth_id: Hash32) -> Result<(), Error> {
            let authorization = self
                .transfer_authorizations
                .get(&auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            if self.env().block_number() <= authorization.expiry {
                return Err(Error::AuthorizationNotActive);
            }
            self.close_transfer_authorization(authorization, AuthorizationStatus::Expired)
        }

        #[ink(message)]
        pub fn check_price_policy(
            &self,
            policy: PricePolicy,
            price: Balance,
            market_fee_bps: u16,
        ) -> Result<Balance, Error> {
            self.validate_price_policy_terms(&policy)?;
            if market_fee_bps > policy.signed_market_fee_cap_bps {
                return Err(Error::FeeOutOfBounds);
            }
            let floor = self.effective_floor(&policy)?;
            if price < floor {
                return Err(Error::BelowPriceFloor);
            }
            Ok(floor)
        }

        #[ink(message)]
        pub fn price_policy_hash(&self, policy: PricePolicy) -> Hash32 {
            self.hash_encoded(&policy)
        }

        #[ink(message)]
        pub fn protocol_genesis_hash(&self) -> Hash32 {
            self.genesis_hash
        }

        #[ink(message)]
        pub fn compute_order_id(&self, order: OrderCommitment) -> Hash32 {
            self.order_id(&order)
        }

        #[ink(message)]
        pub fn compute_order_commitment_hash(&self, order: OrderCommitment) -> Hash32 {
            self.hash_encoded(&order)
        }

        #[ink(message)]
        pub fn issue_right_parameters_hash(&self, auth_id: Hash32) -> Result<Hash32, Error> {
            let authorization = self
                .issue_authorizations
                .get(&auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            Ok(self.issue_parameters_hash(&authorization))
        }

        #[ink(message)]
        pub fn existing_right_parameters_hash(&self, right_id: Hash32) -> Result<Hash32, Error> {
            let right = self
                .right_tokens
                .get(&right_id)
                .ok_or(Error::RightNotFound)?;
            Ok(self.right_parameters_hash(&right))
        }

        #[ink(message)]
        pub fn create_order(
            &mut self,
            order: OrderCommitment,
            price_policy_witness: PricePolicyWitness,
            seller_commitment: SellerCommitment,
            seller_signature: ProtocolSignature,
        ) -> Result<Hash32, Error> {
            self.do_create_order(
                order,
                price_policy_witness,
                seller_commitment,
                seller_signature,
            )
        }

        #[ink(message, payable)]
        pub fn fund_order(
            &mut self,
            order_id: Hash32,
            buyer_commitment: BuyerCommitment,
            buyer_signature: ProtocolSignature,
        ) -> Result<Hash32, Error> {
            let result = self.do_fund_order(order_id, buyer_commitment, buyer_signature);
            Self::revert_on_error(result)
        }

        #[ink(message)]
        pub fn cancel_open(&mut self, order_id: Hash32) -> Result<(), Error> {
            self.do_cancel_open(order_id)
        }

        #[ink(message)]
        pub fn expire_open(&mut self, order_id: Hash32) -> Result<(), Error> {
            self.do_expire_open(order_id)
        }

        #[ink(message)]
        pub fn refund_funded(&mut self, order_id: Hash32) -> Result<(), Error> {
            let result = self.do_refund_funded(order_id);
            Self::revert_on_error(result)
        }

        #[ink(message)]
        pub fn settle(&mut self, order_id: Hash32) -> Result<Hash32, Error> {
            let result = self.do_settle(order_id);
            Self::revert_on_error(result)
        }

        #[ink(message)]
        pub fn get_order(&self, order_id: Hash32) -> Option<ContractOrder> {
            self.orders.get(&order_id)
        }

        #[ink(message)]
        pub fn get_escrow(&self, escrow_id: Hash32) -> Option<ContractEscrow> {
            self.escrows.get(&escrow_id)
        }

        #[ink(message)]
        pub fn get_settlement_record(&self, order_id: Hash32) -> Option<SettlementRecord> {
            self.settlement_records.get(&order_id)
        }

        #[ink(message)]
        pub fn map_common_error(&self, error: Error) -> CommonErrorCode {
            error.common_error_code()
        }

        #[ink(message)]
        pub fn q_meta(&self, asset_id: Hash32) -> QMetaSnapshot {
            let meta = self.meta_tokens.get(&asset_id);
            QMetaSnapshot {
                asset_id,
                owner_index_matches: meta
                    .as_ref()
                    .map(|value| self.meta_owner_index.contains(&(value.owner, asset_id))),
                active_right_count: self.active_right_count.get(&asset_id).unwrap_or(0),
                active_order_id: self.active_meta_order.get(&asset_id),
                meta,
            }
        }

        #[ink(message)]
        pub fn q_right(&self, right_id: Hash32) -> QRightSnapshot {
            let right = self.right_tokens.get(&right_id);
            let valid = right
                .as_ref()
                .map(|value| self.right_is_valid(value))
                .transpose()
                .ok()
                .flatten();
            QRightSnapshot {
                right_id,
                holder_index_matches: right
                    .as_ref()
                    .map(|value| self.right_holder_index.contains(&(value.holder, right_id))),
                parent_index_matches: right.as_ref().map(|value| {
                    self.right_parent_index
                        .contains(&(value.asset_id, right_id))
                }),
                active_order_id: self.active_right_order.get(&right_id),
                right,
                valid,
            }
        }

        #[ink(message)]
        pub fn q_order(&self, order_id: Hash32) -> QOrderSnapshot {
            let order = self.orders.get(&order_id);
            let active_capacity_occupied = order.as_ref().map_or(false, |value| {
                matches!(value.state, OrderState::Open | OrderState::Funded)
            });
            let funded_capacity_occupied = order
                .as_ref()
                .map_or(false, |value| value.state == OrderState::Funded);
            QOrderSnapshot {
                order_id,
                order,
                active_capacity_occupied,
                funded_capacity_occupied,
            }
        }

        #[ink(message)]
        pub fn q_authorization(
            &self,
            order_id: Hash32,
        ) -> Result<NormalizedAuthorizationSnapshot, Error> {
            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            self.normalized_authorization_snapshot(&order)
        }

        #[ink(message)]
        pub fn q_payment(&self, order_id: Hash32) -> Result<QPaymentSnapshot, Error> {
            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            let escrow = order
                .escrow_id
                .and_then(|escrow_id| self.escrows.get(&escrow_id));
            let state = escrow
                .as_ref()
                .map(|value| value.state)
                .unwrap_or(PaymentState::Unfunded);
            let escrow_amount = escrow
                .as_ref()
                .map(|value| value.amounts.gross_price)
                .unwrap_or(0);
            Ok(QPaymentSnapshot {
                order_id,
                state,
                escrow,
                escrow_amount,
                total_escrowed: self.total_escrowed,
                contract_balance_reconciles: self.env().balance() >= self.total_escrowed,
            })
        }

        #[ink(message)]
        pub fn q_evidence(&self, order_id: Hash32) -> QEvidenceSnapshot {
            let settlement = self.settlement_records.get(&order_id);
            QEvidenceSnapshot {
                order_id,
                settlement_record_count: u8::from(settlement.is_some()),
                record_fields_hash: settlement.as_ref().map(|record| self.hash_encoded(record)),
                pre_state_commitment: settlement
                    .as_ref()
                    .map(|record| record.pre_state_commitment),
                settlement,
            }
        }

        #[ink(message)]
        pub fn q_market(&self, market_id: Hash32) -> Option<NormalizedMarketSnapshot> {
            self.markets
                .get(&market_id)
                .map(|market| self.normalized_market_snapshot(&market))
        }

        #[ink(message)]
        pub fn q_identity(&self, order_id: Hash32) -> Result<QIdentitySnapshot, Error> {
            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            let object = self.normalized_object_snapshot(&order)?;
            let buyer_nonce = order
                .buyer_commitment
                .as_ref()
                .map(|buyer| self.buyer_order_nonce.get(&buyer.buyer).unwrap_or(0));
            let authorization_nonce = match order.order_commitment.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                    Some(self.current_authorization_nonce(
                        &Self::protocol_to_account(&order.order_commitment.seller),
                        &object.asset_id,
                        order.order_commitment.operation,
                    ))
                }
                MarketOperation::TransferRight => None,
            };
            Ok(QIdentitySnapshot {
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
                settlement_domain_id: SETTLEMENT_DOMAIN_ID,
                order_commitment_hash: Some(self.hash_encoded(&order.order_commitment)),
                seller_nonce: Some(
                    self.seller_order_nonce
                        .get(&(
                            order.order_commitment.seller,
                            order.order_commitment.object_id,
                        ))
                        .unwrap_or(0),
                ),
                buyer_nonce,
                authorization_nonce,
                object_state_version: match order.order_commitment.operation {
                    MarketOperation::TransferRight => object.right_state_version,
                    MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                        Some(object.asset_state_version)
                    }
                },
            })
        }

        #[ink(message)]
        pub fn q_all(&self, order_id: Hash32) -> Result<QAllSnapshot, Error> {
            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            let asset_id = order
                .order_commitment
                .parent_asset_id
                .unwrap_or(order.order_commitment.object_id);
            let right_id = match order.order_commitment.operation {
                MarketOperation::TransferMeta => None,
                MarketOperation::IssueRight => Some(self.issued_right_id(order_id)),
                MarketOperation::TransferRight => Some(order.order_commitment.object_id),
            };
            let meta = self.q_meta(asset_id);
            let right = right_id.map(|id| self.q_right(id));
            let order_snapshot = self.q_order(order_id);
            let authorization = self.normalized_authorization_snapshot(&order)?;
            let payment = self.q_payment(order_id)?;
            let evidence = self.q_evidence(order_id);
            let market = self
                .markets
                .get(&order.order_commitment.market_id)
                .ok_or(Error::MarketNotRegistered)?;
            let market = self.normalized_market_snapshot(&market);
            let identity = self.q_identity(order_id)?;
            let observed_at = self.env().block_number();
            let state_commitment = self.hash_encoded(&(
                observed_at,
                &meta,
                &right,
                &order_snapshot,
                &authorization,
                &payment,
                &evidence,
                &market,
                &identity,
            ));
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

        #[ink(message)]
        pub fn get_total_escrowed(&self) -> Balance {
            self.total_escrowed
        }

        #[ink(message)]
        pub fn r2_x1_evaluate_policy(
            &mut self,
            base_price: Balance,
            offers: Vec<Balance>,
        ) -> R2X1PolicyDecision {
            let market_fee_bps = 100;
            let gross_price = base_price;
            let market_fee_amount = gross_price.saturating_mul(market_fee_bps as Balance) / 10_000;
            let decision = R2X1PolicyDecision {
                base_price,
                gross_price,
                market_fee_bps,
                market_fee_amount,
                selected_offer_index: 0,
            };
            let _ = offers;
            self.last_policy_decision = Some(decision);
            self.env().emit_event(PolicyEvaluated {
                base_price,
                gross_price,
                market_fee_bps,
                selected_offer_index: 0,
            });
            decision
        }

        #[ink(message)]
        pub fn r2_x1_last_policy_decision(&self) -> Option<R2X1PolicyDecision> {
            self.last_policy_decision
        }

        #[ink(message)]
        pub fn get_seller_order_nonce(&self, seller: ProtocolAccountId, object_id: Hash32) -> u64 {
            self.seller_order_nonce
                .get(&(seller, object_id))
                .unwrap_or(0)
        }

        #[ink(message)]
        pub fn get_buyer_order_nonce(&self, buyer: ProtocolAccountId) -> u64 {
            self.buyer_order_nonce.get(&buyer).unwrap_or(0)
        }

        #[ink(message)]
        pub fn escrow_balance_reconciles(&self) -> bool {
            self.env().balance() >= self.total_escrowed
        }

        #[ink(message)]
        pub fn get_meta(&self, asset_id: Hash32) -> Option<MetaToken> {
            self.meta_tokens.get(&asset_id)
        }

        #[ink(message)]
        pub fn get_right(&self, right_id: Hash32) -> Option<RightToken> {
            self.right_tokens.get(&right_id)
        }

        #[ink(message)]
        pub fn get_market(&self, market_id: Hash32) -> Option<MarketRegistration> {
            self.markets.get(&market_id)
        }

        #[ink(message)]
        pub fn get_issue_right_authorization(
            &self,
            auth_id: Hash32,
        ) -> Option<IssueRightAuthorization> {
            self.issue_authorizations.get(&auth_id)
        }

        #[ink(message)]
        pub fn get_transfer_meta_authorization(
            &self,
            auth_id: Hash32,
        ) -> Option<TransferMetaAuthorization> {
            self.transfer_authorizations.get(&auth_id)
        }

        #[ink(message)]
        pub fn get_issue_quota_reservation(
            &self,
            order_id: Hash32,
        ) -> Option<IssueQuotaReservation> {
            self.issue_quota_reservations.get(&order_id)
        }

        #[ink(message)]
        pub fn get_authorization_nonce(
            &self,
            owner: AccountId,
            asset_id: Hash32,
            operation: MarketOperation,
        ) -> u64 {
            self.current_authorization_nonce(&owner, &asset_id, operation)
        }

        #[ink(message)]
        pub fn get_asset_right_ids(&self, asset_id: Hash32) -> Vec<Hash32> {
            self.asset_right_ids.get(&asset_id).unwrap_or_default()
        }

        #[ink(message)]
        pub fn get_active_right_count(&self, asset_id: Hash32) -> u32 {
            self.active_right_count.get(&asset_id).unwrap_or(0)
        }

        #[ink(message)]
        pub fn get_active_issue_authorization_count(&self, asset_id: Hash32) -> u32 {
            self.active_issue_authorization_count
                .get(&asset_id)
                .unwrap_or(0)
        }

        #[ink(message)]
        pub fn get_outstanding_issue_quota_count(&self, asset_id: Hash32) -> u32 {
            self.outstanding_issue_quota_count
                .get(&asset_id)
                .unwrap_or(0)
        }

        #[ink(message)]
        pub fn is_meta_indexed_for_owner(&self, owner: AccountId, asset_id: Hash32) -> bool {
            self.meta_owner_index.contains(&(owner, asset_id))
        }

        #[ink(message)]
        pub fn is_right_indexed_for_holder(&self, holder: AccountId, right_id: Hash32) -> bool {
            self.right_holder_index.contains(&(holder, right_id))
        }

        #[ink(message)]
        pub fn is_right_indexed_for_asset(&self, asset_id: Hash32, right_id: Hash32) -> bool {
            self.right_parent_index.contains(&(asset_id, right_id))
        }

        #[ink(message)]
        pub fn is_right_valid(&self, right_id: Hash32) -> Result<bool, Error> {
            let right = self
                .right_tokens
                .get(&right_id)
                .ok_or(Error::RightNotFound)?;
            self.right_is_valid(&right)
        }

        fn account_to_protocol(account: &AccountId) -> ProtocolAccountId {
            let mut output = [0u8; 32];
            output.copy_from_slice(account.as_ref());
            output
        }

        fn revert_on_error<T: scale::Encode>(result: Result<T, Error>) -> Result<T, Error> {
            match result {
                Ok(value) => Ok(value),
                Err(error) => {
                    #[cfg(test)]
                    {
                        Err(error)
                    }
                    #[cfg(not(test))]
                    {
                        ink::env::return_value(
                            ink::env::ReturnFlags::REVERT,
                            &Result::<T, Error>::Err(error),
                        )
                    }
                }
            }
        }

        fn protocol_to_account(account: &ProtocolAccountId) -> AccountId {
            AccountId::from(*account)
        }

        fn domain_separator(
            &self,
            operation: MarketOperation,
            message_type: MessageType,
        ) -> Hash32 {
            self.hash_encoded(&(
                self.genesis_hash,
                SETTLEMENT_DOMAIN_ID,
                SETTLEMENT_PROTOCOL_VERSION,
                message_type,
                operation,
            ))
        }

        fn order_id(&self, order: &OrderCommitment) -> Hash32 {
            let domain = self.domain_separator(order.operation, MessageType::Order);
            self.hash_encoded(&(domain, order))
        }

        fn escrow_id(&self, operation: MarketOperation, order_id: Hash32) -> Hash32 {
            let domain = self.domain_separator(operation, MessageType::Settlement);
            self.hash_encoded(&(domain, order_id, *b"escrow"))
        }

        fn settlement_id(&self, operation: MarketOperation, order_id: Hash32) -> Hash32 {
            let domain = self.domain_separator(operation, MessageType::Settlement);
            self.hash_encoded(&(domain, order_id, *b"settlement"))
        }

        fn issued_right_id(&self, order_id: Hash32) -> Hash32 {
            let domain = self.domain_separator(MarketOperation::IssueRight, MessageType::Right);
            self.hash_encoded(&(domain, order_id, *b"right"))
        }

        fn seller_signing_payload(
            &self,
            operation: MarketOperation,
            commitment: &SellerCommitment,
        ) -> Vec<u8> {
            (
                self.domain_separator(operation, MessageType::Seller),
                commitment,
            )
                .encode()
        }

        fn buyer_signing_payload(
            &self,
            operation: MarketOperation,
            commitment: &BuyerCommitment,
        ) -> Vec<u8> {
            (
                self.domain_separator(operation, MessageType::Buyer),
                commitment,
            )
                .encode()
        }

        fn verify_signature(
            &self,
            signer: &ProtocolAccountId,
            signature: &ProtocolSignature,
            payload: &[u8],
        ) -> Result<(), Error> {
            ink::env::sr25519_verify(signature, payload, signer)
                .map_err(|_| Error::InvalidSignature)
        }

        fn validate_order_commitment(&self, order: &OrderCommitment) -> Result<Hash32, Error> {
            if order.settlement_protocol_version != SETTLEMENT_PROTOCOL_VERSION {
                return Err(Error::UnsupportedProtocolVersion);
            }
            if self.env().block_number() > order.expiry {
                return Err(Error::OrderExpired);
            }
            if order.seller_state_version == 0 || order.price == 0 {
                return Err(Error::StateVersionMismatch);
            }
            let shape_valid = match order.operation {
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
            if !shape_valid {
                return Err(Error::MalformedCommitment);
            }
            Ok(self.order_id(order))
        }

        fn current_order_seller_and_version(
            &self,
            order: &OrderCommitment,
        ) -> Result<(ProtocolAccountId, u64), Error> {
            match order.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => {
                    let meta = self
                        .meta_tokens
                        .get(&order.object_id)
                        .ok_or(Error::AssetNotFound)?;
                    if meta.status != AssetStatus::Tradable {
                        return Err(Error::AssetNotTradable);
                    }
                    if !self
                        .meta_owner_index
                        .contains(&(meta.owner, order.object_id))
                    {
                        return Err(Error::RightIndexInconsistent);
                    }
                    Ok((Self::account_to_protocol(&meta.owner), meta.state_version))
                }
                MarketOperation::TransferRight => {
                    let parent_asset_id = order.parent_asset_id.ok_or(Error::ParentMismatch)?;
                    let right = self
                        .right_tokens
                        .get(&order.object_id)
                        .ok_or(Error::RightNotFound)?;
                    if right.asset_id != parent_asset_id
                        || !self
                            .right_parent_index
                            .contains(&(parent_asset_id, order.object_id))
                        || !self
                            .right_holder_index
                            .contains(&(right.holder, order.object_id))
                    {
                        return Err(Error::ParentMismatch);
                    }
                    if !self.right_is_valid(&right)? {
                        return Err(Error::RightInvalid);
                    }
                    Ok((
                        Self::account_to_protocol(&right.holder),
                        right.state_version,
                    ))
                }
            }
        }

        fn validate_seller_commitment(
            &self,
            order: &OrderCommitment,
            commitment: &SellerCommitment,
            signature: &ProtocolSignature,
            order_id: Hash32,
            current_seller: ProtocolAccountId,
            current_state_version: u64,
        ) -> Result<u64, Error> {
            if commitment.order_id != order_id
                || commitment.object_id != order.object_id
                || commitment.operation != order.operation
                || commitment.seller != order.seller
                || commitment.state_version != order.seller_state_version
                || commitment.price_policy_hash != order.policy_hash
                || commitment.right_parameters_hash != order.right_parameters_hash
                || commitment.expires_at != order.expiry
                || commitment.seller_nonce != order.order_nonce
            {
                return Err(Error::MalformedCommitment);
            }
            if order.seller != current_seller {
                return Err(Error::OwnershipMismatch);
            }
            if order.seller_state_version != current_state_version {
                return Err(Error::StateVersionMismatch);
            }
            let current_nonce = self
                .seller_order_nonce
                .get(&(order.seller, order.object_id))
                .unwrap_or(0);
            if order.order_nonce != current_nonce {
                return Err(Error::NonceUsed);
            }
            let payload = self.seller_signing_payload(order.operation, commitment);
            self.verify_signature(&commitment.seller, signature, &payload)?;
            current_nonce.checked_add(1).ok_or(Error::NonceUsed)
        }

        fn validate_buyer_commitment(
            &self,
            order: &OrderCommitment,
            commitment: &BuyerCommitment,
            signature: &ProtocolSignature,
            order_id: Hash32,
        ) -> Result<u64, Error> {
            if commitment.order_id != order_id
                || commitment.price != order.price
                || commitment.currency != order.currency
                || commitment.expires_at != order.expiry
                || order
                    .allowed_buyer
                    .map_or(false, |allowed| allowed != commitment.buyer)
            {
                return Err(Error::MalformedCommitment);
            }
            let current_nonce = self.buyer_order_nonce.get(&commitment.buyer).unwrap_or(0);
            if commitment.buyer_nonce != current_nonce {
                return Err(Error::NonceUsed);
            }
            let payload = self.buyer_signing_payload(order.operation, commitment);
            self.verify_signature(&commitment.buyer, signature, &payload)?;
            current_nonce.checked_add(1).ok_or(Error::NonceUsed)
        }

        fn issue_parameters_hash(&self, authorization: &IssueRightAuthorization) -> Hash32 {
            self.hash_encoded(&(
                authorization.right_type,
                authorization.data_scope,
                &authorization.validity_template,
                authorization.transferable,
                &authorization.allowed_transfer_markets,
                authorization.royalty_bps,
            ))
        }

        fn right_parameters_hash(&self, right: &RightToken) -> Hash32 {
            self.hash_encoded(&(
                right.right_type,
                right.data_scope,
                &right.validity,
                right.transferable,
                &right.allowed_transfer_markets,
                right.royalty_bps,
            ))
        }

        fn resolve_order_policy(
            &self,
            order: &OrderCommitment,
            witness: PricePolicyWitness,
        ) -> Result<PricePolicy, Error> {
            let policy = match (order.operation, witness) {
                (MarketOperation::TransferMeta, PricePolicyWitness::Authorization) => {
                    self.transfer_authorizations
                        .get(&order.authorization_id)
                        .ok_or(Error::AuthorizationMissing)?
                        .price_policy
                }
                (MarketOperation::IssueRight, PricePolicyWitness::Authorization) => {
                    self.issue_authorizations
                        .get(&order.authorization_id)
                        .ok_or(Error::AuthorizationMissing)?
                        .price_policy
                }
                (MarketOperation::TransferRight, PricePolicyWitness::SellerSnapshot(policy)) => {
                    policy
                }
                _ => return Err(Error::MalformedCommitment),
            };
            if self.hash_encoded(&policy) != order.policy_hash {
                return Err(Error::MalformedCommitment);
            }
            Ok(policy)
        }

        fn validate_order_authority(
            &self,
            order: &OrderCommitment,
            policy: &PricePolicy,
        ) -> Result<MarketRegistration, Error> {
            let market = self.ensure_market_operation(&order.market_id, order.operation)?;
            match order.operation {
                MarketOperation::TransferMeta => {
                    let authorization = self
                        .transfer_authorizations
                        .get(&order.authorization_id)
                        .ok_or(Error::AuthorizationMissing)?;
                    self.validate_authorization_status(authorization.status, authorization.expiry)?;
                    self.ensure_authorization_owner(&authorization.owner, &authorization.asset_id)?;
                    if authorization.asset_id != order.object_id
                        || authorization.market_id != order.market_id
                        || authorization.operation != MarketOperation::TransferMeta
                        || authorization
                            .allowed_buyer
                            .map(|buyer| Self::account_to_protocol(&buyer))
                            != order.allowed_buyer
                        || authorization.expiry != order.expiry
                        || authorization.price_policy != *policy
                        || self
                            .active_transfer_authorization
                            .get(&(authorization.asset_id, authorization.market_id))
                            != Some(authorization.auth_id)
                    {
                        return Err(Error::AuthorizationMissing);
                    }
                }
                MarketOperation::IssueRight => {
                    let authorization =
                        self.validate_issue_authorization(&order.authorization_id)?;
                    if authorization.asset_id != order.object_id
                        || authorization.market_id != order.market_id
                        || authorization.expiry != order.expiry
                        || authorization.price_policy != *policy
                        || order.right_parameters_hash
                            != Some(self.issue_parameters_hash(&authorization))
                    {
                        return Err(Error::AuthorizationMissing);
                    }
                }
                MarketOperation::TransferRight => {
                    let right = self
                        .right_tokens
                        .get(&order.object_id)
                        .ok_or(Error::RightNotFound)?;
                    if !right.transferable
                        || !right.allowed_transfer_markets.contains(&order.market_id)
                        || order.right_parameters_hash != Some(self.right_parameters_hash(&right))
                    {
                        return Err(Error::AuthorizationMissing);
                    }
                }
            }
            self.check_price_policy(*policy, order.price, market.market_fee_bps)?;
            if policy.safety_fee_bps != 0 {
                return Err(Error::FeeOutOfBounds);
            }
            Ok(market)
        }

        fn ensure_order_object_available(&self, order: &OrderCommitment) -> Result<(), Error> {
            match order.operation {
                MarketOperation::TransferMeta => {
                    if self.active_meta_order.contains(&order.object_id) {
                        return Err(Error::ActiveOrderConflict);
                    }
                    if self
                        .active_issue_authorization_count
                        .get(&order.object_id)
                        .unwrap_or(0)
                        != 0
                        || self
                            .outstanding_issue_quota_count
                            .get(&order.object_id)
                            .unwrap_or(0)
                            != 0
                        || self.has_valid_right(&order.object_id)?
                    {
                        return Err(Error::MutualExclusionConflict);
                    }
                }
                MarketOperation::IssueRight => {}
                MarketOperation::TransferRight => {
                    if self.active_right_order.contains(&order.object_id) {
                        return Err(Error::ActiveOrderConflict);
                    }
                }
            }
            Ok(())
        }

        fn reserve_order_object(&mut self, order: &OrderCommitment, order_id: Hash32) {
            match order.operation {
                MarketOperation::TransferMeta => {
                    self.active_meta_order.insert(&order.object_id, &order_id);
                }
                MarketOperation::IssueRight => {}
                MarketOperation::TransferRight => {
                    self.active_right_order.insert(&order.object_id, &order_id);
                }
            }
        }

        fn ensure_order_object_reserved(&self, order: &ContractOrder) -> Result<(), Error> {
            let reserved = match order.order_commitment.operation {
                MarketOperation::TransferMeta => self
                    .active_meta_order
                    .get(&order.order_commitment.object_id),
                MarketOperation::IssueRight => return Ok(()),
                MarketOperation::TransferRight => self
                    .active_right_order
                    .get(&order.order_commitment.object_id),
            };
            if reserved != Some(order.order_id) {
                return Err(Error::ActiveOrderInvariant);
            }
            Ok(())
        }

        fn release_order_object(&mut self, order: &ContractOrder) {
            match order.order_commitment.operation {
                MarketOperation::TransferMeta => self
                    .active_meta_order
                    .remove(&order.order_commitment.object_id),
                MarketOperation::IssueRight => {}
                MarketOperation::TransferRight => self
                    .active_right_order
                    .remove(&order.order_commitment.object_id),
            }
        }

        fn amount_split(
            &self,
            gross_price: Balance,
            royalty_bps: u16,
            market_fee_bps: u16,
            safety_fee_bps: u16,
        ) -> Result<AmountSplit, Error> {
            let total_bps = u32::from(royalty_bps)
                .checked_add(u32::from(market_fee_bps))
                .and_then(|value| value.checked_add(u32::from(safety_fee_bps)))
                .ok_or(Error::ArithmeticOverflow)?;
            if total_bps > 10_000 {
                return Err(Error::FeeOutOfBounds);
            }
            let calculate = |bps: u16| -> Result<Balance, Error> {
                let bps = Balance::from(bps);
                let whole = (gross_price / 10_000)
                    .checked_mul(bps)
                    .ok_or(Error::ArithmeticOverflow)?;
                let remainder = (gross_price % 10_000)
                    .checked_mul(bps)
                    .ok_or(Error::ArithmeticOverflow)?
                    / 10_000;
                whole
                    .checked_add(remainder)
                    .ok_or(Error::ArithmeticOverflow)
            };
            let royalty = calculate(royalty_bps)?;
            let market_fee = calculate(market_fee_bps)?;
            let safety_fee = calculate(safety_fee_bps)?;
            let deductions = royalty
                .checked_add(market_fee)
                .and_then(|value| value.checked_add(safety_fee))
                .ok_or(Error::ArithmeticOverflow)?;
            let seller_net = gross_price
                .checked_sub(deductions)
                .ok_or(Error::FeeOutOfBounds)?;
            Ok(AmountSplit {
                gross_price,
                seller_net,
                royalty,
                market_fee,
                safety_fee,
            })
        }

        fn do_create_order(
            &mut self,
            order: OrderCommitment,
            price_policy_witness: PricePolicyWitness,
            seller_commitment: SellerCommitment,
            seller_signature: ProtocolSignature,
        ) -> Result<Hash32, Error> {
            let order_id = self.validate_order_commitment(&order)?;
            if self.orders.contains(&order_id) {
                return Err(Error::NonceUsed);
            }
            let (seller, state_version) = self.current_order_seller_and_version(&order)?;
            let next_seller_nonce = self.validate_seller_commitment(
                &order,
                &seller_commitment,
                &seller_signature,
                order_id,
                seller,
                state_version,
            )?;
            let policy = self.resolve_order_policy(&order, price_policy_witness)?;
            let mut market = self.validate_order_authority(&order, &policy)?;
            let consumes_policy_decision = if let Some(decision) = self.last_policy_decision {
                if order.price != decision.gross_price
                    || market.market_fee_bps != decision.market_fee_bps
                {
                    return Err(Error::InvalidPricePolicy);
                }
                true
            } else {
                false
            };
            self.ensure_order_object_available(&order)?;
            if market.active_orders >= market.capacity_tier.max_active_orders {
                return Err(Error::CapacityExceeded);
            }
            market.active_orders = market
                .active_orders
                .checked_add(1)
                .ok_or(Error::CapacityExceeded)?;

            let created_at = self.env().block_number();
            let payment_asset = order.currency;
            let expiry = order.expiry;
            let seller_nonce_key = (order.seller, order.object_id);
            let market_id = order.market_id;
            let operation = order.operation;
            let object_id = order.object_id;
            let contract_order = ContractOrder {
                order_id,
                authorization_id: order.authorization_id,
                order_commitment: order,
                seller_commitment,
                buyer_commitment: None,
                payment_asset,
                escrow_id: None,
                created_at,
                funded_at: None,
                expiry,
                grace_deadline: None,
                state: OrderState::Open,
            };
            self.reserve_order_object(&contract_order.order_commitment, order_id);
            self.seller_order_nonce
                .insert(&seller_nonce_key, &next_seller_nonce);
            self.order_price_policies.insert(&order_id, &policy);
            self.orders.insert(&order_id, &contract_order);
            self.markets.insert(&market_id, &market);
            self.env().emit_event(OrderCreated {
                order_id,
                market_id,
                operation,
                object_id,
            });
            if consumes_policy_decision {
                self.last_policy_decision = None;
            }
            Ok(order_id)
        }

        fn do_fund_order(
            &mut self,
            order_id: Hash32,
            buyer_commitment: BuyerCommitment,
            buyer_signature: ProtocolSignature,
        ) -> Result<Hash32, Error> {
            let mut order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            if order.state != OrderState::Open {
                return Err(Error::OrderNotOpen);
            }
            let transferred = self.env().transferred_value();
            if transferred != order.order_commitment.price {
                return Err(Error::InsufficientBalance);
            }
            if self.env().block_number() > order.expiry {
                return Err(Error::OrderExpired);
            }
            self.ensure_order_object_reserved(&order)?;
            let next_buyer_nonce = self.validate_buyer_commitment(
                &order.order_commitment,
                &buyer_commitment,
                &buyer_signature,
                order_id,
            )?;
            if Self::account_to_protocol(&self.env().caller()) != buyer_commitment.buyer {
                return Err(Error::PayerBuyerMismatch);
            }
            let (seller, state_version) =
                self.current_order_seller_and_version(&order.order_commitment)?;
            if seller != order.order_commitment.seller
                || state_version != order.order_commitment.seller_state_version
            {
                return Err(Error::StateVersionMismatch);
            }
            let policy = self
                .order_price_policies
                .get(&order_id)
                .ok_or(Error::MalformedCommitment)?;
            let mut market = self.validate_order_authority(&order.order_commitment, &policy)?;
            let next_exposure = market
                .active_exposure_value
                .checked_add(order.order_commitment.price)
                .ok_or(Error::CapacityExceeded)?;
            if market.funded_orders >= market.capacity_tier.max_funded_orders
                || next_exposure > market.capacity_tier.max_active_exposure_value
            {
                return Err(Error::CapacityExceeded);
            }
            let (royalty_bps, royalty_recipient) = match order.order_commitment.operation {
                MarketOperation::TransferMeta | MarketOperation::IssueRight => (0, None),
                MarketOperation::TransferRight => {
                    let right = self
                        .right_tokens
                        .get(&order.order_commitment.object_id)
                        .ok_or(Error::RightNotFound)?;
                    let parent = self
                        .meta_tokens
                        .get(&right.asset_id)
                        .ok_or(Error::ParentMismatch)?;
                    let recipient = if right.royalty_bps == 0 {
                        None
                    } else {
                        Some(Self::account_to_protocol(&parent.owner))
                    };
                    (right.royalty_bps, recipient)
                }
            };
            let amounts = self.amount_split(
                order.order_commitment.price,
                royalty_bps,
                market.market_fee_bps,
                policy.safety_fee_bps,
            )?;
            let grace_deadline = self
                .env()
                .block_number()
                .checked_add(FUNDED_GRACE_BLOCKS)
                .ok_or(Error::ArithmeticOverflow)?;
            let escrow_id = self.escrow_id(order.order_commitment.operation, order_id);
            if self.escrows.contains(&escrow_id) {
                return Err(Error::EscrowMismatch);
            }
            let next_total_escrowed = self
                .total_escrowed
                .checked_add(amounts.gross_price)
                .ok_or(Error::ArithmeticOverflow)?;
            if self.env().balance() < next_total_escrowed {
                return Err(Error::EscrowMismatch);
            }
            if order.order_commitment.operation == MarketOperation::IssueRight {
                self.reserve_issue_quota(&order.authorization_id, order_id)?;
            }

            market.funded_orders = market
                .funded_orders
                .checked_add(1)
                .ok_or(Error::CapacityExceeded)?;
            market.active_exposure_value = next_exposure;
            let buyer = buyer_commitment.buyer;
            let escrow = ContractEscrow {
                escrow_id,
                order_id,
                payer: buyer,
                buyer,
                currency: order.payment_asset,
                amounts,
                royalty_recipient,
                market_fee_recipient: Self::account_to_protocol(&market.operator),
                state: PaymentState::Escrowed,
            };
            order.buyer_commitment = Some(buyer_commitment);
            order.escrow_id = Some(escrow_id);
            order.funded_at = Some(self.env().block_number());
            order.grace_deadline = Some(grace_deadline);
            order.state = OrderState::Funded;
            self.buyer_order_nonce.insert(&buyer, &next_buyer_nonce);
            self.escrows.insert(&escrow_id, &escrow);
            self.orders.insert(&order_id, &order);
            self.markets
                .insert(&order.order_commitment.market_id, &market);
            self.total_escrowed = next_total_escrowed;
            self.env().emit_event(OrderFunded {
                order_id,
                buyer,
                escrow_id,
                gross_price: escrow.amounts.gross_price,
                grace_deadline,
            });
            self.env().emit_event(PaymentEscrowed {
                order_id,
                escrow_id,
                amount: escrow.amounts.gross_price,
            });
            Ok(escrow_id)
        }

        fn open_order_market_after_close(
            &self,
            order: &ContractOrder,
        ) -> Result<MarketRegistration, Error> {
            let mut market = self
                .markets
                .get(&order.order_commitment.market_id)
                .ok_or(Error::MarketNotRegistered)?;
            market.active_orders = market
                .active_orders
                .checked_sub(1)
                .ok_or(Error::ActiveOrderInvariant)?;
            Ok(market)
        }

        fn funded_order_market_after_close(
            &self,
            order: &ContractOrder,
        ) -> Result<MarketRegistration, Error> {
            let mut market = self.open_order_market_after_close(order)?;
            market.funded_orders = market
                .funded_orders
                .checked_sub(1)
                .ok_or(Error::ActiveOrderInvariant)?;
            market.active_exposure_value = market
                .active_exposure_value
                .checked_sub(order.order_commitment.price)
                .ok_or(Error::ActiveOrderInvariant)?;
            Ok(market)
        }

        fn do_cancel_open(&mut self, order_id: Hash32) -> Result<(), Error> {
            let mut order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            if order.state != OrderState::Open {
                return Err(Error::OrderNotOpen);
            }
            self.ensure_order_object_reserved(&order)?;
            let (seller, _) = self.current_order_seller_and_version(&order.order_commitment)?;
            if seller != Self::account_to_protocol(&self.env().caller()) {
                return Err(Error::OwnershipMismatch);
            }
            let market = self.open_order_market_after_close(&order)?;
            order.state = OrderState::Cancelled;
            self.release_order_object(&order);
            self.orders.insert(&order_id, &order);
            self.markets
                .insert(&order.order_commitment.market_id, &market);
            self.env().emit_event(OrderCancelled { order_id });
            Ok(())
        }

        fn do_expire_open(&mut self, order_id: Hash32) -> Result<(), Error> {
            let mut order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            if order.state != OrderState::Open {
                return Err(Error::OrderNotOpen);
            }
            if self.env().block_number() <= order.expiry {
                return Err(Error::OrderExpired);
            }
            self.ensure_order_object_reserved(&order)?;
            let market = self.open_order_market_after_close(&order)?;
            order.state = OrderState::Expired;
            self.release_order_object(&order);
            self.orders.insert(&order_id, &order);
            self.markets
                .insert(&order.order_commitment.market_id, &market);
            self.env().emit_event(OrderExpired { order_id });
            Ok(())
        }

        fn validate_funded_escrow(&self, order: &ContractOrder) -> Result<ContractEscrow, Error> {
            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::EscrowMismatch)?;
            let escrow_id = order.escrow_id.ok_or(Error::EscrowMismatch)?;
            let escrow = self.escrows.get(&escrow_id).ok_or(Error::EscrowMismatch)?;
            if escrow.escrow_id != escrow_id
                || escrow.order_id != order.order_id
                || escrow.state != PaymentState::Escrowed
                || escrow.currency != order.payment_asset
                || escrow.payer != buyer_commitment.buyer
                || escrow.buyer != buyer_commitment.buyer
                || escrow.amounts.gross_price != order.order_commitment.price
            {
                return Err(Error::EscrowMismatch);
            }
            let reconciled = escrow
                .amounts
                .seller_net
                .checked_add(escrow.amounts.royalty)
                .and_then(|value| value.checked_add(escrow.amounts.market_fee))
                .and_then(|value| value.checked_add(escrow.amounts.safety_fee))
                .ok_or(Error::ArithmeticOverflow)?;
            if reconciled != escrow.amounts.gross_price || escrow.amounts.safety_fee != 0 {
                return Err(Error::EscrowMismatch);
            }
            if self.total_escrowed < escrow.amounts.gross_price
                || self.env().balance() < self.total_escrowed
            {
                return Err(Error::EscrowMismatch);
            }
            self.ensure_order_object_reserved(order)?;
            Ok(escrow)
        }

        fn validate_issue_quota_reservation(&self, order: &ContractOrder) -> Result<(), Error> {
            let reservation = self
                .issue_quota_reservations
                .get(&order.order_id)
                .ok_or(Error::QuotaReservationMissing)?;
            let authorization = self
                .issue_authorizations
                .get(&order.authorization_id)
                .ok_or(Error::AuthorizationMissing)?;
            if reservation.auth_id != order.authorization_id
                || reservation.status != IssueQuotaReservationStatus::Reserved
                || reservation.reserved_count != 1
                || authorization.reserved_count < reservation.reserved_count
                || authorization.reserved_use_units < reservation.reserved_use_units
                || self
                    .outstanding_issue_quota_count
                    .get(&authorization.asset_id)
                    .unwrap_or(0)
                    < reservation.reserved_count
            {
                return Err(Error::QuotaReservationStateInvalid);
            }
            Ok(())
        }

        fn do_refund_funded(&mut self, order_id: Hash32) -> Result<(), Error> {
            let mut order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            match order.state {
                OrderState::Funded => {}
                OrderState::Open => return Err(Error::OrderNotFunded),
                OrderState::Settled
                | OrderState::Cancelled
                | OrderState::Expired
                | OrderState::Refunded => return Err(Error::TerminalOrder),
            }
            let grace_deadline = order.grace_deadline.ok_or(Error::EscrowMismatch)?;
            if self.env().block_number() <= grace_deadline {
                return Err(Error::GracePeriodActive);
            }
            let mut escrow = self.validate_funded_escrow(&order)?;
            let buyer_commitment = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::EscrowMismatch)?;
            if escrow.buyer != buyer_commitment.buyer {
                return Err(Error::RefundBeneficiaryMismatch);
            }
            if order.order_commitment.operation == MarketOperation::IssueRight {
                self.validate_issue_quota_reservation(&order)?;
            }
            let market = self.funded_order_market_after_close(&order)?;
            let next_total_escrowed = self
                .total_escrowed
                .checked_sub(escrow.amounts.gross_price)
                .ok_or(Error::EscrowMismatch)?;
            let buyer = Self::protocol_to_account(&buyer_commitment.buyer);
            self.env()
                .transfer(buyer, escrow.amounts.gross_price)
                .map_err(|_| Error::PaymentTransferFailed)?;

            if order.order_commitment.operation == MarketOperation::IssueRight {
                self.release_issue_quota(&order.authorization_id, &order_id)?;
            }
            escrow.state = PaymentState::Refunded;
            order.state = OrderState::Refunded;
            self.release_order_object(&order);
            self.escrows.insert(&escrow.escrow_id, &escrow);
            self.orders.insert(&order_id, &order);
            self.markets
                .insert(&order.order_commitment.market_id, &market);
            self.total_escrowed = next_total_escrowed;
            self.env().emit_event(OrderRefunded {
                order_id,
                buyer: buyer_commitment.buyer,
                amount: escrow.amounts.gross_price,
            });
            self.env().emit_event(PaymentRefunded {
                order_id,
                escrow_id: escrow.escrow_id,
                amount: escrow.amounts.gross_price,
            });
            Ok(())
        }

        fn normalized_object_snapshot(
            &self,
            order: &ContractOrder,
        ) -> Result<NormalizedObjectSnapshot, Error> {
            let commitment = &order.order_commitment;
            let asset_id = commitment.parent_asset_id.unwrap_or(commitment.object_id);
            let meta = self
                .meta_tokens
                .get(&asset_id)
                .ok_or(Error::AssetNotFound)?;
            let right_id = match commitment.operation {
                MarketOperation::TransferMeta => None,
                MarketOperation::IssueRight => Some(self.issued_right_id(order.order_id)),
                MarketOperation::TransferRight => Some(commitment.object_id),
            };
            let right = right_id.and_then(|id| self.right_tokens.get(&id));
            let active_order_id = match commitment.operation {
                MarketOperation::TransferMeta => self.active_meta_order.get(&asset_id),
                MarketOperation::IssueRight => None,
                MarketOperation::TransferRight => {
                    self.active_right_order.get(&commitment.object_id)
                }
            };
            Ok(NormalizedObjectSnapshot {
                asset_id,
                asset_owner: Self::account_to_protocol(&meta.owner),
                asset_status: meta.status.encode()[0],
                asset_state_version: meta.state_version,
                right_id,
                right_exists: right.is_some(),
                right_holder: right
                    .as_ref()
                    .map(|value| Self::account_to_protocol(&value.holder)),
                right_status: right.as_ref().map(|value| value.status.encode()[0]),
                right_state_version: right.as_ref().map(|value| value.state_version),
                right_parameters_hash: right
                    .as_ref()
                    .map(|value| self.right_parameters_hash(value)),
                active_order_id,
            })
        }

        fn normalized_authorization_snapshot(
            &self,
            order: &ContractOrder,
        ) -> Result<NormalizedAuthorizationSnapshot, Error> {
            let commitment = &order.order_commitment;
            match commitment.operation {
                MarketOperation::TransferMeta => {
                    let authorization = self
                        .transfer_authorizations
                        .get(&order.authorization_id)
                        .ok_or(Error::AuthorizationMissing)?;
                    Ok(NormalizedAuthorizationSnapshot {
                        authorization_id: authorization.auth_id,
                        kind: NormalizedAuthorizationKind::TransferMeta,
                        asset_id: authorization.asset_id,
                        owner: Self::account_to_protocol(&authorization.owner),
                        market_id: authorization.market_id,
                        policy_hash: self.hash_encoded(&authorization.price_policy),
                        right_parameters_hash: None,
                        nonce: authorization.nonce,
                        expiry: authorization.expiry,
                        status: authorization.status.encode()[0],
                        reserved_count: 0,
                        reserved_use_units: 0,
                        issued_count: 0,
                        issued_use_units: 0,
                        consumed_order_id: authorization.consumed_order_id,
                    })
                }
                MarketOperation::IssueRight => {
                    let authorization = self
                        .issue_authorizations
                        .get(&order.authorization_id)
                        .ok_or(Error::AuthorizationMissing)?;
                    Ok(NormalizedAuthorizationSnapshot {
                        authorization_id: authorization.auth_id,
                        kind: NormalizedAuthorizationKind::IssueRight,
                        asset_id: authorization.asset_id,
                        owner: Self::account_to_protocol(&authorization.owner),
                        market_id: authorization.market_id,
                        policy_hash: self.hash_encoded(&authorization.price_policy),
                        right_parameters_hash: Some(self.issue_parameters_hash(&authorization)),
                        nonce: authorization.nonce,
                        expiry: authorization.expiry,
                        status: authorization.status.encode()[0],
                        reserved_count: authorization.reserved_count,
                        reserved_use_units: authorization.reserved_use_units,
                        issued_count: authorization.issued_count,
                        issued_use_units: authorization.issued_use_units,
                        consumed_order_id: None,
                    })
                }
                MarketOperation::TransferRight => Ok(NormalizedAuthorizationSnapshot {
                    authorization_id: commitment.authorization_id,
                    kind: NormalizedAuthorizationKind::TransferRightPolicy,
                    asset_id: commitment.parent_asset_id.ok_or(Error::ParentMismatch)?,
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
            &self,
            market: &MarketRegistration,
        ) -> NormalizedMarketSnapshot {
            NormalizedMarketSnapshot {
                market_id: market.market_id,
                operator: Self::account_to_protocol(&market.operator),
                protocol_version: market.protocol_version,
                capabilities: market.capabilities,
                capacity_tier: market.capacity_tier.tier_id,
                active_orders: market.active_orders,
                funded_orders: market.funded_orders,
                active_exposure_value: market.active_exposure_value,
                active_authorizations: market.active_authorizations,
                status: market.status.encode()[0],
                market_fee_bps: market.market_fee_bps,
            }
        }

        fn normalized_settlement_snapshot(
            &self,
            order: &ContractOrder,
            escrow: &ContractEscrow,
            settlement: Option<SettlementRecord>,
            market: &MarketRegistration,
        ) -> Result<NormalizedSettlementSnapshot, Error> {
            Ok(NormalizedSettlementSnapshot {
                object: self.normalized_object_snapshot(order)?,
                order: order.clone(),
                authorization: self.normalized_authorization_snapshot(order)?,
                payment: escrow.clone(),
                settlement,
                market: self.normalized_market_snapshot(market),
            })
        }

        fn pre_state_commitment(
            &self,
            order: &ContractOrder,
            escrow: &ContractEscrow,
        ) -> Result<Hash32, Error> {
            let market = self
                .markets
                .get(&order.order_commitment.market_id)
                .ok_or(Error::MarketNotRegistered)?;
            let snapshot = self.normalized_settlement_snapshot(
                order,
                escrow,
                self.settlement_records.get(&order.order_id),
                &market,
            )?;
            Ok(self.hash_encoded(&snapshot))
        }

        fn push_payout(
            payouts: &mut Vec<(AccountId, Balance)>,
            recipient: AccountId,
            amount: Balance,
        ) -> Result<(), Error> {
            if amount == 0 {
                return Ok(());
            }
            if let Some((_, existing)) = payouts
                .iter_mut()
                .find(|(account, _)| *account == recipient)
            {
                *existing = existing
                    .checked_add(amount)
                    .ok_or(Error::ArithmeticOverflow)?;
            } else {
                payouts.push((recipient, amount));
            }
            Ok(())
        }

        fn release_escrow_payment(
            &mut self,
            escrow: &ContractEscrow,
            seller: AccountId,
        ) -> Result<(), Error> {
            let mut payouts = Vec::new();
            Self::push_payout(&mut payouts, seller, escrow.amounts.seller_net)?;
            if escrow.amounts.royalty != 0 {
                let royalty_recipient = escrow
                    .royalty_recipient
                    .as_ref()
                    .ok_or(Error::EscrowMismatch)?;
                Self::push_payout(
                    &mut payouts,
                    Self::protocol_to_account(royalty_recipient),
                    escrow.amounts.royalty,
                )?;
            } else if escrow.royalty_recipient.is_some() {
                return Err(Error::EscrowMismatch);
            }
            Self::push_payout(
                &mut payouts,
                Self::protocol_to_account(&escrow.market_fee_recipient),
                escrow.amounts.market_fee,
            )?;
            let payout_total = payouts.iter().try_fold(0u128, |total, (_, amount)| {
                total.checked_add(*amount).ok_or(Error::ArithmeticOverflow)
            })?;
            if payout_total != escrow.amounts.gross_price {
                return Err(Error::EscrowMismatch);
            }
            for (recipient, amount) in payouts {
                self.env()
                    .transfer(recipient, amount)
                    .map_err(|_| Error::PaymentTransferFailed)?;
            }
            Ok(())
        }

        fn finalize_successful_settlement(
            &mut self,
            mut order: ContractOrder,
            mut escrow: ContractEscrow,
            market: MarketRegistration,
            record: SettlementRecord,
            next_total_escrowed: Balance,
        ) -> Result<Hash32, Error> {
            let order_id = order.order_id;
            let escrow_id = escrow.escrow_id;
            let settlement_id = record.settlement_id;
            let gross_price = escrow.amounts.gross_price;
            escrow.state = PaymentState::Released;
            order.state = OrderState::Settled;
            self.release_order_object(&order);
            self.escrows.insert(&escrow_id, &escrow);
            self.orders.insert(&order_id, &order);
            self.markets
                .insert(&order.order_commitment.market_id, &market);
            self.settlement_records.insert(&order_id, &record);
            self.settlement_id_index.insert(&settlement_id, &order_id);
            self.total_escrowed = next_total_escrowed;
            let post_snapshot = self.normalized_settlement_snapshot(
                &order,
                &escrow,
                Some(record.clone()),
                &market,
            )?;
            let post_state_commitment = self.hash_encoded(&post_snapshot);
            self.env().emit_event(PaymentReleased {
                order_id,
                escrow_id,
                amount: gross_price,
            });
            self.env().emit_event(OrderSettled { order_id });
            self.env().emit_event(SettlementFinalized {
                settlement_id,
                order_id,
                post_state_commitment,
            });
            Ok(settlement_id)
        }

        fn settlement_record(
            &self,
            order: &ContractOrder,
            escrow: &ContractEscrow,
            settlement_id: Hash32,
            asset_id: Hash32,
            right_id: Option<Hash32>,
            pre_state_version: u64,
            post_state_version: u64,
            pre_state_commitment: Hash32,
        ) -> Result<SettlementRecord, Error> {
            let buyer = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::EscrowMismatch)?
                .buyer;
            Ok(SettlementRecord {
                settlement_id,
                order_id: order.order_id,
                operation: order.order_commitment.operation,
                asset_id,
                right_id,
                authorization_id: order.authorization_id,
                market_id: order.order_commitment.market_id,
                policy_hash: order.order_commitment.policy_hash,
                order_commitment_hash: self.hash_encoded(&order.order_commitment),
                seller: order.order_commitment.seller,
                buyer,
                currency: order.payment_asset,
                gross_price: escrow.amounts.gross_price,
                seller_amount: escrow.amounts.seller_net,
                royalty_recipient: escrow.royalty_recipient,
                royalty_amount: escrow.amounts.royalty,
                market_fee_recipient: escrow.market_fee_recipient,
                market_fee_amount: escrow.amounts.market_fee,
                safety_fee_amount: escrow.amounts.safety_fee,
                pre_state_version,
                post_state_version,
                pre_state_commitment,
                block_number: self.env().block_number(),
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
            })
        }

        fn validate_common_settlement(
            &self,
            order_id: Hash32,
        ) -> Result<
            (
                ContractOrder,
                ContractEscrow,
                MarketRegistration,
                Hash32,
                Hash32,
                Balance,
            ),
            Error,
        > {
            let order = self.orders.get(&order_id).ok_or(Error::OrderNotFound)?;
            match order.state {
                OrderState::Funded => {}
                OrderState::Open => return Err(Error::OrderNotFunded),
                OrderState::Settled => return Err(Error::AlreadySettled),
                OrderState::Cancelled | OrderState::Expired | OrderState::Refunded => {
                    return Err(Error::TerminalOrder)
                }
            }
            if self.settlement_records.contains(&order_id) {
                return Err(Error::AlreadySettled);
            }
            let grace_deadline = order.grace_deadline.ok_or(Error::EscrowMismatch)?;
            if self.env().block_number() > grace_deadline {
                return Err(Error::OrderExpired);
            }
            let escrow = self.validate_funded_escrow(&order)?;
            let market = self.funded_order_market_after_close(&order)?;
            if escrow.market_fee_recipient != Self::account_to_protocol(&market.operator) {
                return Err(Error::EscrowMismatch);
            }
            let pre_state_commitment = self.pre_state_commitment(&order, &escrow)?;
            let settlement_id = self.settlement_id(order.order_commitment.operation, order_id);
            if self.settlement_id_index.contains(&settlement_id) {
                return Err(Error::AlreadySettled);
            }
            let next_total_escrowed = self
                .total_escrowed
                .checked_sub(escrow.amounts.gross_price)
                .ok_or(Error::EscrowMismatch)?;
            Ok((
                order,
                escrow,
                market,
                pre_state_commitment,
                settlement_id,
                next_total_escrowed,
            ))
        }

        fn do_settle(&mut self, order_id: Hash32) -> Result<Hash32, Error> {
            let (order, escrow, market, pre_commitment, settlement_id, next_total) =
                self.validate_common_settlement(order_id)?;
            match order.order_commitment.operation {
                MarketOperation::TransferMeta => self.settle_transfer_meta(
                    order,
                    escrow,
                    market,
                    pre_commitment,
                    settlement_id,
                    next_total,
                ),
                MarketOperation::IssueRight => self.settle_issue_right(
                    order,
                    escrow,
                    market,
                    pre_commitment,
                    settlement_id,
                    next_total,
                ),
                MarketOperation::TransferRight => self.settle_transfer_right(
                    order,
                    escrow,
                    market,
                    pre_commitment,
                    settlement_id,
                    next_total,
                ),
            }
        }

        #[cfg(test)]
        fn set_test_abort_operation(operation: Option<MarketOperation>) {
            TEST_ABORT_OPERATION.with(|value| value.set(operation));
        }

        fn abort_after_first_domain_write_for_test(
            _operation: MarketOperation,
        ) -> Result<(), Error> {
            #[cfg(test)]
            {
                if TEST_ABORT_OPERATION.with(|value| value.get()) == Some(_operation) {
                    return Err(Error::MalformedCommitment);
                }
            }
            Ok(())
        }

        fn settle_transfer_meta(
            &mut self,
            order: ContractOrder,
            escrow: ContractEscrow,
            mut market: MarketRegistration,
            pre_state_commitment: Hash32,
            settlement_id: Hash32,
            next_total_escrowed: Balance,
        ) -> Result<Hash32, Error> {
            let commitment = &order.order_commitment;
            let mut meta = self
                .meta_tokens
                .get(&commitment.object_id)
                .ok_or(Error::AssetNotFound)?;
            if meta.status != AssetStatus::Tradable {
                return Err(Error::AssetNotTradable);
            }
            let mut authorization = self
                .transfer_authorizations
                .get(&order.authorization_id)
                .ok_or(Error::AuthorizationMissing)?;
            if authorization.status == AuthorizationStatus::Consumed
                || authorization.consumed_order_id.is_some()
            {
                return Err(Error::AuthorizationConsumed);
            }
            if authorization.asset_id != commitment.object_id
                || authorization.market_id != commitment.market_id
                || authorization.operation != MarketOperation::TransferMeta
                || authorization.owner != meta.owner
                || commitment.policy_hash != self.hash_encoded(&authorization.price_policy)
                || commitment.allowed_buyer
                    != authorization
                        .allowed_buyer
                        .map(|account| Self::account_to_protocol(&account))
                || commitment.expiry != authorization.expiry
                || authorization.price_policy.safety_fee_bps != 0
            {
                return Err(Error::AuthorizationMissing);
            }
            let buyer = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::EscrowMismatch)?
                .buyer;
            if commitment
                .allowed_buyer
                .map_or(false, |allowed| allowed != buyer)
            {
                return Err(Error::OwnershipMismatch);
            }
            let seller = Self::account_to_protocol(&meta.owner);
            if seller != commitment.seller
                || meta.state_version != commitment.seller_state_version
                || !self
                    .meta_owner_index
                    .contains(&(meta.owner, commitment.object_id))
            {
                return Err(Error::StateVersionMismatch);
            }
            if escrow.amounts.royalty != 0 || escrow.royalty_recipient.is_some() {
                return Err(Error::EscrowMismatch);
            }
            let expected = self.amount_split(
                commitment.price,
                0,
                market.market_fee_bps,
                authorization.price_policy.safety_fee_bps,
            )?;
            if escrow.amounts != expected {
                return Err(Error::EscrowMismatch);
            }
            let post_state_version = meta
                .state_version
                .checked_add(1)
                .ok_or(Error::StateVersionOverflow)?;
            let record = self.settlement_record(
                &order,
                &escrow,
                settlement_id,
                commitment.object_id,
                None,
                meta.state_version,
                post_state_version,
                pre_state_commitment,
            )?;
            let seller_account = meta.owner;
            let buyer_account = Self::protocol_to_account(&buyer);
            if authorization.status == AuthorizationStatus::Active {
                market.active_authorizations = market
                    .active_authorizations
                    .checked_sub(1)
                    .ok_or(Error::AuthorizationCounterInvariant)?;
            }
            self.release_escrow_payment(&escrow, seller_account)?;

            self.meta_owner_index
                .remove(&(seller_account, commitment.object_id));
            self.meta_owner_index
                .insert(&(buyer_account, commitment.object_id), &());
            meta.owner = buyer_account;
            meta.state_version = post_state_version;
            self.meta_tokens.insert(&commitment.object_id, &meta);
            Self::abort_after_first_domain_write_for_test(MarketOperation::TransferMeta)?;
            if authorization.status == AuthorizationStatus::Active {
                self.active_transfer_authorization
                    .remove(&(authorization.asset_id, authorization.market_id));
                self.active_transfer_authorization_by_asset
                    .remove(&authorization.asset_id);
            }
            authorization.status = AuthorizationStatus::Consumed;
            authorization.consumed_order_id = Some(order.order_id);
            self.transfer_authorizations
                .insert(&authorization.auth_id, &authorization);
            self.env().emit_event(AuthorizationStatusChanged {
                auth_id: authorization.auth_id,
                status: AuthorizationStatus::Consumed,
            });
            self.finalize_successful_settlement(order, escrow, market, record, next_total_escrowed)
        }

        fn settle_issue_right(
            &mut self,
            order: ContractOrder,
            escrow: ContractEscrow,
            market: MarketRegistration,
            pre_state_commitment: Hash32,
            settlement_id: Hash32,
            next_total_escrowed: Balance,
        ) -> Result<Hash32, Error> {
            let commitment = &order.order_commitment;
            let meta = self
                .meta_tokens
                .get(&commitment.object_id)
                .ok_or(Error::AssetNotFound)?;
            let mut authorization = self
                .issue_authorizations
                .get(&order.authorization_id)
                .ok_or(Error::AuthorizationMissing)?;
            if authorization.status == AuthorizationStatus::Consumed
                || authorization.asset_id != commitment.object_id
                || authorization.market_id != commitment.market_id
                || authorization.operation != MarketOperation::IssueRight
                || authorization.owner != meta.owner
                || commitment.policy_hash != self.hash_encoded(&authorization.price_policy)
                || commitment.right_parameters_hash
                    != Some(self.issue_parameters_hash(&authorization))
                || authorization.price_policy.safety_fee_bps != 0
            {
                return Err(Error::AuthorizationMissing);
            }
            if Self::account_to_protocol(&meta.owner) != commitment.seller
                || meta.state_version != commitment.seller_state_version
            {
                return Err(Error::StateVersionMismatch);
            }
            self.validate_issue_quota_reservation(&order)?;
            if authorization
                .validity_template
                .valid_until
                .map_or(false, |until| u64::from(self.env().block_number()) > until)
            {
                return Err(Error::RightInvalid);
            }
            let expected = self.amount_split(
                commitment.price,
                0,
                market.market_fee_bps,
                authorization.price_policy.safety_fee_bps,
            )?;
            if escrow.amounts != expected || escrow.royalty_recipient.is_some() {
                return Err(Error::EscrowMismatch);
            }
            let right_id = self.issued_right_id(order.order_id);
            if self.right_tokens.contains(&right_id) {
                return Err(Error::RightAlreadyExists);
            }
            let mut right_ids = self
                .asset_right_ids
                .get(&commitment.object_id)
                .unwrap_or_default();
            if right_ids.len() >= MAX_RIGHTS_PER_ASSET {
                return Err(Error::QuotaExceeded);
            }
            let next_active_count = self
                .active_right_count
                .get(&commitment.object_id)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::QuotaInvariant)?;
            let mut reservation = self
                .issue_quota_reservations
                .get(&order.order_id)
                .ok_or(Error::QuotaReservationMissing)?;
            let next_outstanding = self
                .outstanding_issue_quota_count
                .get(&authorization.asset_id)
                .unwrap_or(0)
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            authorization.reserved_count = authorization
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            authorization.reserved_use_units = authorization
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            authorization.issued_count = authorization
                .issued_count
                .checked_add(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            authorization.issued_use_units = authorization
                .issued_use_units
                .checked_add(reservation.reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            reservation.status = IssueQuotaReservationStatus::Issued;
            let buyer = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::EscrowMismatch)?
                .buyer;
            let right = RightToken {
                right_id,
                asset_id: commitment.object_id,
                holder: Self::protocol_to_account(&buyer),
                issuer: meta.owner,
                right_type: authorization.right_type,
                data_scope: authorization.data_scope,
                validity: authorization.validity_template,
                remaining_uses: authorization.validity_template.max_uses,
                reserved_uses: 0,
                transferable: authorization.transferable,
                allowed_transfer_markets: authorization.allowed_transfer_markets.clone(),
                royalty_bps: authorization.royalty_bps,
                state_version: 1,
                status: RightStatus::Active,
            };
            let record = self.settlement_record(
                &order,
                &escrow,
                settlement_id,
                commitment.object_id,
                Some(right_id),
                0,
                1,
                pre_state_commitment,
            )?;
            self.release_escrow_payment(&escrow, meta.owner)?;

            right_ids.push(right_id);
            self.right_tokens.insert(&right_id, &right);
            Self::abort_after_first_domain_write_for_test(MarketOperation::IssueRight)?;
            self.right_holder_index
                .insert(&(right.holder, right_id), &());
            self.right_parent_index
                .insert(&(right.asset_id, right_id), &());
            self.asset_right_ids.insert(&right.asset_id, &right_ids);
            self.active_right_count
                .insert(&right.asset_id, &next_active_count);
            self.issue_authorizations
                .insert(&authorization.auth_id, &authorization);
            self.issue_quota_reservations
                .insert(&order.order_id, &reservation);
            self.outstanding_issue_quota_count
                .insert(&authorization.asset_id, &next_outstanding);
            self.env().emit_event(IssueQuotaConverted {
                auth_id: authorization.auth_id,
                order_id: order.order_id,
            });
            self.finalize_successful_settlement(order, escrow, market, record, next_total_escrowed)
        }

        fn settle_transfer_right(
            &mut self,
            order: ContractOrder,
            escrow: ContractEscrow,
            market: MarketRegistration,
            pre_state_commitment: Hash32,
            settlement_id: Hash32,
            next_total_escrowed: Balance,
        ) -> Result<Hash32, Error> {
            let commitment = &order.order_commitment;
            let parent_asset_id = commitment.parent_asset_id.ok_or(Error::ParentMismatch)?;
            let mut right = self
                .right_tokens
                .get(&commitment.object_id)
                .ok_or(Error::RightNotFound)?;
            if right.asset_id != parent_asset_id
                || !self
                    .right_parent_index
                    .contains(&(parent_asset_id, commitment.object_id))
                || !self
                    .right_holder_index
                    .contains(&(right.holder, commitment.object_id))
            {
                return Err(Error::ParentMismatch);
            }
            if !self.right_is_valid(&right)?
                || !right.transferable
                || !right
                    .allowed_transfer_markets
                    .contains(&commitment.market_id)
                || commitment.right_parameters_hash != Some(self.right_parameters_hash(&right))
            {
                return Err(Error::RightInvalid);
            }
            if Self::account_to_protocol(&right.holder) != commitment.seller
                || right.state_version != commitment.seller_state_version
            {
                return Err(Error::StateVersionMismatch);
            }
            let parent = self
                .meta_tokens
                .get(&parent_asset_id)
                .ok_or(Error::ParentMismatch)?;
            let expected_royalty_recipient = if right.royalty_bps == 0 {
                None
            } else {
                Some(Self::account_to_protocol(&parent.owner))
            };
            let expected = self.amount_split(
                commitment.price,
                right.royalty_bps,
                market.market_fee_bps,
                0,
            )?;
            if escrow.amounts != expected || escrow.royalty_recipient != expected_royalty_recipient
            {
                return Err(Error::EscrowMismatch);
            }
            let post_state_version = right
                .state_version
                .checked_add(1)
                .ok_or(Error::StateVersionOverflow)?;
            let buyer = order
                .buyer_commitment
                .as_ref()
                .ok_or(Error::EscrowMismatch)?
                .buyer;
            let record = self.settlement_record(
                &order,
                &escrow,
                settlement_id,
                parent_asset_id,
                Some(commitment.object_id),
                right.state_version,
                post_state_version,
                pre_state_commitment,
            )?;
            let seller = right.holder;
            let buyer_account = Self::protocol_to_account(&buyer);
            self.release_escrow_payment(&escrow, seller)?;

            self.right_holder_index
                .remove(&(seller, commitment.object_id));
            self.right_holder_index
                .insert(&(buyer_account, commitment.object_id), &());
            right.holder = buyer_account;
            right.state_version = post_state_version;
            self.right_tokens.insert(&commitment.object_id, &right);
            Self::abort_after_first_domain_write_for_test(MarketOperation::TransferRight)?;
            self.finalize_successful_settlement(order, escrow, market, record, next_total_escrowed)
        }

        fn ensure_admin(&self) -> Result<(), Error> {
            if self.env().caller() != self.admin {
                return Err(Error::NotAdmin);
            }
            Ok(())
        }

        fn hash_encoded<T: Encode>(&self, value: &T) -> Hash32 {
            let encoded = value.encode();
            let mut output = [0u8; 32];
            ink::env::hash_bytes::<ink::env::hash::Blake2x256>(&encoded, &mut output);
            output
        }

        fn validate_capabilities(&self, capabilities: u8) -> Result<(), Error> {
            if capabilities == 0 || capabilities & !ALL_CAPABILITIES != 0 {
                return Err(Error::InvalidCapabilities);
            }
            Ok(())
        }

        fn validate_capacity_tier(&self, tier: &MarketCapacityTier) -> Result<(), Error> {
            if tier.max_active_orders == 0
                || tier.max_funded_orders == 0
                || tier.max_funded_orders > tier.max_active_orders
                || tier.max_active_exposure_value == 0
            {
                return Err(Error::InvalidCapacityTier);
            }
            Ok(())
        }

        fn operation_capability(operation: MarketOperation) -> u8 {
            match operation {
                MarketOperation::TransferMeta => CAP_CREATE_META_ORDER,
                MarketOperation::IssueRight => CAP_CREATE_RIGHT_ISSUE_ORDER,
                MarketOperation::TransferRight => CAP_CREATE_RIGHT_TRANSFER_ORDER,
            }
        }

        fn ensure_market_operation(
            &self,
            market_id: &Hash32,
            operation: MarketOperation,
        ) -> Result<MarketRegistration, Error> {
            let market = self
                .markets
                .get(market_id)
                .ok_or(Error::MarketNotRegistered)?;
            if market.status != MarketStatus::Active {
                return Err(Error::MarketRestricted);
            }
            if market.protocol_version != SETTLEMENT_PROTOCOL_VERSION {
                return Err(Error::UnsupportedProtocolVersion);
            }
            if market.capabilities & Self::operation_capability(operation) == 0 {
                return Err(Error::CapabilityDenied);
            }
            Ok(market)
        }

        fn transition_market(
            &mut self,
            market_id: Hash32,
            next_status: MarketStatus,
        ) -> Result<(), Error> {
            let mut market = self
                .markets
                .get(&market_id)
                .ok_or(Error::MarketNotRegistered)?;
            let valid = matches!(
                (market.status, next_status),
                (MarketStatus::Active, MarketStatus::Restricted)
                    | (MarketStatus::Restricted, MarketStatus::Active)
            );
            if !valid {
                return Err(Error::InvalidMarketTransition);
            }
            market.status = next_status;
            self.markets.insert(&market_id, &market);
            self.env().emit_event(MarketStatusChanged {
                market_id,
                status: next_status,
            });
            Ok(())
        }

        fn ensure_authorization_owner(
            &self,
            owner: &AccountId,
            asset_id: &Hash32,
        ) -> Result<MetaToken, Error> {
            let meta = self.meta_tokens.get(asset_id).ok_or(Error::AssetNotFound)?;
            if meta.owner != *owner {
                return Err(Error::OwnershipMismatch);
            }
            if meta.status != AssetStatus::Tradable {
                return Err(Error::AssetNotTradable);
            }
            if !self.meta_owner_index.contains(&(*owner, *asset_id)) {
                return Err(Error::RightIndexInconsistent);
            }
            Ok(meta)
        }

        fn validate_price_policy_terms(&self, policy: &PricePolicy) -> Result<(), Error> {
            let total_fee_bps = u32::from(policy.signed_market_fee_cap_bps)
                .checked_add(u32::from(policy.safety_fee_bps))
                .ok_or(Error::InvalidPricePolicy)?;
            if !(1..=10_000).contains(&policy.floor_ratio_bps)
                || policy.signed_market_fee_cap_bps > 10_000
                || policy.safety_fee_bps > 10_000
                || total_fee_bps > 10_000
            {
                return Err(Error::InvalidPricePolicy);
            }
            Ok(())
        }

        fn effective_floor(&self, policy: &PricePolicy) -> Result<Balance, Error> {
            let ratio = Balance::from(policy.floor_ratio_bps);
            let whole = (policy.reference_price / 10_000)
                .checked_mul(ratio)
                .ok_or(Error::InvalidPricePolicy)?;
            let remainder = (policy.reference_price % 10_000)
                .checked_mul(ratio)
                .and_then(|value| value.checked_add(9_999))
                .ok_or(Error::InvalidPricePolicy)?
                / 10_000;
            whole
                .checked_add(remainder)
                .ok_or(Error::InvalidPricePolicy)
        }

        fn validate_right_terms(
            &self,
            validity: &RightValidity,
            transferable: bool,
            allowed_markets: &[Hash32],
            royalty_bps: u16,
            require_current_validity: bool,
        ) -> Result<(), Error> {
            if let Some(valid_until) = validity.valid_until {
                if valid_until <= validity.valid_from {
                    return Err(Error::InvalidRightParameters);
                }
            }
            if validity.max_uses == Some(0) || royalty_bps > 10_000 {
                return Err(Error::InvalidRightParameters);
            }
            if !transferable && !allowed_markets.is_empty() {
                return Err(Error::InvalidRightParameters);
            }
            if allowed_markets.len() > MAX_ALLOWED_TRANSFER_MARKETS {
                return Err(Error::InvalidRightParameters);
            }
            for (index, market_id) in allowed_markets.iter().enumerate() {
                if allowed_markets[..index].contains(market_id) {
                    return Err(Error::InvalidRightParameters);
                }
                self.ensure_market_operation(market_id, MarketOperation::TransferRight)?;
            }
            if require_current_validity {
                let now = u64::from(self.env().block_number());
                if validity.valid_from > now
                    || validity.valid_until.map_or(false, |until| now > until)
                {
                    return Err(Error::RightInvalid);
                }
            }
            Ok(())
        }

        fn validate_issue_authorization_terms(
            &self,
            terms: &IssueRightAuthorizationTerms,
        ) -> Result<(), Error> {
            self.validate_price_policy_terms(&terms.price_policy)?;
            self.validate_right_terms(
                &terms.validity_template,
                terms.transferable,
                &terms.allowed_transfer_markets,
                terms.royalty_bps,
                false,
            )?;
            let total_fee_bps = u32::from(terms.royalty_bps)
                .checked_add(u32::from(terms.price_policy.signed_market_fee_cap_bps))
                .and_then(|value| value.checked_add(u32::from(terms.price_policy.safety_fee_bps)))
                .ok_or(Error::InvalidPricePolicy)?;
            if total_fee_bps > 10_000 {
                return Err(Error::InvalidPricePolicy);
            }
            if terms.max_issue_count == 0 {
                return Err(Error::InvalidIssueQuota);
            }
            match terms.validity_template.max_uses {
                Some(use_units) => {
                    if use_units == 0 || terms.max_total_use_units < u64::from(use_units) {
                        return Err(Error::InvalidIssueQuota);
                    }
                }
                None if terms.max_total_use_units != 0 => return Err(Error::InvalidIssueQuota),
                None => {}
            }
            Ok(())
        }

        fn current_authorization_nonce(
            &self,
            owner: &AccountId,
            asset_id: &Hash32,
            operation: MarketOperation,
        ) -> u64 {
            self.authorization_nonce
                .get(&(*owner, *asset_id, operation))
                .unwrap_or(0)
        }

        fn authorization_id(
            &self,
            owner: &AccountId,
            asset_id: &Hash32,
            market_id: &Hash32,
            operation: MarketOperation,
            nonce: u64,
        ) -> Hash32 {
            self.hash_encoded(&(
                b"axc/p2/authorization",
                operation,
                asset_id,
                owner,
                market_id,
                nonce,
            ))
        }

        fn validate_authorization_status(
            &self,
            status: AuthorizationStatus,
            expiry: BlockNumber,
        ) -> Result<(), Error> {
            match status {
                AuthorizationStatus::Active => {}
                AuthorizationStatus::Revoked => return Err(Error::AuthorizationRevoked),
                AuthorizationStatus::Expired => return Err(Error::AuthorizationExpired),
                AuthorizationStatus::Consumed => return Err(Error::AuthorizationConsumed),
            }
            if self.env().block_number() > expiry {
                return Err(Error::AuthorizationExpired);
            }
            Ok(())
        }

        fn validate_issue_authorization(
            &self,
            auth_id: &Hash32,
        ) -> Result<IssueRightAuthorization, Error> {
            let authorization = self
                .issue_authorizations
                .get(auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            self.validate_authorization_status(authorization.status, authorization.expiry)?;
            self.ensure_market_operation(&authorization.market_id, MarketOperation::IssueRight)?;
            self.ensure_authorization_owner(&authorization.owner, &authorization.asset_id)?;
            if self
                .active_issue_authorization
                .get(&(authorization.asset_id, authorization.market_id))
                != Some(*auth_id)
            {
                return Err(Error::AuthorizationMissing);
            }
            Ok(authorization)
        }

        fn has_valid_right(&self, asset_id: &Hash32) -> Result<bool, Error> {
            for right_id in self.asset_right_ids.get(asset_id).unwrap_or_default() {
                let right = self
                    .right_tokens
                    .get(&right_id)
                    .ok_or(Error::RightIndexInconsistent)?;
                if right.asset_id != *asset_id
                    || !self.right_parent_index.contains(&(*asset_id, right_id))
                    || !self.right_holder_index.contains(&(right.holder, right_id))
                {
                    return Err(Error::RightIndexInconsistent);
                }
                if self.right_is_valid(&right)? {
                    return Ok(true);
                }
            }
            Ok(false)
        }

        fn right_is_valid(&self, right: &RightToken) -> Result<bool, Error> {
            let meta = self
                .meta_tokens
                .get(&right.asset_id)
                .ok_or(Error::ParentMismatch)?;
            let now = u64::from(self.env().block_number());
            Ok(right.status == RightStatus::Active
                && meta.status == AssetStatus::Tradable
                && now >= right.validity.valid_from
                && !right
                    .validity
                    .valid_until
                    .map_or(false, |until| now > until)
                && right.remaining_uses.map_or(true, |uses| uses > 0))
        }

        fn decrement_market_authorization(&mut self, market_id: &Hash32) -> Result<(), Error> {
            let mut market = self
                .markets
                .get(market_id)
                .ok_or(Error::MarketNotRegistered)?;
            market.active_authorizations = market
                .active_authorizations
                .checked_sub(1)
                .ok_or(Error::AuthorizationCounterInvariant)?;
            self.markets.insert(market_id, &market);
            Ok(())
        }

        fn close_issue_authorization(
            &mut self,
            mut authorization: IssueRightAuthorization,
            next_status: AuthorizationStatus,
        ) -> Result<(), Error> {
            if authorization.status != AuthorizationStatus::Active {
                return Err(Error::AuthorizationNotActive);
            }
            let next_count = self
                .active_issue_authorization_count
                .get(&authorization.asset_id)
                .unwrap_or(0)
                .checked_sub(1)
                .ok_or(Error::AuthorizationCounterInvariant)?;
            self.decrement_market_authorization(&authorization.market_id)?;
            authorization.status = next_status;
            self.issue_authorizations
                .insert(&authorization.auth_id, &authorization);
            self.active_issue_authorization
                .remove(&(authorization.asset_id, authorization.market_id));
            self.active_issue_authorization_count
                .insert(&authorization.asset_id, &next_count);
            self.env().emit_event(AuthorizationStatusChanged {
                auth_id: authorization.auth_id,
                status: next_status,
            });
            Ok(())
        }

        fn close_transfer_authorization(
            &mut self,
            mut authorization: TransferMetaAuthorization,
            next_status: AuthorizationStatus,
        ) -> Result<(), Error> {
            if authorization.status != AuthorizationStatus::Active {
                return Err(Error::AuthorizationNotActive);
            }
            self.decrement_market_authorization(&authorization.market_id)?;
            authorization.status = next_status;
            self.transfer_authorizations
                .insert(&authorization.auth_id, &authorization);
            self.active_transfer_authorization
                .remove(&(authorization.asset_id, authorization.market_id));
            self.active_transfer_authorization_by_asset
                .remove(&authorization.asset_id);
            self.env().emit_event(AuthorizationStatusChanged {
                auth_id: authorization.auth_id,
                status: next_status,
            });
            Ok(())
        }

        #[allow(dead_code)]
        fn consume_transfer_meta_authorization(
            &mut self,
            auth_id: &Hash32,
            order_id: Hash32,
        ) -> Result<(), Error> {
            let mut authorization = self
                .transfer_authorizations
                .get(auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            if authorization.status != AuthorizationStatus::Active {
                return Err(Error::AuthorizationConsumed);
            }
            self.decrement_market_authorization(&authorization.market_id)?;
            authorization.status = AuthorizationStatus::Consumed;
            authorization.consumed_order_id = Some(order_id);
            self.transfer_authorizations.insert(auth_id, &authorization);
            self.active_transfer_authorization
                .remove(&(authorization.asset_id, authorization.market_id));
            self.active_transfer_authorization_by_asset
                .remove(&authorization.asset_id);
            self.env().emit_event(AuthorizationStatusChanged {
                auth_id: *auth_id,
                status: AuthorizationStatus::Consumed,
            });
            Ok(())
        }

        #[allow(dead_code)]
        fn reserve_issue_quota(&mut self, auth_id: &Hash32, order_id: Hash32) -> Result<(), Error> {
            let mut authorization = self.validate_issue_authorization(auth_id)?;
            if self.issue_quota_reservations.contains(&order_id) {
                return Err(Error::QuotaReservationAlreadyExists);
            }
            let reserved_use_units = authorization
                .validity_template
                .max_uses
                .map(u64::from)
                .unwrap_or(0);
            let next_reserved_count = authorization
                .reserved_count
                .checked_add(1)
                .ok_or(Error::QuotaInvariant)?;
            let next_total_count = authorization
                .issued_count
                .checked_add(next_reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            if next_total_count > authorization.max_issue_count {
                return Err(Error::QuotaExceeded);
            }
            let next_reserved_use_units = authorization
                .reserved_use_units
                .checked_add(reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            let next_total_use_units = authorization
                .issued_use_units
                .checked_add(next_reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            if next_total_use_units > authorization.max_total_use_units {
                return Err(Error::QuotaExceeded);
            }
            let next_outstanding = self
                .outstanding_issue_quota_count
                .get(&authorization.asset_id)
                .unwrap_or(0)
                .checked_add(1)
                .ok_or(Error::QuotaInvariant)?;

            authorization.reserved_count = next_reserved_count;
            authorization.reserved_use_units = next_reserved_use_units;
            let reservation = IssueQuotaReservation {
                order_id,
                auth_id: *auth_id,
                reserved_count: 1,
                reserved_use_units,
                status: IssueQuotaReservationStatus::Reserved,
            };
            self.issue_authorizations.insert(auth_id, &authorization);
            self.issue_quota_reservations
                .insert(&order_id, &reservation);
            self.outstanding_issue_quota_count
                .insert(&authorization.asset_id, &next_outstanding);
            self.env().emit_event(IssueQuotaReserved {
                auth_id: *auth_id,
                order_id,
                use_units: reserved_use_units,
            });
            Ok(())
        }

        #[allow(dead_code)]
        fn convert_issue_quota_to_issued(
            &mut self,
            auth_id: &Hash32,
            order_id: &Hash32,
        ) -> Result<(), Error> {
            let mut reservation = self
                .issue_quota_reservations
                .get(order_id)
                .ok_or(Error::QuotaReservationMissing)?;
            if reservation.auth_id != *auth_id {
                return Err(Error::QuotaReservationAuthorizationMismatch);
            }
            if reservation.status != IssueQuotaReservationStatus::Reserved {
                return Err(Error::QuotaReservationStateInvalid);
            }
            let mut authorization = self
                .issue_authorizations
                .get(auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            let next_reserved_count = authorization
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            let next_reserved_units = authorization
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            let next_issued_count = authorization
                .issued_count
                .checked_add(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            let next_issued_units = authorization
                .issued_use_units
                .checked_add(reservation.reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            let next_outstanding = self
                .outstanding_issue_quota_count
                .get(&authorization.asset_id)
                .unwrap_or(0)
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            if next_issued_count
                .checked_add(next_reserved_count)
                .ok_or(Error::QuotaInvariant)?
                > authorization.max_issue_count
                || next_issued_units
                    .checked_add(next_reserved_units)
                    .ok_or(Error::QuotaInvariant)?
                    > authorization.max_total_use_units
            {
                return Err(Error::QuotaInvariant);
            }
            authorization.reserved_count = next_reserved_count;
            authorization.reserved_use_units = next_reserved_units;
            authorization.issued_count = next_issued_count;
            authorization.issued_use_units = next_issued_units;
            reservation.status = IssueQuotaReservationStatus::Issued;
            self.issue_authorizations.insert(auth_id, &authorization);
            self.issue_quota_reservations.insert(order_id, &reservation);
            self.outstanding_issue_quota_count
                .insert(&authorization.asset_id, &next_outstanding);
            self.env().emit_event(IssueQuotaConverted {
                auth_id: *auth_id,
                order_id: *order_id,
            });
            Ok(())
        }

        #[allow(dead_code)]
        fn release_issue_quota(
            &mut self,
            auth_id: &Hash32,
            order_id: &Hash32,
        ) -> Result<(), Error> {
            let mut reservation = self
                .issue_quota_reservations
                .get(order_id)
                .ok_or(Error::QuotaReservationMissing)?;
            if reservation.auth_id != *auth_id {
                return Err(Error::QuotaReservationAuthorizationMismatch);
            }
            if reservation.status != IssueQuotaReservationStatus::Reserved {
                return Err(Error::QuotaReservationStateInvalid);
            }
            let mut authorization = self
                .issue_authorizations
                .get(auth_id)
                .ok_or(Error::AuthorizationMissing)?;
            let next_reserved_count = authorization
                .reserved_count
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            let next_reserved_units = authorization
                .reserved_use_units
                .checked_sub(reservation.reserved_use_units)
                .ok_or(Error::QuotaInvariant)?;
            let next_outstanding = self
                .outstanding_issue_quota_count
                .get(&authorization.asset_id)
                .unwrap_or(0)
                .checked_sub(reservation.reserved_count)
                .ok_or(Error::QuotaInvariant)?;
            authorization.reserved_count = next_reserved_count;
            authorization.reserved_use_units = next_reserved_units;
            reservation.status = IssueQuotaReservationStatus::Released;
            self.issue_authorizations.insert(auth_id, &authorization);
            self.issue_quota_reservations.insert(order_id, &reservation);
            self.outstanding_issue_quota_count
                .insert(&authorization.asset_id, &next_outstanding);
            self.env().emit_event(IssueQuotaReleased {
                auth_id: *auth_id,
                order_id: *order_id,
            });
            Ok(())
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use ink::env::test;
        use schnorrkel::{ExpansionMode, Keypair, MiniSecretKey};

        const UNIT: Balance = 1_000_000_000_000;
        const TRANSFER_META_PRICE: Balance = 10 * UNIT;
        const ISSUE_RIGHT_PRICE: Balance = 3 * UNIT;
        const TRANSFER_RIGHT_PRICE: Balance = 4 * UNIT;

        fn accounts() -> test::DefaultAccounts<ink::env::DefaultEnvironment> {
            test::default_accounts::<ink::env::DefaultEnvironment>()
        }

        fn set_caller(caller: AccountId) {
            test::set_caller::<ink::env::DefaultEnvironment>(caller);
        }

        fn set_block(block: BlockNumber) {
            test::set_block_number::<ink::env::DefaultEnvironment>(block);
        }

        fn set_timestamp(timestamp: u64) {
            test::set_block_timestamp::<ink::env::DefaultEnvironment>(timestamp);
        }

        fn meta_registration(owner: AccountId, asset_id: Hash32) -> MetaRegistration {
            MetaRegistration {
                asset_id,
                owner,
                registration_provider: owner,
                service_provider: owner,
                metadata_digest: [0x11; 32],
                data_commitment: [0x12; 32],
                validation_program_hash: [0x13; 32],
                status: AssetStatus::Tradable,
                parent_asset_id: None,
                supersedes_asset_id: None,
            }
        }

        fn capacity() -> MarketCapacityTier {
            MarketCapacityTier {
                tier_id: 1,
                max_active_orders: 8,
                max_funded_orders: 4,
                max_active_exposure_value: 100 * UNIT,
            }
        }

        fn register_market(
            contract: &mut MarketPureContract,
            operator: AccountId,
            contract_address: AccountId,
            capabilities: u8,
        ) -> Hash32 {
            contract
                .register_market(
                    operator,
                    contract_address,
                    [0x21; 32],
                    SETTLEMENT_PROTOCOL_VERSION,
                    capabilities,
                    0,
                    capacity(),
                    100,
                )
                .unwrap()
        }

        fn policy() -> PricePolicy {
            PricePolicy {
                reference_price: 10_001,
                floor_ratio_bps: 5_000,
                signed_market_fee_cap_bps: 100,
                safety_fee_bps: 50,
            }
        }

        fn issue_terms(
            transfer_market: Hash32,
            expiry: BlockNumber,
        ) -> IssueRightAuthorizationTerms {
            IssueRightAuthorizationTerms {
                right_type: RightType::Compute,
                data_scope: DataScope::FieldSetHash([0x31; 32]),
                validity_template: RightValidity {
                    valid_from: 0,
                    valid_until: Some(100),
                    max_uses: Some(3),
                },
                max_issue_count: 2,
                max_total_use_units: 6,
                price_policy: policy(),
                royalty_bps: 200,
                transferable: true,
                allowed_transfer_markets: vec![transfer_market],
                expiry,
                nonce: 0,
            }
        }

        fn keypair(seed: u8) -> Keypair {
            MiniSecretKey::from_bytes(&[seed; 32])
                .unwrap()
                .expand_to_keypair(ExpansionMode::Ed25519)
        }

        fn protocol_account(keypair: &Keypair) -> ProtocolAccountId {
            keypair.public.to_bytes()
        }

        fn account(keypair: &Keypair) -> AccountId {
            AccountId::from(protocol_account(keypair))
        }

        fn sign(keypair: &Keypair, payload: &[u8]) -> ProtocolSignature {
            keypair.sign_simple(b"substrate", payload).to_bytes()
        }

        fn settlement_policy(reference_price: Balance, fee_cap: u16) -> PricePolicy {
            PricePolicy {
                reference_price,
                floor_ratio_bps: 10_000,
                signed_market_fee_cap_bps: fee_cap,
                safety_fee_bps: 0,
            }
        }

        fn settlement_market(
            contract: &mut MarketPureContract,
            operator: AccountId,
            capabilities: u8,
            market_fee_bps: u16,
        ) -> Hash32 {
            contract
                .register_market(
                    operator,
                    operator,
                    [0xb1; 32],
                    SETTLEMENT_PROTOCOL_VERSION,
                    capabilities,
                    market_fee_bps,
                    capacity(),
                    100,
                )
                .unwrap()
        }

        fn seller_commitment(
            contract: &MarketPureContract,
            order: &OrderCommitment,
            seller: &Keypair,
        ) -> (SellerCommitment, ProtocolSignature) {
            let order_id = contract.order_id(order);
            let commitment = SellerCommitment {
                order_id,
                object_id: order.object_id,
                operation: order.operation,
                seller: order.seller,
                state_version: order.seller_state_version,
                price_policy_hash: order.policy_hash,
                right_parameters_hash: order.right_parameters_hash,
                expires_at: order.expiry,
                seller_nonce: order.order_nonce,
            };
            let signature = sign(
                seller,
                &contract.seller_signing_payload(order.operation, &commitment),
            );
            (commitment, signature)
        }

        fn buyer_commitment(
            contract: &MarketPureContract,
            order: &OrderCommitment,
            buyer: &Keypair,
            nonce: u64,
        ) -> (BuyerCommitment, ProtocolSignature) {
            let commitment = BuyerCommitment {
                order_id: contract.order_id(order),
                buyer: protocol_account(buyer),
                price: order.price,
                currency: order.currency,
                expires_at: order.expiry,
                buyer_nonce: nonce,
            };
            let signature = sign(
                buyer,
                &contract.buyer_signing_payload(order.operation, &commitment),
            );
            (commitment, signature)
        }

        fn fund(
            contract: &mut MarketPureContract,
            order: &OrderCommitment,
            buyer: &Keypair,
            nonce: u64,
        ) -> Hash32 {
            let buyer_account = account(buyer);
            set_caller(buyer_account);
            test::set_value_transferred::<ink::env::DefaultEnvironment>(order.price);
            let callee = test::callee::<ink::env::DefaultEnvironment>();
            let current =
                test::get_account_balance::<ink::env::DefaultEnvironment>(callee).unwrap_or(0);
            test::set_account_balance::<ink::env::DefaultEnvironment>(
                callee,
                current + order.price,
            );
            let (commitment, signature) = buyer_commitment(contract, order, buyer, nonce);
            let escrow_id = contract
                .fund_order(commitment.order_id, commitment, signature)
                .unwrap();
            test::set_value_transferred::<ink::env::DefaultEnvironment>(0);
            escrow_id
        }

        fn initialize_balances(accounts: &[AccountId]) {
            for account in accounts {
                test::set_account_balance::<ink::env::DefaultEnvironment>(*account, 100 * UNIT);
            }
            let callee = test::callee::<ink::env::DefaultEnvironment>();
            test::set_account_balance::<ink::env::DefaultEnvironment>(callee, 0);
        }

        fn hex_bytes(value: &str) -> Vec<u8> {
            let value = value.strip_prefix("0x").unwrap_or(value);
            assert_eq!(value.len() % 2, 0);
            value
                .as_bytes()
                .chunks_exact(2)
                .map(|pair| {
                    let digit = |byte: u8| match byte {
                        b'0'..=b'9' => byte - b'0',
                        b'a'..=b'f' => byte - b'a' + 10,
                        b'A'..=b'F' => byte - b'A' + 10,
                        _ => panic!("invalid hex digit"),
                    };
                    (digit(pair[0]) << 4) | digit(pair[1])
                })
                .collect()
        }

        fn hash32(value: &str) -> Hash32 {
            hex_bytes(value).try_into().unwrap()
        }

        #[ink::test]
        fn shared_protocol_transfer_meta_codec_anchor_matches_frozen_fixture() {
            let a = accounts();
            set_caller(a.alice);
            let genesis_hash =
                hash32("8adcab6e04d97a0db5f99fe2a33c69f97f235322e43702564281428716f90a85");
            let contract = MarketPureContract::new_with_genesis_hash(genesis_hash);
            let order = OrderCommitment {
                settlement_protocol_version: 1,
                market_id: hash32(
                    "009a716488378838d8a83ed51a65f0a8038563174113b4a484b1b7c56802715e",
                ),
                operation: MarketOperation::TransferMeta,
                object_type: ObjectType::MetaToken,
                object_id: hash32(
                    "d68b2c747fb4fb5029630fb208d0a7aec3271a1cfaf2ede24de605d1273e149a",
                ),
                parent_asset_id: None,
                authorization_id: hash32(
                    "6e383a64d5ee2346d0254949be24c8f360728f1915427c921c33a532613ffd6e",
                ),
                seller: hash32("d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d"),
                allowed_buyer: Some(hash32(
                    "90b5ab205c6974c9ea841be688864633dc9ca8a357843eeacf2314649965fe22",
                )),
                price: TRANSFER_META_PRICE,
                currency: CurrencyId::Native,
                policy_hash: hash32(
                    "24c6bffcbc22695cf9d1c2bf04f3d4b9ffb9fcdcb9f21260212fe7c60630f085",
                ),
                right_parameters_hash: None,
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };
            assert_eq!(MarketOperation::TransferMeta.encode(), vec![0]);
            assert_eq!(ObjectType::MetaToken.encode(), vec![0]);
            assert_eq!(OrderState::Refunded.encode(), vec![5]);
            assert_eq!(PaymentState::Released.encode(), vec![2]);
            assert_eq!(CurrencyId::Native.encode(), vec![0]);
            assert_eq!(
                order.encode(),
                hex_bytes(concat!(
                    "0100009a716488378838d8a83ed51a65f0a8038563174113b4a484b1b7c56802715e",
                    "0000d68b2c747fb4fb5029630fb208d0a7aec3271a1cfaf2ede24de605d1273e149a",
                    "006e383a64d5ee2346d0254949be24c8f360728f1915427c921c33a532613ffd6e",
                    "d43593c715fdd31c61141abd04a99fd6822c8558854ccde39a5684e7a56da27d",
                    "0190b5ab205c6974c9ea841be688864633dc9ca8a357843eeacf2314649965fe22",
                    "00a0724e18090000000000000000000000",
                    "24c6bffcbc22695cf9d1c2bf04f3d4b9ffb9fcdcb9f21260212fe7c60630f085",
                    "000100000000000000000000000000000014000000"
                ))
            );
            let order_id =
                hash32("71b80d5405da2ca9a67e3e01eaf9797495e5b8c9522c2eb5c19f1781fe0035db");
            assert_eq!(contract.order_id(&order), order_id);
            assert_eq!(
                contract.domain_separator(MarketOperation::TransferMeta, MessageType::Order),
                hash32("9c1889c97fcf6acfa0ea76988fb90fdbbf8b12807ae5d05d700385bbc98d5437")
            );
            assert_eq!(
                contract.escrow_id(MarketOperation::TransferMeta, order_id),
                hash32("099d5351761fccfbafab8fd1ae4ce471b4fdb1861d22c6346c60fefe92a103bc")
            );
            assert_eq!(
                contract.settlement_id(MarketOperation::TransferMeta, order_id),
                hash32("f39751362e3df4bf80cebef315f544932b17f9c19be4162e2695620bcde8c479")
            );
        }

        #[ink::test]
        fn meta_and_structured_right_are_primary_indexed_versioned_objects() {
            let a = accounts();
            set_caller(a.alice);
            set_block(1);
            set_timestamp(10_000);
            let mut contract = MarketPureContract::new();
            let asset_id = [0x41; 32];
            let right_id = [0x42; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let market_id = register_market(
                &mut contract,
                a.eve,
                a.frank,
                CAP_CREATE_RIGHT_TRANSFER_ORDER,
            );
            contract
                .initialize_existing_right(ExistingRightRegistration {
                    right_id,
                    asset_id,
                    holder: a.bob,
                    issuer: a.alice,
                    right_type: RightType::Train,
                    data_scope: DataScope::PartitionHash([0x55; 32]),
                    validity: RightValidity {
                        valid_from: 0,
                        valid_until: Some(100),
                        max_uses: Some(5),
                    },
                    transferable: true,
                    allowed_transfer_markets: vec![market_id],
                    royalty_bps: 250,
                })
                .unwrap();

            let meta = contract.get_meta(asset_id).unwrap();
            let right = contract.get_right(right_id).unwrap();
            assert_eq!(meta.owner, a.alice);
            assert_eq!(meta.state_version, 1);
            assert_eq!(right.holder, a.bob);
            assert_eq!(right.issuer, a.alice);
            assert_eq!(right.state_version, 1);
            assert_eq!(right.remaining_uses, Some(5));
            assert_eq!(right.data_scope, DataScope::PartitionHash([0x55; 32]));
            assert!(contract.is_meta_indexed_for_owner(a.alice, asset_id));
            assert!(contract.is_right_indexed_for_holder(a.bob, right_id));
            assert!(contract.is_right_indexed_for_asset(asset_id, right_id));
            assert_eq!(contract.get_asset_right_ids(asset_id), vec![right_id]);
            assert_eq!(contract.get_active_right_count(asset_id), 1);
            assert_eq!(contract.is_right_valid(right_id), Ok(true));

            set_block(101);
            contract.expire_right(right_id).unwrap();
            let expired = contract.get_right(right_id).unwrap();
            assert_eq!(expired.status, RightStatus::Expired);
            assert_eq!(expired.state_version, 2);
            assert_eq!(contract.get_active_right_count(asset_id), 0);
            assert_eq!(contract.is_right_valid(right_id), Ok(false));
        }

        #[ink::test]
        fn market_capability_and_lifecycle_are_enforced() {
            let a = accounts();
            set_caller(a.alice);
            let mut contract = MarketPureContract::new();
            let market_id = register_market(
                &mut contract,
                a.bob,
                a.charlie,
                CAP_CREATE_META_ORDER | CAP_CREATE_RIGHT_ISSUE_ORDER,
            );
            let market = contract.get_market(market_id).unwrap();
            assert_eq!(market.operator, a.bob);
            assert_eq!(market.status, MarketStatus::Active);
            assert_eq!(market.protocol_version, SETTLEMENT_PROTOCOL_VERSION);

            assert_eq!(contract.restrict_market(market_id), Ok(()));
            assert_eq!(
                contract.ensure_market_operation(&market_id, MarketOperation::TransferMeta),
                Err(Error::MarketRestricted)
            );
            assert_eq!(contract.activate_market(market_id), Ok(()));
            assert_eq!(
                contract.ensure_market_operation(&market_id, MarketOperation::TransferRight),
                Err(Error::CapabilityDenied)
            );

            set_caller(a.bob);
            assert_eq!(contract.request_market_exit(market_id), Ok(()));
            set_caller(a.alice);
            assert_eq!(contract.deregister_market(market_id), Ok(()));
            assert_eq!(
                contract.get_market(market_id).unwrap().status,
                MarketStatus::Deregistered
            );

            set_caller(a.django);
            assert_eq!(
                contract.register_market(
                    a.django,
                    a.eve,
                    [0x22; 32],
                    1,
                    CAP_CREATE_META_ORDER,
                    0,
                    capacity(),
                    100,
                ),
                Err(Error::NotAdmin)
            );
        }

        #[ink::test]
        fn authorization_binds_market_policy_owner_and_nonce() {
            let a = accounts();
            set_caller(a.alice);
            set_block(1);
            let mut contract = MarketPureContract::new();
            let asset_id = [0x51; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let market_id = register_market(
                &mut contract,
                a.eve,
                a.frank,
                CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER,
            );
            let auth_id = contract
                .create_issue_right_authorization(asset_id, market_id, issue_terms(market_id, 20))
                .unwrap();
            let authorization = contract.get_issue_right_authorization(auth_id).unwrap();
            assert_eq!(authorization.asset_id, asset_id);
            assert_eq!(authorization.market_id, market_id);
            assert_eq!(authorization.owner, a.alice);
            assert_eq!(authorization.operation, MarketOperation::IssueRight);
            assert_eq!(authorization.reserved_count, 0);
            assert_eq!(authorization.issued_count, 0);
            assert_eq!(contract.get_active_issue_authorization_count(asset_id), 1);
            assert_eq!(
                contract.get_authorization_nonce(a.alice, asset_id, MarketOperation::IssueRight),
                1
            );
            assert_eq!(
                contract
                    .get_market(market_id)
                    .unwrap()
                    .active_authorizations,
                1
            );

            let before = authorization.clone();
            assert_eq!(
                contract.create_issue_right_authorization(
                    asset_id,
                    market_id,
                    issue_terms(market_id, 20)
                ),
                Err(Error::AuthorizationAlreadyActive)
            );
            assert_eq!(
                contract.get_issue_right_authorization(auth_id),
                Some(before)
            );
            assert_eq!(
                contract.get_authorization_nonce(a.alice, asset_id, MarketOperation::IssueRight),
                1
            );
            assert_eq!(
                contract.check_price_policy(policy(), 5_000, 0),
                Err(Error::BelowPriceFloor)
            );
            assert_eq!(contract.check_price_policy(policy(), 5_001, 100), Ok(5_001));
            assert_eq!(
                contract.check_price_policy(policy(), 5_001, 101),
                Err(Error::FeeOutOfBounds)
            );
        }

        #[ink::test]
        fn invalid_authorization_terms_do_not_consume_nonce_or_write_state() {
            let a = accounts();
            set_caller(a.alice);
            set_block(1);
            let mut contract = MarketPureContract::new();
            let asset_id = [0x61; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let market_id = register_market(
                &mut contract,
                a.eve,
                a.frank,
                CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER,
            );
            let mut invalid = issue_terms(market_id, 20);
            invalid.price_policy.floor_ratio_bps = 0;
            assert_eq!(
                contract.create_issue_right_authorization(asset_id, market_id, invalid),
                Err(Error::InvalidPricePolicy)
            );
            assert_eq!(
                contract.get_authorization_nonce(a.alice, asset_id, MarketOperation::IssueRight),
                0
            );
            assert_eq!(contract.get_active_issue_authorization_count(asset_id), 0);
            assert_eq!(
                contract
                    .get_market(market_id)
                    .unwrap()
                    .active_authorizations,
                0
            );

            let mut duplicate_markets = issue_terms(market_id, 20);
            duplicate_markets.allowed_transfer_markets = vec![market_id, market_id];
            assert_eq!(
                contract.create_issue_right_authorization(asset_id, market_id, duplicate_markets),
                Err(Error::InvalidRightParameters)
            );
            assert_eq!(
                contract.get_authorization_nonce(a.alice, asset_id, MarketOperation::IssueRight),
                0
            );
        }

        #[ink::test]
        fn asset_level_mutual_exclusion_blocks_conflicting_authorizations_and_rights() {
            let a = accounts();
            set_caller(a.alice);
            set_block(1);
            let mut contract = MarketPureContract::new();
            let asset_id = [0x71; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let market_id = register_market(
                &mut contract,
                a.eve,
                a.frank,
                CAP_CREATE_META_ORDER
                    | CAP_CREATE_RIGHT_ISSUE_ORDER
                    | CAP_CREATE_RIGHT_TRANSFER_ORDER,
            );
            let issue_auth = contract
                .create_issue_right_authorization(asset_id, market_id, issue_terms(market_id, 20))
                .unwrap();
            assert_eq!(
                contract.create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy(),
                        allowed_buyer: None,
                        expiry: 20,
                        nonce: 0,
                    },
                ),
                Err(Error::MutualExclusionConflict)
            );
            contract
                .revoke_issue_right_authorization(issue_auth)
                .unwrap();
            contract
                .initialize_existing_right(ExistingRightRegistration {
                    right_id: [0x72; 32],
                    asset_id,
                    holder: a.bob,
                    issuer: a.alice,
                    right_type: RightType::View,
                    data_scope: DataScope::FullAsset,
                    validity: RightValidity {
                        valid_from: 0,
                        valid_until: Some(10),
                        max_uses: None,
                    },
                    transferable: true,
                    allowed_transfer_markets: vec![market_id],
                    royalty_bps: 0,
                })
                .unwrap();
            assert_eq!(
                contract.create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy(),
                        allowed_buyer: Some(a.charlie),
                        expiry: 20,
                        nonce: 0,
                    },
                ),
                Err(Error::MutualExclusionConflict)
            );

            set_block(11);
            let transfer_auth = contract
                .create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy(),
                        allowed_buyer: Some(a.charlie),
                        expiry: 20,
                        nonce: 0,
                    },
                )
                .unwrap();
            assert_eq!(
                contract.create_issue_right_authorization(
                    asset_id,
                    market_id,
                    IssueRightAuthorizationTerms {
                        nonce: 1,
                        ..issue_terms(market_id, 20)
                    },
                ),
                Err(Error::MutualExclusionConflict)
            );
            assert_eq!(
                contract
                    .get_transfer_meta_authorization(transfer_auth)
                    .unwrap()
                    .status,
                AuthorizationStatus::Active
            );
        }

        #[ink::test]
        fn quota_ledger_never_exceeds_issued_plus_reserved_limits() {
            let a = accounts();
            set_caller(a.alice);
            set_block(1);
            let mut contract = MarketPureContract::new();
            let asset_id = [0x81; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let market_id = register_market(
                &mut contract,
                a.eve,
                a.frank,
                CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER,
            );
            let auth_id = contract
                .create_issue_right_authorization(asset_id, market_id, issue_terms(market_id, 20))
                .unwrap();
            let order_1 = [0x91; 32];
            let order_2 = [0x92; 32];
            let order_3 = [0x93; 32];
            contract.reserve_issue_quota(&auth_id, order_1).unwrap();
            contract.reserve_issue_quota(&auth_id, order_2).unwrap();
            let before_reject = contract.get_issue_right_authorization(auth_id).unwrap();
            assert_eq!(before_reject.reserved_count, 2);
            assert_eq!(before_reject.reserved_use_units, 6);
            assert_eq!(
                contract.reserve_issue_quota(&auth_id, order_3),
                Err(Error::QuotaExceeded)
            );
            assert_eq!(
                contract.get_issue_right_authorization(auth_id),
                Some(before_reject)
            );
            assert_eq!(contract.get_issue_quota_reservation(order_3), None);

            contract
                .convert_issue_quota_to_issued(&auth_id, &order_1)
                .unwrap();
            contract.release_issue_quota(&auth_id, &order_2).unwrap();
            let final_auth = contract.get_issue_right_authorization(auth_id).unwrap();
            assert_eq!(final_auth.reserved_count, 0);
            assert_eq!(final_auth.reserved_use_units, 0);
            assert_eq!(final_auth.issued_count, 1);
            assert_eq!(final_auth.issued_use_units, 3);
            assert_eq!(contract.get_outstanding_issue_quota_count(asset_id), 0);
            assert_eq!(
                contract
                    .get_issue_quota_reservation(order_1)
                    .unwrap()
                    .status,
                IssueQuotaReservationStatus::Issued
            );
            assert_eq!(
                contract
                    .get_issue_quota_reservation(order_2)
                    .unwrap()
                    .status,
                IssueQuotaReservationStatus::Released
            );
            assert_eq!(
                contract.convert_issue_quota_to_issued(&auth_id, &order_1),
                Err(Error::QuotaReservationStateInvalid)
            );
        }

        #[ink::test]
        fn revocation_and_expiry_close_indexes_without_reusing_nonce() {
            let a = accounts();
            set_caller(a.alice);
            set_block(1);
            let mut contract = MarketPureContract::new();
            let asset_id = [0xa1; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let market_id = register_market(&mut contract, a.eve, a.frank, CAP_CREATE_META_ORDER);
            let auth_id = contract
                .create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy(),
                        allowed_buyer: None,
                        expiry: 5,
                        nonce: 0,
                    },
                )
                .unwrap();
            assert_eq!(
                contract
                    .get_market(market_id)
                    .unwrap()
                    .active_authorizations,
                1
            );
            set_block(6);
            contract
                .expire_transfer_meta_authorization(auth_id)
                .unwrap();
            assert_eq!(
                contract
                    .get_transfer_meta_authorization(auth_id)
                    .unwrap()
                    .status,
                AuthorizationStatus::Expired
            );
            assert_eq!(
                contract
                    .get_market(market_id)
                    .unwrap()
                    .active_authorizations,
                0
            );
            assert_eq!(
                contract.get_authorization_nonce(a.alice, asset_id, MarketOperation::TransferMeta),
                1
            );
            assert_eq!(
                contract.create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy(),
                        allowed_buyer: None,
                        expiry: 20,
                        nonce: 0,
                    },
                ),
                Err(Error::AuthorizationNonceMismatch)
            );
            let next_auth = contract
                .create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy(),
                        allowed_buyer: None,
                        expiry: 20,
                        nonce: 1,
                    },
                )
                .unwrap();
            assert_ne!(auth_id, next_auth);
        }

        #[ink::test]
        fn transfer_meta_uses_native_escrow_and_writes_one_settlement_record() {
            let seller = keypair(1);
            let buyer = keypair(2);
            let operator = keypair(3);
            let relayer = keypair(4);
            let seller_account = account(&seller);
            let buyer_account = account(&buyer);
            let operator_account = account(&operator);
            let relayer_account = account(&relayer);
            initialize_balances(&[
                seller_account,
                buyer_account,
                operator_account,
                relayer_account,
            ]);
            set_caller(seller_account);
            set_block(1);
            let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
            let asset_id = [0xc1; 32];
            contract
                .register_meta(meta_registration(seller_account, asset_id))
                .unwrap();
            let market_id =
                settlement_market(&mut contract, operator_account, CAP_CREATE_META_ORDER, 100);
            let policy = settlement_policy(TRANSFER_META_PRICE, 100);
            let auth_id = contract
                .create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy: policy,
                        allowed_buyer: Some(buyer_account),
                        expiry: 20,
                        nonce: 0,
                    },
                )
                .unwrap();
            let order = OrderCommitment {
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
                market_id,
                operation: MarketOperation::TransferMeta,
                object_type: ObjectType::MetaToken,
                object_id: asset_id,
                parent_asset_id: None,
                authorization_id: auth_id,
                seller: protocol_account(&seller),
                allowed_buyer: Some(protocol_account(&buyer)),
                price: TRANSFER_META_PRICE,
                currency: CurrencyId::Native,
                policy_hash: contract.hash_encoded(&policy),
                right_parameters_hash: None,
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };
            let (seller_commitment, seller_signature) =
                seller_commitment(&contract, &order, &seller);
            set_caller(relayer_account);
            let order_id = contract
                .create_order(
                    order.clone(),
                    PricePolicyWitness::Authorization,
                    seller_commitment,
                    seller_signature,
                )
                .unwrap();

            let (buyer_commitment, buyer_signature) =
                buyer_commitment(&contract, &order, &buyer, 0);
            set_caller(buyer_account);
            test::set_value_transferred::<ink::env::DefaultEnvironment>(TRANSFER_META_PRICE - 1);
            assert_eq!(
                contract.fund_order(order_id, buyer_commitment.clone(), buyer_signature),
                Err(Error::InsufficientBalance)
            );
            assert_eq!(
                contract.get_order(order_id).unwrap().state,
                OrderState::Open
            );
            assert_eq!(contract.get_buyer_order_nonce(protocol_account(&buyer)), 0);

            let escrow_id = fund(&mut contract, &order, &buyer, 0);
            assert_eq!(contract.get_total_escrowed(), TRANSFER_META_PRICE);
            assert_eq!(
                contract.get_escrow(escrow_id).unwrap().state,
                PaymentState::Escrowed
            );
            assert!(contract.escrow_balance_reconciles());
            let funded_order = contract.get_order(order_id).unwrap();
            let funded_escrow = contract.get_escrow(escrow_id).unwrap();
            let funded_market = contract.get_market(market_id).unwrap();
            let expected_pre_state_commitment = contract.hash_encoded(
                &contract
                    .normalized_settlement_snapshot(
                        &funded_order,
                        &funded_escrow,
                        None,
                        &funded_market,
                    )
                    .unwrap(),
            );
            let seller_before =
                test::get_account_balance::<ink::env::DefaultEnvironment>(seller_account).unwrap();
            let operator_before =
                test::get_account_balance::<ink::env::DefaultEnvironment>(operator_account)
                    .unwrap();
            set_caller(relayer_account);
            let settlement_id = contract.settle(order_id).unwrap();

            assert_eq!(contract.get_meta(asset_id).unwrap().owner, buyer_account);
            assert_eq!(contract.get_meta(asset_id).unwrap().state_version, 2);
            assert_eq!(
                contract.get_order(order_id).unwrap().state,
                OrderState::Settled
            );
            assert_eq!(
                contract.get_escrow(escrow_id).unwrap().state,
                PaymentState::Released
            );
            assert_eq!(contract.get_total_escrowed(), 0);
            assert_eq!(
                test::get_account_balance::<ink::env::DefaultEnvironment>(seller_account).unwrap(),
                seller_before + 9_900_000_000_000
            );
            assert_eq!(
                test::get_account_balance::<ink::env::DefaultEnvironment>(operator_account)
                    .unwrap(),
                operator_before + 100_000_000_000
            );
            let record = contract.get_settlement_record(order_id).unwrap();
            assert_eq!(record.settlement_id, settlement_id);
            assert_eq!(record.operation, MarketOperation::TransferMeta);
            assert_eq!(record.asset_id, asset_id);
            assert_eq!(record.right_id, None);
            assert_eq!(record.gross_price, TRANSFER_META_PRICE);
            assert_eq!(record.seller_amount, 9_900_000_000_000);
            assert_eq!(record.market_fee_amount, 100_000_000_000);
            assert_eq!(record.pre_state_version, 1);
            assert_eq!(record.post_state_version, 2);
            assert_eq!(record.pre_state_commitment, expected_pre_state_commitment);
            assert_eq!(record.settlement_protocol_version, 1);
            assert_eq!(contract.settle(order_id), Err(Error::AlreadySettled));
            assert_eq!(contract.get_settlement_record(order_id), Some(record));
        }

        #[ink::test]
        fn issue_right_reserves_on_funding_and_converts_quota_atomically() {
            let owner = keypair(11);
            let buyer = keypair(12);
            let operator = keypair(13);
            let owner_account = account(&owner);
            let buyer_account = account(&buyer);
            let operator_account = account(&operator);
            initialize_balances(&[owner_account, buyer_account, operator_account]);
            set_caller(owner_account);
            set_block(1);
            let mut contract = MarketPureContract::new_with_genesis_hash([0x98; 32]);
            let asset_id = [0xc2; 32];
            contract
                .register_meta(meta_registration(owner_account, asset_id))
                .unwrap();
            let market_id = settlement_market(
                &mut contract,
                operator_account,
                CAP_CREATE_RIGHT_ISSUE_ORDER | CAP_CREATE_RIGHT_TRANSFER_ORDER,
                0,
            );
            let policy = settlement_policy(ISSUE_RIGHT_PRICE, 0);
            let auth_id = contract
                .create_issue_right_authorization(
                    asset_id,
                    market_id,
                    IssueRightAuthorizationTerms {
                        right_type: RightType::Compute,
                        data_scope: DataScope::FieldSetHash([0x44; 32]),
                        validity_template: RightValidity {
                            valid_from: 0,
                            valid_until: Some(100),
                            max_uses: Some(3),
                        },
                        max_issue_count: 2,
                        max_total_use_units: 6,
                        price_policy: policy,
                        royalty_bps: 200,
                        transferable: true,
                        allowed_transfer_markets: vec![market_id],
                        expiry: 20,
                        nonce: 0,
                    },
                )
                .unwrap();
            let order = OrderCommitment {
                settlement_protocol_version: 1,
                market_id,
                operation: MarketOperation::IssueRight,
                object_type: ObjectType::MetaToken,
                object_id: asset_id,
                parent_asset_id: None,
                authorization_id: auth_id,
                seller: protocol_account(&owner),
                allowed_buyer: None,
                price: ISSUE_RIGHT_PRICE,
                currency: CurrencyId::Native,
                policy_hash: contract.hash_encoded(&policy),
                right_parameters_hash: Some(contract.issue_right_parameters_hash(auth_id).unwrap()),
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };
            let (commitment, signature) = seller_commitment(&contract, &order, &owner);
            let order_id = contract
                .create_order(
                    order.clone(),
                    PricePolicyWitness::Authorization,
                    commitment,
                    signature,
                )
                .unwrap();
            let escrow_id = fund(&mut contract, &order, &buyer, 0);
            let funded_auth = contract.get_issue_right_authorization(auth_id).unwrap();
            assert_eq!(funded_auth.reserved_count, 1);
            assert_eq!(funded_auth.reserved_use_units, 3);
            assert_eq!(funded_auth.issued_count, 0);
            assert_eq!(
                contract
                    .get_issue_quota_reservation(order_id)
                    .unwrap()
                    .status,
                IssueQuotaReservationStatus::Reserved
            );

            let settlement_id = contract.settle(order_id).unwrap();
            let record = contract.get_settlement_record(order_id).unwrap();
            let right_id = record.right_id.unwrap();
            assert_eq!(record.settlement_id, settlement_id);
            assert_eq!(record.operation, MarketOperation::IssueRight);
            assert_eq!(record.pre_state_version, 0);
            assert_eq!(record.post_state_version, 1);
            assert_eq!(contract.get_right(right_id).unwrap().holder, buyer_account);
            assert_eq!(contract.get_right(right_id).unwrap().issuer, owner_account);
            assert_eq!(
                contract.get_escrow(escrow_id).unwrap().state,
                PaymentState::Released
            );
            let settled_auth = contract.get_issue_right_authorization(auth_id).unwrap();
            assert_eq!(settled_auth.reserved_count, 0);
            assert_eq!(settled_auth.issued_count, 1);
            assert_eq!(settled_auth.issued_use_units, 3);
            assert_eq!(contract.get_outstanding_issue_quota_count(asset_id), 0);
            assert_eq!(
                contract
                    .get_issue_quota_reservation(order_id)
                    .unwrap()
                    .status,
                IssueQuotaReservationStatus::Issued
            );
        }

        #[ink::test]
        fn transfer_right_releases_seller_royalty_and_market_amounts() {
            let owner = keypair(21);
            let seller = keypair(22);
            let buyer = keypair(23);
            let operator = keypair(24);
            let owner_account = account(&owner);
            let seller_account = account(&seller);
            let buyer_account = account(&buyer);
            let operator_account = account(&operator);
            initialize_balances(&[
                owner_account,
                seller_account,
                buyer_account,
                operator_account,
            ]);
            set_caller(owner_account);
            set_block(1);
            let mut contract = MarketPureContract::new_with_genesis_hash([0x97; 32]);
            let asset_id = [0xc3; 32];
            let right_id = [0xd3; 32];
            contract
                .register_meta(meta_registration(owner_account, asset_id))
                .unwrap();
            let market_id = settlement_market(
                &mut contract,
                operator_account,
                CAP_CREATE_RIGHT_TRANSFER_ORDER,
                100,
            );
            contract
                .initialize_existing_right(ExistingRightRegistration {
                    right_id,
                    asset_id,
                    holder: seller_account,
                    issuer: owner_account,
                    right_type: RightType::Train,
                    data_scope: DataScope::FullAsset,
                    validity: RightValidity {
                        valid_from: 0,
                        valid_until: Some(100),
                        max_uses: Some(5),
                    },
                    transferable: true,
                    allowed_transfer_markets: vec![market_id],
                    royalty_bps: 500,
                })
                .unwrap();
            let policy = settlement_policy(TRANSFER_RIGHT_PRICE, 100);
            let order = OrderCommitment {
                settlement_protocol_version: 1,
                market_id,
                operation: MarketOperation::TransferRight,
                object_type: ObjectType::RightToken,
                object_id: right_id,
                parent_asset_id: Some(asset_id),
                authorization_id: [0xa3; 32],
                seller: protocol_account(&seller),
                allowed_buyer: Some(protocol_account(&buyer)),
                price: TRANSFER_RIGHT_PRICE,
                currency: CurrencyId::Native,
                policy_hash: contract.hash_encoded(&policy),
                right_parameters_hash: Some(
                    contract.existing_right_parameters_hash(right_id).unwrap(),
                ),
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };
            let (commitment, signature) = seller_commitment(&contract, &order, &seller);
            let order_id = contract
                .create_order(
                    order.clone(),
                    PricePolicyWitness::SellerSnapshot(policy),
                    commitment,
                    signature,
                )
                .unwrap();
            let escrow_id = fund(&mut contract, &order, &buyer, 0);
            let owner_before =
                test::get_account_balance::<ink::env::DefaultEnvironment>(owner_account).unwrap();
            let seller_before =
                test::get_account_balance::<ink::env::DefaultEnvironment>(seller_account).unwrap();
            let operator_before =
                test::get_account_balance::<ink::env::DefaultEnvironment>(operator_account)
                    .unwrap();
            contract.settle(order_id).unwrap();

            assert_eq!(contract.get_right(right_id).unwrap().holder, buyer_account);
            assert_eq!(contract.get_right(right_id).unwrap().state_version, 2);
            assert_eq!(
                test::get_account_balance::<ink::env::DefaultEnvironment>(seller_account).unwrap(),
                seller_before + 3_760_000_000_000
            );
            assert_eq!(
                test::get_account_balance::<ink::env::DefaultEnvironment>(owner_account).unwrap(),
                owner_before + 200_000_000_000
            );
            assert_eq!(
                test::get_account_balance::<ink::env::DefaultEnvironment>(operator_account)
                    .unwrap(),
                operator_before + 40_000_000_000
            );
            let escrow = contract.get_escrow(escrow_id).unwrap();
            assert_eq!(escrow.amounts.royalty, 200_000_000_000);
            assert_eq!(escrow.amounts.market_fee, 40_000_000_000);
            assert_eq!(escrow.amounts.seller_net, 3_760_000_000_000);
            assert_eq!(escrow.state, PaymentState::Released);
            let record = contract.get_settlement_record(order_id).unwrap();
            assert_eq!(record.asset_id, asset_id);
            assert_eq!(record.right_id, Some(right_id));
            assert_eq!(record.royalty_recipient, Some(protocol_account(&owner)));
        }

        #[ink::test]
        fn cancel_expire_and_refund_have_no_settlement_evidence() {
            let owner = keypair(31);
            let seller = keypair(32);
            let buyer = keypair(33);
            let operator = keypair(34);
            let owner_account = account(&owner);
            let seller_account = account(&seller);
            let buyer_account = account(&buyer);
            let operator_account = account(&operator);
            initialize_balances(&[
                owner_account,
                seller_account,
                buyer_account,
                operator_account,
            ]);
            set_caller(owner_account);
            set_block(1);
            let mut contract = MarketPureContract::new_with_genesis_hash([0x96; 32]);
            let asset_id = [0xc4; 32];
            let right_id = [0xd4; 32];
            contract
                .register_meta(meta_registration(owner_account, asset_id))
                .unwrap();
            let market_id = settlement_market(
                &mut contract,
                operator_account,
                CAP_CREATE_RIGHT_TRANSFER_ORDER,
                0,
            );
            contract
                .initialize_existing_right(ExistingRightRegistration {
                    right_id,
                    asset_id,
                    holder: seller_account,
                    issuer: owner_account,
                    right_type: RightType::View,
                    data_scope: DataScope::FullAsset,
                    validity: RightValidity {
                        valid_from: 0,
                        valid_until: Some(100),
                        max_uses: None,
                    },
                    transferable: true,
                    allowed_transfer_markets: vec![market_id],
                    royalty_bps: 0,
                })
                .unwrap();
            let policy = settlement_policy(TRANSFER_RIGHT_PRICE, 0);
            let policy_hash = contract.hash_encoded(&policy);
            let right_parameters_hash = contract.existing_right_parameters_hash(right_id).unwrap();
            let make_order = |nonce: u64, expiry: BlockNumber| OrderCommitment {
                settlement_protocol_version: 1,
                market_id,
                operation: MarketOperation::TransferRight,
                object_type: ObjectType::RightToken,
                object_id: right_id,
                parent_asset_id: Some(asset_id),
                authorization_id: [0xa4; 32],
                seller: protocol_account(&seller),
                allowed_buyer: None,
                price: TRANSFER_RIGHT_PRICE,
                currency: CurrencyId::Native,
                policy_hash,
                right_parameters_hash: Some(right_parameters_hash),
                seller_state_version: 1,
                order_nonce: nonce,
                expiry,
            };

            let cancelled_order = make_order(0, 5);
            let (commitment, signature) = seller_commitment(&contract, &cancelled_order, &seller);
            let cancelled_id = contract
                .create_order(
                    cancelled_order,
                    PricePolicyWitness::SellerSnapshot(policy),
                    commitment,
                    signature,
                )
                .unwrap();
            set_caller(seller_account);
            contract.cancel_open(cancelled_id).unwrap();
            assert_eq!(
                contract.get_order(cancelled_id).unwrap().state,
                OrderState::Cancelled
            );
            assert_eq!(contract.get_settlement_record(cancelled_id), None);

            let expired_order = make_order(1, 5);
            let (commitment, signature) = seller_commitment(&contract, &expired_order, &seller);
            let expired_id = contract
                .create_order(
                    expired_order,
                    PricePolicyWitness::SellerSnapshot(policy),
                    commitment,
                    signature,
                )
                .unwrap();
            set_block(6);
            set_caller(operator_account);
            contract.expire_open(expired_id).unwrap();
            assert_eq!(
                contract.get_order(expired_id).unwrap().state,
                OrderState::Expired
            );
            assert_eq!(contract.get_settlement_record(expired_id), None);

            let refundable_order = make_order(2, 20);
            let (commitment, signature) = seller_commitment(&contract, &refundable_order, &seller);
            let refundable_id = contract
                .create_order(
                    refundable_order.clone(),
                    PricePolicyWitness::SellerSnapshot(policy),
                    commitment,
                    signature,
                )
                .unwrap();
            let escrow_id = fund(&mut contract, &refundable_order, &buyer, 0);
            assert_eq!(
                contract.refund_funded(refundable_id),
                Err(Error::GracePeriodActive)
            );
            set_block(17);
            set_caller(operator_account);
            contract.refund_funded(refundable_id).unwrap();
            assert_eq!(
                contract.get_order(refundable_id).unwrap().state,
                OrderState::Refunded
            );
            assert_eq!(
                contract.get_escrow(escrow_id).unwrap().state,
                PaymentState::Refunded
            );
            assert_eq!(contract.get_total_escrowed(), 0);
            assert_eq!(contract.get_settlement_record(refundable_id), None);
            assert_eq!(contract.get_right(right_id).unwrap().holder, seller_account);
        }

        #[ink::test]
        fn rejected_settlement_preserves_funded_order_payment_and_owner() {
            let owner = keypair(41);
            let seller = keypair(42);
            let buyer = keypair(43);
            let operator = keypair(44);
            let owner_account = account(&owner);
            let seller_account = account(&seller);
            let buyer_account = account(&buyer);
            let operator_account = account(&operator);
            initialize_balances(&[
                owner_account,
                seller_account,
                buyer_account,
                operator_account,
            ]);
            set_caller(owner_account);
            set_block(1);
            let mut contract = MarketPureContract::new_with_genesis_hash([0x95; 32]);
            let asset_id = [0xc5; 32];
            let right_id = [0xd5; 32];
            contract
                .register_meta(meta_registration(owner_account, asset_id))
                .unwrap();
            let market_id = settlement_market(
                &mut contract,
                operator_account,
                CAP_CREATE_RIGHT_TRANSFER_ORDER,
                0,
            );
            contract
                .initialize_existing_right(ExistingRightRegistration {
                    right_id,
                    asset_id,
                    holder: seller_account,
                    issuer: owner_account,
                    right_type: RightType::Compute,
                    data_scope: DataScope::FullAsset,
                    validity: RightValidity {
                        valid_from: 0,
                        valid_until: Some(10),
                        max_uses: None,
                    },
                    transferable: true,
                    allowed_transfer_markets: vec![market_id],
                    royalty_bps: 0,
                })
                .unwrap();
            let policy = settlement_policy(TRANSFER_RIGHT_PRICE, 0);
            let order = OrderCommitment {
                settlement_protocol_version: 1,
                market_id,
                operation: MarketOperation::TransferRight,
                object_type: ObjectType::RightToken,
                object_id: right_id,
                parent_asset_id: Some(asset_id),
                authorization_id: [0xa5; 32],
                seller: protocol_account(&seller),
                allowed_buyer: None,
                price: TRANSFER_RIGHT_PRICE,
                currency: CurrencyId::Native,
                policy_hash: contract.hash_encoded(&policy),
                right_parameters_hash: Some(
                    contract.existing_right_parameters_hash(right_id).unwrap(),
                ),
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };
            let (commitment, signature) = seller_commitment(&contract, &order, &seller);
            let order_id = contract
                .create_order(
                    order.clone(),
                    PricePolicyWitness::SellerSnapshot(policy),
                    commitment,
                    signature,
                )
                .unwrap();
            let escrow_id = fund(&mut contract, &order, &buyer, 0);
            set_block(11);
            contract.expire_right(right_id).unwrap();
            assert_eq!(contract.settle(order_id), Err(Error::RightInvalid));
            assert_eq!(
                contract.get_order(order_id).unwrap().state,
                OrderState::Funded
            );
            assert_eq!(
                contract.get_escrow(escrow_id).unwrap().state,
                PaymentState::Escrowed
            );
            assert_eq!(contract.get_total_escrowed(), TRANSFER_RIGHT_PRICE);
            assert_eq!(contract.get_right(right_id).unwrap().holder, seller_account);
            assert_eq!(contract.get_settlement_record(order_id), None);
        }

        #[ink::test]
        fn experiment_fixture_apis_enforce_admin_and_preserve_indexes() {
            let a = accounts();
            set_caller(a.alice);
            let mut contract = MarketPureContract::new();
            let asset_id = [0xe1; 32];
            let right_id = [0xe2; 32];
            contract
                .register_meta(meta_registration(a.alice, asset_id))
                .unwrap();
            let input = ExistingRightRegistration {
                right_id,
                asset_id,
                holder: a.bob,
                issuer: a.alice,
                right_type: RightType::Compute,
                data_scope: DataScope::FullAsset,
                validity: RightValidity {
                    valid_from: 0,
                    valid_until: Some(100),
                    max_uses: Some(5),
                },
                transferable: true,
                allowed_transfer_markets: vec![],
                royalty_bps: 0,
            };

            set_caller(a.bob);
            assert_eq!(
                contract.initialize_right_for_experiment(
                    input.clone(),
                    RightStatus::Active,
                    Some(2),
                ),
                Err(Error::NotAdmin)
            );
            set_caller(a.alice);
            contract
                .initialize_right_for_experiment(input.clone(), RightStatus::Active, Some(2))
                .unwrap();
            let right = contract.right_tokens.get(&right_id).unwrap();
            assert_eq!((right.holder, right.remaining_uses), (a.bob, Some(2)));
            assert_eq!(
                contract.asset_right_ids.get(&asset_id),
                Some(vec![right_id])
            );
            assert_eq!(contract.active_right_count.get(&asset_id), Some(1));
            assert!(contract.right_holder_index.contains(&(a.bob, right_id)));
            assert!(contract.right_parent_index.contains(&(asset_id, right_id)));

            let meta_before = contract.meta_tokens.get(&asset_id).unwrap();
            let right_before = contract.right_tokens.get(&right_id).unwrap();
            set_caller(a.bob);
            assert_eq!(
                contract.configure_meta_status_for_experiment(asset_id, AssetStatus::Retiring,),
                Err(Error::NotAdmin)
            );
            assert_eq!(
                contract.configure_right_status_for_experiment(right_id, RightStatus::Expired),
                Err(Error::NotAdmin)
            );
            set_caller(a.alice);
            contract
                .configure_meta_status_for_experiment(asset_id, AssetStatus::Retiring)
                .unwrap();
            contract
                .configure_right_status_for_experiment(right_id, RightStatus::Expired)
                .unwrap();
            let mut expected_meta = meta_before;
            expected_meta.status = AssetStatus::Retiring;
            let mut expected_right = right_before;
            expected_right.status = RightStatus::Expired;
            assert_eq!(contract.meta_tokens.get(&asset_id), Some(expected_meta));
            assert_eq!(contract.right_tokens.get(&right_id), Some(expected_right));
            assert!(contract.meta_owner_index.contains(&(a.alice, asset_id)));
            assert!(contract.right_holder_index.contains(&(a.bob, right_id)));
            assert!(contract.right_parent_index.contains(&(asset_id, right_id)));
            assert_eq!(contract.active_right_count.get(&asset_id), Some(1));
            assert_eq!(
                contract.configure_meta_status_for_experiment([0xef; 32], AssetStatus::Retiring,),
                Err(Error::AssetNotFound)
            );
            assert_eq!(
                contract.configure_right_status_for_experiment([0xef; 32], RightStatus::Expired,),
                Err(Error::RightNotFound)
            );
            assert_eq!(
                contract.initialize_right_for_experiment(input, RightStatus::Revoked, Some(2),),
                Err(Error::RightAlreadyExists)
            );

            let market_id = register_market(&mut contract, a.charlie, a.django, ALL_CAPABILITIES);
            set_caller(a.bob);
            assert_eq!(
                contract.configure_market_for_experiment(
                    market_id,
                    true,
                    Some(CAP_CREATE_META_ORDER),
                    Some(0),
                ),
                Err(Error::NotAdmin)
            );
            set_caller(a.alice);
            contract
                .configure_market_for_experiment(
                    market_id,
                    true,
                    Some(CAP_CREATE_META_ORDER),
                    Some(0),
                )
                .unwrap();
            let market = contract.markets.get(&market_id).unwrap();
            assert_eq!(market.capabilities, CAP_CREATE_META_ORDER);
            assert_eq!(market.capacity_tier.max_funded_orders, 0);
            contract
                .configure_market_for_experiment(market_id, false, None, None)
                .unwrap();
            assert!(!contract.markets.contains(&market_id));
        }

        #[ink::test]
        fn x1_policy_decision_is_bound_to_order_admission_and_consumed() {
            let seller = keypair(1);
            let buyer = keypair(2);
            let operator = keypair(3);
            let relayer = keypair(4);
            let seller_account = account(&seller);
            let buyer_account = account(&buyer);
            let operator_account = account(&operator);
            let relayer_account = account(&relayer);
            initialize_balances(&[seller_account, buyer_account, operator_account, relayer_account]);
            set_caller(seller_account);
            set_block(1);
            let mut contract = MarketPureContract::new_with_genesis_hash([0x99; 32]);
            let decision = contract.r2_x1_evaluate_policy(TRANSFER_META_PRICE, vec![]);
            assert_eq!(decision.gross_price, TRANSFER_META_PRICE);
            assert_eq!(decision.market_fee_bps, 100);
            assert_eq!(
                decision.market_fee_amount,
                TRANSFER_META_PRICE * 100 / 10_000
            );

            let asset_id = [0xe1; 32];
            contract
                .register_meta(meta_registration(seller_account, asset_id))
                .unwrap();
            let market_id = settlement_market(
                &mut contract,
                operator_account,
                CAP_CREATE_META_ORDER,
                decision.market_fee_bps,
            );
            let price_policy =
                settlement_policy(decision.gross_price, decision.market_fee_bps);
            let auth_id = contract
                .create_transfer_meta_authorization(
                    asset_id,
                    market_id,
                    TransferMetaAuthorizationTerms {
                        price_policy,
                        allowed_buyer: Some(buyer_account),
                        expiry: 20,
                        nonce: 0,
                    },
                )
                .unwrap();
            let order = OrderCommitment {
                settlement_protocol_version: SETTLEMENT_PROTOCOL_VERSION,
                market_id,
                operation: MarketOperation::TransferMeta,
                object_type: ObjectType::MetaToken,
                object_id: asset_id,
                parent_asset_id: None,
                authorization_id: auth_id,
                seller: protocol_account(&seller),
                allowed_buyer: Some(protocol_account(&buyer)),
                price: decision.gross_price,
                currency: CurrencyId::Native,
                policy_hash: contract.hash_encoded(&price_policy),
                right_parameters_hash: None,
                seller_state_version: 1,
                order_nonce: 0,
                expiry: 20,
            };

            let mut mismatched = order.clone();
            mismatched.price += 1;
            let mismatched_id = contract.order_id(&mismatched);
            let (commitment, signature) = seller_commitment(&contract, &mismatched, &seller);
            set_caller(relayer_account);
            assert_eq!(
                contract.create_order(
                    mismatched,
                    PricePolicyWitness::Authorization,
                    commitment,
                    signature,
                ),
                Err(Error::InvalidPricePolicy)
            );
            assert_eq!(contract.r2_x1_last_policy_decision(), Some(decision));
            assert!(contract.get_order(mismatched_id).is_none());
            assert_eq!(contract.get_seller_order_nonce(order.seller, asset_id), 0);
            assert_eq!(contract.get_market(market_id).unwrap().active_orders, 0);

            let (commitment, signature) = seller_commitment(&contract, &order, &seller);
            let order_id = contract
                .create_order(
                    order,
                    PricePolicyWitness::Authorization,
                    commitment,
                    signature,
                )
                .unwrap();
            assert_eq!(contract.r2_x1_last_policy_decision(), None);
            assert!(contract.get_order(order_id).is_some());
            assert_eq!(contract.get_seller_order_nonce(protocol_account(&seller), asset_id), 1);
            assert_eq!(contract.get_market(market_id).unwrap().active_orders, 1);
        }

        mod s35_scenarios {
            use super::*;

            include!("s35_scenarios.rs");
        }
    }
}
