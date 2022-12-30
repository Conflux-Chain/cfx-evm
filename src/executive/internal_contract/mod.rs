// Copyright 2019 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

mod components;
mod contracts;
mod impls;
mod utils;

pub use self::{
    components::{InterfaceTrait, InternalContractMap, InternalContractTrait, InternalRefContext},
    contracts::cross_space::{is_call_create_sig, is_withdraw_sig},
    impls::admin::suicide,
};
