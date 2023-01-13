// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use std::{
    collections::{hash_map::Entry, HashMap},
    sync::Arc,
};

use cfx_bytes::Bytes;
use cfx_internal_common::debug::ComputeEpochDebugRecord;
use cfx_parameters::internal_contract_addresses::SYSTEM_STORAGE_ADDRESS;
use cfx_state::{
    state_trait::{AsStateOpsTrait, CheckpointTrait, StateOpsTrait},
    CleanupMode, StateTrait,
};
use cfx_statedb::{
    ErrorKind as DbErrorKind, Result as DbResult, StateDb, StateDbExt, StateDbTrait,
};
use cfx_types::{AddressSpaceUtil, AddressWithSpace, H256, U256};
use parking_lot::{MappedRwLockWriteGuard, RwLock, RwLockUpgradableReadGuard, RwLockWriteGuard};
#[cfg(test)]
use primitives::storage::STORAGE_LAYOUT_REGULAR_V0;
use primitives::{Account, EpochId, StateKey, StorageLayout};

use crate::hash::KECCAK_EMPTY;

use self::account_entry::{AccountEntry, AccountState};
pub use self::{
    account_entry::OverlayAccount,
    substate::{cleanup_mode, FrameStackInfo, Substate},
};

mod account_entry;
#[cfg(test)]
mod account_entry_tests;
#[cfg(test)]
mod state_tests;
mod substate;

#[derive(Copy, Clone)]
pub enum RequireCache {
    None,
    Code,
}

#[derive(Copy, Clone, Debug)]
struct WorldStatistics {
    // This is the total number of tokens issued.
    total_issued_tokens: U256,
}

pub struct State<'a> {
    db: StateDb<'a>,

    // Only created once for txpool notification.
    // Each element is an Ok(Account) for updated account, or
    // Err(AddressWithSpace) for deleted account.
    accounts_to_notify: Vec<Result<Account, AddressWithSpace>>,

    // Contains the changes to the states and some unchanged state entries.
    cache: RwLock<HashMap<AddressWithSpace, AccountEntry>>,
    // TODO: try not to make it special?
    world_statistics: WorldStatistics,

    // Checkpoint to the changes.
    world_statistics_checkpoints: RwLock<Vec<WorldStatistics>>,
    checkpoints: RwLock<Vec<HashMap<AddressWithSpace, Option<AccountEntry>>>>,
}

impl<'a> StateTrait for State<'a> {
    fn commit(
        &mut self,
        epoch_id: EpochId,
        mut debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> DbResult<()> {
        debug!("Commit epoch[{}]", epoch_id);

        assert!(self.checkpoints.get_mut().is_empty());
        assert!(self.world_statistics_checkpoints.get_mut().is_empty());

        let mut sorted_dirty_accounts = self.cache.get_mut().drain().collect::<Vec<_>>();
        sorted_dirty_accounts.sort_by(|a, b| a.0.cmp(&b.0));

        let mut killed_addresses = Vec::new();
        for (address, entry) in sorted_dirty_accounts.iter_mut() {
            entry.state = AccountState::Committed;
            match &mut entry.account {
                None => {}
                Some(account) if account.removed_without_update() => {
                    killed_addresses.push(*address);
                    self.accounts_to_notify.push(Err(*address));
                }
                Some(account) => {
                    account.commit(self, address, debug_record.as_deref_mut())?;
                    self.accounts_to_notify.push(Ok(account.as_account()));
                }
            }
        }
        self.recycle_storage(killed_addresses, debug_record.as_deref_mut())?;
        self.commit_world_statistics(debug_record.as_deref_mut())?;
        Ok(self.db.commit(epoch_id, debug_record)?)
    }
}

impl<'a> StateOpsTrait for State<'a> {
    /// Maintain `total_issued_tokens`.
    fn add_total_issued(&mut self, v: U256) {
        assert!(self.world_statistics_checkpoints.get_mut().is_empty());
        self.world_statistics.total_issued_tokens += v;
    }

    /// Maintain `total_issued_tokens`. This is only used in the extremely
    /// unlikely case that there are a lot of partial invalid blocks.
    fn subtract_total_issued(&mut self, v: U256) {
        self.world_statistics.total_issued_tokens =
            self.world_statistics.total_issued_tokens.saturating_sub(v);
    }

