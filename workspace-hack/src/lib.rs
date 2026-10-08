//! Holds no code: its manifest turns on, for each third-party crate, the
//! union of the features any workspace member asks of it, so every cargo
//! command builds that crate once (`cargo hakari`, docs/BUILD.md).
