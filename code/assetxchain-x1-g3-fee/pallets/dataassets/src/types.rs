extern crate alloc;
use codec::{Decode, DecodeWithMemTracking, Encode, MaxEncodedLen};
use frame_support::{traits::ConstU32, BoundedVec};
use scale_info::TypeInfo;
use sp_core::H256;
use sp_std::vec::Vec;

// Protocol version constants
pub const ASSET_PROTOCOL_VERSION: &str = "1.0";
pub const RIGHT_TOKEN_PROTOCOL_VERSION: &str = "1.0";

// ---- BoundedVec size constants ----
const MAX_VERSION_LEN: u32 = 16;
const MAX_NAME_LEN: u32 = 256;
const MAX_DESCRIPTION_LEN: u32 = 1024;
const MAX_CID_LEN: u32 = 128;
const MAX_SIGNATURE_LEN: u32 = 128;
const MAX_LABEL_LEN: u32 = 64;
const MAX_CHARACTERISTIC_LEN: u32 = 512;
const MAX_CURRENCY_LEN: u32 = 16;
const MAX_ALGORITHM_LEN: u32 = 32;
const MAX_LABELS_COUNT: u32 = 10;
pub const MAX_ALLOWED_TRANSFER_MARKETS: u32 = 16;
pub const MAX_RIGHTS_PER_ASSET: u32 = 1024;

// ---- Core DataAsset sub-structures ----

/// Core identity, ownership, and state of a data asset
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct AssetCore<AccountId> {
    pub asset_id: [u8; 32],
    pub token_id: u32,
    pub owner: AccountId,
    pub raw_data_hash: H256,
    pub timestamp: u64,
    pub nonce: u32,
    pub state_version: u64,
    pub status: AssetStatus,
    pub updated_at: u64,
}

/// Descriptive metadata for a data asset
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct AssetMetadata {
    pub name: BoundedVec<u8, ConstU32<MAX_NAME_LEN>>,
    pub description: BoundedVec<u8, ConstU32<MAX_DESCRIPTION_LEN>>,
    pub quantity: u64,
    pub labels: BoundedVec<BoundedVec<u8, ConstU32<MAX_LABEL_LEN>>, ConstU32<MAX_LABELS_COUNT>>,
    pub metadata_cid: BoundedVec<u8, ConstU32<MAX_CID_LEN>>,
    pub data_cid: BoundedVec<u8, ConstU32<MAX_CID_LEN>>,
    pub data_size_bytes: u64,
}

/// Data quality characteristics
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct AssetCharacteristics {
    pub statistical_characteristic: BoundedVec<u8, ConstU32<MAX_CHARACTERISTIC_LEN>>,
    pub analyzing_feature: BoundedVec<u8, ConstU32<MAX_CHARACTERISTIC_LEN>>,
    pub integrity: BoundedVec<u8, ConstU32<MAX_CHARACTERISTIC_LEN>>,
}

/// Usage statistics for a data asset
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct AssetStatistics {
    pub view_count: u64,
    pub total_revenue: u128,
} // 这个数据更新频率较高，移动到主链-pallet中存储信息，可以添加更多的统计字段，如下载次数、活跃用户数等，避免频繁更新整个DataAsset结构体导致的性能问题

/// Trade asset category recorded in settlement evidence.
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
pub enum TradeAssetType {
    DataAsset,
    Certificate,
}

/// Runtime trade settlement evidence for data asset market transactions.
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct TradeSettlement<AccountId, Balance, BlockNumber> {
    pub trade_id: [u8; 32],
    pub market: AccountId,
    pub seller: AccountId,
    pub buyer: AccountId,
    pub asset_id: [u8; 32],
    pub certificate_id: [u8; 32],
    pub asset_type: TradeAssetType,
    pub price: Balance,
    pub settled_at: BlockNumber,
}

/// 运行时侧的订单最小投影。
/// 合约在创建订单后通过 Chain Extension (func_id=6) 写入，
/// 运行时结算时读取此投影进行验证。
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct MarketOrder<AccountId, Balance, BlockNumber> {
    pub order_id: [u8; 32],
    pub order_digest: [u8; 32],
    pub market: AccountId,
    pub seller: AccountId,
    pub buyer: Option<AccountId>,
    pub object_type: TradeAssetType,
    pub object_id: [u8; 32],
    pub parent_asset_id: Option<[u8; 32]>,
    pub price: Balance,
    pub status: MarketOrderStatus,
    pub created_at: BlockNumber,
}

