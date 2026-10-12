//! Valeurs manipulées par les expressions : scalaires des tables, plus listes,
//! objets et dates.

use std::cmp::Ordering;
use std::sync::Arc;

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, TimeZone, Timelike};
use serde_json::{Map, Value as Json};

use crate::value::{Value, format_float};

#[derive(Debug, Clone)]
pub enum V {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    Str(String),
    List(Arc<Vec<V>>),
    Obj(Arc<Vec<(String, V)>>),
    /// Instant en millisecondes depuis l'époque Unix (UTC).
    Date(i64),
}

impl From<Value> for V {
    fn from(v: Value) -> Self {
        match v {
            Value::Null => V::Null,
            Value::Bool(b) => V::Bool(b),
            Value::Int(i) => V::Int(i),
            Value::Float(f) => V::Float(f),
            Value::Str(s) => V::Str(s),
        }
    }
}

impl V {
    pub fn list(items: Vec<V>) -> V {
        V::List(Arc::new(items))
    }

    pub fn obj(fields: Vec<(String, V)>) -> V {
        V::Obj(Arc::new(fields))
    }

    pub fn is_null(&self) -> bool {
        matches!(self, V::Null)
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            V::Null => "null",
            V::Bool(_) => "booléen",
            V::Int(_) => "entier",
            V::Float(_) => "nombre",
            V::Str(_) => "texte",
            V::List(_) => "liste",
            V::Obj(_) => "objet",
            V::Date(_) => "date",
        }
    }

    /// Type en anglais (complétion côté interface).
    pub fn kind(&self) -> &'static str {
        match self {
            V::Null => "null",
            V::Bool(_) => "boolean",
            V::Int(_) | V::Float(_) => "number",
            V::Str(_) => "string",
            V::List(_) => "list",
            V::Obj(_) => "object",
            V::Date(_) => "date",
        }
    }

    pub fn truthy(&self) -> bool {
        match self {
            V::Null => false,
            V::Bool(b) => *b,
            V::Int(i) => *i != 0,
            V::Float(f) => *f != 0.0 && !f.is_nan(),
            V::Str(s) => !s.is_empty(),
            V::List(_) | V::Obj(_) | V::Date(_) => true,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            V::Int(i) => Some(*i as f64),
            V::Float(f) => Some(*f),
            V::Bool(b) => Some(*b as i64 as f64),
            V::Str(s) => parse_number(s).map(|v| v.as_f64().unwrap_or(f64::NAN)),
            V::Date(ms) => Some(*ms as f64),
            _ => None,
        }
    }

    /// Conversion numérique « à la JavaScript » (`Number(x)`).
    pub fn to_number(&self) -> V {
        match self {
            V::Int(_) | V::Float(_) => self.clone(),
            V::Bool(b) => V::Int(*b as i64),
            V::Str(s) => parse_number(s).unwrap_or(V::Null),
            V::Date(ms) => V::Int(*ms),
            _ => V::Null,
        }
    }

    /// Texte affiché (gabarits, `String(x)`).
    pub fn to_text(&self, offset: i32) -> String {
        match self {
            V::Null => String::new(),
            V::Bool(b) => b.to_string(),
            V::Int(i) => i.to_string(),
            V::Float(f) => format_float(*f),
            V::Str(s) => s.clone(),
            V::Date(ms) => iso(*ms, offset),
            V::List(_) | V::Obj(_) => self.to_json(offset).to_string(),
        }
    }

    pub fn to_json(&self, offset: i32) -> Json {
        match self {
            V::Null => Json::Null,
            V::Bool(b) => Json::Bool(*b),
            V::Int(i) => Json::from(*i),
            V::Float(f) => serde_json::Number::from_f64(*f).map_or(Json::Null, Json::Number),
            V::Str(s) => Json::String(s.clone()),
            V::Date(ms) => Json::String(iso(*ms, offset)),
            V::List(items) => Json::Array(items.iter().map(|v| v.to_json(offset)).collect()),
            V::Obj(fields) => Json::Object(
                fields
                    .iter()
                    .map(|(k, v)| (k.clone(), v.to_json(offset)))
                    .collect::<Map<_, _>>(),
            ),
        }
    }

    pub fn from_json(j: &Json) -> V {
        match j {
            Json::Null => V::Null,
            Json::Bool(b) => V::Bool(*b),
            Json::Number(n) => n
                .as_i64()
                .map(V::Int)
                .unwrap_or_else(|| V::Float(n.as_f64().unwrap_or(f64::NAN))),
            Json::String(s) => V::Str(s.clone()),
            Json::Array(a) => V::list(a.iter().map(V::from_json).collect()),
            Json::Object(o) => V::obj(
                o.iter()
                    .map(|(k, v)| (k.clone(), V::from_json(v)))
                    .collect(),
            ),
        }
    }

    /// Valeur scalaire écrite dans une table (listes, objets et dates en texte).
    pub fn into_value(self, offset: i32) -> Value {
        match self {
            V::Null => Value::Null,
            V::Bool(b) => Value::Bool(b),
            V::Int(i) => Value::Int(i),
            V::Float(f) if f.is_finite() => Value::Float(f),
            V::Float(_) => Value::Null,
            V::Str(s) => Value::Str(s),
            other => Value::Str(other.to_text(offset)),
        }
    }

    pub fn field(&self, name: &str) -> Option<&V> {
        match self {
            V::Obj(fields) => fields.iter().find(|(k, _)| k == name).map(|(_, v)| v),
            _ => None,
        }
    }
}

