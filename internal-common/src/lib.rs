extern crate serde;
extern crate serde_derive;

pub mod chain_id;

pub mod debug {
    use serde_derive::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ComputeEpochDebugRecord;
}

pub use self::chain_id::{ChainIdParams, ChainIdParamsInner};
