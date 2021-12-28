use std::{
    fmt::{Display, Formatter},
    num::ParseIntError,
    ops::Add,
    str::FromStr,
};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Date {
    pub d: u32, // days since UNIX epoch (0 = 1970-01-01)
}

impl Date {
    pub fn today() -> Self {
        Self {
            d: (Utc::now().timestamp() / 86400) as u32,
        }
    }
}

impl From<u32> for Date {
    fn from(d: u32) -> Self {
        Self { d }
    }
}

impl From<Date> for u32 {
    fn from(inst: Date) -> u32 {
        inst.d
    }
}

impl FromStr for Date {
    type Err = ParseIntError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split("-").collect();
        let result = Self {
            d: (Utc
                .ymd(parts[0].parse()?, parts[1].parse()?, parts[2].parse()?)
                .and_hms(0, 0, 0)
                .timestamp()
                / 86400) as u32,
        };
        Ok(result)
    }
}

impl Display for Date {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        let date_time = Utc.timestamp(self.d as i64 * 86400, 0);
        write!(f, "{}", date_time.format("%Y-%m-%d"))
    }
}

impl Add<u16> for Date
{
    type Output = Self;

    fn add(self, duration: u16) -> Self {
        Self {
            d: self.d + duration as u32,
        }
    }
}
