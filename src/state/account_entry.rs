// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use crate::{
    bytes::Bytes,
    hash::{keccak, KECCAK_EMPTY},
    state::{AccountEntryProtectedMethods, State},
};
use cfx_internal_common::debug::ComputeEpochDebugRecord;
use cfx_statedb::{Result as DbResult, StateDb, StateDbExt};
#[cfg(test)]
use cfx_types::AddressSpaceUtil;
use cfx_types::{address_util::AddressUtil, Address, AddressWithSpace, H256, U256};
use parking_lot::RwLock;
use primitives::{
    is_default::IsDefault, Account, CodeInfo, StorageKey, StorageLayout, StorageValue,
};
use std::{collections::HashMap, sync::Arc};

lazy_static! {
    static ref COMMISSION_PRIVILEGE_STORAGE_VALUE: U256 = U256::one();
    /// If we set this key, it means every account has commission privilege.
    pub static ref COMMISSION_PRIVILEGE_SPECIAL_KEY: Address = Address::zero();
}

#[derive(Debug)]
/// Single account in the system.
/// Keeps track of changes to the code and storage.
/// The changes are applied in `commit_storage` and `commit_code`
pub struct OverlayAccount {
    address: AddressWithSpace,

    // Balance of the account.
    balance: U256,
    // Nonce of the account,
    nonce: U256,

    // FIXME: there are changes, so no need to have cache for both storage and
    // ownership

    // This is a read cache for storage values of the current account in db.
    // The underlying db will not change while computing transactions in an
    // epoch. So all the contents in the read cache is always available.
    storage_value_read_cache: Arc<RwLock<HashMap<Vec<u8>, U256>>>,
    // This is a write cache for changing storage value in db. It will be
    // written to db when committing overlay account.
    storage_value_write_cache: Arc<HashMap<Vec<u8>, U256>>,

    // Storage layout change.
    storage_layout_change: Option<StorageLayout>,

    // Code hash of the account.
    code_hash: H256,
    // When code_hash isn't KECCAK_EMPTY, the code has been initialized for
    // the account. The code field can be None, which means that the code
    // has not been loaded from storage. When code_hash is KECCAK_EMPTY, this
    // field always None.
    code: Option<CodeInfo>,

    // This flag indicates whether it is a newly created contract. For such
    // account, we will skip looking data from the disk. This flag will stay
    // true until the contract being committed and cleared from the memory.
    //
    // If the contract account at the same address is killed, then the same
    // account is re-created, this flag is also true, to indicate that any
    // pending cleanups must be done. The re-creation of the account can
    // also be caused by a simple payment transaction, which result into a new
    // basic account at the same address.
    is_newly_created_contract: bool,
    invalidated_storage: bool,
}

impl OverlayAccount {
    /// Create an OverlayAccount from loaded account.
    pub fn from_loaded(address: &AddressWithSpace, account: Account) -> Self {
        let overlay_account = OverlayAccount {
            address: address.clone(),
            balance: account.balance,
            nonce: account.nonce,
            storage_value_read_cache: Default::default(),
            storage_value_write_cache: Default::default(),
            storage_layout_change: None,
            code_hash: account.code_hash,
            code: None,
            is_newly_created_contract: false,
            invalidated_storage: false,
        };

        overlay_account
    }

    /// Create an OverlayAccount of basic account when the account doesn't exist
    /// before.
    pub fn new_basic(address: &AddressWithSpace, balance: U256, nonce: U256) -> Self {
        OverlayAccount {
            address: address.clone(),
            balance,
            nonce,
            storage_value_read_cache: Default::default(),
            storage_value_write_cache: Default::default(),
            storage_layout_change: None,
            code_hash: KECCAK_EMPTY,
            code: None,
            is_newly_created_contract: false,
            invalidated_storage: false,
        }
    }

    /// Create an OverlayAccount of basic account when the account doesn't exist
    /// before.
    pub fn new_removed(address: &AddressWithSpace) -> Self {
        OverlayAccount {
            address: address.clone(),
            balance: Default::default(),
            nonce: Default::default(),
            storage_value_read_cache: Default::default(),
            storage_value_write_cache: Default::default(),
            storage_layout_change: None,
            code_hash: KECCAK_EMPTY,
            code: None,
            is_newly_created_contract: false,
            invalidated_storage: true,
        }
    }