    fn new_contract(
        &mut self,
        contract: &AddressWithSpace,
        balance: U256,
        nonce: U256,
        storage_layout: Option<StorageLayout>,
    ) -> DbResult<()> {
        // Check if the new contract is deployed on a killed contract in the
        // same block.
        let invalidated_storage =
            self.ensure_account_loaded(contract, RequireCache::None, |maybe_overlay| {
                maybe_overlay.map_or(false, |overlay| overlay.invalidated_storage())
            })?;
        Self::update_cache(
            self.cache.get_mut(),
            self.checkpoints.get_mut(),
            contract,
            AccountEntry::new_dirty(Some(OverlayAccount::new_contract(
                contract,
                balance,
                nonce,
                invalidated_storage,
                storage_layout,
            ))),
        );
        Ok(())
    }

    fn balance(&self, address: &AddressWithSpace) -> DbResult<U256> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| {
            acc.map_or(U256::zero(), |account| *account.balance())
        })
    }

    fn is_contract_with_code(&self, address: &AddressWithSpace) -> DbResult<bool> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| {
            acc.map_or(false, |acc| acc.code_hash() != KECCAK_EMPTY)
        })
    }

    // TODO: maybe return error for reserved address? Not sure where is the best
    //  place to do the check.
    fn nonce(&self, address: &AddressWithSpace) -> DbResult<U256> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| {
            acc.map_or(U256::zero(), |account| *account.nonce())
        })
    }

    fn init_code(&mut self, address: &AddressWithSpace, code: Bytes) -> DbResult<()> {
        self.require_exists(address, false)?.init_code(code);
        Ok(())
    }

    fn code_hash(&self, address: &AddressWithSpace) -> DbResult<Option<H256>> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| {
            acc.and_then(|acc| Some(acc.code_hash()))
        })
    }

    fn code_size(&self, address: &AddressWithSpace) -> DbResult<Option<usize>> {
        self.ensure_account_loaded(address, RequireCache::Code, |acc| {
            acc.and_then(|acc| acc.code_size())
        })
    }

    fn code(&self, address: &AddressWithSpace) -> DbResult<Option<Arc<Vec<u8>>>> {
        self.ensure_account_loaded(address, RequireCache::Code, |acc| {
            acc.as_ref().map_or(None, |acc| acc.code())
        })
    }

    fn clean_account(&mut self, address: &AddressWithSpace) -> DbResult<()> {
        *&mut *self.require_or_new_basic_account(address, &U256::zero())? =
            OverlayAccount::from_loaded(address, Account::new_empty(address));
        Ok(())
    }

    fn inc_nonce(
        &mut self,
        address: &AddressWithSpace,
        account_start_nonce: &U256,
    ) -> DbResult<()> {
        self.require_or_new_basic_account(address, account_start_nonce)
            .map(|mut x| x.inc_nonce())
    }

    fn set_nonce(&mut self, address: &AddressWithSpace, nonce: &U256) -> DbResult<()> {
        self.require_or_new_basic_account(address, nonce)
            .map(|mut x| x.set_nonce(&nonce))
    }

    fn sub_balance(
        &mut self,
        address: &AddressWithSpace,
        by: &U256,
        cleanup_mode: &mut CleanupMode,
    ) -> DbResult<()> {
        if !by.is_zero() {
            self.require_exists(address, false)?.sub_balance(by);
        }

        if let CleanupMode::TrackTouched(ref mut set) = *cleanup_mode {
            if self.exists(address)? {
                set.insert(*address);
            }
        }
        Ok(())
    }

    fn add_balance(
        &mut self,
        address: &AddressWithSpace,
        by: &U256,
        cleanup_mode: CleanupMode,
        account_start_nonce: U256,
    ) -> DbResult<()> {
        let exists = self.exists(address)?;

        // The caller should guarantee the validity of address.

        if !by.is_zero() || (cleanup_mode == CleanupMode::ForceCreate && !exists) {
            self.require_or_new_basic_account(address, &account_start_nonce)?
                .add_balance(by);
        }

        if let CleanupMode::TrackTouched(set) = cleanup_mode {
            if exists {
                set.insert(*address);
            }
        }
        Ok(())
    }

    fn transfer_balance(
        &mut self,
        from: &AddressWithSpace,
        to: &AddressWithSpace,
        by: &U256,
        mut cleanup_mode: CleanupMode,
        account_start_nonce: U256,
    ) -> DbResult<()> {
        self.sub_balance(from, by, &mut cleanup_mode)?;
        self.add_balance(to, by, cleanup_mode, account_start_nonce)?;
        Ok(())
    }

    fn total_issued_tokens(&self) -> U256 {
        self.world_statistics.total_issued_tokens
    }

    fn remove_contract(&mut self, address: &AddressWithSpace) -> DbResult<()> {
        Self::update_cache(
            self.cache.get_mut(),
            self.checkpoints.get_mut(),
            address,
            AccountEntry::new_dirty(Some(OverlayAccount::new_removed(address))),
        );

        Ok(())
    }

    fn exists(&self, address: &AddressWithSpace) -> DbResult<bool> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| acc.is_some())
    }

    fn exists_and_not_null(&self, address: &AddressWithSpace) -> DbResult<bool> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| {
            acc.map_or(false, |acc| !acc.is_null())
        })
    }

    fn storage_at(&self, address: &AddressWithSpace, key: &[u8]) -> DbResult<U256> {
        self.ensure_account_loaded(address, RequireCache::None, |acc| {
            acc.map_or(Ok(U256::zero()), |account| {
                account.storage_at(&self.db, key)
            })
        })?
    }

    fn set_storage(
        &mut self,
        address: &AddressWithSpace,
        key: Vec<u8>,
        value: U256,
    ) -> DbResult<()> {
        if self.storage_at(address, &key)? != value {
            self.require_exists(address, false)?.set_storage(key, value)
        }
        Ok(())
    }

    fn set_system_storage(&mut self, key: Vec<u8>, value: U256) -> DbResult<()> {
        self.set_storage(&SYSTEM_STORAGE_ADDRESS.with_evm_space(), key, value)
    }

    fn get_system_storage(&self, key: &[u8]) -> DbResult<U256> {
        self.storage_at(&SYSTEM_STORAGE_ADDRESS.with_evm_space(), key)
    }
}

