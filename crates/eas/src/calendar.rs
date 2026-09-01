//! Lossless, validated Calendar recurrence and exception values.

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::{CalendarFields, Patch};

/// Gregorian recurrence selector supported by EAS 14.1.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RecurrencePattern {
    /// Every N days.
    Daily,
    /// Selected weekdays; Sunday occupies bit zero.
    Weekly {
        /// Weekday bit mask.
        days: u8,
    },
    /// A numbered day in the month.
    Monthly {
        /// Day of month, from 1 through 31.
        day: u8,
    },
    /// An ordinal matching weekday in the month.
    MonthlyRelative {
        /// Weekday bit mask.
        days: u8,
        /// Ordinal 1-4, or 5 for last.
        week: u8,
    },
    /// A date in the year.
    Yearly {
        /// Month, from 1 through 12.
        month: u8,
        /// Day, from 1 through 31.
        day: u8,
    },
    /// An ordinal matching weekday in a month of the year.
    YearlyRelative {
        /// Month, from 1 through 12.
        month: u8,
        /// Weekday bit mask.
        days: u8,
        /// Ordinal 1-4, or 5 for last.
        week: u8,
    },
}

/// Exclusive recurrence termination alternatives.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum RecurrenceEnd {
    /// No specified ending.
    Never,
    /// Number of generated occurrences, including deleted exceptions.
    Count(u16),
    /// Last allowed original start, inclusive, in UTC.
    Until(DateTime<Utc>),
}

/// Validated EAS recurrence rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarRecurrence {
    /// Gregorian recurrence pattern.
    pub pattern: RecurrencePattern,
    /// Positive interval, at most 999.
    pub interval: u16,
    /// Week boundary, Sunday=0.
    pub first_day_of_week: u8,
    /// Explicit ending policy.
    pub end: RecurrenceEnd,
}

/// A changed or deleted occurrence, keyed by its original UTC start.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CalendarException {
    /// Original start, unaffected by a later move.
    pub original_start: DateTime<Utc>,
    /// Whether this occurrence was removed.
    pub deleted: bool,
    /// Presence-aware changes; recurrence nesting is never permitted.
    pub fields: CalendarFields,
}

/// Additional Calendar data retained across full-item writes.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize)]
pub struct CalendarProperties {
    /// Recurrence of the master, absent for a single event.
    pub recurrence: Option<CalendarRecurrence>,
    /// Existing overrides, at most 256.
    pub exceptions: Vec<CalendarException>,
    /// Original instance start for notification rendering only.
    pub instance_start: Option<DateTime<Utc>>,
    /// Original occurrence value type for RECURRENCE-ID, unaffected by a type change.
    pub instance_all_day: Option<bool>,
    /// Present EAS sensitivity, from 0 through 3; absence inherits the master.
    pub sensitivity: Option<u8>,
    /// Present user categories; an empty list explicitly clears inherited categories.
    pub categories: Option<Vec<String>>,
    /// A field could not be preserved; reads remain possible, writes must fail.
    pub unsupported: bool,
}

impl CalendarProperties {
    /// Returns whether supported Calendar data can be written without losing fields.
    pub fn can_write(&self) -> bool {
        !self.unsupported
            && self.exceptions.len() <= 256
            && self.exceptions.iter().all(|exception| {
                exception.fields.properties.as_ref().is_none_or(|value| {
                    !value.unsupported && value.recurrence.is_none() && value.exceptions.is_empty()
                }) && !matches!(exception.fields.body_truncated, Patch::Value(true))
            })
    }

    /// Whether a retained exception invites participants absent from the master.
    pub fn has_attendee_overrides(&self) -> bool {
        self.exceptions.iter().any(|exception| {
            !exception.deleted
                && matches!(&exception.fields.attendees, Patch::Value(values) if !values.is_empty())
        })
    }
}
