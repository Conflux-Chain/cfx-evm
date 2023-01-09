// Put StateDb in mod to make sure that methods from statedb_ext don't access
// its fields directly.

use super::*;
use cfx_internal_common::debug::ComputeEpochDebugRecord;

use cfx_storage::StorageTrait;
use primitives::StateKey;

// Use generic type for better test-ability.
pub struct StateDb {
    storage: Box<dyn StorageTrait<StateKey = Vec<u8>>>,
}

impl StateDb {
    pub fn new(storage: Box<dyn StorageTrait<StateKey = Vec<u8>>>) -> Self {
        StateDb { storage }
    }

    fn to_storage_key(key: StateKey) -> Vec<u8> {
        const STORAGE_PREFIX: [u8; 5] = *b"store";
        const CODE_PREFIX: [u8; 4] = *b"code";

        match key {
            StateKey::AccountKey(address) => [&address.address.0[..]].concat(),
            StateKey::StorageKey {
                address,
                storage_key,
            } => [&address.address.0[..], &STORAGE_PREFIX, storage_key].concat(),
            StateKey::CodeKey(address) => [&address.address.0[..], &CODE_PREFIX].concat(),
        }
    }
}

impl StateDbTrait for StateDb {
    fn get_raw(&self, key: StateKey) -> Result<Option<Box<[u8]>>> {
        self.storage
            .get(StateDb::to_storage_key(key))
            .map_err(Into::into)
    }
    fn set_raw(
        &mut self,
        key: StateKey,
        value: Box<[u8]>,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        self.storage
            .set(StateDb::to_storage_key(key), value)
            .map_err(Into::into)
    }

    fn delete(
        &mut self,
        key: StateKey,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        self.storage
            .delete(StateDb::to_storage_key(key))
            .map_err(Into::into)
    }

    fn commit(
        &mut self,
        epoch_id: EpochId,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        self.storage.commit(epoch_id).map_err(Into::into)
    }
}