impl<'a> CheckpointTrait for State<'a> {
    /// Create a recoverable checkpoint of this state. Return the checkpoint
    /// index. The checkpoint records any old value which is alive at the
    /// creation time of the checkpoint and updated after that and before
    /// the creation of the next checkpoint.
    fn checkpoint(&mut self) -> usize {
        self.world_statistics_checkpoints
            .get_mut()
            .push(self.world_statistics.clone());
        let checkpoints = self.checkpoints.get_mut();
        let index = checkpoints.len();
        checkpoints.push(HashMap::new());
        index
    }

    /// Merge last checkpoint with previous.
    /// Caller should make sure the function
    /// `collect_ownership_changed()` was called before calling
    /// this function.
    fn discard_checkpoint(&mut self) {
        // merge with previous checkpoint
        let last = self.checkpoints.get_mut().pop();
        if let Some(mut checkpoint) = last {
            self.world_statistics_checkpoints.get_mut().pop();
            if let Some(ref mut prev) = self.checkpoints.get_mut().last_mut() {
                if prev.is_empty() {
                    **prev = checkpoint;
                } else {
                    for (k, v) in checkpoint.drain() {
                        prev.entry(k).or_insert(v);
                    }
                }
            }
        }
    }

    /// Revert to the last checkpoint and discard it.
    fn revert_to_checkpoint(&mut self) {
        if let Some(mut checkpoint) = self.checkpoints.get_mut().pop() {
            self.world_statistics = self
                .world_statistics_checkpoints
                .get_mut()
                .pop()
                .expect("staking_state_checkpoint should exist");
            for (k, v) in checkpoint.drain() {
                match v {
                    Some(v) => match self.cache.get_mut().entry(k) {
                        Entry::Occupied(mut e) => {
                            e.get_mut().overwrite_with(v);
                        }
                        Entry::Vacant(e) => {
                            e.insert(v);
                        }
                    },
                    None => {
                        if let Entry::Occupied(e) = self.cache.get_mut().entry(k) {
                            if e.get().is_dirty() {
                                e.remove();
                            }
                        }
                    }
                }
            }
        }
    }
}

