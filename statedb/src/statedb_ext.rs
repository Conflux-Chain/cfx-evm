// Copyright 2020 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use rlp::Rlp;

use cfx_internal_common::debug::ComputeEpochDebugRecord;
use cfx_parameters::internal_contract_addresses::STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS;
use cfx_types::{AddressWithSpace, H256, U256};
use primitives::{is_default::IsDefault, Account, CodeInfo, StorageKey, StorageKeyWithSpace};

use super::{Result, StateDb};

pub trait StateDbExt {
    fn get<T>(&self, key: StorageKeyWithSpace) -> Result<Option<T>>
    where
        T: ::rlp::Decodable;

    fn set<T>(
        &mut self,
        key: StorageKeyWithSpace,
        value: &T,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>
    where
        T: ::rlp::Encodable + IsDefault;

    fn get_account(&self, address: &AddressWithSpace) -> Result<Option<Account>>;

    fn get_code(&self, address: &AddressWithSpace, code_hash: &H256) -> Result<Option<CodeInfo>>;

    fn get_total_issued_tokens(&self) -> Result<U256>;
    fn set_total_issued_tokens(
        &mut self,
        total_issued_tokens: &U256,
        debug_record: Option<&mut ComputeEpochDebugRecord>,
    ) -> Result<()>;

    // This function is used to check whether the db has been initialized when
    // create a state. So we can know the loaded `None` represents "not
    // initialized" or "zero value".
    fn is_initialized(&self) -> Result<bool>;
}

pub const ACCUMULATE_INTEREST_RATE_KEY: &'static [u8] = b"accumulate_interest_rate";
pub const INTEREST_RATE_KEY: &'static [u8] = b"interest_rate";
pub const TOTAL_BANK_TOKENS_KEY: &'static [u8] = b"total_staking_tokens";
pub const TOTAL_STORAGE_TOKENS_KEY: &'static [u8] = b"total_storage_tokens";
pub const TOTAL_TOKENS_KEY: &'static [u8] = b"total_issued_tokens";
pub const TOTAL_POS_STAKING_TOKENS_KEY: &'static [u8] = b"total_pos_staking_tokens";
pub const DISTRIBUTABLE_POS_INTEREST_KEY: &'static [u8] = b"distributable_pos_interest";
pub const LAST_DISTRIBUTE_BLOCK_KEY: &'static [u8] = b"last_distribute_block";

impl StateDbExt for StateDb {
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

    fn is_initialized(&self) -> Result<bool> {
        let interest_rate_key = StorageKey::new_storage_key(
            &STORAGE_INTEREST_STAKING_CONTRACT_ADDRESS,
            INTEREST_RATE_KEY,
        )
        .with_evm_space();
        let interest_rate_opt = self.get::<U256>(interest_rate_key)?;
        Ok(interest_rate_opt.is_some())
    }
}
