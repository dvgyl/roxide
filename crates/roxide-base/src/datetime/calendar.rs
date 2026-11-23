pub(super) const DAY: f64 = 86_400.0;

#[derive(Clone, Copy)]
pub(super) struct Date {
    pub year: i64,
    pub month: i64,
    pub day: i64,
}

impl Date {
    // Use March as month zero so the leap day ends the year. A 400-year
    // Gregorian cycle contains 146097 days. This is Hinnant's civil-date method.
    pub fn days(self) -> i64 {
        let year = self.year - i64::from(self.month < 3);
        let era = year.div_euclid(400);
        let year = year.rem_euclid(400);
        let month = (self.month + 9) % 12;
        let day = (153 * month + 2) / 5 + self.day - 1;
        era * 146_097 + 365 * year + year / 4 - year / 100 + day - 719_468
    }

    pub fn from_days(days: i64) -> Self {
        let days = days + 719_468;
        let era = days.div_euclid(146_097);
        let day = days.rem_euclid(146_097);
        let year = (day - day / 1460 + day / 36_524 - day / 146_096) / 365;
        let day = day - (365 * year + year / 4 - year / 100);
        let month = (5 * day + 2) / 153;
        let month_day = day - (153 * month + 2) / 5 + 1;
        let month = (month + 2) % 12 + 1;
        Self {
            year: era * 400 + year + i64::from(month < 3),
            month,
            day: month_day,
        }
    }

    pub fn jan1(year: i64) -> i64 {
        Self {
            year,
            month: 1,
            day: 1,
        }
        .days()
    }

    pub fn weekday(self) -> i64 {
        (self.days() + 4).rem_euclid(7)
    }

    pub fn ordinal(self) -> i64 {
        self.days() - Self::jan1(self.year)
    }

    pub fn add_months(self, months: i64) -> Self {
        let month = self.month - 1 + months;
        Self {
            year: self.year + month.div_euclid(12),
            month: month.rem_euclid(12) + 1,
            day: self.day,
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Civil {
    pub date: Date,
    pub hour: i64,
    pub minute: i64,
    pub second: f64,
}

impl Civil {
    pub fn from_seconds(seconds: f64) -> Option<Self> {
        // The POSIXlt year field is a signed 32-bit integer.
        if !seconds.is_finite() || seconds.abs() > 6.8e16 {
            return None;
        }
        let whole = seconds.floor() as i64;
        let within_day = whole.rem_euclid(DAY as i64);
        let date = Date::from_days(whole.div_euclid(DAY as i64));
        i32::try_from(date.year - 1900)
            .ok()
            .filter(|y| *y != i32::MIN)?;
        Some(Self {
            date,
            hour: within_day / 3600,
            minute: within_day % 3600 / 60,
            second: (within_day % 60) as f64 + (seconds - seconds.floor()),
        })
    }

    pub fn seconds(self) -> f64 {
        self.date.days() as f64 * DAY + (self.hour * 3600 + self.minute * 60) as f64 + self.second
    }
}
