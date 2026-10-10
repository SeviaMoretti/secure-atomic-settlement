#![cfg_attr(not(feature = "std"), no_std, no_main)]

/// OS-Legacy market: models the pre-fix (attack-era) marketplace semantics of [7].
///
/// - Listing book (orderbook: price / expiry / cancel) lives OFF-CHAIN (driven by the
///   probe script); the contract never reads listing state.
/// - On-chain approval (ERC-721-style `approve`) is long-lived until explicitly revoked.
/// - `settle` validates ONLY on-chain ownership + un-revoked approval, then transfers
///   the asset to the payer. It does NOT check listing expiry or listing cancellation.
#[ink::contract]
mod os_legacy_market {
    use ink::storage::Mapping;

    /// approval record: (seller, approved_until_block) — `u32::MAX` means open-ended.
    #[derive(Debug, Clone, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(
        feature = "std",
        derive(scale_info::TypeInfo, ink::storage::traits::StorageLayout)
    )]
    pub struct Approval {
        pub seller: AccountId,
        pub approved_until: BlockNumber,
    }

    #[ink(storage)]
    pub struct OsLegacyMarket {
        owners: Mapping<Hash, AccountId>,
        approvals: Mapping<Hash, Approval>,
    }

    #[derive(Debug, PartialEq, Eq, scale::Encode, scale::Decode)]
    #[cfg_attr(feature = "std", derive(scale_info::TypeInfo))]
    pub enum Error {
        /// asset already registered
        AssetExists,
        /// caller is not the asset owner
        NotOwner,
        /// no on-chain approval for the asset
        NoApproval,
        /// approval was revoked by the seller
        ApprovalRevoked,
        /// paid amount does not match the agreed price
        WrongPayment,
        /// transfer to seller failed
        TransferFailed,
        /// caller is not the approved seller
        NotApprovedSeller,
    }

    #[ink(event)]
    pub struct AssetRegistered {
        #[ink(topic)]
        asset: Hash,
        #[ink(topic)]
        owner: AccountId,
    }

    #[ink(event)]
    pub struct Approved {
        #[ink(topic)]
        asset: Hash,
        #[ink(topic)]
        seller: AccountId,
        approved_until: BlockNumber,
    }

    #[ink(event)]
    pub struct ApprovalRevoked {
        #[ink(topic)]
        asset: Hash,
        #[ink(topic)]
        seller: AccountId,
    }

    #[ink(event)]
    pub struct Settled {
        #[ink(topic)]
        asset: Hash,
        #[ink(topic)]
        seller: AccountId,
        #[ink(topic)]
        buyer: AccountId,
        price: Balance,
    }

    impl OsLegacyMarket {
        #[ink(constructor)]
        pub fn new() -> Self {
            Self {
                owners: Mapping::default(),
                approvals: Mapping::default(),
            }
        }

        /// Register an asset; the caller becomes its owner.
        /// Listing (price / expiry / cancel) is maintained off-chain by the market index.
        #[ink(message)]
        pub fn register_asset(&mut self, asset: Hash) -> Result<(), Error> {
            if self.owners.contains(asset) {
                return Err(Error::AssetExists);
            }
            let caller = self.env().caller();
            self.owners.insert(asset, &caller);
            self.env().emit_event(AssetRegistered { asset, owner: caller });
            Ok(())
        }

        /// Seller approves the market to transfer `asset`; open-ended until revoked
        /// (models the OpenSea-era on-chain approval that stays active while the
        /// user does not pay the cancellation fee to revoke it).
        #[ink(message)]
        pub fn approve(&mut self, asset: Hash) -> Result<(), Error> {
            let caller = self.env().caller();
            if self.owners.get(asset) != Some(caller) {
                return Err(Error::NotOwner);
            }
            let approval = Approval {
                seller: caller,
                approved_until: u32::MAX,
            };
            self.approvals.insert(asset, &approval);
            self.env()
                .emit_event(Approved { asset, seller: caller, approved_until: u32::MAX });
            Ok(())
        }

        /// Seller revokes the on-chain approval explicitly.
        #[ink(message)]
        pub fn revoke(&mut self, asset: Hash) -> Result<(), Error> {
            let caller = self.env().caller();
            match self.approvals.get(asset) {
                Some(approval) if approval.seller == caller => {
                    self.approvals.remove(asset);
                    self.env().emit_event(ApprovalRevoked { asset, seller: caller });
                    Ok(())
                }
                Some(_) => Err(Error::NotOwner),
                None => Err(Error::NoApproval),
            }
        }

        /// Settlement: validates ONLY on-chain ownership and un-revoked approval,
        /// then transfers the asset to the paying buyer.
        ///
        /// NOTE: listing state (expiry / cancellation) is NOT checked — this is the
        /// attack-era behaviour reproduced by the OS probe: a stale listing
        /// (expired or cancelled off-chain) can still be executed on-chain.
        #[ink(message, payable)]
        pub fn settle(
            &mut self,
            asset: Hash,
            seller: AccountId,
            price: Balance,
        ) -> Result<(), Error> {
            let buyer = self.env().caller();
            if self.owners.get(asset) != Some(seller) {
                return Err(Error::NotOwner);
            }
            let approval = self.approvals.get(asset).ok_or(Error::NoApproval)?;
            if approval.seller != seller {
                return Err(Error::NotApprovedSeller);
            }
            let now = self.env().block_number();
            if now > approval.approved_until {
                return Err(Error::ApprovalRevoked);
            }
            let paid = self.env().transferred_value();
            if paid != price {
                return Err(Error::WrongPayment);
            }
            if self.env().transfer(seller, price).is_err() {
                return Err(Error::TransferFailed);
            }
            self.owners.insert(asset, &buyer);
            self.env().emit_event(Settled { asset, seller, buyer, price });
            Ok(())
        }

        #[ink(message)]
        pub fn owner_of(&self, asset: Hash) -> Option<AccountId> {
            self.owners.get(asset)
        }

        #[ink(message)]
        pub fn approval_of(&self, asset: Hash) -> Option<Approval> {
            self.approvals.get(asset)
        }
    }
}
