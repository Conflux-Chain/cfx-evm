// Copyright 2021 Conflux Foundation. All rights reserved.
// Conflux is free software and distributed under GNU General Public License.
// See http://www.gnu.org/licenses/

pub mod state_trait;
pub mod tracer;

use cfx_types::AddressWithSpace;
use std::collections::HashSet;

pub use state_trait::StateTrait;

/// Mode of dealing with null accounts.
#[derive(PartialEq)]
pub enum CleanupMode<'a> {
    /// Create accounts which would be null.
    ForceCreate,
    /// Don't delete null accounts upon touching, but also don't create them.
    NoEmpty,
    /// Mark all touched accounts.
    /// TODO: We have not implemented the correct behavior of TrackTouched for
    /// internal Contracts.
    TrackTouched(&'a mut HashSet<AddressWithSpace>),
}
