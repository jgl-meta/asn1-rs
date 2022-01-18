use crate::datetime::decode_decimal;
use crate::*;
use alloc::format;
use alloc::string::String;
#[cfg(feature = "datetime")]
use chrono::{DateTime, TimeZone, Utc};
use core::convert::TryFrom;
use core::fmt;

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GeneralizedTime(pub ASN1DateTime);

impl GeneralizedTime {
    pub const fn new(datetime: ASN1DateTime) -> Self {
        GeneralizedTime(datetime)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        // X.680 section 42 defines a GeneralizedTime as a VisibleString restricted to:
        //
        // a) a string representing the calendar date, as specified in ISO 8601, with a four-digit representation of the
        //    year, a two-digit representation of the month and a two-digit representation of the day, without use of
        //    separators, followed by a string representing the time of day, as specified in ISO 8601, without separators
        //    other than decimal comma or decimal period (as provided for in ISO 8601), and with no terminating Z (as
        //    provided for in ISO 8601); or
        // b) the characters in a) above followed by an upper-case letter Z ; or
        // c) he characters in a) above followed by a string representing a local time differential, as specified in
        //    ISO 8601, without separators.
        let (year, month, day, hour, minute, rem) = match bytes {
            [year1, year2, year3, year4, mon1, mon2, day1, day2, hour1, hour2, min1, min2, rem @ ..] =>
            {
                let year_hi = decode_decimal(Self::TAG, *year1, *year2)?;
                let year_lo = decode_decimal(Self::TAG, *year3, *year4)?;
                let year = (year_hi as u32) * 100 + (year_lo as u32);
                let month = decode_decimal(Self::TAG, *mon1, *mon2)?;
                let day = decode_decimal(Self::TAG, *day1, *day2)?;
                let hour = decode_decimal(Self::TAG, *hour1, *hour2)?;
                let minute = decode_decimal(Self::TAG, *min1, *min2)?;
                (year, month, day, hour, minute, rem)
            }
            _ => return Err(Self::TAG.invalid_value("malformed time string (not yymmddhhmm)")),
        };
        if rem.is_empty() {
            return Err(Self::TAG.invalid_value("malformed time string"));
        }
        // check for seconds
        let (second, rem) = match rem {
            [sec1, sec2, rem @ ..] => {
                let second = decode_decimal(Self::TAG, *sec1, *sec2)?;
                (second, rem)
            }
            _ => (0, rem),
        };
        if month > 12 || day > 31 || hour > 23 || minute > 59 || second > 59 {
            // eprintln!("GeneralizedTime: time checks failed");
            // eprintln!(" month:{}", month);
            // eprintln!(" day:{}", day);
            // eprintln!(" hour:{}", hour);
            // eprintln!(" minute:{}", minute);
            // eprintln!(" second:{}", second);
            return Err(Self::TAG.invalid_value("time components with invalid values"));
        }
        if rem.is_empty() {
            // case a): no fractional seconds part, and no terminating Z
            return Ok(GeneralizedTime(ASN1DateTime::new(
                year,
                month,
                day,
                hour,
                minute,
                second,
                None,
                ASN1TimeZone::Undefined,
            )));
        }
        // check for fractional seconds
        let (millisecond, rem) = match rem {
            [b'.' | b',', rem @ ..] => {
                let mut fsecond = 0;
                let mut rem = rem;
                for idx in 0..=4 {
                    if rem.is_empty() {
                        if idx == 0 {
                            // dot or comma, but no following digit
                            return Err(Self::TAG.invalid_value(
                                "malformed time string (dot or comma but no digits)",
                            ));
                        }
                        break;
                    }
                    if idx == 4 {
                        return Err(
                            Self::TAG.invalid_value("malformed time string (invalid milliseconds)")
                        );
                    }
                    match rem[0] {
                        b'0'..=b'9' => {
                            // XXX check for overflow in mul
                            fsecond = fsecond * 10 + (rem[0] - b'0') as u32;
                        }
                        b'Z' | b'+' | b'-' => {
                            break;
                        }
                        _ => {
                            return Err(Self::TAG.invalid_value(
                                "malformed time string (invalid milliseconds/timezone)",
                            ))
                        }
                    }
                    rem = &rem[1..];
                }
                (Some(fsecond), rem)
            }
            _ => (None, rem),
        };
        // check timezone
        if rem.is_empty() {
            // case a): fractional seconds part, and no terminating Z
            return Ok(GeneralizedTime(ASN1DateTime::new(
                year,
                month,
                day,
                hour,
                minute,
                second,
                millisecond,
                ASN1TimeZone::Undefined,
            )));
        }
        let tz = match rem {
            [b'Z'] => ASN1TimeZone::Z,
            [b'+', h1, h2, m1, m2] => {
                let hh = decode_decimal(Self::TAG, *h1, *h2)?;
                let mm = decode_decimal(Self::TAG, *m1, *m2)?;
                ASN1TimeZone::Offset(1, hh, mm)
            }
            [b'-', h1, h2, m1, m2] => {
                let hh = decode_decimal(Self::TAG, *h1, *h2)?;
                let mm = decode_decimal(Self::TAG, *m1, *m2)?;
                ASN1TimeZone::Offset(-1, hh, mm)
            }
            _ => return Err(Self::TAG.invalid_value("malformed time string: no time zone")),
        };
        Ok(GeneralizedTime(ASN1DateTime::new(
            year,
            month,
            day,
            hour,
            minute,
            second,
            millisecond,
            tz,
        )))
    }