/// 订单投影的状态机
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum MarketOrderStatus {
    Open,
    Locked,
    Settled,
    Canceled,
}

/// Legacy v1 record written beside TradeSettlements. Settlement v2 uses the
/// authoritative 24-field protocol::SettlementRecord instead.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct SettlementRecord<AccountId, Balance, BlockNumber> {
    pub trade_id: [u8; 32],
    pub order_id: [u8; 32],
    pub market: AccountId,
    pub seller: AccountId,
    pub buyer: AccountId,
    pub asset_id: [u8; 32],
    pub certificate_id: [u8; 32],
    pub object_type: TradeAssetType,
    pub right_action: RightAction,
    pub price: Balance,
    pub order_digest: [u8; 32],
    pub settled_at: BlockNumber,
    pub state_root_at_settlement: sp_core::H256,
}

/// 结算中权属变更的类型，支撑审计时区分"使用权新增"和"使用权流转"
#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum RightAction {
    TransferAsset, // MetaToken 所有权转移
    IssueRight,    // RightToken 发行（新增使用权）
    TransferRight, // RightToken 转让（使用权流转）
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
pub struct PricePolicy {
    pub reference_price: u128,
    pub floor_ratio_bps: u16,
    pub signed_market_fee_cap_bps: u16,
    pub safety_fee_bps: u16,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct IssueRightAuthorizationTerms<BlockNumber> {
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity_template: RightValidity,
    pub max_issue_count: u32,
    pub max_total_use_units: u64,
    pub price_policy: PricePolicy,
    pub royalty_bps: u16,
    pub transferable: bool,
    pub allowed_transfer_markets: BoundedVec<[u8; 32], ConstU32<MAX_ALLOWED_TRANSFER_MARKETS>>,
    pub expiry: BlockNumber,
    pub nonce: u64,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct TransferMetaAuthorizationTerms<AccountId, BlockNumber> {
    pub price_policy: PricePolicy,
    pub allowed_buyer: Option<AccountId>,
    pub expiry: BlockNumber,
    pub nonce: u64,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct IssueRightAuthorization<AccountId, BlockNumber> {
    pub auth_id: [u8; 32],
    pub asset_id: [u8; 32],
    pub owner: AccountId,
    pub market_id: [u8; 32],
    pub operation: pallet_shared_traits::MarketOperation,
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity_template: RightValidity,
    pub max_issue_count: u32,
    pub max_total_use_units: u64,
    pub price_policy: PricePolicy,
    pub royalty_bps: u16,
    pub transferable: bool,
    pub allowed_transfer_markets: BoundedVec<[u8; 32], ConstU32<MAX_ALLOWED_TRANSFER_MARKETS>>,
    pub reserved_count: u32,
    pub reserved_use_units: u64,
    pub issued_count: u32,
    pub issued_use_units: u64,
    pub expiry: BlockNumber,
    pub nonce: u64,
    pub status: AuthorizationStatus,
}

#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct TransferMetaAuthorization<AccountId, BlockNumber> {
    pub auth_id: [u8; 32],
    pub asset_id: [u8; 32],
    pub owner: AccountId,
    pub market_id: [u8; 32],
    pub operation: pallet_shared_traits::MarketOperation,
    pub price_policy: PricePolicy,
    pub allowed_buyer: Option<AccountId>,
    pub expiry: BlockNumber,
    pub nonce: u64,
    pub status: AuthorizationStatus,
    pub consumed_order_id: Option<[u8; 32]>,
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
pub struct IssueQuotaReservation {
    pub order_id: [u8; 32],
    pub auth_id: [u8; 32],
    pub reserved_count: u32,
    pub reserved_use_units: u64,
    pub status: IssueQuotaReservationStatus,
}

// ---- Main structures ----

/// Data Asset Structure
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct DataAsset<AccountId> {
    pub version: BoundedVec<u8, ConstU32<MAX_VERSION_LEN>>,
    pub core: AssetCore<AccountId>,
    pub metadata: AssetMetadata,
    pub characteristics: AssetCharacteristics,
    pub statistics: AssetStatistics,
    pub encryption_info: EncryptionInfo,
    pub signature: BoundedVec<u8, ConstU32<MAX_SIGNATURE_LEN>>,
    pub pricing_config: PricingConfig,
}

/// Right Token (Certificate) Structure
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct RightToken<AccountId> {
    pub version: BoundedVec<u8, ConstU32<MAX_VERSION_LEN>>,
    pub right_id: [u8; 32],
    pub token_id: u32,
    pub right_type: RightType,
    pub data_scope: DataScope,
    pub validity: RightValidity,
    pub remaining_uses: Option<u32>,
    pub created_at: u64,
    pub holder: AccountId,
    pub issuer: AccountId,
    pub asset_id: [u8; 32],
    pub transferable: bool,
    pub allowed_transfer_markets: BoundedVec<[u8; 32], ConstU32<MAX_ALLOWED_TRANSFER_MARKETS>>,
    pub royalty_bps: u16,
    pub state_version: u64,
    pub status: CertificateStatus,
    pub signature: BoundedVec<u8, ConstU32<MAX_SIGNATURE_LEN>>,
}

/// Structured purpose of a right. The first two variants preserve the meaning
/// of certificates created by the v1 API; new paper-minimum rights use the
/// remaining purpose variants.
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
    FieldSetHash(H256),
    #[codec(index = 2)]
    PartitionHash(H256),
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

/// Collateral Information for Asset
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct CollateralInfo<AccountId, Balance, BlockNumber> {
    pub depositor: AccountId,
    pub total_amount: Balance,
    pub reserved_amount: Balance,
    pub released_amount: Balance,
    pub release_schedule: BoundedVec<ReleasePhase<BlockNumber, Balance>, ConstU32<5>>,
    pub status: CollateralStatus<Balance>,
}

/// Release Phase for Collateral
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct ReleasePhase<BlockNumber, Balance> {
    pub percentage: u8,
    pub amount: Balance,
    pub unlock_block: BlockNumber,
    pub condition: ReleaseCondition,
    pub is_released: bool,
}

/// Conditions for Releasing Collateral
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum ReleaseCondition {
    TimeOnly,
    TimeAndVerification,
    TimeAndUsage,
    TimeAndAvailability,
}

/// Collateral Status
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum CollateralStatus<Balance> {
    FullyLocked,
    PartiallyReleased,
    FullyReleased,
    Slashed(Balance),
}

/// Encryption Information
#[derive(
    Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen,
)]
pub struct EncryptionInfo {
    pub algorithm: BoundedVec<u8, ConstU32<MAX_ALGORITHM_LEN>>,
    pub key_length: u32,
    pub parameters_hash: H256,
    pub is_encrypted: bool,
}

/// Merkle Tree Node
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo)]
pub struct MerkleNode {
    pub hash: H256,
    pub is_leaf: bool,
    pub data: Option<Vec<u8>>,
}

