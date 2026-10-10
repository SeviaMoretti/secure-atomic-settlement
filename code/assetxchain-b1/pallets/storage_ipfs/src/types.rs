extern crate alloc;

use codec::{Decode, Encode, MaxEncodedLen};
use frame_support::{traits::ConstU32, BoundedVec};
use scale_info::TypeInfo;
use sp_runtime::RuntimeDebug;

const MAX_CID_LEN: u32 = 128;

// 存储订单状态
#[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub enum StorageStatus {
    Unfunded,   // 未提供资金
    PendingXCM, // 跨链请求已发送，等待对方确认
    Active,     // 存储链已确认提供可用性证明（PoSt 正常运行中）
    Expired,    // 存储订单已到期
    DataLost,   // 存储链通报数据完全丢失（矿工被大面积 Slash）
}

// 跨链存储订单信息
#[derive(Clone, Encode, Decode, PartialEq, Eq, RuntimeDebug, TypeInfo, MaxEncodedLen)]
pub struct StorageOrder<Balance, BlockNumber> {
    pub cid: BoundedVec<u8, ConstU32<MAX_CID_LEN>>, // IPFS CID
    pub size: u64,                                  // 数据大小 (Bytes)
    pub status: StorageStatus,                      // 跨链存储状态
    pub paid_fee: Balance,                          // 为此订单支付的跨链费用
    pub ordered_at: BlockNumber,                    // 下单高度
    pub valid_until: BlockNumber,                   // 预估在主链视角的过期高度
}
