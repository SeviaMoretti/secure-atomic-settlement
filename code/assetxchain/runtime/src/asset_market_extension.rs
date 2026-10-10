// 数据资产扩展模块
// 与pallet-contracts交互，实现数据资产扩展，市场交易成功后，更新数据资产的状态

use log;
use pallet_contracts::chain_extension::{
    ChainExtension, Environment, Ext, InitState, RetVal, SysConfig,
};
use sp_core::crypto::UncheckedFrom;
use sp_runtime::DispatchError;

// 定义 Function IDs
const TRANSFER_ASSET_FUNC_ID: u16 = 1;
const TRANSFER_CERT_FUNC_ID: u16 = 2;
const ISSUE_CERT_FUNC_ID: u16 = 3;
const SETTLE_ASSET_TRADE_FUNC_ID: u16 = 4;
const SETTLE_CERT_TRADE_FUNC_ID: u16 = 5;
const CREATE_ORDER_PROJECTION_FUNC_ID: u16 = 6;
const LOCK_ORDER_FUNC_ID: u16 = 7;
const UPDATE_ORDER_STATUS_FUNC_ID: u16 = 8;
const TRANSFER_FAILED_STATUS: u32 = 1;
const ASSET_NOT_FOUND_STATUS: u32 = 2;
const PERMISSION_DENIED_STATUS: u32 = 3;
const CERTIFICATE_NOT_FOUND_STATUS: u32 = 4;
const CERTIFICATE_NOT_ACTIVE_STATUS: u32 = 5;

fn dataassets_error_status<T: pallet_dataassets::Config>(error: DispatchError) -> u32 {
    if error == pallet_dataassets::Error::<T>::AssetNotFound.into() {
        ASSET_NOT_FOUND_STATUS
    } else if error == pallet_dataassets::Error::<T>::CertificateNotFound.into() {
        CERTIFICATE_NOT_FOUND_STATUS
    } else if error == pallet_dataassets::Error::<T>::CertificateNotActive.into() {
        CERTIFICATE_NOT_ACTIVE_STATUS
    } else if error == pallet_dataassets::Error::<T>::NotOwner.into()
        || error == pallet_dataassets::Error::<T>::NotAuthorized.into()
    {
        PERMISSION_DENIED_STATUS
    } else {
        TRANSFER_FAILED_STATUS
    }
}

#[derive(Default)]
pub struct DataAssetsExtension;