/// Asset Status Enumeration
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
    #[codec(index = 1)]
    Private,
    #[codec(index = 2)]
    Locked,
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

/// Certificate Status Enumeration
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
pub enum CertificateStatus {
    #[codec(index = 1)]
    Active,
    #[codec(index = 2)]
    Expired,
    #[codec(index = 3)]
    Exhausted,
    #[codec(index = 4)]
    Revoked,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum PriceType {
    Fixed,
    Negotiable,
}

/// Pricing Configuration
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct PricingConfig {
    pub price_type: PriceType,
    pub currency: BoundedVec<u8, ConstU32<MAX_CURRENCY_LEN>>,
    pub base_price: u128,
    pub usage_price: u128,
    pub access_price: u128,
}

// The v1 child tries contain unversioned SCALE values. These exact legacy
// layouts are retained only for decode fallback; every subsequent write uses
// the authoritative object layouts above.
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub(crate) struct LegacyAssetCore<AccountId> {
    pub asset_id: [u8; 32],
    pub token_id: u32,
    pub owner: AccountId,
    pub raw_data_hash: H256,
    pub timestamp: u64,
    pub nonce: u32,
    pub status: LegacyAssetStatus,
    pub updated_at: u64,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub(crate) struct LegacyDataAsset<AccountId> {
    pub version: BoundedVec<u8, ConstU32<MAX_VERSION_LEN>>,
    pub core: LegacyAssetCore<AccountId>,
    pub metadata: AssetMetadata,
    pub characteristics: AssetCharacteristics,
    pub statistics: AssetStatistics,
    pub encryption_info: EncryptionInfo,
    pub signature: BoundedVec<u8, ConstU32<MAX_SIGNATURE_LEN>>,
    pub pricing_config: PricingConfig,
}

#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub(crate) enum LegacyRightType {
    Usage = 1,
    Access = 2,
}

#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub(crate) enum LegacyAssetStatus {
    Private = 1,
    Locked = 2,
}

