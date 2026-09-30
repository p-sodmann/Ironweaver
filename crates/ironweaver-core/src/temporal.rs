// temporal.rs
//
// Attribute value types beyond JSON's: calendar dates, date-times and byte
// strings. They serialize as readable text in human-readable formats (JSON:
// "2024-05-01", "2024-05-01T12:30:00.250+02:00", base64) and compactly in
// binary ones (postcard: days, microseconds and offset, raw bytes).

use chrono::{DateTime as ChronoDateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, Timelike};
use serde::de::{self, Deserializer, Visitor};
use serde::ser::Serializer;
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt;

use crate::GraphError;

const EPOCH: NaiveDate = match NaiveDate::from_ymd_opt(1970, 1, 1) {
    Some(d) => d,
    None => panic!("valid date"),
};

/// A calendar date: days since 1970-01-01 (negative before).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date(pub i32);

impl Date {
    /// The date for a year, month (1-12) and day (1-31).
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Result<Self, GraphError> {
        let d = NaiveDate::from_ymd_opt(year, month, day)
            .ok_or_else(|| GraphError::InvalidArgument(format!("invalid date {year}-{month}-{day}")))?;
        Ok(Date::from_naive(d))
    }

    fn from_naive(d: NaiveDate) -> Self {
        Date(d.signed_duration_since(EPOCH).num_days() as i32)
    }

    fn naive(self) -> NaiveDate {
        EPOCH + chrono::Duration::days(self.0 as i64)
    }

    /// (year, month, day).
    pub fn ymd(self) -> (i32, u32, u32) {
        let d = self.naive();
        (d.year(), d.month(), d.day())
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.naive().format("%Y-%m-%d"))
    }
}

impl std::str::FromStr for Date {
    type Err = GraphError;

    fn from_str(s: &str) -> Result<Self, GraphError> {
        NaiveDate::parse_from_str(s, "%Y-%m-%d")
            .map(Date::from_naive)
            .map_err(|_| GraphError::InvalidArgument(format!("invalid date '{s}' (expected YYYY-MM-DD)")))
    }
}

/// A point in time with microsecond precision.
///
/// With an `offset` (seconds east of UTC) it is an instant: `micros` counts
/// from 1970-01-01T00:00 UTC, and the offset is kept for display. Without,
/// it is a wall-clock time in an unknown zone (Python's "naive" datetimes):
/// `micros` counts from 1970-01-01T00:00 local. Two instants are equal when
/// they are the same moment, whatever their offsets; an instant never
/// equals (or compares with) a wall-clock time.
#[derive(Clone, Copy, Debug)]
pub struct DateTime {
    pub micros: i64,
    pub offset: Option<i32>,
}

/// Date and time of day, as `DateTime::from_parts` takes them and
/// `DateTime::parts` returns them (in the value's own offset).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parts {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    pub microsecond: u32,
}

impl DateTime {
    /// The date-time with these wall-clock parts, at `offset` seconds east
    /// of UTC (an instant) or without offset (wall-clock).
    pub fn from_parts(p: Parts, offset: Option<i32>) -> Result<Self, GraphError> {
        let invalid = || GraphError::InvalidArgument(format!("invalid date-time {p:?}"));
        let naive = NaiveDate::from_ymd_opt(p.year, p.month, p.day)
            .and_then(|d| d.and_hms_micro_opt(p.hour, p.minute, p.second, p.microsecond))
            .ok_or_else(invalid)?;
        if let Some(o) = offset {
            FixedOffset::east_opt(o).ok_or_else(|| {
                GraphError::InvalidArgument(format!("UTC offset of {o} seconds is out of range (under a day)"))
            })?;
        }
        let local = naive.and_utc().timestamp_micros();
        Ok(DateTime { micros: local - offset.unwrap_or(0) as i64 * 1_000_000, offset })
    }

    /// Wall-clock date and time (in the value's offset, if it has one).
    pub fn parts(self) -> Parts {
        let n = self.local();
        Parts {
            year: n.year(),
            month: n.month(),
            day: n.day(),
            hour: n.hour(),
            minute: n.minute(),
            second: n.second(),
            microsecond: n.nanosecond() / 1000 % 1_000_000,
        }
    }

    fn local(self) -> NaiveDateTime {
        let micros = self.micros + self.offset.unwrap_or(0) as i64 * 1_000_000;
        ChronoDateTime::from_timestamp_micros(micros).map_or(NaiveDateTime::MIN, |d| d.naive_utc())
    }

