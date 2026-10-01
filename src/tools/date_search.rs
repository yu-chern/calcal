use super::calendar::{date, describe};
use chrono::{Datelike, Days, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    pub anchor: String,
    pub direction: String,
    pub include_anchor: bool,
    pub limit: usize,
    pub within_days: u64,
    pub months: Vec<u32>,
    pub month_days: Vec<u32>,
    pub weekdays: Vec<u32>,
    pub leap_year: Option<bool>,
    pub month_end: Option<bool>,
}
impl Search {
    fn matches(&self, d: NaiveDate) -> bool {
        (self.months.is_empty() || self.months.contains(&d.month()))
            && (self.month_days.is_empty() || self.month_days.contains(&d.day()))
            && (self.weekdays.is_empty()
                || self.weekdays.contains(&d.weekday().number_from_monday()))
            && self.leap_year.is_none_or(|v| v == d.leap_year())
            && self
                .month_end
                .is_none_or(|v| v == d.succ_opt().is_some_and(|next| next.month() != d.month()))
    }
    pub(super) async fn execute(&self) -> Result<Value, String> {
        let anchor = date(Some(&self.anchor))?;
        if !matches!(self.direction.as_str(), "past" | "future" | "nearest")
            || !(1..=16).contains(&self.limit)
            || !(1..=146097).contains(&self.within_days)
            || self.months.len() > 12
            || self.month_days.len() > 31
            || self.weekdays.len() > 7
            || self.months.iter().any(|m| !(1..=12).contains(m))
            || self.month_days.iter().any(|d| !(1..=31).contains(d))
            || self.weekdays.iter().any(|d| !(1..=7).contains(d))
        {
            return Err("日期搜索方向、条件或边界无效".into());
        }
        let mut dates = Vec::new();
        let mut checked = 0;
        let mut through = 0;
        for distance in u64::from(!self.include_anchor)..=self.within_days {
            // A bounded CPU loop must yield so the tool and run timeouts can cancel it.
            if distance % 1024 == 0 {
                tokio::task::yield_now().await;
            }
            through = distance;
            let delta = Days::new(distance);
            let candidates = if distance == 0 {
                [Some(anchor), None]
            } else {
                [
                    (self.direction != "future")
                        .then(|| anchor.checked_sub_days(delta))
                        .flatten(),
                    (self.direction != "past")
                        .then(|| anchor.checked_add_days(delta))
                        .flatten(),
                ]
            };
            for d in candidates
                .into_iter()
                .flatten()
                .filter(|d| (1..=9999).contains(&d.year()))
            {
                checked += 1;
                if self.matches(d) {
                    let mut item = describe(d);
                    item["distance_days"] = json!(distance);
                    dates.push(item);
                }
            }
            // Finish both sides of this distance: never silently choose one of two ties.
            if dates.len() >= self.limit {
                break;
            }
        }
        Ok(
            json!({"dates":dates,"anchor":self.anchor,"direction":self.direction,
            "within_days":self.within_days,"include_anchor":self.include_anchor,
            "requested_limit":self.limit,"tie_at_limit":dates.len()>self.limit,
            "checked_dates":checked,"checked_through_distance_days":through,
            "range_exhausted":dates.len()<self.limit,
            "calendar":"Gregorian","weekday_convention":"Monday=1"}),
        )
    }
}
