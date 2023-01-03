// Put StateDb in mod to make sure that methods from statedb_ext don't access
// its fields directly.

use super::*;
use cfx_internal_common::{debug::ComputeEpochDebugRecord, StateRootWithAuxInfo};
use cfx_storage::{utils::access_mode, MptKeyValue};
use cfx_types::AddressWithSpace;

use primitives::{EpochId, StorageKeyWithSpace, StorageLayout};
use std::sync::Arc;

// Use generic type for better test-ability.
pub struct StateDb {}

impl StateDb {
    pub fn new() -> Self {
        StateDb {}
    }

    pub fn get_raw(&self, key: StorageKeyWithSpace) -> Result<Option<Arc<[u8]>>> {
        todo!()
    }
    pub fn set_raw(
        &mut self,
        key: StorageKeyWithSpace,
        value: Box<[u8]>,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        todo!()
    }

    pub fn delete(
        &mut self,
        key: StorageKeyWithSpace,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        todo!()
    }

    pub fn delete_all<AM: access_mode::AccessMode>(
        &mut self,
        key_prefix: StorageKeyWithSpace,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<Vec<MptKeyValue>> {
        todo!()
    }

    pub fn set_storage_layout(
        &mut self,
        address: &AddressWithSpace,
        storage_layout: StorageLayout,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        todo!()
    }

    pub fn compute_state_root(
        &mut self,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<StateRootWithAuxInfo> {
        todo!()
    }

    pub fn commit(
        &mut self,
        epoch_id: EpochId,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<StateRootWithAuxInfo> {
        todo!()
    }
}
