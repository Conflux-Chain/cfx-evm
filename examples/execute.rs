extern crate cfx_evm;

use cfx_evm::{
    new_machine_with_builtin, CommonParams, Env, ExecutionOutcome, State, TXExecutor,
    TransactOptions, VmFactory,
};
use cfx_state::state_trait::StateOpsTrait;
use cfx_statedb::StateDb;
use cfx_storage::InMemoryDb;
use cfx_types::{Address, AddressSpaceUtil, U256};
use cfxkey::Generator;
use cfxkey::Random;
use primitives::{Action, Eip155Transaction, SignedTransaction, Transaction};

fn main() {
    // 1. Prepare for context
    let params = CommonParams::default();
    let vm_factory = VmFactory::new(20480 * 1024);
    let machine = new_machine_with_builtin(params, vm_factory);
    let spec = machine.params().spec(1);
    let mut env = Env::default();

    // 2. Prepare for backend
    let storage = Box::new(InMemoryDb::new());
    let state_db = StateDb::new(storage);
    let mut state = State::new(state_db).unwrap();

    // 3. Prepare for task
    let sender_key = Random.generate().unwrap();
    let sender = sender_key.address();
    let sender_with_space = sender.with_evm_space();
    let address = Address::random();
    // let address_with_space = address.with_evm_space();

    let tx: SignedTransaction = Transaction::from(Eip155Transaction {
        nonce: 0.into(),
        gas_price: U256::from(1),
        gas: U256::from(100_000),
        value: U256::from(1_000_000),
        action: Action::Call(address),
        chain_id: Some(1),
        data: vec![],
    })
    .sign(&sender_key.secret());

    state
        .add_balance(
            &sender_with_space,
            &U256::from(1_000_000_000),
            cfx_state::CleanupMode::NoEmpty,
            U256::zero(),
        )
        .expect("no db error");

    // 4. Execute
    let mut executor = TXExecutor::new(&mut state, &env, &machine, &spec);
    let outcome = executor
        .transact(&tx, TransactOptions::exec_with_no_tracing())
        .expect("no db error");
    dbg!(outcome);
}