/// Nombre écrit en texte (espaces et virgule décimale tolérés : « 1 234,5 »).
pub fn parse_number(s: &str) -> Option<V> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(i) = t.parse::<i64>() {
        return Some(V::Int(i));
    }
    if let Ok(f) = t.parse::<f64>() {
        return Some(V::Float(f));
    }
    let cleaned: String = t
        .chars()
        .filter(|c| !matches!(c, ' ' | '\u{a0}' | '\u{202f}' | '_'))
        .map(|c| if c == ',' { '.' } else { c })
        .collect();
    cleaned.parse::<f64>().ok().map(V::Float)
}

/// Égalité souple (`==`) : `"3" == 3`, `null == null`.
pub fn loose_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Null, _) | (_, V::Null) => false,
        (V::Int(x), V::Int(y)) => x == y,
        (V::Date(x), V::Date(y)) => x == y,
        (V::Date(x), V::Str(s)) | (V::Str(s), V::Date(x)) => {
            parse_date(s, 0).is_some_and(|y| *x == y)
        }
        (V::List(_) | V::Obj(_), _) | (_, V::List(_) | V::Obj(_)) => strict_eq(a, b),
        _ if is_num(a) || is_num(b) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x == y,
            _ => false,
        },
        (V::Bool(x), V::Bool(y)) => x == y,
        _ => a.to_text(0) == b.to_text(0),
    }
}

/// Égalité stricte (`===`) : mêmes types (entiers et décimaux confondus).
pub fn strict_eq(a: &V, b: &V) -> bool {
    match (a, b) {
        (V::Null, V::Null) => true,
        (V::Bool(x), V::Bool(y)) => x == y,
        (V::Int(x), V::Int(y)) => x == y,
        (V::Int(_) | V::Float(_), V::Int(_) | V::Float(_)) => a.as_f64() == b.as_f64(),
        (V::Str(x), V::Str(y)) => x == y,
        (V::Date(x), V::Date(y)) => x == y,
        (V::List(x), V::List(y)) => {
            x.len() == y.len() && x.iter().zip(y.iter()).all(|(a, b)| strict_eq(a, b))
        }
        (V::Obj(x), V::Obj(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| b.field(k).is_some_and(|w| strict_eq(v, w)))
        }
        _ => false,
    }
}

