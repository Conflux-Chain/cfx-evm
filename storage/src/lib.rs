#[macro_use]
extern crate error_chain;

error_chain! {
    links {
    }

    foreign_links {
    }

    errors {
    }
}

pub type MptKeyValue = (Vec<u8>, Box<[u8]>);

pub mod access_mode {
    pub trait AccessMode {
        fn is_read_only() -> bool;
    }

    pub struct Read {}
    pub struct Write {}

    impl AccessMode for Read {
        fn is_read_only() -> bool {
            return true;
        }
    }

    impl AccessMode for Write {
        fn is_read_only() -> bool {
            return false;
        }
    }
}

pub mod utils {
    pub use super::access_mode;
}
