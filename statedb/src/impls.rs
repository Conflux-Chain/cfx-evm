// Put StateDb in mod to make sure that methods from statedb_ext don't access
// its fields directly.

use super::*;
use cfx_internal_common::debug::ComputeEpochDebugRecord;

use primitives::StorageKeyWithSpace;
use std::sync::Arc;

// Use generic type for better test-ability.
pub struct StateDb {}

impl StateDb {
    pub fn new() -> Self {
        StateDb {}
    }
}

impl StateDbTrait for StateDb {
    fn get_raw(&self, key: StorageKeyWithSpace) -> Result<Option<Arc<[u8]>>> {
        todo!()
    }
    fn set_raw(
        &mut self,
        key: StorageKeyWithSpace,
        value: Box<[u8]>,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        todo!()
    }

    fn delete(
        &mut self,
        key: StorageKeyWithSpace,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        todo!()
    }
}