fn is_num(v: &V) -> bool {
    matches!(v, V::Int(_) | V::Float(_))
}

/// Ordre de tri ; `None` si une valeur est nulle (comparaison fausse).
pub fn compare(a: &V, b: &V) -> Option<Ordering> {
    match (a, b) {
        (V::Null, _) | (_, V::Null) => None,
        (V::Int(x), V::Int(y)) => Some(x.cmp(y)),
        (V::Date(x), V::Date(y)) => Some(x.cmp(y)),
        (V::Date(x), V::Str(s)) => parse_date(s, 0).map(|y| x.cmp(&y)),
        (V::Str(s), V::Date(y)) => parse_date(s, 0).map(|x| x.cmp(y)),
        _ if is_num(a) || is_num(b) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => x.partial_cmp(&y),
            _ => Some(a.to_text(0).cmp(&b.to_text(0))),
        },
        (V::Bool(x), V::Bool(y)) => Some(x.cmp(y)),
        _ => Some(a.to_text(0).cmp(&b.to_text(0))),
    }
}

// ------------------------------------------------------------------ dates

fn offset_of(minutes: i32) -> FixedOffset {
    FixedOffset::east_opt(minutes * 60).unwrap_or(FixedOffset::east_opt(0).expect("UTC"))
}

pub fn local(ms: i64, offset: i32) -> Option<DateTime<FixedOffset>> {
    DateTime::from_timestamp_millis(ms).map(|d| d.with_timezone(&offset_of(offset)))
}

fn from_local(naive: NaiveDateTime, offset: i32) -> Option<i64> {
    offset_of(offset)
        .from_local_datetime(&naive)
        .single()
        .map(|d| d.timestamp_millis())
}

/// ISO 8601 avec décalage horaire : `2026-10-12T08:30:00.000+02:00`.
pub fn iso(ms: i64, offset: i32) -> String {
    local(ms, offset).map_or_else(
        || ms.to_string(),
        |d| d.format("%Y-%m-%dT%H:%M:%S%.3f%:z").to_string(),
    )
}