    /// Create an OverlayAccount of contract account when the account doesn't
    /// exist before.
    #[cfg(test)]
    pub fn new_contract(
        address: &Address,
        balance: U256,
        nonce: U256,
        invalidated_storage: bool,
        storage_layout: Option<StorageLayout>,
    ) -> Self {
        Self::new_contract_with_admin(
            &address.with_native_space(),
            balance,
            nonce,
            &Address::zero(),
            invalidated_storage,
            storage_layout,
        )
    }

    /// Create an OverlayAccount of contract account when the account doesn't
    /// exist before.
    pub fn new_contract(
        address: &AddressWithSpace,
        balance: U256,
        nonce: U256,
        invalidated_storage: bool,
        storage_layout: Option<StorageLayout>,
    ) -> Self {
        OverlayAccount {
            address: address.clone(),
            balance,
            nonce,
            storage_value_read_cache: Default::default(),
            storage_value_write_cache: Default::default(),
            storage_layout_change: storage_layout,
            code_hash: KECCAK_EMPTY,
            code: None,
            is_newly_created_contract: true,
            invalidated_storage,
        }
    }

    pub fn as_account(&self) -> Account {
        let mut account = Account::new_empty(self.address());

        account.balance = self.balance;
        account.nonce = self.nonce;
        account.code_hash = self.code_hash;
        account.set_address(self.address);
        account
    }

    pub fn is_contract(&self) -> bool {
        self.code_hash != KECCAK_EMPTY || self.is_newly_created_contract
    }

    fn fresh_storage(&self) -> bool {
        let builtin_address = self.address.address.is_builtin_address();
        (self.is_newly_created_contract && !builtin_address) || self.invalidated_storage
    }

    pub fn removed_without_update(&self) -> bool {
        self.invalidated_storage && self.as_account().is_default()
    }

    pub fn invalidated_storage(&self) -> bool {
        self.invalidated_storage
    }

    pub fn address(&self) -> &AddressWithSpace {
        &self.address
    }

    pub fn balance(&self) -> &U256 {
        &self.balance
    }

    #[cfg(test)]
    pub fn is_newly_created_contract(&self) -> bool {
        self.is_newly_created_contract
    }

    pub fn nonce(&self) -> &U256 {
        &self.nonce
    }

    pub fn code_hash(&self) -> H256 {
        self.code_hash.clone()
    }

    pub fn is_code_loaded(&self) -> bool {
        self.code.is_some() || self.code_hash == KECCAK_EMPTY
    }

    pub fn is_null(&self) -> bool {
        self.balance.is_zero() && self.nonce.is_zero() && self.code_hash == KECCAK_EMPTY
    }

    pub fn is_basic(&self) -> bool {
        self.code_hash == KECCAK_EMPTY
    }

    pub fn set_nonce(&mut self, nonce: &U256) {
        self.nonce = *nonce;
    }

    pub fn inc_nonce(&mut self) {
        self.nonce = self.nonce + U256::from(1u8);
    }

    pub fn add_balance(&mut self, by: &U256) {
        self.balance = self.balance + *by;
    }

    pub fn sub_balance(&mut self, by: &U256) {
        assert!(self.balance >= *by);
        self.balance = self.balance - *by;
    }

    pub fn cache_code(&mut self, db: &StateDb) -> DbResult<bool> {
        trace!(
            "OverlayAccount::cache_code: ic={}; self.code_hash={:?}, self.code_cache={:?}",
            self.is_code_loaded(),
            self.code_hash,
            self.code
        );

        if self.is_code_loaded() {
            return Ok(true);
        }

        self.code = db.get_code(&self.address, &self.code_hash)?;
        match &self.code {
            Some(_) => Ok(true),
            _ => {
                warn!(
                    "Failed to get code {:?} for address {:?}",
                    self.code_hash, self.address
                );
                Ok(false)
            }
        }
    }

    pub fn clone_basic(&self) -> Self {
        OverlayAccount {
            address: self.address,
            balance: self.balance,
            nonce: self.nonce,
            storage_value_read_cache: Default::default(),
            storage_value_write_cache: Default::default(),
            storage_layout_change: None,
            code_hash: self.code_hash,
            code: self.code.clone(),
            is_newly_created_contract: self.is_newly_created_contract,
            invalidated_storage: self.invalidated_storage,
        }
    }