    /// Return a ISO 8601 combined date and time with time zone.
    #[cfg(feature = "datetime")]
    #[cfg_attr(docsrs, doc(cfg(feature = "datetime")))]
    pub fn utc_datetime(&self) -> DateTime<Utc> {
        let dt = &self.0;
        // XXX Utc only if Z
        Utc.ymd(dt.year as i32, dt.month as u32, dt.day as u32)
            .and_hms(dt.hour as u32, dt.minute as u32, dt.second as u32)
    }
}

impl<'a> TryFrom<Any<'a>> for GeneralizedTime {
    type Error = Error;

    fn try_from(any: Any<'a>) -> Result<GeneralizedTime> {
        any.tag().assert_eq(Self::TAG)?;
        #[allow(clippy::trivially_copy_pass_by_ref)]
        fn is_visible(b: &u8) -> bool {
            0x20 <= *b && *b <= 0x7f
        }
        if !any.data.iter().all(is_visible) {
            return Err(Error::StringInvalidCharset);
        }

        GeneralizedTime::from_bytes(any.data)
    }
}

impl fmt::Display for GeneralizedTime {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let dt = &self.0;
        let fsec = match self.0.millisecond {
            Some(v) => format!(".{}", v),
            None => String::new(),
        };
        match dt.tz {
            ASN1TimeZone::Undefined => write!(
                f,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{}",
                dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, fsec
            ),
            ASN1TimeZone::Z => write!(
                f,
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{} Z",
                dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, fsec
            ),
            ASN1TimeZone::Offset(sign, hh, mm) => {
                let s = if sign > 0 { '+' } else { '-' };
                write!(
                    f,
                    "{:04}-{:02}-{:02} {:02}:{:02}:{:02}{} {}{:02}{:02}",
                    dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second, fsec, s, hh, mm
                )
            }
        }
    }
}

impl<'a> CheckDerConstraints for GeneralizedTime {
    fn check_constraints(any: &Any) -> Result<()> {
        // X.690 section 11.7.1: The encoding shall terminate with a "Z"
        if any.data.last() != Some(&b'Z') {
            return Err(Error::DerConstraintFailed(DerConstraint::MissingTimeZone));
        }
        // X.690 section 11.7.2: The seconds element shall always be present.
        // XXX
        // X.690 section 11.7.4: The decimal point element, if present, shall be the point option "."
        if any.data.iter().any(|&b| b == b',') {
            return Err(Error::DerConstraintFailed(DerConstraint::MissingSeconds));
        }
        Ok(())
    }
}

impl<'a> Tagged for GeneralizedTime {
    const TAG: Tag = Tag::GeneralizedTime;
}

#[cfg(feature = "std")]
impl ToDer for GeneralizedTime {
    fn to_der_len(&self) -> Result<usize> {
        // data:
        // - 8 bytes for YYYYMMDD
        // - 6 for hhmmss in DER (X.690 section 11.7.2)
        // - (variable) the fractional part, without trailing zeros, with a point "."
        // - 1 for the character Z in DER (X.690 section 11.7.1)
        // data length: 15 + fractional part
        //
        // thus, length will always be on 1 byte (short length) and
        // class+structure+tag also on 1
        //
        // total: = 1 (class+constructed+tag) + 1 (length) + 13 + fractional
        let num_digits = match self.0.millisecond {
            None => 0,
            Some(v) => 1 + v.to_string().len(),
        };
        Ok(15 + num_digits)
    }

    fn write_der_header(&self, writer: &mut dyn std::io::Write) -> SerializeResult<usize> {
        // see above for length value
        let num_digits = match self.0.millisecond {
            None => 0,
            Some(v) => 1 + v.to_string().len() as u8,
        };
        writer
            .write(&[Self::TAG.0 as u8, 15 + num_digits])
            .map_err(Into::into)
    }

    fn write_der_content(&self, writer: &mut dyn std::io::Write) -> SerializeResult<usize> {
        let fractional = match self.0.millisecond {
            None => "".to_string(),
            Some(v) => format!(".{}", v),
        };
        let num_digits = fractional.len();
        let _ = write!(
            writer,
            "{:04}{:02}{:02}{:02}{:02}{:02}{}Z",
            self.0.year,
            self.0.month,
            self.0.day,
            self.0.hour,
            self.0.minute,
            self.0.second,
            fractional,
        )?;
        // write_fmt returns (), see above for length value
        Ok(15 + num_digits)
    }
}