/// Lit une date : ISO 8601 (avec ou sans heure / fuseau), `jj/mm/aaaa [hh:mm[:ss]]`.
/// Sans fuseau, l'heure est locale (`offset` en minutes).
pub fn parse_date(s: &str, offset: i32) -> Option<i64> {
    let t = s.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(t) {
        return Some(d.timestamp_millis());
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M:%S%.f%z",
        "%Y-%m-%d %H:%M:%S%.f%z",
    ] {
        if let Ok(d) = DateTime::parse_from_str(t, fmt) {
            return Some(d.timestamp_millis());
        }
    }
    let t = t.trim_end_matches('Z');
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M",
        "%d/%m/%Y %H:%M:%S",
        "%d/%m/%Y %H:%M",
    ] {
        if let Ok(n) = NaiveDateTime::parse_from_str(t, fmt) {
            let off = if s.trim().ends_with('Z') { 0 } else { offset };
            return from_local(n, off);
        }
    }
    for fmt in ["%Y-%m-%d", "%d/%m/%Y"] {
        if let Ok(d) = NaiveDate::parse_from_str(t, fmt) {
            return from_local(d.and_hms_opt(0, 0, 0)?, offset);
        }
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Unit {
    Year,
    Quarter,
    Month,
    Week,
    Day,
    Hour,
    Minute,
    Second,
    Millisecond,
}

pub fn unit(name: &str) -> Option<Unit> {
    Some(match name.trim().to_lowercase().as_str() {
        "year" | "years" | "y" | "an" | "ans" | "année" | "années" | "annee" | "annees" => {
            Unit::Year
        }
        "quarter" | "quarters" | "trimestre" | "trimestres" => Unit::Quarter,
        "month" | "months" | "mois" => Unit::Month,
        "week" | "weeks" | "w" | "semaine" | "semaines" => Unit::Week,
        "day" | "days" | "d" | "jour" | "jours" => Unit::Day,
        "hour" | "hours" | "h" | "heure" | "heures" => Unit::Hour,
        "minute" | "minutes" | "min" | "m" => Unit::Minute,
        "second" | "seconds" | "s" | "sec" | "seconde" | "secondes" => Unit::Second,
        "millisecond" | "milliseconds" | "ms" | "milliseconde" | "millisecondes" => {
            Unit::Millisecond
        }
        _ => return None,
    })
}

/// Ajoute `n` unités (mois et années en calendrier local).
pub fn plus(ms: i64, n: f64, u: Unit, offset: i32) -> Option<i64> {
    let fixed = |factor: f64| -> Option<i64> {
        let delta = n * factor;
        delta
            .is_finite()
            .then(|| ms.saturating_add(delta.round() as i64))
    };
    match u {
        Unit::Millisecond => fixed(1.0),
        Unit::Second => fixed(1_000.0),
        Unit::Minute => fixed(60_000.0),
        Unit::Hour => fixed(3_600_000.0),
        Unit::Day | Unit::Week => {
            // Jours calendaires : l'heure locale est conservée.
            let days = n.trunc() as i64 * if u == Unit::Week { 7 } else { 1 };
            let d = local(ms, offset)?;
            let shifted = d
                .naive_local()
                .checked_add_signed(chrono::TimeDelta::try_days(days)?)?;
            let rest =
                (n.fract() * if u == Unit::Week { 7.0 } else { 1.0 } * 86_400_000.0).round() as i64;
            from_local(shifted, offset).map(|v| v + rest)
        }
        Unit::Month | Unit::Quarter | Unit::Year => {
            let months = n.trunc() as i64
                * match u {
                    Unit::Year => 12,
                    Unit::Quarter => 3,
                    _ => 1,
                };
            let d = local(ms, offset)?.naive_local();
            let m = u32::try_from(months.unsigned_abs()).ok()?;
            let shifted = if months >= 0 {
                d.checked_add_months(chrono::Months::new(m))?
            } else {
                d.checked_sub_months(chrono::Months::new(m))?
            };
            from_local(shifted, offset)
        }
    }
}

/// Début de la période contenant la date (`startOf('month')`).
pub fn start_of(ms: i64, u: Unit, offset: i32) -> Option<i64> {
    let d = local(ms, offset)?.naive_local();
    let date = d.date();
    let naive = match u {
        Unit::Year => NaiveDate::from_ymd_opt(date.year(), 1, 1)?.and_hms_opt(0, 0, 0)?,
        Unit::Quarter => NaiveDate::from_ymd_opt(date.year(), (date.month0() / 3) * 3 + 1, 1)?
            .and_hms_opt(0, 0, 0)?,
        Unit::Month => {
            NaiveDate::from_ymd_opt(date.year(), date.month(), 1)?.and_hms_opt(0, 0, 0)?
        }
        Unit::Week => (date
            - chrono::TimeDelta::try_days(date.weekday().num_days_from_monday() as i64)?)
        .and_hms_opt(0, 0, 0)?,
        Unit::Day => date.and_hms_opt(0, 0, 0)?,
        Unit::Hour => date.and_hms_opt(d.hour(), 0, 0)?,
        Unit::Minute => date.and_hms_opt(d.hour(), d.minute(), 0)?,
        Unit::Second => date.and_hms_opt(d.hour(), d.minute(), d.second())?,
        Unit::Millisecond => d,
    };
    from_local(naive, offset)
}

pub fn end_of(ms: i64, u: Unit, offset: i32) -> Option<i64> {
    let start = start_of(ms, u, offset)?;
    plus(start, 1.0, u, offset).map(|v| v - 1)
}

/// Écart `a - b` exprimé dans l'unité demandée.
pub fn diff(a: i64, b: i64, u: Unit, offset: i32) -> f64 {
    let ms = (a - b) as f64;
    match u {
        Unit::Millisecond => ms,
        Unit::Second => ms / 1_000.0,
        Unit::Minute => ms / 60_000.0,
        Unit::Hour => ms / 3_600_000.0,
        Unit::Day => ms / 86_400_000.0,
        Unit::Week => ms / 604_800_000.0,
        Unit::Month | Unit::Quarter | Unit::Year => {
            let (Some(x), Some(y)) = (local(a, offset), local(b, offset)) else {
                return f64::NAN;
            };
            let months = (x.year() - y.year()) as f64 * 12.0 + x.month() as f64 - y.month() as f64
                + (x.day() as f64 - y.day() as f64) / 31.0;
            match u {
                Unit::Year => months / 12.0,
                Unit::Quarter => months / 3.0,
                _ => months,
            }
        }
    }
}

const MONTHS: [&str; 12] = [
    "janvier",
    "février",
    "mars",
    "avril",
    "mai",
    "juin",
    "juillet",
    "août",
    "septembre",
    "octobre",
    "novembre",
    "décembre",
];
const MONTHS_SHORT: [&str; 12] = [
    "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
    "déc.",
];
const DAYS: [&str; 7] = [
    "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi", "dimanche",
];

/// Mise en forme avec les jetons de Luxon (comme n8n) : `dd/MM/yyyy HH:mm`,
/// `EEEE d MMMM yyyy`… Le texte entre apostrophes est recopié tel quel.
pub fn format(ms: i64, pattern: &str, offset: i32) -> Option<String> {
    let d = local(ms, offset)?;
    let chars: Vec<char> = pattern.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            i += 1;
            while i < chars.len() && chars[i] != '\'' {
                out.push(chars[i]);
                i += 1;
            }
            i += 1;
            continue;
        }
        let mut n = 1;
        while i + n < chars.len() && chars[i + n] == c {
            n += 1;
        }
        let pad = |v: u32, w: usize| format!("{v:0w$}");
        let piece = match (c, n) {
            ('y', 2) => pad((d.year().rem_euclid(100)) as u32, 2),
            ('y', _) => d.year().to_string(),
            ('M', 1) => d.month().to_string(),
            ('M', 2) => pad(d.month(), 2),
            ('M', 3) => MONTHS_SHORT[d.month0() as usize].to_string(),
            ('M', _) => MONTHS[d.month0() as usize].to_string(),
            ('L', n) => {
                // Mois « autonome » de Luxon : identique en français.
                return format(
                    ms,
                    &pattern.replacen(&"L".repeat(n), &"M".repeat(n), 1),
                    offset,
                );
            }
            ('d', 1) => d.day().to_string(),
            ('d', _) => pad(d.day(), 2),
            ('E', 1) => (d.weekday().number_from_monday()).to_string(),
            ('E', 3) => DAYS[d.weekday().num_days_from_monday() as usize][..3].to_string() + ".",
            ('E', _) => DAYS[d.weekday().num_days_from_monday() as usize].to_string(),
            ('H', 1) => d.hour().to_string(),
            ('H', _) => pad(d.hour(), 2),
            ('h', 1) => (((d.hour() + 11) % 12) + 1).to_string(),
            ('h', _) => pad(((d.hour() + 11) % 12) + 1, 2),
            ('a', _) => (if d.hour() < 12 { "AM" } else { "PM" }).to_string(),
            ('m', 1) => d.minute().to_string(),
            ('m', _) => pad(d.minute(), 2),
            ('s', 1) => d.second().to_string(),
            ('s', _) => pad(d.second(), 2),
            ('S', _) => pad(d.timestamp_subsec_millis(), 3),
            ('q', _) => (d.month0() / 3 + 1).to_string(),
            ('W', _) => pad(d.iso_week().week(), n.min(2)),
            ('o', _) => pad(d.ordinal(), 3),
            ('Z', 1) => {
                let m = offset;
                format!("{}{}", if m < 0 { '-' } else { '+' }, (m.abs() / 60))
            }
            ('Z', _) => d.format("%:z").to_string(),
            ('X', _) => d.timestamp().to_string(),
            ('x', _) => d.timestamp_millis().to_string(),
            _ => chars[i..i + n].iter().collect(),
        };
        out.push_str(&piece);
        i += n;
    }
    Some(out)
}

