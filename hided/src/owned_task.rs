//! Tasks owned by the code that spawned them.

/// Ends a task when its owner ends: a handler future dropped mid-await
/// takes the task it spawned with it.
pub(crate) struct AbortOnDrop(pub(crate) tokio::task::AbortHandle);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}
