// Put StateDb in mod to make sure that methods from statedb_ext don't access
// its fields directly.

use std::marker::PhantomData;

use super::*;
use cfx_internal_common::debug::ComputeEpochDebugRecord;

use cfx_storage::{StorageKeyWrapper, StorageTrait};
use primitives::{OwnedStateKey, StateKey};

// Use generic type for better test-ability.
pub struct StateDb<'a> {
    storage: Box<dyn StorageTrait<StorageKey = OwnedStateKey> + 'a>,
}

impl<'a> StateDb<'a> {
    pub fn new<T, U>(storage: T) -> Self
    where
        T: StorageTrait<StorageKey = U> + 'a,
        U: From<OwnedStateKey>,
    {
        let storage = Box::new(StorageKeyWrapper {
            inner: storage,
            _key: PhantomData::<OwnedStateKey>,
        });
        StateDb { storage }
    }
}

impl<'a> StateDbTrait for StateDb<'a> {
    fn get_raw(&self, key: StateKey) -> Result<Option<Box<[u8]>>> {
        self.storage.get(key.into_owned()).map_err(Into::into)
    }
    fn set_raw(
        &mut self,
        key: StateKey,
        value: Box<[u8]>,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        self.storage
            .set(key.into_owned(), value)
            .map_err(Into::into)
    }

    fn delete(
        &mut self,
        key: StateKey,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        self.storage.delete(key.into_owned()).map_err(Into::into)
    }

    fn commit(
        &mut self,
        epoch_id: EpochId,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        self.storage.commit(epoch_id).map_err(Into::into)
    }
}