    /// Order of two instants or two wall-clock times; `None` for a mix.
    pub fn partial_order(&self, other: &DateTime) -> Option<Ordering> {
        (self.offset.is_some() == other.offset.is_some()).then(|| self.micros.cmp(&other.micros))
    }
}

impl PartialEq for DateTime {
    fn eq(&self, other: &DateTime) -> bool {
        self.partial_order(other) == Some(Ordering::Equal)
    }
}

impl fmt::Display for DateTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let local = self.local();
        write!(f, "{}", local.format("%Y-%m-%dT%H:%M:%S"))?;
        let micros = local.nanosecond() / 1000 % 1_000_000;
        if micros != 0 {
            write!(f, ".{micros:06}")?;
        }
        if let Some(o) = self.offset {
            let (sign, o) = if o < 0 { ('-', -o) } else { ('+', o) };
            write!(f, "{sign}{:02}:{:02}", o / 3600, o / 60 % 60)?;
            if o % 60 != 0 {
                write!(f, ":{:02}", o % 60)?;
            }
        }
        Ok(())
    }
}

impl std::str::FromStr for DateTime {
    type Err = GraphError;

    /// ISO 8601 / RFC 3339: `2024-05-01T12:30:00`, optional fraction, and an
    /// optional `Z` / `+HH:MM[:SS]` offset.
    fn from_str(s: &str) -> Result<Self, GraphError> {
        let invalid = || GraphError::InvalidArgument(format!("invalid date-time '{s}'"));
        let (body, offset) = split_offset(s).ok_or_else(invalid)?;
        let naive = NaiveDateTime::parse_from_str(body, "%Y-%m-%dT%H:%M:%S%.f").map_err(|_| invalid())?;
        let p = Parts {
            year: naive.year(),
            month: naive.month(),
            day: naive.day(),
            hour: naive.hour(),
            minute: naive.minute(),
            second: naive.second(),
            microsecond: naive.nanosecond() / 1000,
        };
        DateTime::from_parts(p, offset)
    }
}

/// Splits a trailing `Z` or `±HH:MM[:SS]` off; the offset in seconds.
fn split_offset(s: &str) -> Option<(&str, Option<i32>)> {
    if let Some(body) = s.strip_suffix('Z') {
        return Some((body, Some(0)));
    }
    // The offset starts at the last sign after the time's 'T'
    let t = s.find('T')?;
    let Some(at) = s[t..].rfind(['+', '-']).map(|i| t + i) else {
        return Some((s, None));
    };
    let sign = if s.as_bytes()[at] == b'-' { -1 } else { 1 };
    let mut seconds = 0;
    let fields: Vec<&str> = s[at + 1..].split(':').collect();
    if !(2..=3).contains(&fields.len()) {
        return None;
    }
    for (field, scale) in fields.iter().zip([3600, 60, 1]) {
        if field.len() != 2 || !field.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        seconds += field.parse::<i32>().ok()? * scale;
    }
    Some((&s[..at], Some(sign * seconds)))
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(self)
        } else {
            s.serialize_i32(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
            s.parse().map_err(de::Error::custom)
        } else {
            i32::deserialize(d).map(Date)
        }
    }
}

impl Serialize for DateTime {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.collect_str(self)
        } else {
            (self.micros, self.offset).serialize(s)
        }
    }
}

impl<'de> Deserialize<'de> for DateTime {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        if d.is_human_readable() {
            let s = <std::borrow::Cow<'de, str>>::deserialize(d)?;
            s.parse().map_err(de::Error::custom)
        } else {
            let (micros, offset) = <(i64, Option<i32>)>::deserialize(d)?;
            if offset.is_some_and(|o| o.abs() >= 86_400) {
                return Err(de::Error::custom("UTC offset out of range"));
            }
            Ok(DateTime { micros, offset })
        }
    }
}

