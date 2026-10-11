//! The hotkey side: the line protocol and the client command. The daemon's end of the socket is
//! served by [`crate::daemon`].

pub(crate) mod client;
pub(crate) mod protocol;