    pub fn clone_dirty(&self) -> Self {
        let mut account = self.clone_basic();
        account.storage_value_write_cache = self.storage_value_write_cache.clone();
        account.storage_value_read_cache = self.storage_value_read_cache.clone();
        account.storage_layout_change = self.storage_layout_change.clone();
        account
    }

    pub fn set_storage(&mut self, key: Vec<u8>, value: U256) {
        Arc::make_mut(&mut self.storage_value_write_cache).insert(key.clone(), value);
    }

    #[cfg(test)]
    pub fn storage_layout_change(&self) -> Option<&StorageLayout> {
        self.storage_layout_change.as_ref()
    }

    #[cfg(test)]
    pub fn set_storage_layout(&mut self, layout: StorageLayout) {
        self.storage_layout_change = Some(layout);
    }

    pub fn cached_storage_at(&self, key: &[u8]) -> Option<U256> {
        if let Some(value) = self.storage_value_write_cache.get(key) {
            return Some(value.clone());
        }
        if let Some(value) = self.storage_value_read_cache.read().get(key) {
            return Some(value.clone());
        }
        None
    }

    // If a contract is removed, and then some one transfer balance to it,
    // `storage_at` will return incorrect value. But this case should never
    // happens.
    pub fn storage_at(&self, db: &StateDb, key: &[u8]) -> DbResult<U256> {
        if let Some(value) = self.cached_storage_at(key) {
            return Ok(value);
        }
        if self.fresh_storage() {
            Ok(U256::zero())
        } else {
            Self::get_and_cache_storage(
                &mut self.storage_value_read_cache.write(),
                db,
                &self.address,
                key,
            )
        }
    }

    fn get_and_cache_storage(
        storage_value_read_cache: &mut HashMap<Vec<u8>, U256>,
        db: &StateDb,
        address: &AddressWithSpace,
        key: &[u8],
    ) -> DbResult<U256> {
        if let Some(value) = db.get::<StorageValue>(
            StorageKey::new_storage_key(&address.address, key.as_ref()).with_space(address.space),
        )? {
            storage_value_read_cache.insert(key.to_vec(), value.value);
            Ok(value.value)
        } else {
            storage_value_read_cache.insert(key.to_vec(), U256::zero());
            Ok(U256::zero())
        }
    }

    pub fn init_code(&mut self, code: Bytes) {
        self.code_hash = keccak(&code);
        self.code = Some(CodeInfo {
            code: Arc::new(code),
        });
    }

    pub fn overwrite_with(&mut self, other: OverlayAccount) {
        self.balance = other.balance;
        self.nonce = other.nonce;
        self.code_hash = other.code_hash;
        self.code = other.code;
        self.storage_value_read_cache = other.storage_value_read_cache;
        self.storage_value_write_cache = other.storage_value_write_cache;
        self.storage_layout_change = other.storage_layout_change;
        self.is_newly_created_contract = other.is_newly_created_contract;
        self.invalidated_storage = other.invalidated_storage;
    }

