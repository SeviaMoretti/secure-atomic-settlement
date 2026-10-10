#![cfg_attr(not(feature = "std"), no_std)]

use scale_info::TypeInfo;
use sp_core::H256;

#[derive(Debug, PartialEq, Eq, codec::Encode, codec::Decode)]
pub enum AssetQueryError {
    AssetNotFound,
    InvalidOwner,
    OwnerAccountDoesNotExist,
}

#[derive(codec::Encode, codec::Decode, Clone, PartialEq, Eq, Debug, TypeInfo, codec::DecodeWithMemTracking)]
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
    fn distribute_first_create_reward(recipient: &AccountId, asset_id: &AssetId) -> Result<(), &'static str>;
    
    /// 登记资产交易（用于优质数据判定）
    fn register_asset_trade(asset_id: &AssetId);
    
    /// 分发流动性奖励
    fn distribute_liquidity_reward(recipient: &AccountId, order_amount: Balance) -> Result<(), &'static str>;
    
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