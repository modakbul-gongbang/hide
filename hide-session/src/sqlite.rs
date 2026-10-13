//! URI policy for native databases: never create or write a WAL index.
use std::path::Path;

pub(crate) fn read_uri(path: &Path, wal: bool) -> Result<String, &'static str> {
    use percent_encoding::{AsciiSet, CONTROLS, utf8_percent_encode};
    const URI_PATH: &AsciiSet = &CONTROLS.add(b'%').add(b'?').add(b'#').add(b' ');
    let spelling = hide_platform::path::to_wire(path).map_err(|_| "database_path_unconfirmed")?;
    if spelling.starts_with("//") {
        return Err("database_path_unconfirmed");
    }
    let leading = if spelling.starts_with('/') {
        "file:"
    } else {
        "file:/"
    };
    let policy = if wal { "readonly_shm=1" } else { "immutable=1" };
    Ok(format!(
        "{leading}{}?mode=ro&{policy}",
        utf8_percent_encode(&spelling, URI_PATH)
    ))
}
