#![cfg_attr(not(feature = "std"), no_std)]

use scale_info::TypeInfo;
use sp_core::H256;

pub trait ProtocolAccountIdConverter<AccountId> {
    fn to_protocol_account_id(account: &AccountId) -> Option<[u8; 32]>;

    fn from_protocol_account_id(account: &[u8; 32]) -> Option<AccountId>;
}

pub struct ScaleEncodedAccountId;

impl<AccountId: codec::Encode + codec::Decode> ProtocolAccountIdConverter<AccountId>
    for ScaleEncodedAccountId
{
    fn to_protocol_account_id(account: &AccountId) -> Option<[u8; 32]> {
        account.encode().try_into().ok()
    }

    fn from_protocol_account_id(account: &[u8; 32]) -> Option<AccountId> {
        let mut encoded = &account[..];
        let decoded = AccountId::decode(&mut encoded).ok()?;
        encoded.is_empty().then_some(decoded)
    }
}

#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    TypeInfo,
    codec::Encode,
    codec::Decode,
    codec::DecodeWithMemTracking,
    codec::MaxEncodedLen,
)]
pub enum MarketOperation {
    #[codec(index = 0)]
    TransferMeta,
    #[codec(index = 1)]
    IssueRight,
    #[codec(index = 2)]
    TransferRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarketAccessError {
    NotRegistered,
    Restricted,
    CapabilityDenied,
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MarketPaymentTerms<AccountId> {
    pub market_fee_bps: u16,
    pub market_fee_recipient: AccountId,
}

#[derive(
    Clone,
    Debug,
    PartialEq,
    Eq,
    TypeInfo,
    codec::Encode,
    codec::Decode,
    codec::DecodeWithMemTracking,
    codec::MaxEncodedLen,
)]
pub struct MarketStateSnapshot<AccountId> {
    pub market_id: [u8; 32],
    pub operator: AccountId,
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

/// Read-only market admission plus authorization-count bookkeeping.
pub trait MarketRegistryProvider<AccountId> {
    fn market_state(
        market_id: &[u8; 32],
    ) -> Result<MarketStateSnapshot<AccountId>, MarketAccessError>;

    fn ensure_market_operation(
        market_id: &[u8; 32],
        operation: MarketOperation,
    ) -> Result<(), MarketAccessError>;

    fn note_authorization_created(market_id: &[u8; 32]) -> Result<(), MarketAccessError>;

    fn note_authorization_closed(market_id: &[u8; 32]) -> Result<(), MarketAccessError>;

    fn note_active_order_created(market_id: &[u8; 32]) -> Result<(), MarketAccessError>;

    fn note_active_order_closed(market_id: &[u8; 32]) -> Result<(), MarketAccessError>;

    fn market_payment_terms(
        market_id: &[u8; 32],
    ) -> Result<MarketPaymentTerms<AccountId>, MarketAccessError>;

    fn ensure_funding_capacity(
        market_id: &[u8; 32],
        gross_price: u128,
    ) -> Result<(), MarketAccessError>;

    fn note_order_funded(market_id: &[u8; 32], gross_price: u128) -> Result<(), MarketAccessError>;

    fn note_funded_order_closed(
        market_id: &[u8; 32],
        gross_price: u128,
    ) -> Result<(), MarketAccessError>;
}

impl<AccountId> MarketRegistryProvider<AccountId> for () {
    fn market_state(
        _market_id: &[u8; 32],
    ) -> Result<MarketStateSnapshot<AccountId>, MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn ensure_market_operation(
        _market_id: &[u8; 32],
        _operation: MarketOperation,
    ) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn note_authorization_created(_market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn note_authorization_closed(_market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn note_active_order_created(_market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn note_active_order_closed(_market_id: &[u8; 32]) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn market_payment_terms(
        _market_id: &[u8; 32],
    ) -> Result<MarketPaymentTerms<AccountId>, MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn ensure_funding_capacity(
        _market_id: &[u8; 32],
        _gross_price: u128,
    ) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn note_order_funded(
        _market_id: &[u8; 32],
        _gross_price: u128,
    ) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }

    fn note_funded_order_closed(
        _market_id: &[u8; 32],
        _gross_price: u128,
    ) -> Result<(), MarketAccessError> {
        Err(MarketAccessError::NotRegistered)
    }
}

pub trait MarketContractVerifier<AccountId> {
    fn is_market(operator: &AccountId, contract_address: &AccountId) -> bool;
}

pub trait MarketStakeHandler<AccountId, Balance> {
    fn pledge(operator: &AccountId, amount: Balance) -> frame_support::dispatch::DispatchResult;

    fn unbond(operator: &AccountId) -> frame_support::dispatch::DispatchResult;
}

#[derive(Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub enum AssetQueryError {
    AssetNotFound,
    InvalidOwner,
    OwnerAccountDoesNotExist,
}

#[derive(
    codec::Encode,
    codec::Decode,
    Clone,
    PartialEq,
    Eq,
    Debug,
    TypeInfo,
    codec::DecodeWithMemTracking,
)]
pub struct EncryptionInfo {
    pub algorithm: Vec<u8>,
    pub key_length: u32,
    pub parameters_hash: H256,
    pub is_encrypted: bool,
}

use sp_std::prelude::*;

/// 激励处理器Trait - dataassets模块调用
pub trait IncentiveHandler<AccountId, AssetId, Balance> {
    /// 分发首次创建奖励
    fn distribute_first_create_reward(
        recipient: &AccountId,
        asset_id: &AssetId,
    ) -> Result<(), &'static str>;

    /// 登记资产交易（用于优质数据判定）
    fn register_asset_trade(asset_id: &AssetId);

    /// 分发流动性奖励
    fn distribute_liquidity_reward(
        recipient: &AccountId,
        order_amount: Balance,
    ) -> Result<(), &'static str>;

    /// 分发提案通过奖励
    fn distribute_proposal_reward(recipient: &AccountId) -> Result<(), &'static str>;
}

/// 数据资产提供者Trait - incentive模块调用
pub trait DataAssetProvider<AccountId, AssetId> {
    /// 获取资产信息，主要向incentive模块提供查询资产是否存在的功能
    fn get_asset_owner(asset_id: &AssetId) -> Result<AccountId, AssetQueryError>;
}

pub trait DataAssetInternal<AccountId, Balance> {
    fn register_asset(
        owner: AccountId,
        name: Vec<u8>,
        description: Vec<u8>,
        raw_data_hash: sp_core::H256,
        data_size: u64,
        metadata_cid: Vec<u8>,
        encryption_info: EncryptionInfo,
    ) -> frame_support::dispatch::DispatchResult;
}
