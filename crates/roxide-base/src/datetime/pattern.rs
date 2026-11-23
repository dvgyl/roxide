use std::fmt::Write as _;

use super::calendar::{Civil, Date};
use super::zone::Offset;

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];
const WEEKDAYS: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

#[derive(Clone, Copy)]
enum Token {
    Literal(char),
    Field(char),
    Fraction(Option<usize>),
}

pub(super) struct Pattern {
    tokens: Vec<Token>,
}

impl Pattern {
    pub fn new(format: &str) -> Self {
        let mut tokens = Vec::new();
        push_tokens(&mut tokens, format);
        Self { tokens }
    }

    pub fn has_zone_name(&self) -> bool {
        self.tokens
            .iter()
            .any(|token| matches!(token, Token::Field('Z')))
    }

    pub fn parse(&self, mut input: &str, today: Date) -> Option<(Civil, Option<i32>)> {
        let mut year = None;
        let mut month = None;
        let mut day = None;
        let mut ordinal = None;
        let mut hour = 0;
        let mut minute = 0;
        let mut second = 0.0;
        let mut offset = None;
        let mut meridiem = None;
        let mut hour12 = false;

        input = input.trim_start_matches(|c: char| c.is_ascii_whitespace());
        for &token in &self.tokens {
            match token {
                Token::Literal(c) if c.is_ascii_whitespace() => {
                    input = input.trim_start_matches(|c: char| c.is_ascii_whitespace());
                }
                Token::Literal(c) | Token::Field(c @ '%') => input = input.strip_prefix(c)?,
                Token::Fraction(_) => {
                    let whole = digits(&mut input, 2)?;
                    if whole > 60 {
                        return None;
                    }
                    if let Some(tail) = input.strip_prefix('.') {
                        input = tail;
                        let length = input.bytes().take_while(u8::is_ascii_digit).count();
                        if length > 0 {
                            let fraction = &input[..length];
                            input = &input[length..];
                            second = format!("0.{fraction}").parse::<f64>().ok()?;
                        }
                    }
                    second += whole as f64;
                }
                Token::Field(field) => match field {
                    'Y' => year = Some(digits(&mut input, 4)?),
                    'y' => {
                        let y = digits(&mut input, 2)?;
                        year = Some(if y <= 68 { 2000 + y } else { 1900 + y });
                    }
                    'm' => month = Some(bounded(&mut input, 2, 1, 12)?),
                    'b' | 'B' | 'h' => month = Some(read_name(&mut input, &MONTHS)? as i64 + 1),
                    'a' | 'A' => {
                        read_name(&mut input, &WEEKDAYS)?;
                    }
                    'w' => {
                        bounded(&mut input, 1, 0, 6)?;
                    }
                    'u' => {
                        bounded(&mut input, 1, 1, 7)?;
                    }
                    'd' | 'e' => {
                        input = input.trim_start_matches(' ');
                        day = Some(bounded(&mut input, 2, 1, 31)?);
                    }
                    'j' => ordinal = Some(bounded(&mut input, 3, 1, 366)?),
                    'H' | 'k' => {
                        input = input.trim_start_matches(' ');
                        hour = bounded(&mut input, 2, 0, 24)?;
                    }
                    'I' | 'l' => {
                        input = input.trim_start_matches(' ');
                        hour = bounded(&mut input, 2, 1, 12)? % 12;
                        hour12 = true;
                    }
                    'M' => minute = bounded(&mut input, 2, 0, 59)?,
                    'S' => second = bounded(&mut input, 2, 0, 60)? as f64,
                    'p' => meridiem = Some(read_name(&mut input, &["AM", "PM"])?),
                    'z' => offset = Some(read_offset(&mut input)?),
                    _ => return None,
                },
            }
        }
        let year = year.unwrap_or(today.year);
        let date = if let Some(ordinal) = ordinal {
            let date = Date::from_days(Date::jan1(year) + ordinal - 1);
            if date.year != year {
                return None;
            }
            date
        } else {
            if month.is_some() && day.is_none() {
                return None;
            }
            let date = Date {
                year,
                month: month.unwrap_or(today.month),
                day: day.unwrap_or(today.day),
            };
            if Date::from_days(date.days()).month != date.month {
                return None;
            }
            date
        };
        if hour12 && meridiem == Some(1) {
            hour += 12;
        }
        let (date, hour) = if hour == 24 {
            if minute != 0 || second != 0.0 {
                return None;
            }
            (Date::from_days(date.days() + 1), 0)
        } else {
            (date, hour)
        };
        Some((
            Civil {
                date,
                hour,
                minute,
                second,
            },
            offset,
        ))
    }

