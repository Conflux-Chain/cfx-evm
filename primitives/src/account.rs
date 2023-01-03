// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

use crate::{bytes::Bytes, hash::KECCAK_EMPTY};
use cfx_types::{Address, AddressSpaceUtil, AddressWithSpace, Space, H256, U256};
use rlp::{Decodable, DecoderError, Encodable, Rlp, RlpStream};
use rlp_derive::{RlpDecodable, RlpEncodable};
use serde_derive::{Deserialize, Serialize};

use std::{fmt, sync::Arc};

#[derive(Debug, PartialEq, Clone)]
pub enum AddressSpace {
    Builtin,
    User,
    Contract,
}

#[derive(Debug, PartialEq, Clone)]
pub enum AccountError {
    ReservedAddressSpace(Address),
    AddressSpaceMismatch(Address, AddressSpace),
    InvalidRlp(DecoderError),
}

#[derive(
    Clone, Debug, RlpDecodable, RlpEncodable, Ord, PartialOrd, Eq, PartialEq, Serialize, Deserialize,
)]
#[serde(rename_all = "camelCase")]
pub struct VoteStakeInfo {
    /// This is the number of tokens should be locked before
    /// `unlock_block_number`.
    pub amount: U256,
    /// This is the timestamp when the vote right will be invalid, measured in
    /// the number of past blocks.
    pub unlock_block_number: u64,
}
#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct CodeInfo {
    pub code: Arc<Bytes>,
}

impl CodeInfo {
    #[inline]
    pub fn code_size(&self) -> usize {
        self.code.len()
    }
}

impl Encodable for CodeInfo {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.begin_list(2).append(&*self.code);
    }
}

impl Decodable for CodeInfo {
    fn decode(rlp: &Rlp) -> Result<Self, DecoderError> {
        Ok(Self {
            code: Arc::new(rlp.val_at(0)?),
        })
    }
}

#[derive(Clone, Debug, Ord, PartialOrd, Eq, PartialEq)]
pub struct Account {
    /// This field is not part of Account data, but kept for convenience. It
    /// should be rarely used except for debugging.
    address_local_info: AddressWithSpace,
    pub balance: U256,
    pub nonce: U256,
    pub code_hash: H256,
}

/// Defined for Rlp serialization/deserialization.
#[derive(RlpEncodable, RlpDecodable)]
pub struct BasicAccount {
    pub balance: U256,
    pub nonce: U256,
}

/// Defined for Rlp serialization/deserialization.
#[derive(RlpEncodable, RlpDecodable)]
pub struct ContractAccount {
    pub balance: U256,
    pub nonce: U256,
    pub code_hash: H256,
}

#[derive(RlpEncodable, RlpDecodable)]
pub struct EthereumAccount {
    pub balance: U256,
    pub nonce: U256,
    pub code_hash: H256,
}

impl Account {
    pub fn address(&self) -> &AddressWithSpace {
        &self.address_local_info
    }

    pub fn set_address(&mut self, address: AddressWithSpace) {
        self.address_local_info = address;
    }

    pub fn new_empty(address: &AddressWithSpace) -> Account {
        Self::new_empty_with_balance(address, &U256::from(0), &U256::from(0))
    }

    pub fn new_empty_with_balance(
        address: &AddressWithSpace,
        balance: &U256,
        nonce: &U256,
    ) -> Account {
        Self {
            address_local_info: *address,
            balance: *balance,
            nonce: *nonce,
            code_hash: KECCAK_EMPTY,
        }
    }

    fn from_ethereum_account(address: Address, a: EthereumAccount) -> Self {
        let address = address.with_evm_space();
        Self {
            address_local_info: address,
            balance: a.balance,
            nonce: a.nonce,
            code_hash: a.code_hash,
            ..Self::new_empty(&address)
        }
    }

    pub fn to_evm_account(&self) -> EthereumAccount {
        assert_eq!(self.address_local_info.space, Space::Ethereum);
        EthereumAccount {
            balance: self.balance,
            nonce: self.nonce,
            code_hash: self.code_hash,
        }
    }

    pub fn new_from_rlp(address: Address, rlp: &Rlp) -> Result<Self, AccountError> {
        let account = match rlp.item_count()? {
            3 => Self::from_ethereum_account(address, EthereumAccount::decode(rlp)?),
            _ => {
                return Err(AccountError::InvalidRlp(DecoderError::RlpIncorrectListLen));
            }
        };
        Ok(account)
    }
}

impl Encodable for Account {
    fn rlp_append(&self, stream: &mut RlpStream) {
        stream.append_internal(&self.to_evm_account());
        return;
    }
}

impl From<DecoderError> for AccountError {
    fn from(err: DecoderError) -> Self {
        AccountError::InvalidRlp(err)
    }
}

impl fmt::Display for AccountError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let msg = match self {
            AccountError::ReservedAddressSpace(address) => {
                format!("Address space is reserved for {:?}", address)
            }
            AccountError::AddressSpaceMismatch(address, address_space) => format!(
                "Address {:?} not in address space {:?}",
                address, address_space
            ),
            AccountError::InvalidRlp(err) => {
                format!("Transaction has invalid RLP structure: {}.", err)
            }
        };

        f.write_fmt(format_args!("Account error ({})", msg))
    }
}

impl std::error::Error for AccountError {
    fn description(&self) -> &str {
        "Account error"
    }
}

#[cfg(test)]
fn test_random_account(type_bit: Option<u8>, non_empty_hash: bool, contract_type: bool) {
    let mut address = Address::random();
    address.set_address_type_bits(type_bit.unwrap_or(0x40));

    let admin = Address::random();
    let sponsor_info = SponsorInfo {
        sponsor_for_gas: Address::random(),
        sponsor_for_collateral: Address::random(),
        sponsor_balance_for_gas: U256::from(123),
        sponsor_balance_for_collateral: U256::from(124),
        sponsor_gas_bound: U256::from(2),
    };

    let code_hash = if non_empty_hash {
        H256::random()
    } else {
        KECCAK_EMPTY
    };

    let account = if contract_type {
        Account::from_contract_account(
            address,
            ContractAccount {
                balance: 1000.into(),
                nonce: 123.into(),
                code_hash,
                staking_balance: 10000000.into(),
                collateral_for_storage: 23.into(),
                accumulated_interest_return: 456.into(),
                admin,
                sponsor_info,
            },
        )
    } else {
        Account::from_basic_account(
            address,
            BasicAccount {
                balance: 1000.into(),
                nonce: 123.into(),
                staking_balance: 10000000.into(),
                collateral_for_storage: 23.into(),
                accumulated_interest_return: 456.into(),
            },
        )
    };
    assert_eq!(
        account,
        Account::new_from_rlp(
            account.address_local_info.address,
            &Rlp::new(&account.rlp_bytes()),
        )
        .unwrap()
    );
}

#[test]
fn test_account_serde() {
    // Original normal address
    test_random_account(Some(0x10), false, false);
    // Original contract address
    test_random_account(Some(0x80), true, true);
    // Uninitialized contract address && new normal address
    test_random_account(Some(0x80), false, true);

    // New normal address
    test_random_account(None, false, false);
    test_random_account(Some(0x80), false, false);

    test_random_account(None, true, true);
    test_random_account(Some(0x80), true, true);
}
