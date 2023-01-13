use std::marker::PhantomData;

use primitives::EpochId;

#[macro_use]
extern crate error_chain;

mod in_memory;

pub use in_memory::InMemoryDb;

error_chain! {
    links {
    }

    foreign_links {
    }

    errors {
    }
}

pub trait StorageTrait {
    type StorageKey;

    // Actions.
    fn get(&self, key: Self::StorageKey) -> Result<Option<Box<[u8]>>>;
    fn set(&mut self, access_key: Self::StorageKey, value: Box<[u8]>) -> Result<()>;
    fn delete(&mut self, access_key: Self::StorageKey) -> Result<()>;
    fn commit(&mut self, epoch: EpochId) -> Result<()>;
}

pub struct StorageKeyWrapper<T, Key> {
    pub inner: T,
    pub _key: PhantomData<Key>,
}

impl<T, Key> StorageTrait for StorageKeyWrapper<T, Key>
where
    T: StorageTrait,
    <T as StorageTrait>::StorageKey: From<Key>,
{
    type StorageKey = Key;

    fn get(&self, key: Self::StorageKey) -> Result<Option<Box<[u8]>>> {
        self.inner.get(key.into())
    }

    fn set(&mut self, access_key: Self::StorageKey, value: Box<[u8]>) -> Result<()> {
        self.inner.set(access_key.into(), value)
    }

    fn delete(&mut self, access_key: Self::StorageKey) -> Result<()> {
        self.inner.delete(access_key.into())
    }

    fn commit(&mut self, epoch: EpochId) -> Result<()> {
        self.inner.commit(epoch)
    }
}
