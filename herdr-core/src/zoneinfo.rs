//! Wall-clock time in a named zone, from the system's own tz database.
//!
//! `claude -p /usage` prints a reset as a local wall time and an IANA zone
//! name (`Sep 24 at 1pm (Asia/Seoul)`), and the popover needs the instant.
//! The core carries no calendar dependency, so this reads the zone's TZif
//! file under `/usr/share/zoneinfo`, the same data `zoneinfo(3)` and Python's
//! `ZoneInfo` answer from. It reads the version 2 block, whose transition
//! times are 64-bit, and is deliberately narrow: it answers for instants the
//! file's transition table covers, and reports the ones it does not.
//!
//! macOS ships "fat" files whose tables run to 2037, so the footer rule for
//! later years is not parsed; a zone with a DST rule asked about an instant
//! past its last transition is refused as `zone_range` rather than answered
//! with the wrong offset. A zone without a rule (`Asia/Seoul`) keeps its last
//! offset indefinitely, which is what the footer would say.

use std::path::{Component, Path, PathBuf};

const ZONEINFO_ROOT: &str = "/usr/share/zoneinfo";
/// The largest zone file in a distribution is a few kilobytes; this refuses a
/// path that resolved to something else.
const MAX_FILE_BYTES: u64 = 256 * 1024;

pub(crate) struct Zone {
    /// `(utc_seconds, utoff_seconds)` per transition, ascending.
    transitions: Vec<(i64, i32)>,
    /// The offset before the first transition, or always when there is none.
    first_offset: i32,
    /// True when the footer names a DST rule for years past the table.
    footer_has_rule: bool,
}

impl Zone {
    /// Loads a zone by IANA name. The name is validated as a relative path of
    /// plain components before it touches the filesystem.
    pub(crate) fn load(name: &str) -> Result<Self, &'static str> {
        Self::load_from(Path::new(ZONEINFO_ROOT), name)
    }

    fn load_from(root: &Path, name: &str) -> Result<Self, &'static str> {
        let relative = zone_path(name).ok_or("zone_name")?;
        let path = root.join(relative);
        let size = std::fs::metadata(&path).map_err(|_| "zone_file")?.len();
        if size > MAX_FILE_BYTES {
            return Err("zone_file");
        }
        let bytes = std::fs::read(&path).map_err(|_| "zone_file")?;
        Self::parse(&bytes)
    }

    /// The UTC offset in force at `unix`.
    pub(crate) fn offset_at(&self, unix: i64) -> Result<i32, &'static str> {
        let index = self.transitions.partition_point(|(at, _)| *at <= unix);
        if index == 0 {
            return Ok(self.first_offset);
        }
        if index == self.transitions.len() && self.footer_has_rule {
            return Err("zone_range");
        }
        Ok(self.transitions[index - 1].1)
    }

    /// Every instant at which the zone's wall clock reads `local` (seconds
    /// since the epoch as if the wall time were UTC): one normally, none
    /// inside a spring-forward gap, two inside a fall-back overlap.
    pub(crate) fn instants_of(&self, local: i64) -> Result<Vec<i64>, &'static str> {
        let mut offsets = self
            .transitions
            .iter()
            .map(|(_, offset)| *offset)
            .chain(std::iter::once(self.first_offset))
            .collect::<Vec<_>>();
        offsets.sort_unstable();
        offsets.dedup();
        let mut instants = Vec::new();
        for offset in offsets {
            let candidate = local - i64::from(offset);
            if self.offset_at(candidate)? == offset {
                instants.push(candidate);
            }
        }
        instants.sort_unstable();
        instants.dedup();
        Ok(instants)
    }

    fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        let first = Header::read(bytes, 0)?;
        if first.version < 2 {
            return Err("zone_format");
        }
        // Skip the 32-bit block: the 64-bit one repeats it with wider times.
        let second_at = Header::LEN + first.block_len(4);
        let header = Header::read(bytes, second_at)?;
        let mut cursor = second_at + Header::LEN;
        let take = |cursor: &mut usize, len: usize| -> Result<&[u8], &'static str> {
            let slice = bytes.get(*cursor..*cursor + len).ok_or("zone_format")?;
            *cursor += len;
            Ok(slice)
        };
        let times = take(&mut cursor, header.timecnt * 8)?
            .as_chunks::<8>()
            .0
            .iter()
            .map(|chunk| i64::from_be_bytes(*chunk))
            .collect::<Vec<_>>();
        let indices = take(&mut cursor, header.timecnt)?.to_vec();
        let types = take(&mut cursor, header.typecnt * 6)?
            .as_chunks::<6>()
            .0
            .iter()
            .map(|chunk| i32::from_be_bytes(chunk[..4].try_into().expect("4 bytes")))
            .collect::<Vec<_>>();
        cursor += header.charcnt + header.leapcnt * 12 + header.isstdcnt + header.isutcnt;
        let footer = bytes.get(cursor..).ok_or("zone_format")?;
        let first_offset = *types.first().ok_or("zone_format")?;
        let mut transitions = Vec::with_capacity(times.len());
        for (at, index) in times.into_iter().zip(indices) {
            let offset = *types.get(usize::from(index)).ok_or("zone_format")?;
            transitions.push((at, offset));
        }
        if transitions.windows(2).any(|pair| pair[0].0 >= pair[1].0) {
            return Err("zone_format");
        }
        Ok(Self {
            transitions,
            first_offset,
            // A POSIX TZ string with a comma carries DST rules
            // (`EST5EDT,M3.2.0,M11.1.0`); without one it is a fixed offset.
            footer_has_rule: footer.contains(&b','),
        })
    }
}