impl<'a> AsStateOpsTrait for State<'a> {
    fn as_state_ops(&self) -> &dyn StateOpsTrait {
        self
    }

    fn as_mut_state_ops(&mut self) -> &mut dyn StateOpsTrait {
        self
    }
}

impl<'a> State<'a> {
    pub fn new(db: StateDb<'a>) -> DbResult<Self> {
        let total_issued_tokens = db.get_total_issued_tokens()?;

        let world_statistics = WorldStatistics {
            total_issued_tokens,
        };

        Ok(State {
            db,
            cache: Default::default(),
            world_statistics_checkpoints: Default::default(),
            checkpoints: Default::default(),
            world_statistics,
            accounts_to_notify: Default::default(),
        })
    }

    fn needs_update(require: RequireCache, account: &OverlayAccount) -> bool {
        trace!("update_account_cache account={:?}", account);
        match require {
            RequireCache::None => false,
            RequireCache::Code => !account.is_code_loaded(),
        }
    }

    /// Load required account data from the databases. Returns whether the
    /// cache succeeds.
    fn update_account_cache(
        require: RequireCache,
        account: &mut OverlayAccount,
        db: &StateDb,
    ) -> DbResult<bool> {
        match require {
            RequireCache::None => Ok(true),
            RequireCache::Code => account.cache_code(db),
        }
    }