/// Serde for byte strings: base64 text in human-readable formats, raw bytes
/// otherwise (`#[serde(with = "bytes")]`).
pub mod bytes {
    use super::*;

    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    /// Standard base64 with padding.
    pub fn encode(data: &[u8]) -> String {
        let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
        for chunk in data.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                if i <= chunk.len() {
                    out.push(ALPHABET[(n >> (18 - 6 * i) & 63) as usize] as char);
                } else {
                    out.push('=');
                }
            }
        }
        out
    }

    /// Decodes standard base64 (padding required).
    pub fn decode(s: &str) -> Option<Vec<u8>> {
        let s = s.as_bytes();
        if s.len() % 4 != 0 {
            return None;
        }
        let mut out = Vec::with_capacity(s.len() / 4 * 3);
        for (c, chunk) in s.chunks(4).enumerate() {
            let last = c == s.len() / 4 - 1;
            let pad = chunk.iter().rev().take_while(|&&b| b == b'=').count();
            if pad > 2 || (pad > 0 && !last) {
                return None;
            }
            let mut n = 0u32;
            for &b in &chunk[..4 - pad] {
                let v = ALPHABET.iter().position(|&a| a == b)? as u32;
                n = n << 6 | v;
            }
            n <<= 6 * pad as u32;
            let bytes = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            out.extend_from_slice(&bytes[..3 - pad]);
        }
        Some(out)
    }

    pub fn serialize<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
        if s.is_human_readable() {
            s.serialize_str(&encode(v))
        } else {
            s.serialize_bytes(v)
        }
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
        struct BytesVisitor;

        impl<'de> Visitor<'de> for BytesVisitor {
            type Value = Vec<u8>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("bytes (base64 in text formats)")
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Vec<u8>, E> {
                decode(v).ok_or_else(|| E::custom("invalid base64"))
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Vec<u8>, E> {
                Ok(v.to_vec())
            }
            fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Vec<u8>, E> {
                Ok(v)
            }
        }

        if d.is_human_readable() {
            d.deserialize_str(BytesVisitor)
        } else {
            // Copied anyway, so no need to borrow (and streams can't lend)
            d.deserialize_byte_buf(BytesVisitor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let d = Date::from_ymd(2024, 2, 29).unwrap();
        assert_eq!(d.ymd(), (2024, 2, 29));
        assert_eq!(d.to_string(), "2024-02-29");
        assert_eq!("2024-02-29".parse::<Date>().unwrap(), d);
        assert_eq!(Date::from_ymd(1970, 1, 1).unwrap(), Date(0));
        assert_eq!(Date::from_ymd(1969, 12, 31).unwrap(), Date(-1));
        assert_eq!(Date::from_ymd(1, 1, 1).unwrap().ymd(), (1, 1, 1));
        assert!(Date::from_ymd(2023, 2, 29).is_err());
        assert!("2024-13-01".parse::<Date>().is_err());
    }

    #[test]
    fn date_times() {
        let p = Parts { year: 2024, month: 5, day: 1, hour: 12, minute: 30, second: 5, microsecond: 250 };
        let aware = DateTime::from_parts(p, Some(7200)).unwrap();
        assert_eq!(aware.parts(), p);
        assert_eq!(aware.to_string(), "2024-05-01T12:30:05.000250+02:00");
        let utc = DateTime::from_parts(Parts { hour: 10, ..p }, Some(0)).unwrap();
        assert_eq!(aware, utc);
        assert_eq!(utc.to_string(), "2024-05-01T10:30:05.000250+00:00");
        let naive = DateTime::from_parts(p, None).unwrap();
        assert_ne!(naive, aware);
        assert_eq!(naive.partial_order(&aware), None);
        assert_eq!(naive.to_string(), "2024-05-01T12:30:05.000250");
        for s in ["2024-05-01T12:30:05.000250+02:00", "2024-05-01T12:30:05", "1901-01-01T00:00:00-05:30:15"] {
            assert_eq!(s.parse::<DateTime>().unwrap().to_string(), s);
        }
        assert_eq!("2024-05-01T10:30:05.000250Z".parse::<DateTime>().unwrap(), aware);
        assert!("2024-05-01".parse::<DateTime>().is_err());
        assert!("2024-05-01T25:00:00".parse::<DateTime>().is_err());
        assert!(DateTime::from_parts(p, Some(86_400)).is_err());
    }

    #[test]
    fn base64() {
        for data in [&b""[..], b"f", b"fo", b"foo", b"foob", b"fooba", b"foobar", &[0, 255, 128, 7]] {
            let e = bytes::encode(data);
            assert_eq!(bytes::decode(&e).unwrap(), data, "{e}");
        }
        assert_eq!(bytes::encode(b"foobar"), "Zm9vYmFy");
        assert_eq!(bytes::encode(b"fo"), "Zm8=");
        assert!(bytes::decode("Zm8").is_none());
        assert!(bytes::decode("Z=8=").is_none());
        assert!(bytes::decode("Zm8*").is_none());
    }
}
