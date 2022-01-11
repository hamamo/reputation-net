use std::{
    fmt::{Display, Formatter},
    num::ParseIntError,
    ops::Add,
    str::FromStr,
};

use chrono::{TimeZone, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{Database, Decode, Encode, Type, TypeInfo};

#[derive(Clone, Copy, Debug, PartialEq, PartialOrd)]
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
        if parts.len() != 3 {
            let d: u32 = parts[0].parse()?;
            return Ok(Self::from(d));
        }
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

impl Add<u16> for Date {
    type Output = Self;

    fn add(self, duration: u16) -> Self {
        Self {
            d: self.d + duration as u32,
        }
    }
}

impl Serialize for Date {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.d.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let d = <u32 as Deserialize>::deserialize(deserializer)?;
        Ok(Self::from(d))
    }
}

impl<'r, DB> Decode<'r, DB> for Date
where
    DB: Database,
    u32: Decode<'r, DB>,
{
    fn decode(
        value: <DB as sqlx::database::HasValueRef<'r>>::ValueRef,
    ) -> Result<Self, sqlx::error::BoxDynError> {
        let value = <u32 as Decode<DB>>::decode(value)?;
        Ok(Self::from(value))
    }
}

impl<'q, DB> Encode<'q, DB> for Date
where
    DB: Database,
    u32: Encode<'q, DB>,
{
    fn encode_by_ref(
        &self,
        buf: &mut <DB as sqlx::database::HasArguments<'q>>::ArgumentBuffer,
    ) -> sqlx::encode::IsNull {
        self.d.encode(buf)
    }
}

impl<DB> Type<DB> for Date
where
    DB: Database,
    u32: Type<DB>,
{
    fn type_info() -> DB::TypeInfo {
        <u32 as Type<DB>>::type_info()
    }

    fn compatible(ty: &DB::TypeInfo) -> bool {
        ty.name() == Self::type_info().name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize() {
        let date = Date::from(12345);
        assert_eq!(serde_json::to_string(&date).unwrap(), "12345")
    }

    #[test]
    fn deserialize() {
        let date = Date::from(12345);
        assert_eq!(serde_json::from_str::<Date>("12345").unwrap(), date)
    }
}
