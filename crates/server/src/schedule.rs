//! Planification des déclencheurs « Planification » : intervalle, quotidien, hebdomadaire, cron.
//! Les heures s'entendent dans le fuseau horaire choisi (changements d'heure gérés).

use chrono::{DateTime, Datelike, Duration, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Toutes les N minutes.
    Interval(u32),
    /// Expression de type cron (quotidien et hebdomadaire s'y ramènent).
    Cron(Box<Cron>, Tz),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Cron {
    minutes: [bool; 60],
    hours: [bool; 24],
    days: [bool; 32],
    months: [bool; 13],
    /// 0 = dimanche.
    weekdays: [bool; 7],
    days_restricted: bool,
    weekdays_restricted: bool,
}

fn param_str(p: &Map<String, Value>, key: &str) -> String {
    match p.get(key) {
        Some(Value::String(s)) => s.trim().to_string(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

impl Plan {
    pub fn from_params(p: &Map<String, Value>) -> Result<Plan, String> {
        let tz = || -> Result<Tz, String> {
            let name = param_str(p, "timezone");
            let name = if name.is_empty() {
                "Europe/Paris".to_string()
            } else {
                name
            };
            name.parse::<Tz>()
                .map_err(|_| format!("Fuseau horaire inconnu : {name}"))
        };
        match param_str(p, "mode").as_str() {
            "" | "intervalle" => {
                let every = param_str(p, "every").parse::<f64>().unwrap_or(0.0);
                if !(1.0..=525_600.0).contains(&every) {
                    return Err("Intervalle invalide (1 minute minimum)".into());
                }
                Ok(Plan::Interval(every as u32))
            }
            "quotidien" => {
                let (h, m) = parse_time(&param_str(p, "time"))?;
                Ok(Plan::Cron(
                    Box::new(Cron::parse(&format!("{m} {h} * * *"))?),
                    tz()?,
                ))
            }
            "hebdomadaire" => {
                let (h, m) = parse_time(&param_str(p, "time"))?;
                let days = parse_weekdays(&param_str(p, "days"))?;
                Ok(Plan::Cron(
                    Box::new(Cron::parse(&format!("{m} {h} * * {days}"))?),
                    tz()?,
                ))
            }
            "cron" => Ok(Plan::Cron(
                Box::new(Cron::parse(&param_str(p, "cron"))?),
                tz()?,
            )),
            other => Err(format!("Fréquence inconnue : {other}")),
        }
    }

    /// Prochaine échéance strictement après `after`.
    pub fn next_after(&self, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        match self {
            Plan::Interval(minutes) => Some(after + Duration::minutes(i64::from(*minutes))),
            Plan::Cron(cron, tz) => cron.next_after(*tz, after),
        }
    }
}

fn parse_time(s: &str) -> Result<(u32, u32), String> {
    let invalid = || format!("Heure invalide « {s} » (format HH:MM)");
    let (h, m) = s.split_once(':').ok_or_else(invalid)?;
    let (h, m) = (
        h.trim().parse::<u32>().map_err(|_| invalid())?,
        m.trim().parse::<u32>().map_err(|_| invalid())?,
    );
    if h > 23 || m > 59 {
        return Err(invalid());
    }
    Ok((h, m))
}

/// « lun, mer, ven » → « 1,3,5 ».
fn parse_weekdays(s: &str) -> Result<String, String> {
    let mut out = Vec::new();
    for d in s.split([',', ' ']).map(str::trim).filter(|d| !d.is_empty()) {
        let n = match d.to_lowercase().trim_end_matches('.') {
            "dim" | "dimanche" => 0,
            "lun" | "lundi" => 1,
            "mar" | "mardi" => 2,
            "mer" | "mercredi" => 3,
            "jeu" | "jeudi" => 4,
            "ven" | "vendredi" => 5,
            "sam" | "samedi" => 6,
            other => return Err(format!("Jour inconnu : {other}")),
        };
        out.push(n.to_string());
    }
    if out.is_empty() {
        return Err("Choisissez au moins un jour".into());
    }
    Ok(out.join(","))
}

/// Analyse un champ cron (`*`, `*/n`, `a`, `a-b`, `a-b/n`, listes) borné à `[min, max]`.
fn parse_field(field: &str, min: u32, max: u32, out: &mut [bool]) -> Result<bool, String> {
    let invalid = || format!("Champ cron invalide : « {field} »");
    for part in field.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => (r, s.parse::<u32>().map_err(|_| invalid())?),
            None => (part, 1),
        };
        if step == 0 {
            return Err(invalid());
        }
        let (lo, hi) = if range == "*" {
            (min, max)
        } else {
            match range.split_once('-') {
                Some((a, b)) => (
                    a.parse().map_err(|_| invalid())?,
                    b.parse().map_err(|_| invalid())?,
                ),
                None => {
                    let v: u32 = range.parse().map_err(|_| invalid())?;
                    (v, if part.contains('/') { max } else { v })
                }
            }
        };
        if lo < min || hi > max || lo > hi {
            return Err(invalid());
        }
        let mut v = lo;
        while v <= hi {
            out[v as usize] = true;
            v += step;
        }
    }
    // Règle classique : un champ commençant par « * » n'est pas restreint.
    Ok(!field.starts_with('*'))
}

impl Cron {
    pub fn parse(expr: &str) -> Result<Cron, String> {
        let fields: Vec<&str> = expr.split_whitespace().collect();
        if fields.len() != 5 {
            return Err("Expression cron invalide : 5 champs attendus (minute heure jour mois jour-semaine)".into());
        }
        let mut c = Cron {
            minutes: [false; 60],
            hours: [false; 24],
            days: [false; 32],
            months: [false; 13],
            weekdays: [false; 7],
            days_restricted: false,
            weekdays_restricted: false,
        };
        parse_field(fields[0], 0, 59, &mut c.minutes)?;
        parse_field(fields[1], 0, 23, &mut c.hours)?;
        c.days_restricted = parse_field(fields[2], 1, 31, &mut c.days)?;
        parse_field(fields[3], 1, 12, &mut c.months)?;
        let mut dow = [false; 8];
        c.weekdays_restricted = parse_field(fields[4], 0, 7, &mut dow)?;
        for (i, on) in dow.iter().enumerate() {
            if *on {
                c.weekdays[i % 7] = true; // 7 = dimanche
            }
        }
        Ok(c)
    }

    fn day_matches(&self, date: NaiveDate) -> bool {
        if !self.months[date.month() as usize] {
            return false;
        }
        let dom = self.days[date.day() as usize];
        let dow = self.weekdays[date.weekday().num_days_from_sunday() as usize];
        // Règle cron : si jour du mois ET jour de semaine sont restreints, l'un ou l'autre suffit.
        if self.days_restricted && self.weekdays_restricted {
            dom || dow
        } else {
            dom && dow
        }
    }

    pub fn next_after(&self, tz: Tz, after: DateTime<Utc>) -> Option<DateTime<Utc>> {
        let mut date = after.with_timezone(&tz).date_naive();
        for _ in 0..(366 * 5) {
            if self.day_matches(date) {
                for h in (0..24).filter(|h| self.hours[*h as usize]) {
                    for m in (0..60).filter(|m| self.minutes[*m as usize]) {
                        let Some(naive) = date.and_hms_opt(h, m, 0) else {
                            continue;
                        };
                        // Heure inexistante (passage à l'heure d'été) : ignorée.
                        let Some(local) = tz.from_local_datetime(&naive).earliest() else {
                            continue;
                        };
                        let utc = local.with_timezone(&Utc);
                        if utc > after {
                            return Some(utc);
                        }
                    }
                }
            }
            date = date.succ_opt()?;
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn plan(v: Value) -> Result<Plan, String> {
        Plan::from_params(v.as_object().unwrap())
    }

    fn utc(s: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(s).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn interval() {
        let p = plan(json!({"mode": "intervalle", "every": 15})).unwrap();
        assert_eq!(
            p.next_after(utc("2026-10-12T10:00:00Z")),
            Some(utc("2026-10-12T10:15:00Z"))
        );
        assert!(plan(json!({"mode": "intervalle", "every": 0})).is_err());
    }

    #[test]
    fn daily_in_paris_time() {
        let p = plan(json!({"mode": "quotidien", "time": "08:30", "timezone": "Europe/Paris"}))
            .unwrap();
        // Octobre : UTC+2.
        assert_eq!(
            p.next_after(utc("2026-10-12T05:00:00Z")),
            Some(utc("2026-10-12T06:30:00Z"))
        );
        assert_eq!(
            p.next_after(utc("2026-10-12T06:30:00Z")),
            Some(utc("2026-10-13T06:30:00Z"))
        );
        // Après le passage à l'heure d'hiver (25 octobre 2026) : UTC+1.
        assert_eq!(
            p.next_after(utc("2026-10-26T00:00:00Z")),
            Some(utc("2026-10-26T07:30:00Z"))
        );
    }

    #[test]
    fn weekly() {
        let p = plan(
            json!({"mode": "hebdomadaire", "time": "09:00", "days": "lun, ven", "timezone": "UTC"}),
        )
        .unwrap();
        // 12/10/2026 = lundi.
        assert_eq!(
            p.next_after(utc("2026-10-12T09:00:00Z")),
            Some(utc("2026-10-16T09:00:00Z"))
        );
        assert!(
            plan(json!({"mode": "hebdomadaire", "time": "09:00", "days": "", "timezone": "UTC"}))
                .is_err()
        );
    }

    #[test]
    fn cron_expressions() {
        let p =
            plan(json!({"mode": "cron", "cron": "*/15 9-17 * * 1-5", "timezone": "UTC"})).unwrap();
        assert_eq!(
            p.next_after(utc("2026-10-12T09:07:00Z")),
            Some(utc("2026-10-12T09:15:00Z"))
        );
        assert_eq!(
            p.next_after(utc("2026-10-16T17:45:00Z")),
            Some(utc("2026-10-19T09:00:00Z"))
        );
        // Jour du mois OU jour de semaine quand les deux sont restreints.
        let p = plan(json!({"mode": "cron", "cron": "0 0 1 * 0", "timezone": "UTC"})).unwrap();
        assert_eq!(
            p.next_after(utc("2026-10-12T00:00:00Z")),
            Some(utc("2026-10-18T00:00:00Z"))
        );
        for bad in [
            "* * *",
            "60 * * * *",
            "* * * * 8",
            "*/0 * * * *",
            "a * * * *",
        ] {
            assert!(plan(json!({"mode": "cron", "cron": bad})).is_err(), "{bad}");
        }
        assert!(
            plan(json!({"mode": "cron", "cron": "* * * * *", "timezone": "Mars/Olympus"})).is_err()
        );
    }
}
