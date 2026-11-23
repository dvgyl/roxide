use std::io::Read;
use std::path::Path;

use roxide_host::{Host, OpenMode};

use super::calendar::{Civil, DAY, Date};

#[derive(Clone)]
pub(super) struct Offset {
    pub seconds: i32,
    pub dst: bool,
    pub name: String,
}

pub(super) struct Zone {
    pub name: String,
    pub utc: bool,
    offsets: Vec<Offset>,
    transitions: Vec<(i64, usize)>,
    future: Option<Rules>,
}

impl Zone {
    pub fn fixed(name: &str, seconds: i32) -> Self {
        Self {
            name: name.to_owned(),
            utc: true,
            offsets: vec![Offset {
                seconds,
                dst: false,
                name: name.to_owned(),
            }],
            transitions: Vec::new(),
            future: None,
        }
    }

    pub fn load(host: &mut dyn Host, name: &str) -> Self {
        if matches!(name, "UTC" | "GMT") {
            return Self::fixed(name, 0);
        }
        let local = host.env_var("TZ");
        let key = if name.is_empty() {
            match local
                .as_deref()
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            {
                Some("") => "UTC",
                Some(key) => key,
                None => {
                    let mut data = Vec::new();
                    if let Ok(file) = host.open(Path::new("/etc/localtime"), OpenMode::read_only())
                        && file.take(1024 * 1024).read_to_end(&mut data).is_ok()
                        && let Some(zone) = Self::from_tzif(name, &data)
                    {
                        return zone;
                    }
                    "UTC"
                }
            }
        } else {
            name
        };
        let key = key.strip_prefix(':').unwrap_or(key);
        let mut zone = host
            .tzif(key)
            .ok()
            .and_then(|data| Self::from_tzif(name, &data))
            .or_else(|| {
                let future = Rules::parse(key)?;
                let mut zone = Self::fixed(name, future.standard.seconds);
                zone.future = Some(future);
                Some(zone)
            })
            .or_else(|| {
                let hours = key.strip_prefix("Etc/GMT")?.parse::<i32>().ok()?;
                if !(-14..=12).contains(&hours) {
                    return None;
                }
                let label = format!("{:+03}", -hours);
                Some(Self::fixed(&label, -3600 * hours))
            })
            .unwrap_or_else(|| {
                Self::fixed(
                    if matches!(key, "UTC" | "Etc/UTC" | "UTC0") {
                        "UTC"
                    } else {
                        "GMT"
                    },
                    0,
                )
            });
        zone.name = name.to_owned();
        zone.utc = false;
        zone
    }

    pub fn attributes(&self) -> Vec<&str> {
        if self.utc {
            return vec![&self.name];
        }
        if let Some(rules) = &self.future {
            let daylight = rules
                .daylight
                .as_ref()
                .map_or("   ", |dst| dst.offset.name.as_str());
            return vec![&self.name, &rules.standard.name, daylight];
        }
        let mut offsets = self
            .transitions
            .iter()
            .rev()
            .map(|&(_, index)| &self.offsets[index])
            .chain(self.offsets.iter().rev());
        let standard = offsets
            .clone()
            .find(|offset| !offset.dst)
            .map_or("", |offset| &offset.name);
        let daylight = offsets
            .find(|offset| offset.dst)
            .map_or("   ", |offset| &offset.name);
        vec![&self.name, standard, daylight]
    }

    pub fn at(&self, epoch: f64) -> &Offset {
        let position = self
            .transitions
            .partition_point(|&(time, _)| time <= epoch.floor() as i64);
        if position == self.transitions.len()
            && let Some(rules) = &self.future
        {
            return rules.at(epoch);
        }
        let index = position.checked_sub(1).map_or(0, |i| self.transitions[i].1);
        &self.offsets[index]
    }

    pub fn resolve(&self, civil: Civil, dst: Option<bool>) -> Option<(f64, &Offset)> {
        let local = civil.seconds();
        let mut fallback = None;
        let rule_offsets = self.future.iter().flat_map(|rules| {
            std::iter::once(&rules.standard).chain(rules.daylight.iter().map(|dst| &dst.offset))
        });
        for offset in self.offsets.iter().chain(rule_offsets) {
            let epoch = local - f64::from(offset.seconds);
            let actual = self.at(epoch);
            if offset.seconds == actual.seconds {
                if dst.is_some_and(|dst| dst == actual.dst) {
                    return Some((epoch, actual));
                }
                if fallback.is_none_or(|(old, _)| epoch < old) {
                    fallback = Some((epoch, actual));
                }
            }
        }
        fallback
    }

