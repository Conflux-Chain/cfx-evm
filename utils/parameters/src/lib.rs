// Copyright 2020 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

#[macro_use]
extern crate lazy_static;

pub mod internal_contract_addresses;

pub mod consensus {
    pub const ONE_CFX_IN_DRIP: u64 = 1_000_000_000_000_000_000;
}

pub mod block {
    // The following parameter controls how many blocks are allowed to
    // contain EVM Space transactions. Setting it to N means that one block
    // must has a height of the multiple of N to contain EVM transactions.
    pub const EVM_TRANSACTION_BLOCK_RATIO: u64 = 5;
    // The following parameter controls the ratio of gas limit allowed for
    // EVM space transactions. Setting it to N means that only 1/N of th
    // block gas limit can be used for EVM transaction enabled blocks.
    pub const EVM_TRANSACTION_GAS_RATIO: u64 = 2;
    // The following parameter controls the ratio of gas can be passed to EVM
    // space in the cross space call. Setting it to N means that only 1/N of gas
    // left can be passed to the cross space call.
    pub const CROSS_SPACE_GAS_RATIO: u64 = 10;
}
