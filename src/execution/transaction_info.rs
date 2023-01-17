use cfx_types::{AddressWithSpace, Space, U256};
use primitives::{Action, SignedTransaction};
use std::borrow::Cow;
use Cow::{Borrowed, Owned};

pub trait TransactionInfo {
    fn sender(&self) -> Cow<AddressWithSpace>;
    fn nonce(&self) -> Cow<U256>;
    fn gas(&self) -> Cow<U256>;
    fn gas_price(&self) -> Cow<U256>;
    fn data(&self) -> Cow<[u8]>;
    fn action(&self) -> Cow<Action>;
    fn value(&self) -> Cow<U256>;

    fn space(&self) -> Space {
        Space::Ethereum
    }
}

impl TransactionInfo for SignedTransaction {
    fn sender(&self) -> Cow<AddressWithSpace> {
        Owned(self.sender())
    }

    fn nonce(&self) -> Cow<U256> {
        Borrowed(self.nonce())
    }

    fn gas(&self) -> Cow<U256> {
        Borrowed(self.gas())
    }

    fn gas_price(&self) -> Cow<U256> {
        Borrowed(self.gas_price())
    }

    fn data(&self) -> Cow<[u8]> {
        Borrowed((**self).data().as_ref())
    }

    fn action(&self) -> Cow<Action> {
        Borrowed((**self).action())
    }

    fn value(&self) -> Cow<U256> {
        Borrowed((**self).value())
    }
}
