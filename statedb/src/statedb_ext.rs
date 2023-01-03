// Copyright 2020 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use rlp::Rlp;

use crate::StateDbTrait;
use cfx_internal_common::debug::ComputeEpochDebugRecord;
use cfx_parameters::internal_contract_addresses::STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS;
use cfx_types::{AddressWithSpace, H256, U256};
use primitives::{is_default::IsDefault, Account, CodeInfo, StorageKey, StorageKeyWithSpace};

use super::Result;

pub const TOTAL_TOKENS_KEY: &'static [u8] = b"total_issued_tokens";

pub trait StateDbExt: StateDbTrait {
    fn get<T>(&self, key: StorageKeyWithSpace) -> Result<Option<T>>
    where
        T: ::rlp::Decodable,
    {
        match self.get_raw(key) {
            Ok(None) => Ok(None),
            Ok(Some(raw)) => Ok(Some(::rlp::decode::<T>(raw.as_ref())?)),
            Err(e) => bail!(e),
        }
    }

    fn set<T>(
        &mut self,
        key: StorageKeyWithSpace,
        value: &T,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    where
        T: ::rlp::Encodable + IsDefault,
    {
        if value.is_default() {
            self.delete(key, debug_record)
        } else {
            self.set_raw(key, ::rlp::encode(value).into_boxed_slice(), debug_record)
        }
    }

    fn get_account(&self, address: &AddressWithSpace) -> Result<Option<Account>> {
        match self.get_raw(StorageKey::new_account_key(&address.address).with_space(address.space))
        {
            Ok(None) => Ok(None),
            Ok(Some(raw)) => Ok(Some(Account::new_from_rlp(
                address.address,
                &Rlp::new(&raw),
            )?)),
            Err(e) => bail!(e),
        }
    }

    fn get_code(&self, address: &AddressWithSpace, code_hash: &H256) -> Result<Option<CodeInfo>> {
        self.get::<CodeInfo>(
            StorageKey::new_code_key(&address.address, code_hash).with_space(address.space),
        )
    }
    fn get_total_issued_tokens(&self) -> Result<U256> {
        let total_issued_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            TOTAL_TOKENS_KEY,
        )
        .with_evm_space();
        let total_issued_tokens_opt = self.get::<U256>(total_issued_tokens_key)?;
        Ok(total_issued_tokens_opt.unwrap_or_default())
    }

    fn set_total_issued_tokens(
        &mut self,
        total_issued_tokens: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()> {
        let total_issued_tokens_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            TOTAL_TOKENS_KEY,
        )
        .with_evm_space();
        self.set::<U256>(total_issued_tokens_key, total_issued_tokens, debug_record)
    }
}

impl<T: StateDbTrait> StateDbExt for T {}
