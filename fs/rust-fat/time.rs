// SPDX-License-Identifier: GPL-2.0-or-later

//! Utilities for handling time in FAT.

use kernel::prelude::*;
use kernel::time::Timespec;

const SECONDS_PER_MINUTE: u64 = 60;
const SECONDS_PER_HOUR: u64 = SECONDS_PER_MINUTE * 60;
const SECONDS_PER_DAY: u64 = SECONDS_PER_HOUR * 24;
/// Days between 1970-01-01 and 1980-01-01 (2 leap days).
const FAT_EPOCH_DAY_SHIFT: u64 = 365 * 10 + 2;
const FAT_YEAR_2100: u16 = 120;
/// Cumulative number of days until the respective 1sts in non-leap years.
#[rustfmt::skip]
const CUMULATIVE_DAYS_IN_YEAR: [u32; 12] = [
    // Jan  Feb  Mar  Apr  May  Jun  Jul  Aug  Sep  Oct  Nov  Dec
	     0,  31,  59,  90, 120, 151, 181, 212, 243, 273, 304, 334,
];

/// Convert FAT time fields into a [`Timespec`].
pub(crate) fn timespec_from_fat(date: u16, time: u16, centiseconds: u8) -> Result<Timespec> {
    let year = date >> 9;
    let month = ((date >> 5) & 0b1111).clamp(1, 12) - 1;
    let day = (date & 0b11111).clamp(1, 31) - 1;

    let hour = time >> 11;
    let minute = ((time >> 5) & 0b111111).clamp(0, 59);
    let second = (time & 0b11111).clamp(0, 29) * 2 + (centiseconds as u16 / 100);

    let mut leap_days = (year + 3) / 4;
    if year > FAT_YEAR_2100 {
        // 2100 is not a leap year
        leap_days -= 1;
    }
    if year % 4 == 0 && year != FAT_YEAR_2100 && month > 2 {
        leap_days += 1;
    }

    let sec = 0
        + SECONDS_PER_DAY
            * (FAT_EPOCH_DAY_SHIFT
                + year as u64 * 365
                + CUMULATIVE_DAYS_IN_YEAR[month as usize] as u64
                + day as u64
                + leap_days as u64)
        + SECONDS_PER_HOUR * hour as u64
        + SECONDS_PER_MINUTE * minute as u64
        + second as u64;

    // TODO: time zone offset

    Timespec::new(sec, (centiseconds % 100) as u32 * 10000000)
}
