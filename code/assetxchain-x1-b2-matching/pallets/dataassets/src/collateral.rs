/// Collateral Management Module
/// 
/// This module handles all collateral-related operations for data assets:
/// - Calculate collateral amounts based on data size
/// - Lock collateral when assets are registered
/// - Manage phased release schedules (50% @ 24h, 30% @ 30d, 20% @ 90d)
/// - Check release conditions before unlocking funds
/// - Handle collateral slashing for violations

use super::*;
use frame_support::{
    BoundedVec,
    traits::{Currency, ReservableCurrency, Get, ConstU32},
    ensure,
    pallet_prelude::DispatchResult,
};
use frame_system::pallet_prelude::BlockNumberFor;
use sp_runtime::traits::{Zero, Saturating, SaturatedConversion, CheckedDiv};
use frame_support::weights::Weight;
use crate::types::*;
use alloc::vec;

/// Type alias for Balance
pub type BalanceOf<T> = <<T as Config>::Currency as Currency<<T as frame_system::Config>::AccountId>>::Balance;

impl<T: Config> Pallet<T> {
    /// Calculate required collateral based on data size
    /// 
    /// Formula: BaseCollateral + (data_size_mb * CollateralPerMB)
    /// Maximum: MaxCollateral
    /// 
    /// # Arguments
    /// * `data_size_bytes` - Size of data in bytes
    /// 
    /// # Returns
    /// * Calculated collateral amount (capped at MaxCollateral), capped flag
    pub fn calculate_collateral(data_size_bytes: u64) -> (BalanceOf<T>, bool) {
        // Convert bytes to MB (minimum 1 MB)
        let data_size_mb = ((data_size_bytes as u128) / (1024 * 1024)).max(1);
        
        // Calculate variable collateral based on data size
        let variable_collateral = T::CollateralPerMB::get()
            .saturating_mul(data_size_mb.saturated_into());
            
        // Total collateral = base + variable
        let total_uncapped = T::BaseCollateral::get()
            .saturating_add(variable_collateral);
        
        let max_collateral = T::MaxCollateral::get();
        // 最终结果：取base+variable与MaxCollateral的较小值
        let total_capped = total_uncapped.min(max_collateral);
        // 是否超过MaxCollateral
        let is_over_capped = total_uncapped > max_collateral;

        (total_capped, is_over_capped)
    }
    
    /// Create a phased release schedule for collateral
    /// 
    /// Phase 1: 50% after 24 hours (+ verification)
    /// Phase 2: 30% after 30 days (+ usage)
    /// Phase 3: 20% after 90 days (+ availability)
    /// 
    /// # Arguments
    /// * `total_amount` - Total collateral amount
    /// * `start_block` - Block number when asset is registered
    pub fn create_release_schedule(
        total_amount: BalanceOf<T>,
        start_block: BlockNumberFor<T>,
    ) -> BoundedVec<ReleasePhase<BlockNumberFor<T>, BalanceOf<T>>, ConstU32<5>> {
        use sp_runtime::traits::CheckedDiv;
        
        // Phase 1: 50% of total
        let hundred: BalanceOf<T> = 100u32.into();
        let phase1_amount = total_amount
            .saturating_mul(50u32.into())
            .checked_div(&hundred)
            .unwrap_or_else(Zero::zero);

        // Phase 2: 30% of total
        let phase2_amount = total_amount
            .saturating_mul(30u32.into())
            .checked_div(&hundred)
            .unwrap_or_else(Zero::zero);

        // Phase 3: Remainder (20% of total, handles rounding)
        let phase3_amount = total_amount
            .saturating_sub(phase1_amount)
            .saturating_sub(phase2_amount);
        
        let phases_vec = vec![
            // Phase 1: 50% after 24 hours (with verification)
            // 注意：泛型参数顺序是 <BlockNumber, Balance>
            ReleasePhase {
                percentage: 50,
                amount: phase1_amount,
                unlock_block: start_block.saturating_add(Self::blocks_in_hours(24)),
                condition: ReleaseCondition::TimeAndVerification,
                is_released: false,
            },
            // Phase 2: 30% after 30 days (with usage)
            ReleasePhase {
                percentage: 30,
                amount: phase2_amount,
                unlock_block: start_block.saturating_add(Self::blocks_in_days(30)),
                condition: ReleaseCondition::TimeAndUsage,
                is_released: false,
            },
            // Phase 3: 20% after 90 days (with availability)
            ReleasePhase {
                percentage: 20,
                amount: phase3_amount,
                unlock_block: start_block.saturating_add(Self::blocks_in_days(90)),
                condition: ReleaseCondition::TimeAndAvailability,
                is_released: false,
            },
        ];
        BoundedVec::try_from(phases_vec)
            .expect("Phases count exceeds MaxReleasePhases; qed")
    }
    
