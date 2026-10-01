use super::{Tool, ToolContext};
use async_trait::async_trait;
use chrono::{Datelike, Days, Months, NaiveDate};
use serde::Deserialize;
use serde_json::{Value, json};

pub struct Calendar;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Arguments {
    operation: String,
    date: Option<String>,
    start: Option<String>,
    end: Option<String>,
    amount: Option<i64>,
    unit: Option<String>,
}
pub(super) fn date(value: Option<&str>) -> Result<NaiveDate, String> {
    let value = value.ok_or("缺少日期")?;
    if value.len() != 10 {
        return Err("日期格式必须为 YYYY-MM-DD".into());
    }
    let d = NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(|_| "日期不存在或格式错误")?;
    if !(1..=9999).contains(&d.year()) {
        return Err("日期超出支持范围 0001—9999".into());
    }
    Ok(d)
}
pub(super) fn describe(d: NaiveDate) -> Value {
    json!({"date":d.to_string(),"weekday":d.weekday().number_from_monday(),"leap_year":d.leap_year()})
}
#[async_trait]
impl Tool for Calendar {
    async fn execute(&self, args: Value, ctx: &ToolContext) -> Result<Value, String> {
        let a: Arguments = serde_json::from_value(args).map_err(|_| "日期工具参数无效")?;
        let valid_fields = match a.operation.as_str() {
            "today" => {
                a.date.is_none()
                    && a.start.is_none()
                    && a.end.is_none()
                    && a.amount.is_none()
                    && a.unit.is_none()
            }
            "inspect" => {
                a.date.is_some()
                    && a.start.is_none()
                    && a.end.is_none()
                    && a.amount.is_none()
                    && a.unit.is_none()
            }
            "diff" => {
                a.date.is_none()
                    && a.start.is_some()
                    && a.end.is_some()
                    && a.amount.is_none()
                    && a.unit.is_none()
            }
            "add" => {
                a.date.is_some()
                    && a.start.is_none()
                    && a.end.is_none()
                    && a.amount.is_some()
                    && a.unit.is_some()
            }
            _ => false,
        };
        if !valid_fields {
            return Err("日期操作的必需参数缺失或存在不适用的参数，请先确认计算口径".into());
        }
        match a.operation.as_str() {
            "today" => {
                let mut result =
                    describe(ctx.reference_time.with_timezone(&ctx.timezone).date_naive());
                result["timezone"] = json!(ctx.timezone.to_string());
                Ok(result)
            }
            "inspect" => Ok(describe(date(a.date.as_deref())?)),
            "diff" => {
                let start = date(a.start.as_deref())?;
                let end = date(a.end.as_deref())?;
                Ok(
                    json!({"days":(end-start).num_days(),"start":start.to_string(),"end":end.to_string()}),
                )
            }
            "add" => {
                let d = date(a.date.as_deref())?;
                let n = a.amount.ok_or("缺少 amount")?;
                if n.unsigned_abs() > 100000 {
                    return Err("日期偏移过大".into());
                }
                let unit = a.unit.as_deref().ok_or("缺少 unit")?;
                let result = match unit {
                    "days" | "weeks" => {
                        let days =
                            Days::new(n.unsigned_abs() * if unit == "weeks" { 7 } else { 1 });
                        if n >= 0 {
                            d.checked_add_days(days)
                        } else {
                            d.checked_sub_days(days)
                        }
                    }
                    "months" | "years" => {
                        let months = Months::new(
                            n.unsigned_abs() as u32 * if unit == "years" { 12 } else { 1 },
                        );
                        let next = if n >= 0 {
                            d.checked_add_months(months)
                        } else {
                            d.checked_sub_months(months)
                        };
                        if next.is_some_and(|v| v.day() != d.day()) {
                            return Err("目标月份不存在该日，请确认是否使用月末日期".into());
                        }
                        next
                    }
                    _ => return Err("不支持的日期单位".into()),
                }
                .ok_or("日期运算溢出")?;
                if !(1..=9999).contains(&result.year()) {
                    return Err("日期超出支持范围 0001—9999".into());
                }
                Ok(describe(result))
            }
            _ => Err("不支持的日期操作".into()),
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn date_boundaries_and_timezone() {
        let ctx = ToolContext {
            reference_time: "2026-09-28T23:30:00Z".parse().unwrap(),
            timezone: chrono_tz::Europe::Berlin,
        };
        assert_eq!(
            Calendar
                .execute(json!({"operation":"today"}), &ctx)
                .await
                .unwrap()["date"],
            "2026-09-29"
        );
        assert_eq!(
            Calendar
                .execute(
                    json!({"operation":"diff","start":"2026-09-28","end":"2026-10-12"}),
                    &ctx
                )
                .await
                .unwrap()["days"],
            14
        );
        assert_eq!(
            Calendar
                .execute(
                    json!({"operation":"add","date":"2024-02-28","amount":1,"unit":"days"}),
                    &ctx
                )
                .await
                .unwrap()["date"],
            "2024-02-29"
        );
        for args in [
            json!({"operation":"inspect","date":"2025-02-29"}),
            json!({"operation":"add","date":"2026-01-31","amount":1,"unit":"months"}),
            json!({"operation":"add","date":"9999-12-31","amount":1,"unit":"days"}),
        ] {
            assert!(Calendar.execute(args, &ctx).await.is_err());
        }
    }
}
