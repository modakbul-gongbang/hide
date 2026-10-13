//! Opt-in fixed-build responsiveness comparison, using the actual private SSH fixture.
#![cfg(unix)]

#[test]
#[ignore = "measurement requires built release CLIs, pinned Herdr and a private Chrome"]
fn connected_catalog_measurement() -> anyhow::Result<()> {
    crate::support::remote_delivery::Fixture::measurement_journey(|fixture| {
        crate::support::remote_delivery::responsive::measure(fixture)
    })
}