    /// Lock collateral for a new asset
    /// 
    /// # Arguments
    /// * `asset_id` - The asset's unique identifier
    /// * `who` - Account that will have collateral locked
    /// * `data_size_bytes` - Size of the asset's data
    pub fn lock_collateral(
        asset_id: &[u8; 32],
        who: &T::AccountId,
        collateral_amount: BalanceOf<T>,
    ) -> DispatchResult { 
        // 从who的余额中扣除collateral_amount，如果余额不足则提示错误
        T::Currency::reserve(who, collateral_amount)
            .map_err(|_| Error::<T>::InsufficientBalance)?;
        
        // Get current block for schedule
        let current_block = frame_system::Pallet::<T>::block_number();
        
        // Create release schedule
        let release_schedule = Self::create_release_schedule(collateral_amount, current_block);

        // Enqueue asset in the release agenda for each phase's unlock block
        // Must be done before release_schedule is moved into CollateralInfo
        for phase in &release_schedule {
            CollateralReleaseAgenda::<T>::mutate(phase.unlock_block, |assets| {
                let _ = assets.try_push(*asset_id);
            });
        }

        // Store collateral info
        let collateral_info = CollateralInfo {
            depositor: who.clone(),
            total_amount: collateral_amount,
            reserved_amount: collateral_amount,
            released_amount: Zero::zero(),
            release_schedule,
            status: CollateralStatus::FullyLocked,
        };

        AssetCollateral::<T>::insert(asset_id, collateral_info);

        // Emit event
        Self::deposit_event(Event::CollateralLocked {
            asset_id: *asset_id,
            depositor: who.clone(),
            amount: collateral_amount,
        });
        
        Ok(())
    }
    
    /// Process collateral releases due at the current block (called in on_initialize).
    /// Uses an agenda indexed by unlock block to avoid full iteration over all assets.
    ///
    /// # Arguments
    /// * `current_block` - Current block number
    ///
    /// # Returns
    /// * Weight consumed by this operation
    pub fn process_releases_at_block(current_block: BlockNumberFor<T>) -> Weight {
        let mut weight = T::DbWeight::get().reads(1);

        // Take the agenda for this block (clears storage, avoids re-processing)
        let due_assets = CollateralReleaseAgenda::<T>::take(current_block);
        if due_assets.is_empty() {
            return weight;
        }

        for asset_id in due_assets.iter() {
            weight = weight.saturating_add(T::DbWeight::get().reads(1));

            if let Some(mut collateral_info) = AssetCollateral::<T>::get(asset_id) {
                let mut updated = false;
                let mut has_pending = false;

                for phase in collateral_info.release_schedule.iter_mut() {
                    if phase.is_released {
                        continue;
                    }
                    if current_block < phase.unlock_block {
                        has_pending = true;
                        continue;
                    }

                    if Self::check_release_condition(asset_id, &phase.condition) {
                        let unreserved = T::Currency::unreserve(
                            &collateral_info.depositor,
                            phase.amount,
                        );
                        if unreserved == phase.amount {
                            phase.is_released = true;
                            collateral_info.released_amount =
                                collateral_info.released_amount.saturating_add(phase.amount);
                            collateral_info.reserved_amount =
                                collateral_info.reserved_amount.saturating_sub(phase.amount);
                            updated = true;

                            Self::deposit_event(Event::CollateralReleased {
                                asset_id: *asset_id,
                                amount: phase.amount,
                                phase: phase.percentage,
                            });
                            weight = weight.saturating_add(T::DbWeight::get().writes(1));
                        }
                    } else {
                        // Condition not yet met, re-enqueue for retry next block
                        has_pending = true;
                    }
                }

                if updated {
                    if collateral_info.reserved_amount.is_zero() {
                        collateral_info.status = CollateralStatus::FullyReleased;
                    } else {
                        collateral_info.status = CollateralStatus::PartiallyReleased;
                    }
                    AssetCollateral::<T>::insert(asset_id, collateral_info);
                    weight = weight.saturating_add(T::DbWeight::get().writes(1));
                }

                // Re-enqueue if any phase is still pending (failed condition check)
                if has_pending {
                    let next_block = current_block.saturating_add(1u32.into());
                    CollateralReleaseAgenda::<T>::mutate(next_block, |assets| {
                        let _ = assets.try_push(*asset_id);
                    });
                }
            }
        }

        weight
    }
    