#[derive(Encode, Decode, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub(crate) enum LegacyCertificateStatus {
    Active = 1,
    Expired = 2,
}

#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub(crate) struct LegacyRightToken<AccountId> {
    pub version: BoundedVec<u8, ConstU32<MAX_VERSION_LEN>>,
    pub certificate_id: [u8; 32],
    pub token_id: u32,
    pub right_type: LegacyRightType,
    pub create_time: u64,
    pub valid_from: u64,
    pub valid_until: Option<u64>,
    pub owner: AccountId,
    pub issuer: AccountId,
    pub nonce: u32,
    pub parent_asset_id: [u8; 32],
    pub status: LegacyCertificateStatus,
    pub signature: BoundedVec<u8, ConstU32<MAX_SIGNATURE_LEN>>,
}

impl<AccountId> From<LegacyDataAsset<AccountId>> for DataAsset<AccountId> {
    fn from(legacy: LegacyDataAsset<AccountId>) -> Self {
        Self {
            version: legacy.version,
            core: AssetCore {
                asset_id: legacy.core.asset_id,
                token_id: legacy.core.token_id,
                owner: legacy.core.owner,
                raw_data_hash: legacy.core.raw_data_hash,
                timestamp: legacy.core.timestamp,
                nonce: legacy.core.nonce,
                state_version: u64::from(legacy.core.nonce).saturating_add(1),
                status: match legacy.core.status {
                    LegacyAssetStatus::Private => AssetStatus::Private,
                    LegacyAssetStatus::Locked => AssetStatus::Locked,
                },
                updated_at: legacy.core.updated_at,
            },
            metadata: legacy.metadata,
            characteristics: legacy.characteristics,
            statistics: legacy.statistics,
            encryption_info: legacy.encryption_info,
            signature: legacy.signature,
            pricing_config: legacy.pricing_config,
        }
    }
}

impl<AccountId> From<LegacyRightToken<AccountId>> for RightToken<AccountId> {
    fn from(legacy: LegacyRightToken<AccountId>) -> Self {
        Self {
            version: legacy.version,
            right_id: legacy.certificate_id,
            token_id: legacy.token_id,
            right_type: match legacy.right_type {
                LegacyRightType::Usage => RightType::LegacyUsage,
                LegacyRightType::Access => RightType::LegacyAccess,
            },
            data_scope: DataScope::FullAsset,
            validity: RightValidity {
                valid_from: legacy.valid_from,
                valid_until: legacy.valid_until,
                max_uses: None,
            },
            remaining_uses: None,
            created_at: legacy.create_time,
            holder: legacy.owner,
            issuer: legacy.issuer,
            asset_id: legacy.parent_asset_id,
            transferable: true,
            allowed_transfer_markets: BoundedVec::default(),
            royalty_bps: 0,
            state_version: u64::from(legacy.nonce).saturating_add(1),
            status: match legacy.status {
                LegacyCertificateStatus::Active => CertificateStatus::Active,
                LegacyCertificateStatus::Expired => CertificateStatus::Expired,
            },
            signature: legacy.signature,
        }
    }
}

fn decode_exact<Value: Decode>(raw: &[u8]) -> Option<Value> {
    let mut input = raw;
    let value = Value::decode(&mut input).ok()?;
    input.is_empty().then_some(value)
}

pub(crate) fn decode_data_asset_compat<AccountId: Decode>(
    raw: &[u8],
) -> Option<DataAsset<AccountId>> {
    decode_exact(raw).or_else(|| decode_exact::<LegacyDataAsset<AccountId>>(raw).map(Into::into))
}

pub(crate) fn decode_right_token_compat<AccountId: Decode>(
    raw: &[u8],
) -> Option<RightToken<AccountId>> {
    decode_exact(raw).or_else(|| decode_exact::<LegacyRightToken<AccountId>>(raw).map(Into::into))
}

// ---- Default implementations ----

impl<AccountId: Default> Default for AssetCore<AccountId> {
    fn default() -> Self {
        Self {
            asset_id: [0u8; 32],
            token_id: 0,
            owner: AccountId::default(),
            raw_data_hash: H256::zero(),
            timestamp: 0,
            nonce: 0,
            state_version: 1,
            status: AssetStatus::Private,
            updated_at: 0,
        }
    }
}