    fn commit_world_statistics(
        &mut self,
        mut debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> DbResult<()> {
        self.db.set_total_issued_tokens(
            &self.world_statistics.total_issued_tokens,
            debug_record.as_deref_mut(),
        )?;
        Ok(())
    }

    /// Assume that only contract with zero `collateral_for_storage` will be
    /// killed.
    pub fn recycle_storage(
        &mut self,
        killed_addresses: Vec<AddressWithSpace>,
        mut debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> DbResult<()> {
        // TODO: Think about kill_dust and collateral refund.
        for address in &killed_addresses {
            // self.db.delete_all::<access_mode::Write>(
            //     StorageKey::new_storage_root_key(&address.address).with_space(address.space),
            //     debug_record.as_deref_mut(),
            // )?;
            // self.db.delete_all::<access_mode::Write>(
            //     StorageKey::new_code_root_key(&address.address).with_space(address.space),
            //     debug_record.as_deref_mut(),
            // )?;
            self.db.delete(
                StateKey::new_account_key(&address),
                debug_record.as_deref_mut(),
            )?;
        }
        Ok(())
    }

    fn update_cache(
        cache: &mut HashMap<AddressWithSpace, AccountEntry>,
        checkpoints: &mut Vec<HashMap<AddressWithSpace, Option<AccountEntry>>>,
        address: &AddressWithSpace,
        account: AccountEntry,
    ) {
        let is_dirty = account.is_dirty();
        let old_value = cache.insert(*address, account);
        if is_dirty {
            if let Some(ref mut checkpoint) = checkpoints.last_mut() {
                checkpoint.entry(*address).or_insert(old_value);
            }
        }
    }

    fn insert_cache_if_fresh_account(
        cache: &mut HashMap<AddressWithSpace, AccountEntry>,
        address: &AddressWithSpace,
        maybe_account: Option<OverlayAccount>,
    ) -> bool {
        if !cache.contains_key(address) {
            cache.insert(*address, AccountEntry::new_clean(maybe_account));
            true
        } else {
            false
        }
    }

    pub fn ensure_account_loaded<F, U>(
        &self,
        address: &AddressWithSpace,
        require: RequireCache,
        f: F,
    ) -> DbResult<U>
    where
        F: Fn(Option<&OverlayAccount>) -> U,
    {
        // Return immediately when there is no need to have db operation.
        if let Some(maybe_acc) = self.cache.read().get(address) {
            if let Some(account) = &maybe_acc.account {
                let needs_update = Self::needs_update(require, account);
                if !needs_update {
                    return Ok(f(Some(account)));
                }
            } else {
                return Ok(f(None));
            }
        }

        let mut cache_write_lock = {
            let upgradable_lock = self.cache.upgradable_read();
            if upgradable_lock.contains_key(address) {
                // TODO: the account can be updated here if the relevant methods
                //  to update account can run with &OverlayAccount.
                RwLockUpgradableReadGuard::upgrade(upgradable_lock)
            } else {
                // Load the account from db.
                let mut maybe_loaded_acc = self
                    .db
                    .get_account(address)?
                    .map(|acc| OverlayAccount::from_loaded(address, acc));
                if let Some(account) = &mut maybe_loaded_acc {
                    Self::update_account_cache(require, account, &self.db)?;
                }
                let mut cache_write_lock = RwLockUpgradableReadGuard::upgrade(upgradable_lock);
                Self::insert_cache_if_fresh_account(
                    &mut *cache_write_lock,
                    address,
                    maybe_loaded_acc,
                );

                cache_write_lock
            }
        };

        let cache = &mut *cache_write_lock;
        let account = cache.get_mut(address).unwrap();
        if let Some(maybe_acc) = &mut account.account {
            if !Self::update_account_cache(require, maybe_acc, &self.db)? {
                return Err(
                    DbErrorKind::IncompleteDatabase(maybe_acc.address().address.clone()).into(),
                );
            }
        }

        Ok(f(cache
            .get(address)
            .and_then(|entry| entry.account.as_ref())))
    }

    fn require_exists(
        &self,
        address: &AddressWithSpace,
        require_code: bool,
    ) -> DbResult<MappedRwLockWriteGuard<OverlayAccount>> {
        fn no_account_is_an_error(address: &AddressWithSpace) -> DbResult<OverlayAccount> {
            bail!(DbErrorKind::IncompleteDatabase(address.address));
        }
        self.require_or_set(address, require_code, no_account_is_an_error)
    }

    fn require_or_new_basic_account(
        &self,
        address: &AddressWithSpace,
        account_start_nonce: &U256,
    ) -> DbResult<MappedRwLockWriteGuard<OverlayAccount>> {
        self.require_or_set(address, false, |address| {
            // It is guaranteed that the address is valid.

            // Note that it is possible to first send money to a pre-calculated
            // contract address and then deploy contracts. So we are
            // going to *allow* sending to a contract address and
            // use new_basic() to create a *stub* there. Because the contract
            // serialization is a super-set of the normal address
            // serialization, this should just work.
            Ok(OverlayAccount::new_basic(
                address,
                U256::zero(),
                account_start_nonce.into(),
            ))
        })
    }

    fn require_or_set<F>(
        &self,
        address: &AddressWithSpace,
        require_code: bool,
        default: F,
    ) -> DbResult<MappedRwLockWriteGuard<OverlayAccount>>
    where
        F: FnOnce(&AddressWithSpace) -> DbResult<OverlayAccount>,
    {
        let mut cache;
        if !self.cache.read().contains_key(address) {
            let account = self
                .db
                .get_account(address)?
                .map(|acc| OverlayAccount::from_loaded(address, acc));
            cache = self.cache.write();
            Self::insert_cache_if_fresh_account(&mut *cache, address, account);
        } else {
            cache = self.cache.write();
        };

        // Save the value before modification into the checkpoint.
        if let Some(ref mut checkpoint) = self.checkpoints.write().last_mut() {
            checkpoint
                .entry(*address)
                .or_insert_with(|| cache.get(address).map(AccountEntry::clone_dirty));
        }

        let entry = (*cache)
            .get_mut(address)
            .expect("entry known to exist in the cache");

        // Set the dirty flag.
        entry.state = AccountState::Dirty;

        if entry.account.is_none() {
            entry.account = Some(default(address)?);
        }

        if require_code {
            if !Self::update_account_cache(
                RequireCache::Code,
                entry
                    .account
                    .as_mut()
                    .expect("Required account must exist."),
                &self.db,
            )? {
                bail!(DbErrorKind::IncompleteDatabase(address.address));
            }
        }

        Ok(RwLockWriteGuard::map(cache, |c| {
            c.get_mut(address)
                .expect("Entry known to exist in the cache.")
                .account
                .as_mut()
                .expect("Required account must exist.")
        }))
    }
}

/// Methods that are intentionally kept private because the fields may not have
/// been loaded from db.
trait AccountEntryProtectedMethods {
    fn code_size(&self) -> Option<usize>;
    fn code(&self) -> Option<Arc<Bytes>>;
}