impl<T> ChainExtension<T> for DataAssetsExtension
where
    // T 必须配置了 pallet_contracts 和 pallet_dataassets
    T: pallet_contracts::Config + pallet_dataassets::Config,
    // 确保 AccountId 可以从 Hash 转换 (这是 pallet-contracts 要求的)
    <T as SysConfig>::AccountId: UncheckedFrom<<T as SysConfig>::Hash> + AsRef<[u8]>,
{
    fn call<E: Ext>(&mut self, env: Environment<E, InitState>) -> Result<RetVal, DispatchError>
    where
        E: Ext<T = T>,
    {
        let func_id = env.func_id();

        match func_id {
            // 交易元证
            TRANSFER_ASSET_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling TRANSFER_ASSET_FUNC_ID");
                let mut env = env.buf_in_buf_out();

                // 1. 读取输入 (AssetId: [u8; 32], To AccountId)，！！！！应该增加一个price
                // 之后调用 Incentive 模块：登记交易者（买家）月度交易额以及登记市场月度交易额
                let (asset_id_bytes, to_account): ([u8; 32], T::AccountId) = env.read_as()?;

                // 2. 获取调用合约的地址 (Contract Address)
                // 合约地址就是资产转移中的 Operator/Market
                let caller_account = env.ext().address().clone();

                // 3. 调用 pallet-dataassets 的内部函数
                // Runtime 会检查 caller_account (合约) 是否被授权
                pallet_dataassets::Pallet::<T>::transfer_by_market_internal(
                    &asset_id_bytes,
                    &caller_account,
                    &to_account,
                )?;

                // 4. 返回成功代码 0
                Ok(RetVal::Converging(0))
            }

            // 交易权证
            TRANSFER_CERT_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling TRANSFER_CERT_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let (asset_id, certificate_id, to_account): ([u8; 32], [u8; 32], T::AccountId) =
                    env.read_as()?;
                let caller_account = env.ext().address().clone();

                if pallet_dataassets::Pallet::<T>::get_certificate(&asset_id, &certificate_id)
                    .is_none()
                {
                    return Ok(RetVal::Converging(CERTIFICATE_NOT_FOUND_STATUS));
                }

                match pallet_dataassets::Pallet::<T>::transfer_certificate_internal(
                    &asset_id,
                    &certificate_id,
                    &caller_account,
                    &to_account,
                ) {
                    Ok(()) => Ok(RetVal::Converging(0)),
                    Err(error) => Ok(RetVal::Converging(dataassets_error_status::<T>(error))),
                }
            }
            ISSUE_CERT_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling ISSUE_CERT_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let (asset_id, issuer, holder, right_type, valid_until): (
                    [u8; 32],
                    T::AccountId,
                    T::AccountId,
                    u8,
                    Option<u64>,
                ) = env.read_as()?;
                let caller_account = env.ext().address().clone();

                match pallet_dataassets::Pallet::<T>::issue_certificate_by_market_internal(
                    &asset_id,
                    &caller_account,
                    &issuer,
                    &holder,
                    right_type,
                    valid_until,
                ) {
                    Ok(_) => Ok(RetVal::Converging(0)),
                    Err(error) => Ok(RetVal::Converging(dataassets_error_status::<T>(error))),
                }
            }
            SETTLE_ASSET_TRADE_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling SETTLE_ASSET_TRADE_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let (asset_id, to_account, price, order_id, order_digest): (
                    [u8; 32],
                    T::AccountId,
                    pallet_dataassets::BalanceOf<T>,
                    [u8; 32],
                    [u8; 32],
                ) = env.read_as()?;
                let caller_account = env.ext().address().clone();

                match pallet_dataassets::Pallet::<T>::settle_asset_trade_by_market_internal(
                    &asset_id,
                    &caller_account,
                    &to_account,
                    price,
                    &order_id,
                    &order_digest,
                ) {
                    Ok(_) => Ok(RetVal::Converging(0)),
                    Err(error) => Ok(RetVal::Converging(dataassets_error_status::<T>(error))),
                }
            }
            SETTLE_CERT_TRADE_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling SETTLE_CERT_TRADE_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let (asset_id, certificate_id, to_account, price, order_id, order_digest): (
                    [u8; 32],
                    [u8; 32],
                    T::AccountId,
                    pallet_dataassets::BalanceOf<T>,
                    [u8; 32],
                    [u8; 32],
                ) = env.read_as()?;
                let caller_account = env.ext().address().clone();

                match pallet_dataassets::Pallet::<T>::settle_certificate_trade_internal(
                    &asset_id,
                    &certificate_id,
                    &caller_account,
                    &to_account,
                    price,
                    &order_id,
                    &order_digest,
                ) {
                    Ok(_) => Ok(RetVal::Converging(0)),
                    Err(error) => Ok(RetVal::Converging(dataassets_error_status::<T>(error))),
                }
            }
            CREATE_ORDER_PROJECTION_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling CREATE_ORDER_PROJECTION_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let (order_id, order_digest, object_type, object_id, parent_asset_id, seller, price): (
                    [u8; 32],
                    [u8; 32],
                    pallet_dataassets::types::TradeAssetType,
                    [u8; 32],
                    Option<[u8; 32]>,
                    T::AccountId,
                    pallet_dataassets::BalanceOf<T>,
                ) = env.read_as()?;
                let caller = env.ext().address().clone();

                let order = pallet_dataassets::types::MarketOrder {
                    order_id,
                    order_digest,
                    market: caller,
                    seller,
                    buyer: None,
                    object_type,
                    object_id,
                    parent_asset_id,
                    price,
                    status: pallet_dataassets::types::MarketOrderStatus::Open,
                    created_at: frame_system::Pallet::<T>::block_number(),
                };
                pallet_dataassets::MarketOrders::<T>::insert(order_id, order);
                Ok(RetVal::Converging(0))
            }
            LOCK_ORDER_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling LOCK_ORDER_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let order_id: [u8; 32] = env.read_as()?;
                let caller = env.ext().address().clone();

                pallet_dataassets::MarketOrders::<T>::try_mutate(&order_id, |maybe_order| -> Result<(), DispatchError> {
                    let order = maybe_order.as_mut().ok_or(DispatchError::Other("OrderNotFound"))?;
                    if order.status != pallet_dataassets::types::MarketOrderStatus::Open {
                        return Err(DispatchError::Other("OrderNotOpen"));
                    }
                    order.status = pallet_dataassets::types::MarketOrderStatus::Locked;
                    order.buyer = Some(caller);
                    Ok(())
                })?;
                Ok(RetVal::Converging(0))
            }
            UPDATE_ORDER_STATUS_FUNC_ID => {
                log::debug!(target: "runtime", "DataAssetsExtension: Calling UPDATE_ORDER_STATUS_FUNC_ID");
                let mut env = env.buf_in_buf_out();
                let (order_id, new_status): ([u8; 32], pallet_dataassets::types::MarketOrderStatus) =
                    env.read_as()?;

                pallet_dataassets::MarketOrders::<T>::try_mutate(&order_id, |maybe_order| -> Result<(), DispatchError> {
                    let order = maybe_order.as_mut().ok_or(DispatchError::Other("OrderNotFound"))?;
                    order.status = new_status;
                    Ok(())
                })?;
                Ok(RetVal::Converging(0))
            }
            _ => Err(DispatchError::Other("Unregistered function")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn certificate_transfer_status_codes_are_explicit_errors() {
        assert_ne!(CERTIFICATE_NOT_FOUND_STATUS, 0);
        assert_ne!(CERTIFICATE_NOT_ACTIVE_STATUS, 0);
        assert_ne!(TRANSFER_FAILED_STATUS, CERTIFICATE_NOT_FOUND_STATUS);
        assert_ne!(CERTIFICATE_NOT_FOUND_STATUS, CERTIFICATE_NOT_ACTIVE_STATUS);
    }

    #[test]
    fn issue_certificate_function_id_is_3() {
        assert_eq!(ISSUE_CERT_FUNC_ID, 3);
        assert_ne!(ISSUE_CERT_FUNC_ID, TRANSFER_ASSET_FUNC_ID);
        assert_ne!(ISSUE_CERT_FUNC_ID, TRANSFER_CERT_FUNC_ID);
    }

    #[test]
    fn settlement_function_ids_are_distinct_from_transfer_ids() {
        assert_eq!(SETTLE_ASSET_TRADE_FUNC_ID, 4);
        assert_eq!(SETTLE_CERT_TRADE_FUNC_ID, 5);
        assert_ne!(SETTLE_ASSET_TRADE_FUNC_ID, TRANSFER_ASSET_FUNC_ID);
        assert_ne!(SETTLE_CERT_TRADE_FUNC_ID, TRANSFER_CERT_FUNC_ID);
    }
}