impl Default for AssetMetadata {
    fn default() -> Self {
        Self {
            name: BoundedVec::default(),
            description: BoundedVec::default(),
            quantity: 0,
            labels: BoundedVec::default(),
            metadata_cid: BoundedVec::default(),
            data_cid: BoundedVec::default(),
            data_size_bytes: 0,
        }
    }
}

impl Default for AssetCharacteristics {
    fn default() -> Self {
        Self {
            statistical_characteristic: BoundedVec::default(),
            analyzing_feature: BoundedVec::default(),
            integrity: BoundedVec::default(),
        }
    }
}

impl Default for AssetStatistics {
    fn default() -> Self {
        Self {
            view_count: 0,
            total_revenue: 0,
        }
    }
}

impl<AccountId: Default> Default for DataAsset<AccountId> {
    fn default() -> Self {
        Self {
            version: BoundedVec::default(),
            core: AssetCore::default(),
            metadata: AssetMetadata::default(),
            characteristics: AssetCharacteristics::default(),
            statistics: AssetStatistics::default(),
            encryption_info: EncryptionInfo::default(),
            signature: BoundedVec::default(),
            pricing_config: PricingConfig::default(),
        }
    }
}

impl<AccountId: Default> Default for RightToken<AccountId> {
    fn default() -> Self {
        Self {
            version: BoundedVec::default(),
            token_id: 0,
            right_id: [0u8; 32],
            right_type: RightType::LegacyUsage,
            data_scope: DataScope::FullAsset,
            validity: RightValidity {
                valid_from: 0,
                valid_until: None,
                max_uses: None,
            },
            remaining_uses: None,
            created_at: 0,
            holder: AccountId::default(),
            issuer: AccountId::default(),
            asset_id: [0u8; 32],
            transferable: false,
            allowed_transfer_markets: BoundedVec::default(),
            royalty_bps: 0,
            state_version: 1,
            status: CertificateStatus::Active,
            signature: BoundedVec::default(),
        }
    }
}

impl Default for EncryptionInfo {
    fn default() -> Self {
        Self {
            algorithm: BoundedVec::default(),
            key_length: 0,
            parameters_hash: H256::zero(),
            is_encrypted: false,
        }
    }
}

impl Default for PricingConfig {
    fn default() -> Self {
        Self {
            price_type: PriceType::Fixed,
            currency: BoundedVec::truncate_from(b"NATIVE".to_vec()),
            base_price: 0,
            usage_price: 0,
            access_price: 0,
        }
    }
}

// ---- Utility methods ----

impl<AccountId: Clone> DataAsset<AccountId> {
    /// Generate asset ID from owner, timestamp, and data hash
    pub fn generate_asset_id(owner: &AccountId, timestamp: u64, data_hash: &H256) -> [u8; 32]
    where
        AccountId: Encode,
    {
        use sp_io::hashing::blake2_256;

        let mut input = Vec::new();
        input.extend_from_slice(&owner.encode());
        input.extend_from_slice(&timestamp.to_le_bytes());
        input.extend_from_slice(data_hash.as_bytes());

        blake2_256(&input)
    }

    /// Check if asset is locked (status-based, no redundant bool)
    pub fn is_locked(&self) -> bool {
        matches!(
            self.core.status,
            AssetStatus::Locked
                | AssetStatus::Retiring
                | AssetStatus::Retired
                | AssetStatus::Rejected
                | AssetStatus::Challenged
                | AssetStatus::Invalidated
        )
    }

    /// Check if asset is active (not locked)
    pub fn is_active(&self) -> bool {
        !self.is_locked()
    }

    pub fn is_tradable(&self) -> bool {
        matches!(
            self.core.status,
            AssetStatus::Private | AssetStatus::Tradable
        )
    }
}

impl<AccountId: Clone> RightToken<AccountId> {
    /// Generate unique certificate ID
    pub fn generate_certificate_id(
        parent_asset_id: &[u8; 32],
        timestamp: u64,
        issuer: &AccountId,
        token_id: u32,
    ) -> [u8; 32]
    where
        AccountId: Encode,
    {
        use sp_io::hashing::blake2_256;

        let mut input = Vec::new();
        input.extend_from_slice(parent_asset_id);
        input.extend_from_slice(&timestamp.to_le_bytes());
        input.extend_from_slice(&issuer.encode());
        input.extend_from_slice(&token_id.to_le_bytes());

        blake2_256(&input)
    }