/// Propriétés d'une date (`$now.year`, `.weekday`…).
pub fn date_part(ms: i64, name: &str, offset: i32) -> Option<V> {
    let d = local(ms, offset)?;
    Some(V::Int(match name {
        "year" => d.year() as i64,
        "quarter" => (d.month0() / 3 + 1) as i64,
        "month" => d.month() as i64,
        "day" => d.day() as i64,
        "weekday" => d.weekday().number_from_monday() as i64,
        "weekNumber" => d.iso_week().week() as i64,
        "ordinal" => d.ordinal() as i64,
        "hour" => d.hour() as i64,
        "minute" => d.minute() as i64,
        "second" => d.second() as i64,
        "millisecond" => d.timestamp_subsec_millis() as i64,
        "daysInMonth" => {
            let first = NaiveDate::from_ymd_opt(d.year(), d.month(), 1)?;
            let next = first.checked_add_months(chrono::Months::new(1))?;
            (next - first).num_days()
        }
        _ => return None,
    }))
}

pub const DATE_PROPS: &[&str] = &[
    "year",
    "quarter",
    "month",
    "day",
    "weekday",
    "weekNumber",
    "ordinal",
    "hour",
    "minute",
    "second",
    "millisecond",
    "daysInMonth",
    "monthLong",
    "weekdayLong",
];

