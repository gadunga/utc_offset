#![allow(clippy::doc_markdown, clippy::missing_errors_doc)]

#[cfg(test)]
mod test;

use std::{process::Command, str};

use once_cell::sync::OnceCell;
use parking_lot::RwLock;
use thiserror::Error;
use time::{
    error::{ComponentRange, Format},
    format_description::FormatItem,
    macros::format_description,
    Duration, OffsetDateTime, UtcOffset,
};

/// Wrapper with error defaulted to our [enum@Error].
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Soft errors that may occur in the process of initializing the global utc offset.
#[derive(Debug)]
pub struct Errors(Vec<Error>);
impl Errors {
    fn new() -> Self {
        Self(vec![])
    }
    fn push(&mut self, e: Error) {
        self.0.push(e)
    }
}

/// An enumeration of all possible error that may occur here.
#[derive(Error, Debug)]
pub enum Error {
    /// Failure acquiring the write lock as it was likely poisoned.
    #[error("Unable to acquire a write lock")]
    WriteLock,

    /// Failure acquiring the read lock as it was likely poisoned.
    #[error("Unable to acquire a read lock")]
    ReadLock,

    /// An error occurred Parsing a time string.
    #[error("Unable to parse time: {0}")]
    Parse(#[from] time::error::Parse),

    /// The values used to create a UTC Offset were invalid.
    #[error("Unable to construct offset from offset hours/minutes: {0}")]
    Time(#[from] ComponentRange),

    /// The library was failed to create a timestamp string from a date/time
    /// struct
    #[error("Unable to format timestamp: {0}")]
    TimeFormat(#[from] Format),

    /// An invalid value for the offset hours was passed in.
    #[error("Invalid offset hours: {0}")]
    InvalidOffsetHours(i8),

    /// An invalid value for the offset minutes was passed in.
    #[error("Invalid offset minutes: {0}")]
    InvalidOffsetMinutes(i8),

    /// An invalid value for the offset minutes was passed in.
    #[error("Unable to parse offset string")]
    InvalidOffsetString,

    /// An error occurred executing the system-specific command to get the current time.
    #[error("Error executing command to get system time: {0}")]
    TimeCommand(std::io::Error),

    /// There was an overflow computing the Datetime
    #[error("Datetime overflow")]
    DatetimeOverflow,

    /// The global offset is not initialized.
    #[error("The global offset is not initialized.")]
    Uninitialized,
}

static OFFSET: OnceCell<RwLock<UtcOffset>> = OnceCell::new();
const TIME_FORMAT: &[FormatItem<'static>] = format_description!(
    "[year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_second]"
);
const PARSE_FORMAT: &[FormatItem<'static>] =
    format_description!("[offset_hour][optional [:]][offset_minute]");

/// Returns the global offset value if it is initialized, otherwise it
/// returns an error. Unlike the `try_set_` functions, this waits for a read lock.
pub fn get_global_offset() -> Result<UtcOffset> {
    if let Some(o) = OFFSET.get() {
        Ok(o.read().clone())
    } else {
        Err(Error::Uninitialized)
    }
}
/// Attempts to set the global offset, returning an error if the
/// write lock cannot be obtained.
pub fn try_set_global_offset(o: UtcOffset) -> Result<()> {
    let o_ref = OFFSET.get_or_init(|| RwLock::new(o));
    if let Some(mut o_lock) = o_ref.try_write() {
        *o_lock = o;
        Ok(())
    } else {
        Err(Error::WriteLock)
    }
}

/// Sets a static UTC offset, from an input string, to use with future calls to
/// `get_local_timestamp_rfc3339`. The format should be [+/-]HHMM.
///
/// # Arguments
/// * input - The UTC offset as a string. Example values are: +0900, -0930,
///   1000, +09:00, -09:30, 10:00
///
/// # Error
/// If we fail to parse the input offset string we'll return an `Error::InvalidOffsetString`.
pub fn try_set_global_offset_from_str(input: &str) -> Result<()> {
    let trimmed = trim_new_lines(input);
    let o = UtcOffset::parse(trimmed, &PARSE_FORMAT).map_err(|_| Error::InvalidOffsetString)?;
    try_set_global_offset(o)
}

/// Sets a static UTC offset to use with future calls to
/// `get_local_timestamp_rfc3339`
///
/// # Arguments
/// * offset_hours - the hour value of the UTC offset, cannot be less than -12
///   or greater than 14
/// * offset_minutes - the minute value of the UTC offset, cannot be less than 0
///   or greater than 59
///
/// # Errors
/// If the offsets are out of range or there is an issue setting the offset an error will be returned.
#[allow(clippy::manual_range_contains)]
pub fn try_set_global_offset_from_pair(offset_hours: i8, offset_minutes: i8) -> Result<()> {
    let o = from_offset_pair(offset_hours, offset_minutes)?;
    try_set_global_offset(o)
}

/// Gets a timestamp string using in either the local offset or +00:00
///
/// # Returns
/// Returns a `Result` of either the timestamp in the following format or the error encountered during its construction.
/// ```text
/// [year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_second]
/// ```
/// , or an error if the method fails.
/// The timezone will be in the local offset IF any of the following succeed:
///     1.) set_global_offset is called.
///     2.) `time::UtcOffset::current_local_offset()` works
///     3.) The library is able to query the timezone using system commands.
/// If none succeed, we default to UTC.
pub fn get_local_timestamp_rfc3339() -> Result<(String, Errors)> {
    let (offset, errs) = get_utc_offset();
    let res = get_local_timestamp_from_offset_rfc3339(offset)?;
    Ok((res, errs))
}

/// Gets a timestamp string using the specified offset
///
/// # Arguments
/// * utc_offset - A caller specified offset
///
/// # Returns
/// Returns a `Result` timestamp in the following format or the error encountered during its construction.
/// ```text
/// [year]-[month]-[day]T[hour]:[minute]:[second][offset_hour sign:mandatory]:[offset_second]
/// ```
#[allow(clippy::cast_lossless)]
pub fn get_local_timestamp_from_offset_rfc3339(utc_offset: UtcOffset) -> Result<String> {
    let dt_now = OffsetDateTime::now_utc();
    let offset_dt_now = if utc_offset == UtcOffset::UTC {
        dt_now
    } else {
        // verify: I changed this to minutes resolution, any reason it used to be `whole_minutes`?
        if let Some(t) = dt_now.checked_add(Duration::minutes(utc_offset.whole_minutes() as i64)) {
            t.replace_offset(utc_offset)
        } else {
            // datetime overflow (not just hours, total representable time)
            return Err(Error::DatetimeOverflow);
        }
    };

    let formatted = offset_dt_now.format(&TIME_FORMAT)?;
    Ok(formatted)
}

/// Do whatever it takes to get a utc offset and cache it.
/// Worst case scenario we just assume UTC time.
pub fn get_utc_offset() -> (UtcOffset, Errors) {
    let mut errs = Errors::new();
    if let Ok(o) = get_global_offset() {
        return (o, errs);
    }

    let o = match construct_offset() {
        Ok(o) => o,
        Err(e) => {
            errs.push(e);
            UtcOffset::UTC
        }
    };

    if let Err(e) = try_set_global_offset(o) {
        errs.push(e)
    }
    (o, errs)
}

fn parse_cmd_output(stdout: &[u8], formatter: &[FormatItem<'static>]) -> Result<UtcOffset> {
    let output = String::from_utf8_lossy(stdout);
    let trimmed = trim_new_lines(&output);
    let offset = UtcOffset::parse(trimmed, &formatter)?;
    Ok(offset)
}

fn offset_from_process() -> Result<UtcOffset> {
    let cmd = if cfg!(target_os = "windows") {
        || {
            Command::new("powershell")
                .arg("Get-Date")
                .arg("-Format")
                .arg("\"K \"")
                .output()
        }
    } else {
        || Command::new("date").arg("+%z").output()
    };

    match cmd() {
        Ok(output) => parse_cmd_output(&output.stdout, &PARSE_FORMAT),
        Err(e) => Err(Error::TimeCommand(e)),
    }
}

fn trim_new_lines(s: &str) -> &str {
    s.trim().trim_end_matches("\r\n").trim_matches('\n')
}

fn from_offset_pair(offset_hours: i8, offset_minutes: i8) -> Result<UtcOffset> {
    if offset_hours < -12 || offset_hours > 14 {
        return Err(Error::InvalidOffsetHours(offset_hours));
    } else if !(0..=59).contains(&offset_minutes) {
        return Err(Error::InvalidOffsetMinutes(offset_minutes));
    }

    Ok(UtcOffset::from_hms(offset_hours, offset_minutes, 0)?)
}

/// Construct an offset.
fn construct_offset() -> Result<UtcOffset> {
    UtcOffset::current_local_offset().or_else(|_| offset_from_process())
}
