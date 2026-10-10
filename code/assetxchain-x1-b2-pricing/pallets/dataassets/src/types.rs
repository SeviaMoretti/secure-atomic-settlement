extern crate alloc;
use codec::{Encode, Decode, DecodeWithMemTracking, MaxEncodedLen};
use sp_std::vec::Vec;
use sp_core::H256;
use scale_info::TypeInfo;
use frame_support::{BoundedVec, traits::ConstU32};

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
	pub status: AssetStatus,
	pub updated_at: u64,
}

/// Descriptive metadata for a data asset
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct AssetMetadata {
	pub name: BoundedVec<u8, ConstU32<MAX_NAME_LEN>>,
	pub description: BoundedVec<u8, ConstU32<MAX_DESCRIPTION_LEN>>,
	pub quantity: u64,
	pub labels: BoundedVec<BoundedVec<u8, ConstU32<MAX_LABEL_LEN>>,
		ConstU32<MAX_LABELS_COUNT>>,
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
}// 这个数据更新频率较高，移动到主链-pallet中存储信息，可以添加更多的统计字段，如下载次数、活跃用户数等，避免频繁更新整个DataAsset结构体导致的性能问题

/// Trade asset category recorded in settlement evidence.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, Copy, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum TradeAssetType {
	DataAsset,
	Certificate,
}

/// Runtime trade settlement evidence for data asset market transactions.
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
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

/// 以 order_id 为主键的标准化结算记录
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
	TransferAsset,   // MetaToken 所有权转移
	IssueRight,      // RightToken 发行（新增使用权）
	TransferRight,   // RightToken 转让（使用权流转）
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
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub struct RightToken<AccountId> {
	pub version: BoundedVec<u8, ConstU32<MAX_VERSION_LEN>>,
	pub certificate_id: [u8; 32],
	pub token_id: u32,
	pub right_type: RightType,
	pub create_time: u64,
	pub valid_from: u64,
	pub valid_until: Option<u64>,
	pub owner: AccountId,
	pub issuer: AccountId,
	pub nonce: u32,
	pub parent_asset_id: [u8; 32],
	pub status: CertificateStatus,
	pub signature: BoundedVec<u8, ConstU32<MAX_SIGNATURE_LEN>>,
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
#[derive(Encode, Decode, DecodeWithMemTracking, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
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

/// Right Type Enumeration
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum RightType {
	Usage = 1,
	Access = 2,
}

/// Asset Status Enumeration
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum AssetStatus {
	Private = 1,
	Locked = 2,
}

/// Certificate Status Enumeration
#[derive(Encode, Decode, Clone, PartialEq, Eq, Debug, TypeInfo, MaxEncodedLen)]
pub enum CertificateStatus {
	Active = 1,
	Expired = 2,
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
			certificate_id: [0u8; 32],
			right_type: RightType::Usage,
			create_time: 0,
			valid_from: 0,
			valid_until: None,
			owner: AccountId::default(),
			issuer: AccountId::default(),
			nonce: 0,
			parent_asset_id: [0u8; 32],
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
		self.core.status == AssetStatus::Locked
	}

	/// Check if asset is active (not locked)
	pub fn is_active(&self) -> bool {
		!self.is_locked()
	}
}

impl<AccountId: Clone> RightToken<AccountId> {
	/// Generate unique certificate ID
	pub fn generate_certificate_id(parent_asset_id: &[u8; 32], timestamp: u64, issuer: &AccountId, token_id: u32) -> [u8; 32]
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
		self.status == CertificateStatus::Active &&
		current_time >= self.valid_from &&
		self.valid_until.map_or(true, |until| current_time <= until)
	}

	/// Check if certificate is expired
	pub fn is_expired(&self, current_time: u64) -> bool {
		self.valid_until.map_or(false, |until| current_time > until)
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
				status: AssetStatus::Private,
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
		valid_until: Option<u64>
	) -> Self {
		let certificate_id = Self::generate_certificate_id(&parent_asset_id, current_time, &issuer, token_id);

		Self {
			version: BoundedVec::truncate_from(b"1.0".to_vec()),
			token_id,
			certificate_id,
			right_type,
			create_time: current_time,
			valid_from: current_time,
			valid_until,
			owner: holder,
			issuer,
			nonce: 0,
			parent_asset_id,
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
		self.certificate_id
	}

	#[inline]
	pub fn parent_asset_id(&self) -> [u8; 32] {
		self.parent_asset_id
	}
}