    /// Check if certificate is valid at current time
    pub fn is_valid(&self, current_time: u64) -> bool {
        self.status == CertificateStatus::Active
            && current_time >= self.validity.valid_from
            && self
                .validity
                .valid_until
                .map_or(true, |until| current_time <= until)
            && self.remaining_uses.map_or(true, |remaining| remaining > 0)
    }

    /// Check if certificate is expired
    pub fn is_expired(&self, current_time: u64) -> bool {
        self.validity
            .valid_until
            .map_or(false, |until| current_time > until)
    }

    pub fn allows_transfer_on(&self, market_id: &[u8; 32]) -> bool {
        self.transferable
            && self
                .allowed_transfer_markets
                .iter()
                .any(|allowed| allowed == market_id)
    }

    pub fn has_valid_parameters(&self) -> bool {
        let validity_range_is_well_formed = matches!(
            self.right_type,
            RightType::LegacyUsage | RightType::LegacyAccess
        ) || self
            .validity
            .valid_until
            .map_or(true, |until| until >= self.validity.valid_from);
        self.state_version > 0
            && self.royalty_bps <= 10_000
            && validity_range_is_well_formed
            && self.validity.max_uses.map_or(true, |max| max > 0)
            && match (self.validity.max_uses, self.remaining_uses) {
                (Some(max), Some(remaining)) => remaining <= max,
                (Some(_), None) | (None, Some(_)) => false,
                (None, None) => true,
            }
    }
}

// ---- Builder pattern constructors ----

impl<AccountId: Clone + Encode> DataAsset<AccountId> {
    /// Create a minimal DataAsset with only required fields
    pub fn minimal(
        owner: AccountId,
        name: Vec<u8>,
        description: Vec<u8>,
        raw_data_hash: H256,
        timestamp: u64,
    ) -> Self {
        let name = BoundedVec::try_from(name).unwrap_or_default();
        let description = BoundedVec::try_from(description).unwrap_or_default();

        Self {
            version: BoundedVec::truncate_from(b"1.0".to_vec()),
            core: AssetCore {
                asset_id: [0u8; 32],
                token_id: 0,
                owner,
                raw_data_hash,
                timestamp,
                nonce: 0,
                state_version: 1,
                status: AssetStatus::Tradable,
                updated_at: timestamp,
            },
            metadata: AssetMetadata {
                name,
                description,
                quantity: 0,
                labels: BoundedVec::default(),
                metadata_cid: BoundedVec::default(),
                data_cid: BoundedVec::default(),
                data_size_bytes: 0,
            },
            characteristics: AssetCharacteristics::default(),
            statistics: AssetStatistics::default(),
            encryption_info: EncryptionInfo::default(),
            signature: BoundedVec::default(),
            pricing_config: PricingConfig::default(),
        }
    }
}

impl<AccountId: Clone + Encode> RightToken<AccountId> {
    /// Create a minimal RightToken with only required fields
    pub fn minimal(
        token_id: u32,
        right_type: RightType,
        holder: AccountId,
        issuer: AccountId,
        parent_asset_id: [u8; 32],
        current_time: u64,
        valid_until: Option<u64>,
    ) -> Self {
        let certificate_id =
            Self::generate_certificate_id(&parent_asset_id, current_time, &issuer, token_id);

        Self {
            version: BoundedVec::truncate_from(b"1.0".to_vec()),
            token_id,
            right_id: certificate_id,
            right_type,
            data_scope: DataScope::FullAsset,
            validity: RightValidity {
                valid_from: current_time,
                valid_until,
                max_uses: None,
            },
            remaining_uses: None,
            created_at: current_time,
            holder,
            issuer,
            asset_id: parent_asset_id,
            transferable: true,
            allowed_transfer_markets: BoundedVec::default(),
            royalty_bps: 0,
            state_version: 1,
            status: CertificateStatus::Active,
            signature: BoundedVec::default(),
        }
    }
}

// ---- Field accessor methods ----

impl<AccountId> DataAsset<AccountId> {
    #[inline]
    pub fn asset_id(&self) -> [u8; 32] {
        self.core.asset_id
    }
}

impl<AccountId> RightToken<AccountId> {
    #[inline]
    pub fn certificate_id(&self) -> [u8; 32] {
        self.right_id
    }

    #[inline]
    pub fn parent_asset_id(&self) -> [u8; 32] {
        self.asset_id
    }
}
