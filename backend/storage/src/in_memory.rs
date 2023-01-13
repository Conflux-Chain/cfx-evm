use std::{collections::HashMap, sync::RwLock};

use crate::StorageTrait;

type Bytes = Vec<u8>;
#[derive(Default)]
pub struct InMemoryDb {
    inner: RwLock<HashMap<Bytes, Box<[u8]>>>,
}

impl InMemoryDb {
    pub fn new() -> Self {
        Self::default()
    }
}

impl StorageTrait for InMemoryDb {
    type StorageKey = Bytes;

    fn get(&self, key: Self::StorageKey) -> crate::Result<Option<Box<[u8]>>> {
        Ok(self.inner.read().unwrap().get(&key).cloned())
    }

    fn set(&mut self, access_key: Self::StorageKey, value: Box<[u8]>) -> crate::Result<()> {
        self.inner.get_mut().unwrap().insert(access_key, value);
        Ok(())
    }

    fn delete(&mut self, access_key: Self::StorageKey) -> crate::Result<()> {
        self.inner.get_mut().unwrap().remove(&access_key);
        Ok(())
    }

    fn commit(&mut self, _epoch: primitives::EpochId) -> crate::Result<()> {
        Ok(())
    }
}