    pub fn commit(
        &mut self,
        state: &mut State,
        address: &AddressWithSpace,
        mut debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> DbResult<()> {
        assert_eq!(Arc::strong_count(&self.storage_value_write_cache), 1);

        if self.invalidated_storage() {
            state.recycle_storage(vec![self.address], debug_record.as_deref_mut())?;
        }

        for (k, v) in Arc::make_mut(&mut self.storage_value_write_cache).drain() {
            let address_key = StorageKey::new_storage_key(&self.address.address, k.as_ref())
                .with_space(self.address.space);
            match v.is_zero() {
                true => state.db.delete(address_key, debug_record.as_deref_mut())?,
                false => state.db.set::<StorageValue>(
                    address_key,
                    &StorageValue { value: v },
                    debug_record.as_deref_mut(),
                )?,
            }
        }

        if let Some(code_info) = self.code.as_ref() {
            let storage_key = StorageKey::new_code_key(&self.address.address, &self.code_hash)
                .with_space(self.address.space);
            state
                .db
                .set::<CodeInfo>(storage_key, code_info, debug_record.as_deref_mut())?;
        }

        if let Some(layout) = self.storage_layout_change.clone() {
            state
                .db
                .set_storage_layout(&self.address, layout, debug_record.as_deref_mut())?;
        }

        state.db.set::<Account>(
            StorageKey::new_account_key(&address.address).with_space(address.space),
            &self.as_account(),
            debug_record,
        )?;

        Ok(())
    }
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
/// Account modification state. Used to check if the account was
/// Modified in between commits and overall.
#[allow(dead_code)]
pub enum AccountState {
    /// Account was loaded from disk and never modified in this state object.
    CleanFresh,
    /// Account was loaded from the global cache and never modified.
    CleanCached,
    /// Account has been modified and is not committed to the trie yet.
    /// This is set if any of the account data is changed, including
    /// storage and code.
    Dirty,
    /// Account was modified and committed to the trie.
    Committed,
}

#[derive(Debug)]
/// In-memory copy of the account data. Holds the optional account
/// and the modification status.
/// Account entry can contain existing (`Some`) or non-existing
/// account (`None`)
pub struct AccountEntry {
    /// Account proxy. `None` if account known to be non-existent.
    pub account: Option<OverlayAccount>,
    /// Unmodified account balance.
    pub old_balance: Option<U256>,
    // FIXME: remove it.
    /// Entry state.
    pub state: AccountState,
}

impl AccountEntry {
    // FIXME: remove it.
    pub fn is_dirty(&self) -> bool {
        self.state == AccountState::Dirty
    }

    pub fn overwrite_with(&mut self, other: AccountEntry) {
        self.state = other.state;
        match other.account {
            Some(acc) => {
                if let Some(ref mut ours) = self.account {
                    ours.overwrite_with(acc);
                } else {
                    self.account = Some(acc);
                }
            }
            None => self.account = None,
        }
    }

    /// Clone dirty data into new `AccountEntry`. This includes
    /// basic account data and modified storage keys.
    pub fn clone_dirty(&self) -> AccountEntry {
        AccountEntry {
            old_balance: self.old_balance,
            account: self.account.as_ref().map(OverlayAccount::clone_dirty),
            state: self.state,
        }
    }

    pub fn new_dirty(account: Option<OverlayAccount>) -> AccountEntry {
        AccountEntry {
            old_balance: account.as_ref().map(|acc| acc.balance().clone()),
            account,
            state: AccountState::Dirty,
        }
    }

    pub fn new_clean(account: Option<OverlayAccount>) -> AccountEntry {
        AccountEntry {
            old_balance: account.as_ref().map(|acc| acc.balance().clone()),
            account,
            state: AccountState::CleanFresh,
        }
    }
}

impl AccountEntryProtectedMethods for OverlayAccount {
    /// This method is intentionally kept private because the field may not have
    /// been loaded from db.
    fn code_size(&self) -> Option<usize> {
        self.code.as_ref().map(|c| c.code_size())
    }

    /// This method is intentionally kept private because the field may not have
    /// been loaded from db.
    fn code(&self) -> Option<Arc<Bytes>> {
        self.code.as_ref().map(|c| c.code.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_helpers::get_state_for_genesis_write;
    use cfx_storage::tests::new_state_manager_for_unit_test;
    use primitives::is_default::IsDefault;
    use std::str::FromStr;

    fn test_account_is_default(account: &mut OverlayAccount) {
        let storage_manager = new_state_manager_for_unit_test();
        let state = get_state_for_genesis_write(&storage_manager);

        assert!(account.as_account().is_default());

        account.cache_staking_info(true, true, &state.db).unwrap();
        assert!(account.vote_stake_list().unwrap().is_default());
        assert!(account.deposit_list().unwrap().is_default());
    }

    #[test]
    fn new_overlay_account_is_default() {
        let normal_addr = Address::from_str("1000000000000000000000000000000000000000")
            .unwrap()
            .with_native_space();
        let contract_addr = Address::from_str("8000000000000000000000000000000000000000")
            .unwrap()
            .with_native_space();
        let builtin_addr = Address::from_str("0000000000000000000000000000000000000000")
            .unwrap()
            .with_native_space();

        test_account_is_default(&mut OverlayAccount::new_basic(
            &normal_addr,
            U256::zero(),
            U256::zero(),
        ));
        test_account_is_default(&mut OverlayAccount::new_contract(
            &contract_addr.address,
            U256::zero(),
            U256::zero(),
            false,
            /* storage_layout = */ None,
        ));
        test_account_is_default(&mut OverlayAccount::new_basic(
            &builtin_addr,
            U256::zero(),
            U256::zero(),
        ));
    }
}
