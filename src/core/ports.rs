//! What the core needs from the outside world, as two narrow traits.

use std::io;

use crate::core::command::Command;
use crate::core::input::WindowInfo;
use crate::state_file::StateFile;

/// Persists the desired state. Called before any compositor command of an operation.
pub(crate) trait StateStore {
    fn save(&mut self, state: &StateFile) -> io::Result<()>;
}

/// The compositor: carries out commands and answers the current window list.
pub(crate) trait Compositor {
    /// Carry out these commands, in order.
    fn execute(&mut self, commands: &[Command]) -> io::Result<()>;
    /// The current window list; the engine reads it back after each batch.
    fn list_windows(&mut self) -> io::Result<Vec<WindowInfo>>;
}

/// Both ports behind one reference, which is how the engine takes them.
pub(crate) trait Ports: StateStore + Compositor {}
impl<T: StateStore + Compositor> Ports for T {}

/// Composes a separate store and compositor into one `Ports`.
pub(crate) struct PortPair<S, C> {
    pub(crate) store: S,
    pub(crate) compositor: C,
}

impl<S: StateStore, C: Compositor> StateStore for PortPair<S, C> {
    fn save(&mut self, state: &StateFile) -> io::Result<()> {
        self.store.save(state)
    }
}

impl<S: StateStore, C: Compositor> Compositor for PortPair<S, C> {
    fn execute(&mut self, commands: &[Command]) -> io::Result<()> {
        self.compositor.execute(commands)
    }
    fn list_windows(&mut self) -> io::Result<Vec<WindowInfo>> {
        self.compositor.list_windows()
    }
}