    fn from_tzif(name: &str, data: &[u8]) -> Option<Self> {
        let first = Header::read(data)?;
        let (header, block, width) = if matches!(*data.get(4)?, b'2' | b'3' | b'4') {
            let start = 44usize.checked_add(first.block_len(4)?)?;
            let second = data.get(start..)?;
            (Header::read(second)?, second.get(44..)?, 8)
        } else {
            (first, data.get(44..)?, 4)
        };
        let (block, footer) = block.split_at_checked(header.block_len(width)?)?;
        let future = std::str::from_utf8(footer)
            .ok()
            .and_then(|s| s.strip_prefix('\n')?.split_once('\n'))
            .and_then(|(s, _)| Rules::parse(s));
        let (times, rest) = block.split_at_checked(header.times.checked_mul(width)?)?;
        let (indices, rest) = rest.split_at_checked(header.times)?;
        let (types, rest) = rest.split_at_checked(header.types.checked_mul(6)?)?;
        let labels = rest.get(..header.chars)?;
        let mut offsets = Vec::new();
        for entry in types.as_chunks::<6>().0 {
            let label = labels.get(usize::from(entry[5])..)?;
            let end = label.iter().position(|&byte| byte == 0)?;
            offsets.push(Offset {
                seconds: read_i32(entry)?,
                dst: entry[4] != 0,
                name: String::from_utf8_lossy(&label[..end]).into_owned(),
            });
        }
        if offsets.is_empty() {
            return None;
        }
        let mut transitions = Vec::with_capacity(header.times);
        for (time, &index) in times.chunks_exact(width).zip(indices) {
            let time = if width == 8 {
                i64::from_be_bytes(time.try_into().ok()?)
            } else {
                i64::from(read_i32(time)?)
            };
            let index = usize::from(index);
            if index >= offsets.len() || transitions.last().is_some_and(|&(last, _)| last > time) {
                return None;
            }
            transitions.push((time, index));
        }
        Some(Self {
            name: name.to_owned(),
            utc: false,
            offsets,
            transitions,
            future,
        })
    }
}

struct Header {
    utc: usize,
    standard: usize,
    leaps: usize,
    times: usize,
    types: usize,
    chars: usize,
}

impl Header {
    fn read(data: &[u8]) -> Option<Self> {
        if data.get(..4)? != b"TZif" {
            return None;
        }
        let count = |i| usize::try_from(read_i32(data.get(i..)?)?).ok();
        Some(Self {
            utc: count(20)?,
            standard: count(24)?,
            leaps: count(28)?,
            times: count(32)?,
            types: count(36)?,
            chars: count(40)?,
        })
    }

    fn block_len(&self, width: usize) -> Option<usize> {
        self.times
            .checked_mul(width + 1)?
            .checked_add(self.types.checked_mul(6)?)?
            .checked_add(self.chars)?
            .checked_add(self.leaps.checked_mul(width + 4)?)?
            .checked_add(self.standard)?
            .checked_add(self.utc)
    }
}

fn read_i32(bytes: &[u8]) -> Option<i32> {
    Some(i32::from_be_bytes(bytes.get(..4)?.try_into().ok()?))
}

// RFC 9636 section 3.3 defines the rules after the last stored transition.
struct Rules {
    standard: Offset,
    daylight: Option<Daylight>,
}

struct Daylight {
    offset: Offset,
    start: Transition,
    end: Transition,
}

struct Transition {
    day: RuleDay,
    seconds: i32,
}

enum RuleDay {
    Julian(i64),
    Ordinal(i64),
    Weekday { month: i64, week: i64, weekday: i64 },
}

