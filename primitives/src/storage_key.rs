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
}
