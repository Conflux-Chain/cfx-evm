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

pub trait StorageTrait: Sync + Send {
    type StateKey;

    // Actions.
    fn get(&self, key: Self::StateKey) -> Result<Option<Box<[u8]>>>;
    fn set(&mut self, access_key: Self::StateKey, value: Box<[u8]>) -> Result<()>;
    fn delete(&mut self, access_key: Self::StateKey) -> Result<()>;
    fn commit(&mut self, epoch: EpochId) -> Result<()>;
}
