// Copyright 2021 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

pub trait StateTrait: CheckpointTrait + AsStateOpsTrait {
    type Substate: SubstateTrait;

    fn compute_state_root(
        &mut self,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> DbResult<StateRootWithAuxInfo>;

    fn commit(
        &mut self,
        epoch_id: EpochId,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> DbResult<StateRootWithAuxInfo>;
}

pub trait StateOpsTrait {
    /// Maintain `total_issued_tokens`.s
    fn add_total_issued(&mut self, v: U256);

    /// Maintain `total_issued_tokens`. This is only used in the extremely
    /// unlikely case that there are a lot of partial invalid blocks.
    fn subtract_total_issued(&mut self, v: U256);

    fn new_contract(
        &mut self,
        contract: &AddressWithSpace,
        balance: U256,
        nonce: U256,
        storage_layout: Option<StorageLayout>,
    ) -> DbResult<()>;

    fn balance(&self, address: &AddressWithSpace) -> DbResult<U256>;

    fn is_contract_with_code(&self, address: &AddressWithSpace) -> DbResult<bool>;

    fn nonce(&self, address: &AddressWithSpace) -> DbResult<U256>;

    fn init_code(&mut self, address: &AddressWithSpace, code: Vec<u8>) -> DbResult<()>;

    fn code_hash(&self, address: &AddressWithSpace) -> DbResult<Option<H256>>;

    fn code_size(&self, address: &AddressWithSpace) -> DbResult<Option<usize>>;

    fn code(&self, address: &AddressWithSpace) -> DbResult<Option<Arc<Vec<u8>>>>;

    fn clean_account(&mut self, address: &AddressWithSpace) -> DbResult<()>;

    fn inc_nonce(&mut self, address: &AddressWithSpace, account_start_nonce: &U256)
        -> DbResult<()>;

    fn set_nonce(&mut self, address: &AddressWithSpace, nonce: &U256) -> DbResult<()>;

    fn sub_balance(
        &mut self,
        address: &AddressWithSpace,
        by: &U256,
        cleanup_mode: &mut CleanupMode,
    ) -> DbResult<()>;

    fn add_balance(
        &mut self,
        address: &AddressWithSpace,
        by: &U256,
        cleanup_mode: CleanupMode,
        account_start_nonce: U256,
    ) -> DbResult<()>;

    fn transfer_balance(
        &mut self,
        from: &AddressWithSpace,
        to: &AddressWithSpace,
        by: &U256,
        cleanup_mode: CleanupMode,
        account_start_nonce: U256,
    ) -> DbResult<()>;

    fn total_issued_tokens(&self) -> U256;

    fn remove_contract(&mut self, address: &AddressWithSpace) -> DbResult<()>;

    fn exists(&self, address: &AddressWithSpace) -> DbResult<bool>;

    fn exists_and_not_null(&self, address: &AddressWithSpace) -> DbResult<bool>;

    fn storage_at(&self, address: &AddressWithSpace, key: &[u8]) -> DbResult<U256>;

    fn set_storage(
        &mut self,
        address: &AddressWithSpace,
        key: Vec<u8>,
        value: U256,
    ) -> DbResult<()>;

    fn set_system_storage(&mut self, key: Vec<u8>, value: U256) -> DbResult<()>;

    fn get_system_storage(&self, key: &[u8]) -> DbResult<U256>;
}

pub trait AsStateOpsTrait: StateOpsTrait {
    fn as_state_ops(&self) -> &dyn StateOpsTrait;
    fn as_mut_state_ops(&mut self) -> &mut dyn StateOpsTrait;
}

pub trait CheckpointTrait: StateOpsTrait {
    /// Create a recoverable checkpoint of this state. Return the checkpoint
    /// index. The checkpoint records any old value which is alive at the
    /// creation time of the checkpoint and updated after that and before
    /// the creation of the next checkpoint.
    fn checkpoint(&mut self) -> usize;

    /// Merge last checkpoint with previous.
    /// Caller should make sure the function
    /// `collect_ownership_changed()` was called before calling
    /// this function.
    fn discard_checkpoint(&mut self);

    /// Revert to the last checkpoint and discard it.
    fn revert_to_checkpoint(&mut self);
}

use super::CleanupMode;
use crate::substate_trait::SubstateTrait;
use cfx_internal_common::{debug::ComputeEpochDebugRecord, StateRootWithAuxInfo};
use cfx_statedb::Result as DbResult;
use cfx_types::{AddressWithSpace, H256, U256};
use primitives::{EpochId, StorageLayout};
use std::sync::Arc;
