// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use cfx_types::AddressWithSpace;

// The original StorageKeys unprocessed, in contrary to StorageKey which is
// processed to use in DeltaMpt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StateKey<'a> {
    AccountKey(&'a AddressWithSpace),
    StorageKey {
        address: &'a AddressWithSpace,
        storage_key: &'a [u8],
    },
    CodeKey(&'a AddressWithSpace),
}

impl<'a> StateKey<'a> {
    pub fn new_account_key(address: &'a AddressWithSpace) -> Self {
        StateKey::AccountKey(address)
    }

    pub fn new_storage_key(address: &'a AddressWithSpace, storage_key: &'a [u8]) -> Self {
        StateKey::StorageKey {
            address,
            storage_key,
        }
    }

    pub fn new_code_key(address: &'a AddressWithSpace) -> Self {
        StateKey::CodeKey(address)
    }

    pub fn into_owned(self) -> OwnedStateKey {
        match self {
            StateKey::AccountKey(address) => OwnedStateKey::AccountKey(address.clone()),
            StateKey::StorageKey {
                address,
                storage_key,
            } => OwnedStateKey::StorageKey {
                address: address.clone(),
                storage_key: storage_key.to_vec(),
            },
            StateKey::CodeKey(address) => OwnedStateKey::CodeKey(address.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum OwnedStateKey {
    AccountKey(AddressWithSpace),
    StorageKey {
        address: AddressWithSpace,
        storage_key: Vec<u8>,
    },
    CodeKey(AddressWithSpace),
}

impl<'a> From<OwnedStateKey> for Vec<u8> {
    fn from(key: OwnedStateKey) -> Self {
        const STORAGE_PREFIX: [u8; 5] = *b"store";
        const CODE_PREFIX: [u8; 4] = *b"code";

        match key {
            OwnedStateKey::AccountKey(address) => [&address.address.0[..]].concat(),
            OwnedStateKey::StorageKey {
                address,
                storage_key,
            } => [&address.address.0[..], &STORAGE_PREFIX, &storage_key].concat(),
            OwnedStateKey::CodeKey(address) => [&address.address.0[..], &CODE_PREFIX].concat(),
        }
    }
}
