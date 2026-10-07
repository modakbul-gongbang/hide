//! The machine's local time zone.

use std::io;

/// The offset from UTC the machine's local time zone applies now, in
/// milliseconds, east positive (Seoul answers nine hours). A count by the
/// local day adds it to a UTC time before dividing by the day.
#[cfg(unix)]
pub fn local_utc_offset_ms() -> io::Result<i64> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs() as libc::time_t;
    let mut local = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: `localtime_r` reads `now` and either fills the whole `tm` it is
    // handed or returns null without promising anything about it.
    let filled = unsafe { libc::localtime_r(&now, local.as_mut_ptr()) };
    if filled.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: a non-null answer means `localtime_r` filled `local`.
    let local = unsafe { local.assume_init() };
    Ok(i64::from(local.tm_gmtoff) * 1000)
}

/// The offset from UTC the machine's local time zone applies now, in
/// milliseconds, east positive (Seoul answers nine hours). A count by the
/// local day adds it to a UTC time before dividing by the day.
#[cfg(windows)]
pub fn local_utc_offset_ms() -> io::Result<i64> {
    use windows_sys::Win32::System::Time::{GetTimeZoneInformation, TIME_ZONE_INFORMATION};
    const TIME_ZONE_ID_INVALID: u32 = u32::MAX;
    const TIME_ZONE_ID_STANDARD: u32 = 1;
    const TIME_ZONE_ID_DAYLIGHT: u32 = 2;
    // SAFETY: TIME_ZONE_INFORMATION is plain data; all zeroes is a valid value.
    let mut zone: TIME_ZONE_INFORMATION = unsafe { std::mem::zeroed() };
    // SAFETY: the pointer is to a live, writable TIME_ZONE_INFORMATION.
    let id = unsafe { GetTimeZoneInformation(&mut zone) };
    // Windows keeps the bias as minutes to add to local time to reach UTC.
    let bias = match id {
        TIME_ZONE_ID_INVALID => return Err(io::Error::last_os_error()),
        TIME_ZONE_ID_STANDARD => zone.Bias + zone.StandardBias,
        TIME_ZONE_ID_DAYLIGHT => zone.Bias + zone.DaylightBias,
        _ => zone.Bias,
    };
    Ok(-i64::from(bias) * 60_000)
}