impl Rules {
    fn parse(mut input: &str) -> Option<Self> {
        let name = designation(&mut input)?.to_owned();
        let seconds = -hms(&mut input)?;
        let standard = Offset {
            name,
            seconds,
            dst: false,
        };
        let daylight = if input.is_empty() {
            None
        } else {
            let name = designation(&mut input)?.to_owned();
            let seconds = if input.starts_with(',') {
                seconds + 3600
            } else {
                -hms(&mut input)?
            };
            let (start, end) = input.strip_prefix(',')?.split_once(',')?;
            Some(Daylight {
                offset: Offset {
                    name,
                    seconds,
                    dst: true,
                },
                start: Transition::parse(start)?,
                end: Transition::parse(end)?,
            })
        };
        Some(Self { standard, daylight })
    }

    fn at(&self, epoch: f64) -> &Offset {
        let Some(dst) = &self.daylight else {
            return &self.standard;
        };
        let Some(civil) = Civil::from_seconds(epoch) else {
            return &self.standard;
        };
        let year = civil.date.year;
        let mut latest = f64::NEG_INFINITY;
        let mut daylight = false;
        for year in (year - 1)..=(year + 1) {
            let start = dst.start.at(year, self.standard.seconds);
            let end = dst.end.at(year, dst.offset.seconds);
            for (time, dst) in [(end, false), (start, true)] {
                if time <= epoch && (time > latest || (time == latest && dst)) {
                    latest = time;
                    daylight = dst;
                }
            }
        }
        if daylight {
            &dst.offset
        } else {
            &self.standard
        }
    }
}

impl Transition {
    fn parse(mut input: &str) -> Option<Self> {
        let day = if let Some(tail) = input.strip_prefix('M') {
            input = tail;
            let month = number(&mut input, 1, 12)?;
            input = input.strip_prefix('.')?;
            let week = number(&mut input, 1, 5)?;
            input = input.strip_prefix('.')?;
            let weekday = number(&mut input, 0, 6)?;
            RuleDay::Weekday {
                month,
                week,
                weekday,
            }
        } else if let Some(tail) = input.strip_prefix('J') {
            input = tail;
            RuleDay::Julian(number(&mut input, 1, 365)?)
        } else {
            RuleDay::Ordinal(number(&mut input, 0, 365)?)
        };
        let seconds = if let Some(tail) = input.strip_prefix('/') {
            input = tail;
            hms(&mut input)?
        } else {
            7200
        };
        input.is_empty().then_some(Self { day, seconds })
    }

    fn at(&self, year: i64, wall_offset: i32) -> f64 {
        let jan1 = Date::jan1(year);
        let day = match self.day {
            RuleDay::Julian(n) => {
                jan1 + n - 1 + i64::from(n >= 60 && Date::jan1(year + 1) - jan1 == 366)
            }
            RuleDay::Ordinal(n) => jan1 + n,
            RuleDay::Weekday {
                month,
                week,
                weekday,
            } => {
                let first = Date {
                    year,
                    month,
                    day: 1,
                };
                let candidate =
                    first.days() + (weekday - first.weekday()).rem_euclid(7) + 7 * (week - 1);
                if candidate >= first.add_months(1).days() {
                    candidate - 7
                } else {
                    candidate
                }
            }
        };
        day as f64 * DAY + f64::from(self.seconds - wall_offset)
    }
}

fn designation<'a>(input: &mut &'a str) -> Option<&'a str> {
    let name = if let Some(tail) = input.strip_prefix('<') {
        let (name, rest) = tail.split_once('>')?;
        *input = rest;
        name
    } else {
        let len = input.bytes().take_while(u8::is_ascii_alphabetic).count();
        let (name, rest) = input.split_at(len);
        *input = rest;
        name
    };
    (name.len() >= 3).then_some(name)
}

fn number(input: &mut &str, min: i64, max: i64) -> Option<i64> {
    let len = input.bytes().take_while(u8::is_ascii_digit).count();
    let (value, rest) = input.split_at(len);
    let value = value.parse().ok()?;
    *input = rest;
    (min..=max).contains(&value).then_some(value)
}

fn hms(input: &mut &str) -> Option<i32> {
    let sign = if let Some(tail) = input.strip_prefix('-') {
        *input = tail;
        -1
    } else {
        *input = input.strip_prefix('+').unwrap_or(input);
        1
    };
    let mut seconds = number(input, 0, 167)? as i32 * 3600;
    for scale in [60, 1] {
        let Some(tail) = input.strip_prefix(':') else {
            break;
        };
        *input = tail;
        seconds += number(input, 0, 59)? as i32 * scale;
    }
    Some(sign * seconds)
}
