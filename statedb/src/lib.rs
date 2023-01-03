#![allow(unused_variables)]
// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

#[macro_use]
extern crate error_chain;
#[allow(unused)]
#[macro_use]
extern crate log;

mod error;
mod impls;
mod statedb_ext;

#[cfg(test)]
mod tests;

pub use self::{
    error::{Error, ErrorKind, Result},
    impls::StateDb,
    statedb_ext::{
        StateDbExt, ACCUMULATE_INTEREST_RATE_KEY, DISTRIBUTABLE_POS_INTEREST_KEY,
        INTEREST_RATE_KEY, LAST_DISTRIBUTE_BLOCK_KEY, TOTAL_BANK_TOKENS_KEY,
        TOTAL_POS_STAKING_TOKENS_KEY, TOTAL_STORAGE_TOKENS_KEY, TOTAL_TOKENS_KEY,
    },
};