struct Header {
    version: u8,
    isutcnt: usize,
    isstdcnt: usize,
    leapcnt: usize,
    timecnt: usize,
    typecnt: usize,
    charcnt: usize,
}

impl Header {
    const LEN: usize = 44;

    fn read(bytes: &[u8], at: usize) -> Result<Self, &'static str> {
        let header = bytes.get(at..at + Self::LEN).ok_or("zone_format")?;
        if &header[..4] != b"TZif" {
            return Err("zone_format");
        }
        let version = match header[4] {
            0 => 1,
            digit @ b'2'..=b'9' => digit - b'0',
            _ => return Err("zone_format"),
        };
        let count = |index: usize| -> usize {
            let start = 20 + index * 4;
            u32::from_be_bytes(header[start..start + 4].try_into().expect("4 bytes")) as usize
        };
        Ok(Self {
            version,
            isutcnt: count(0),
            isstdcnt: count(1),
            leapcnt: count(2),
            timecnt: count(3),
            typecnt: count(4),
            charcnt: count(5),
        })
    }

    /// The data block's length for the given transition-time width.
    fn block_len(&self, time_width: usize) -> usize {
        self.timecnt * time_width
            + self.timecnt
            + self.typecnt * 6
            + self.charcnt
            + self.leapcnt * (time_width + 4)
            + self.isstdcnt
            + self.isutcnt
    }
}

/// A zone name as a relative path: `Area/Location` components of the
/// characters IANA names use, never absolute and never climbing.
fn zone_path(name: &str) -> Option<PathBuf> {
    if name.is_empty()
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'_' | b'-' | b'+'))
    {
        return None;
    }
    let path = Path::new(name);
    path.components()
        .all(|component| matches!(component, Component::Normal(_)))
        .then(|| path.to_path_buf())
}

/// Days since 1970-01-01 of a proleptic Gregorian date; the inverse of
/// [`civil_from_days`]. Howard Hinnant's algorithm.
pub(crate) fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let year_of_era = year - era * 400;
    let month_prime = month + if month > 2 { -3 } else { 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

/// `(year, month, day)` of a day count since 1970-01-01.
pub(crate) fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let days = days + 719_468;
    let era = if days >= 0 { days } else { days - 146_096 } / 146_097;
    let day_of_era = days - era * 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    (era * 400 + year_of_era + i64::from(month <= 2), month, day)
}

pub(crate) fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn local(year: i64, month: i64, day: i64, hour: i64, minute: i64) -> i64 {
        days_from_civil(year, month, day) * 86_400 + hour * 3_600 + minute * 60
    }

    /// The expected instants are from `date -j -f` / Python `ZoneInfo`, not
    /// from this reader.
    #[test]
    fn seoul_has_one_instant_per_wall_time_at_plus_nine() {
        let zone = Zone::load("Asia/Seoul").unwrap();
        assert_eq!(
            zone.instants_of(local(2026, 9, 24, 13, 0)).unwrap(),
            vec![1_790_222_400]
        );
        assert_eq!(zone.offset_at(1_790_222_400).unwrap(), 9 * 3_600);
    }

    #[test]
    fn a_dst_zone_answers_the_gap_with_nothing_and_the_overlap_with_both() {
        let zone = Zone::load("America/New_York").unwrap();
        // 2026-03-08 02:30 does not exist: the clock jumps from 02:00 to 03:00.
        assert_eq!(
            zone.instants_of(local(2026, 3, 8, 2, 30)).unwrap(),
            Vec::<i64>::new()
        );
        // 2026-11-01 01:30 happens twice, EDT then EST.
        assert_eq!(
            zone.instants_of(local(2026, 11, 1, 1, 30)).unwrap(),
            vec![1_793_511_000, 1_793_514_600]
        );
        // 2026-09-24 13:00 EDT.
        assert_eq!(
            zone.instants_of(local(2026, 9, 24, 13, 0)).unwrap(),
            vec![1_790_269_200]
        );
    }

    #[test]
    fn a_rule_zone_refuses_years_past_its_table_and_a_fixed_zone_does_not() {
        let year_2050 = local(2050, 6, 1, 12, 0);
        assert_eq!(
            Zone::load("America/New_York").unwrap().offset_at(year_2050),
            Err("zone_range")
        );
        assert_eq!(
            Zone::load("Asia/Seoul").unwrap().offset_at(year_2050),
            Ok(9 * 3_600)
        );
    }

    #[test]
    fn a_zone_name_never_leaves_the_database_directory() {
        for name in [
            "../../etc/passwd",
            "/etc/localtime",
            "Asia/../Seoul",
            "",
            "Asia/Seoul\n",
        ] {
            assert_eq!(Zone::load(name).err(), Some("zone_name"), "{name:?}");
        }
        assert_eq!(Zone::load("Mars/Olympus").err(), Some("zone_file"));
    }

    #[test]
    fn civil_conversions_round_trip_across_leap_days() {
        for (year, month, day) in [(1970, 1, 1), (2000, 2, 29), (2026, 9, 24), (2100, 3, 1)] {
            let days = days_from_civil(year, month, day);
            assert_eq!(civil_from_days(days), (year, month, day));
        }
        assert_eq!(days_from_civil(2026, 9, 24), 20_720);
    }
}