    pub fn format(&self, civil: Civil, offset: Option<&Offset>, digits_secs: usize) -> String {
        let date = civil.date;
        let mut out = String::new();
        for &token in &self.tokens {
            let result = match token {
                Token::Literal(c) => out.write_char(c),
                Token::Fraction(digits) => {
                    let digits = digits.unwrap_or(digits_secs).min(6);
                    let second = civil.second.floor() as i64;
                    if digits == 0 {
                        write!(out, "{second:02}")
                    } else {
                        let scale = 10_i64.pow(digits as u32);
                        let fraction = (civil.second * scale as f64).trunc() as i64 % scale;
                        write!(out, "{second:02}.{fraction:0digits$}")
                    }
                }
                Token::Field(field) => match field {
                    'Y' => write!(out, "{:04}", date.year),
                    'y' => write!(out, "{:02}", date.year.rem_euclid(100)),
                    'C' => write!(out, "{:02}", date.year.div_euclid(100)),
                    'm' => write!(out, "{:02}", date.month),
                    'd' => write!(out, "{:02}", date.day),
                    'e' => write!(out, "{:2}", date.day),
                    'H' => write!(out, "{:02}", civil.hour),
                    'k' => write!(out, "{:2}", civil.hour),
                    'I' | 'l' => {
                        let hour = (civil.hour + 11) % 12 + 1;
                        if field == 'I' {
                            write!(out, "{hour:02}")
                        } else {
                            write!(out, "{hour:2}")
                        }
                    }
                    'p' => out.write_str(if civil.hour < 12 { "AM" } else { "PM" }),
                    'M' => write!(out, "{:02}", civil.minute),
                    'S' => write!(out, "{:02}", civil.second.floor() as i64),
                    'j' => write!(out, "{:03}", date.ordinal() + 1),
                    'w' => write!(out, "{}", date.weekday()),
                    'u' => write!(out, "{}", (date.weekday() + 6) % 7 + 1),
                    'b' | 'h' => out.write_str(&MONTHS[(date.month - 1) as usize][..3]),
                    'B' => out.write_str(MONTHS[(date.month - 1) as usize]),
                    'a' => out.write_str(&WEEKDAYS[date.weekday() as usize][..3]),
                    'A' => out.write_str(WEEKDAYS[date.weekday() as usize]),
                    'z' => {
                        if let Some(offset) = offset {
                            let seconds = offset.seconds.unsigned_abs();
                            let sign = if offset.seconds < 0 { '-' } else { '+' };
                            write!(out, "{sign}{:02}{:02}", seconds / 3600, seconds % 3600 / 60)
                        } else {
                            Ok(())
                        }
                    }
                    'Z' => out.write_str(offset.map_or("", |offset| &offset.name)),
                    '%' => out.write_char('%'),
                    _ => write!(out, "%{field}"),
                },
            };
            let _ = result;
        }
        out
    }
}

fn push_tokens(tokens: &mut Vec<Token>, format: &str) {
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            tokens.push(Token::Literal(c));
            continue;
        }
        let Some(mut field) = chars.next() else {
            tokens.push(Token::Literal('%'));
            break;
        };
        if matches!(field, 'E' | 'O') {
            let modifier = field;
            field = chars.next().unwrap_or(modifier);
            if modifier == 'O' && field == 'S' {
                let digits = chars
                    .next_if(char::is_ascii_digit)
                    .and_then(|c| c.to_digit(10));
                tokens.push(Token::Fraction(digits.map(|d| d as usize)));
                continue;
            }
        }
        let expansion = match field {
            'F' => "%Y-%m-%d",
            'T' | 'X' => "%H:%M:%S",
            'R' => "%H:%M",
            'D' | 'x' => "%m/%d/%y",
            'r' => "%I:%M:%S %p",
            'c' => "%a %b %e %T %Y",
            'n' => "\n",
            't' => "\t",
            _ => {
                tokens.push(Token::Field(field));
                continue;
            }
        };
        push_tokens(tokens, expansion);
    }
}

fn digits(input: &mut &str, max: usize) -> Option<i64> {
    let length = input
        .bytes()
        .take(max)
        .take_while(u8::is_ascii_digit)
        .count();
    let value = input[..length].parse().ok()?;
    *input = &input[length..];
    Some(value)
}

fn bounded(input: &mut &str, width: usize, min: i64, max: i64) -> Option<i64> {
    let value = digits(input, width)?;
    (min..=max).contains(&value).then_some(value)
}

fn read_name(input: &mut &str, names: &[&str]) -> Option<usize> {
    for (i, name) in names.iter().enumerate() {
        for prefix in [*name, &name[..name.len().min(3)]] {
            if input
                .get(..prefix.len())
                .is_some_and(|s| s.eq_ignore_ascii_case(prefix))
            {
                *input = &input[prefix.len()..];
                return Some(i);
            }
        }
    }
    None
}

fn read_offset(input: &mut &str) -> Option<i32> {
    let trimmed = input.trim_start_matches(|c: char| c.is_ascii_whitespace());
    let (sign, tail) = if let Some(tail) = trimmed.strip_prefix('+') {
        (1, tail)
    } else {
        (-1, trimmed.strip_prefix('-')?)
    };
    let mut offset = tail.get(..4)?;
    if !offset.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let hour = bounded(&mut offset, 2, 0, 14)? as i32;
    let minute = bounded(&mut offset, 2, 0, 59)? as i32;
    if hour == 14 && minute != 0 {
        return None;
    }
    *input = &tail[4..];
    Some(sign * (hour * 3600 + minute * 60))
}
