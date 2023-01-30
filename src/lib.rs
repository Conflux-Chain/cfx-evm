extern crate cfx_bytes as bytes;
extern crate keccak_hash as hash;
extern crate substrate_bn as bn;
#[macro_use]
extern crate lazy_static;
#[macro_use]
extern crate log;
#[macro_use]
extern crate error_chain;

mod builtin;
mod call_create_frame;
mod evm;
pub mod execution;
mod internal_contract;
mod machine;
pub mod observer;
mod spec;
mod state;
pub mod vm;
mod vm_factory;

pub use call_create_frame::contract_address;
pub use execution::TXExecutor;
pub use execution::{ExecutionOutcome, TransactOptions, TransactionInfo};
pub use machine::{new_machine_with_builtin, Machine};
pub use spec::CommonParams;
pub use state::State;
pub use vm::{Env, Spec};
pub use vm_factory::VmFactory;

pub use cfx_state::StateTrait;
pub use cfx_statedb::{StateDb, StateDbExt, StateDbTrait};
pub use cfx_storage::StorageTrait;