pub fn month_name(ms: i64, offset: i32) -> Option<String> {
    local(ms, offset).map(|d| MONTHS[d.month0() as usize].to_string())
}

pub fn day_name(ms: i64, offset: i32) -> Option<String> {
    local(ms, offset).map(|d| DAYS[d.weekday().num_days_from_monday() as usize].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        let paris = 120;
        let ms = parse_date("2026-03-31T10:15:00+02:00", 0).unwrap();
        assert_eq!(iso(ms, paris), "2026-03-31T10:15:00.000+02:00");
        assert_eq!(
            format(ms, "dd/MM/yyyy HH'h'mm", paris).unwrap(),
            "31/03/2026 10h15"
        );
        assert_eq!(
            format(ms, "EEEE d MMMM yyyy", paris).unwrap(),
            "mardi 31 mars 2026"
        );
        // 31 mars + 1 mois = 30 avril (fin de mois).
        let next = plus(ms, 1.0, Unit::Month, paris).unwrap();
        assert_eq!(format(next, "yyyy-MM-dd", paris).unwrap(), "2026-04-30");
        let start = start_of(ms, Unit::Month, paris).unwrap();
        assert_eq!(iso(start, paris), "2026-03-01T00:00:00.000+02:00");
        assert_eq!(
            parse_date("2026-03-31", paris),
            parse_date("31/03/2026", paris)
        );
        assert_eq!(diff(next, ms, Unit::Day, paris), 30.0);
        assert_eq!(unit("jours"), Some(Unit::Day));
        assert_eq!(unit("months"), Some(Unit::Month));
    }

    #[test]
    fn numbers_and_equality() {
        assert!(matches!(parse_number("1 234,5"), Some(V::Float(f)) if f == 1234.5));
        assert!(loose_eq(&V::Str("3".into()), &V::Int(3)));
        assert!(!strict_eq(&V::Str("3".into()), &V::Int(3)));
        assert!(strict_eq(&V::Int(3), &V::Float(3.0)));
    }
}
