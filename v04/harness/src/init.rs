//! A world whose host disagrees with the engine's `Init` (ADR-065): the host is deployed with
//! edited `Params`, and `Init` has not run.

use mock_host_core::Params;

use crate::World;

impl World {
    /// [`World::bare`] with the host's `Params` edited by `edit` before the host is initialised.
    /// The engine is not initialised, so a test sends the `Init` it expects the host check to
    /// refuse (or accept) itself.
    pub fn bare_with_host(edit: impl FnOnce(&mut Params)) -> Self {
        Self::bare_with(crate::world::RAY, edit)
    }
}
