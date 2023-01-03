use crate::{
    account::{Account, CodeInfo},
    hash::KECCAK_EMPTY,
    StorageValue,
};
use cfx_types::U256;

/// This trait checks whether a variable equals to initialization value.
/// For a variable equals to the initialization value, the world-state should
/// treat is as None value.
pub trait IsDefault {
    fn is_default(&self) -> bool;
}

impl IsDefault for Account {
    fn is_default(&self) -> bool {
        self.balance == U256::zero() && self.nonce == U256::zero() && self.code_hash == KECCAK_EMPTY
    }
}

impl IsDefault for CodeInfo {
    fn is_default(&self) -> bool {
        self.code.len() == 0
    }
}

impl IsDefault for StorageValue {
    fn is_default(&self) -> bool {
        self.value == U256::zero()
    }
}

impl IsDefault for U256 {
    fn is_default(&self) -> bool {
        self.is_zero()
    }
}

impl IsDefault for bool {
    fn is_default(&self) -> bool {
        !self
    }
}
