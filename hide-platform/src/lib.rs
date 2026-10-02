//! Operating-system primitives, one module per concern.
//!
//! Every function here answers the same way on macOS, Linux and Windows, or
//! says `ErrorKind::Unsupported` where the system cannot answer; none guesses
//! a default. `tests/` states each module's contract in terms a caller can
//! observe and runs unchanged on all three systems.

pub mod fs;
pub mod ipc;
pub mod process;
