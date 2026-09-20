use alloc::string::String;
use core::fmt::Write;

use x86_64::instructions::interrupts;
use x86_64::instructions::port::Port;

#[derive(Debug, Clone, Copy)]
pub struct RtcTime {
    pub sec: u8,
    pub min: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub century: u16,
}

fn read_cmos(register: u8) -> u8 {
    interrupts::without_interrupts(|| {
        let mut index = Port::new(0x70);
        let mut data = Port::new(0x71);
        unsafe {
            index.write(register);
            data.read()
        }
    })
}

fn bcd_to_binary(value: u8, is_bcd: bool) -> u8 {
    if is_bcd {
        ((value >> 4) * 10) + (value & 0xF)
    } else {
        value
    }
}

fn is_leap_year(year: u16) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_in_month(year: u16, month: u8) -> u16 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 31,
    }
}

pub fn read_rtc_time() -> Option<RtcTime> {
    let mut last = (0, 0, 0, 0, 0, 0, 0);
    for _ in 0..5 {
        let status_a = read_cmos(0x0A);
        if status_a & 0x80 != 0 {
            continue;
        }
        let sec = read_cmos(0x00);
        let min = read_cmos(0x02);
        let hour = read_cmos(0x04);
        let day = read_cmos(0x07);
        let month = read_cmos(0x08);
        let year = read_cmos(0x09);
        let century = read_cmos(0x32);
        let status_b = read_cmos(0x0B);
        let is_bcd = status_b & 0x04 == 0;
        let is_24h = status_b & 0x02 != 0;
        let sec = bcd_to_binary(sec, is_bcd);
        let min = bcd_to_binary(min, is_bcd);
        let mut hour = bcd_to_binary(hour & 0x7F, is_bcd);
        if !is_24h {
            let pm = hour & 0x80 != 0;
            if pm && hour < 12 {
                hour = hour.wrapping_add(12);
            }
            if !pm && hour == 12 {
                hour = 0;
            }
        }
        let day = bcd_to_binary(day, is_bcd);
        let month = bcd_to_binary(month, is_bcd);
        let year = bcd_to_binary(year, is_bcd);
        let century = if century != 0 {
            (bcd_to_binary(century, is_bcd) as u16) * 100
        } else {
            2000
        };
        let current = (sec, min, hour, day, month, year, century);
        if current == last {
            return Some(RtcTime {
                sec,
                min,
                hour,
                day,
                month,
                year,
                century,
            });
        }
        last = current;
    }
    None
}

pub fn unix_seconds(time: &RtcTime) -> u64 {
    let year = time.century + time.year as u16;
    let mut days = 0u64;
    for y in 2000..year {
        days += if is_leap_year(y) { 366 } else { 365 };
    }
    for m in 1..time.month {
        days += days_in_month(year, m) as u64;
    }
    days += (time.day - 1) as u64;
    days * 86400 + (time.hour as u64) * 3600 + (time.min as u64) * 60 + (time.sec as u64)
}

pub fn format_human(time: &RtcTime) -> String {
    let mut buf = String::new();
    let year = time.century + time.year as u16;
    let _ = write!(
        &mut buf,
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        year, time.month, time.day, time.hour, time.min, time.sec
    );
    buf
}
