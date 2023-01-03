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
    statedb_ext::{StateDbExt, TOTAL_TOKENS_KEY},
};
