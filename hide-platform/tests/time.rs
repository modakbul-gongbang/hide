//! The local time zone offset, the same contract on every system.

use hide_platform::time::local_utc_offset_ms;

#[test]
fn the_local_offset_is_a_real_time_zone_offset() {
    let offset = local_utc_offset_ms().expect("the system names its time zone");
    // Every time zone in use sits within fourteen hours of UTC, on a quarter hour.
    assert!(offset.abs() <= 14 * 3_600_000, "{offset}");
    assert_eq!(offset % (15 * 60_000), 0, "{offset}");
}