    /// Check if release condition is satisfied
    /// 
    /// # Arguments
    /// * `asset_id` - The asset's unique identifier
    /// * `condition` - The condition to check
    fn check_release_condition(asset_id: &[u8; 32], condition: &ReleaseCondition) -> bool {
        match condition {
            ReleaseCondition::TimeOnly => {
                // No additional conditions
                true
            }
            ReleaseCondition::TimeAndVerification => {
                // Check if asset has been verified
                // For now, we assume verification is automatic after 24h
                // In production, this should check actual verification status
                if let Some(_asset) = Self::get_asset(asset_id) {
                    // TODO: 实际应检查验证状态
                    // 当前默认通过
                    true
                } else {
                    false
                }
            }
            ReleaseCondition::TimeAndUsage => {
                // Check if asset has at least one certificate issued
                if let Some(asset) = Self::get_asset(asset_id) {
                    // Check view count or transaction count as proxy for usage
                    asset.statistics.view_count > 0 || asset.core.nonce > 0
                } else {
                    false
                }
            }
            ReleaseCondition::TimeAndAvailability => {
                // Check if IPFS data is continuously available
                // This should be verified by off-chain workers
                // For now, we assume availability if asset exists
                if let Some(_asset) = Self::get_asset(asset_id) {
                    // TODO: 应该检查 IPFS 数据是否可访问
                    // This will require off-chain worker integration
                    true
                } else {
                    false
                }
            }
        }
    }
    
    /// Slash collateral due to violation
    /// 
    /// # Arguments
    /// * `asset_id` - The asset's unique identifier
    /// * `slash_percentage` - Percentage to slash (0-100)
    pub fn slash_collateral(
        asset_id: &[u8; 32],
        slash_percentage: u8,
    ) -> DispatchResult {
        ensure!(slash_percentage <= 100, Error::<T>::InvalidSlashPercentage);
        
        let mut collateral_info = AssetCollateral::<T>::get(asset_id)
            .ok_or(Error::<T>::CollateralNotFound)?;
        
        // Calculate slash amount from reserved collateral
        let hundred: BalanceOf<T> = 100u32.into();
        let slash_amount = collateral_info.reserved_amount
            .saturating_mul(slash_percentage.into())
            .checked_div(&hundred)  // 使用 checked_div 而不是 saturating_div
            .unwrap_or_else(Zero::zero);
        
        // Slash the reserved collateral
        // slash_reserved 返回 (NegativeImbalance, Balance)
        let (slashed_imbalance, remaining) = T::Currency::slash_reserved(
            &collateral_info.depositor, 
            slash_amount
        );
        
        // 从 NegativeImbalance 中提取实际被 slash 的金额
        // 实际 slashed 的金额 = 请求的金额 - 剩余未 slash 的金额
        let actual_slashed = slash_amount.saturating_sub(remaining);
        
        // 销毁 NegativeImbalance (这会从总供应量中移除这些代币)
        drop(slashed_imbalance);
        
        // Update collateral info
        collateral_info.reserved_amount = collateral_info.reserved_amount.saturating_sub(actual_slashed);
        collateral_info.status = CollateralStatus::Slashed(actual_slashed);
        
        AssetCollateral::<T>::insert(asset_id, collateral_info);
        
        // Emit event
        Self::deposit_event(Event::CollateralSlashed {
            asset_id: *asset_id,
            amount: actual_slashed,
            percentage: slash_percentage,
        });
        
        Ok(())
    }
    
    /// Calculate blocks in hours based on block time
    fn blocks_in_hours(hours: u32) -> BlockNumberFor<T> {
        // MILLI_SECS_PER_BLOCK is defined in your runtime (e.g., 18000ms = 18s)
        // Assuming 18s per block: 3600s / 18s = 200 blocks per hour
        let blocks_per_hour: u32 = 3600 / (crate::MILLI_SECS_PER_BLOCK / 1000) as u32;
        (blocks_per_hour.saturating_mul(hours)).into()
    }
    
    /// Calculate blocks in days
    fn blocks_in_days(days: u32) -> BlockNumberFor<T> {
        Self::blocks_in_hours(days.saturating_mul(24))
    }
    
    /// Get collateral info for an asset
    pub fn get_collateral_info(asset_id: &[u8; 32]) -> Option<CollateralInfo<T::AccountId, BalanceOf<T>, BlockNumberFor<T>>> {
        AssetCollateral::<T>::get(asset_id)
    }
}