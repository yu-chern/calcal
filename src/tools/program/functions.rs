use super::*;
use chrono::{Days, Months};

impl Engine {
    pub(super) fn call(&mut self, name: &str, args: &[Expr]) -> Result<Datum, String> {
        if name == "if" {
            arity(args.len(), 3)?;
            let test = self.eval(&args[0])?.boolean()?;
            return self.eval(&args[if test { 1 } else { 2 }]);
        }
        if matches!(name, "map" | "filter") {
            arity(args.len(), 2)?;
            let values = self.eval(&args[0])?.list()?.to_vec();
            let previous = self.env.get("item").cloned();
            let mut out = vec![];
            for item in values {
                self.env.insert("item".into(), item.clone());
                let result = self.eval(&args[1]);
                if let Some(v) = &previous {
                    self.env.insert("item".into(), v.clone());
                } else {
                    self.env.remove("item");
                }
                let result = result?;
                if name == "map" {
                    out.push(result);
                } else if result.boolean()? {
                    out.push(item);
                }
            }
            return Ok(Datum::List(out));
        }
        let v = args
            .iter()
            .map(|e| self.eval(e))
            .collect::<Result<Vec<_>, _>>()?;
        let a = |i: usize| v.get(i).ok_or_else(|| "函数缺少参数".to_owned());
        match name {
            "square" | "cube" | "abs" | "floor" | "ceil" | "round" | "sqrt" | "sin" | "cos"
            | "tan" | "ln" | "exp" | "approx" | "percent" => {
                arity(v.len(), 1)?;
                let n = a(0)?.number()?;
                let result = match name {
                    "square" => n.mul(n)?,
                    "cube" => n.mul(n)?.mul(n)?,
                    "abs" => {
                        if n.cmp(&Number::integer(0))?.is_lt() {
                            n.neg()?
                        } else {
                            n.clone()
                        }
                    }
                    "floor" | "ceil" | "round" => n.round(name)?,
                    "percent" => n.div(&Number::integer(100))?,
                    "sqrt" => {
                        if let Number::Exact(n, d) = n {
                            if *n < 0 {
                                return Err("负数的实数平方根不存在".into());
                            }
                            let (a, b) = (isqrt(*n), isqrt(*d));
                            if a * a == *n && b * b == *d {
                                Number::rational(a, b)?
                            } else {
                                Number::approx((*n as f64 / *d as f64).sqrt())?
                            }
                        } else {
                            Number::approx(n.float().sqrt())?
                        }
                    }
                    _ => Number::approx(match name {
                        "sin" => n.float().sin(),
                        "cos" => n.float().cos(),
                        "tan" => n.float().tan(),
                        "ln" => n.float().ln(),
                        "exp" => n.float().exp(),
                        _ => n.float(),
                    })?,
                };
                Ok(Datum::Number(result))
            }
            "round_places" => {
                arity(v.len(), 2)?;
                let places = a(1)?.integer()?;
                if !(-18..=18).contains(&places) {
                    return Err("舍入位数必须在-18至18之间".into());
                }
                let factor = Number::integer(10).pow(&Number::integer(places))?;
                Ok(Datum::Number(
                    a(0)?.number()?.mul(&factor)?.round("round")?.div(&factor)?,
                ))
            }
            "sum" | "mean" | "min" | "max" | "sort" | "count" => {
                let items = if v.len() == 1 && matches!(&v[0], Datum::List(_)) {
                    v[0].list()?.to_vec()
                } else if matches!(name, "min" | "max") {
                    v.clone()
                } else {
                    return Err("该函数需要一个列表参数".into());
                };
                if name == "count" {
                    return Ok(Datum::Number(Number::integer(items.len() as i128)));
                }
                if matches!(name, "sum" | "mean") {
                    if name == "mean" && items.is_empty() {
                        return Err("空列表没有平均值".into());
                    }
                    let mut n = Number::integer(0);
                    for item in &items {
                        self.tick()?;
                        n = n.add(item.number()?)?;
                    }
                    if name == "mean" {
                        n = n.div(&Number::integer(items.len() as i128))?;
                    }
                    return Ok(Datum::Number(n));
                }
                // Fallible insertion sort with the same global fuel/deadline.
                let mut sorted: Vec<Datum> = vec![];
                for item in items {
                    let mut i = sorted.len();
                    for (j, other) in sorted.iter().enumerate() {
                        self.tick()?;
                        if compare(&item, other)?.is_lt() {
                            i = j;
                            break;
                        }
                    }
                    sorted.insert(i, item);
                }
                if name == "sort" {
                    Ok(Datum::List(sorted))
                } else {
                    if name == "max" {
                        sorted.reverse();
                    }
                    sorted
                        .first()
                        .cloned()
                        .ok_or_else(|| "空列表没有极值".into())
                }
            }
            "at" => {
                arity(v.len(), 2)?;
                let i = usize::try_from(a(1)?.integer()?).map_err(|_| "下标必须为非负整数")?;
                a(0)?
                    .list()?
                    .get(i)
                    .cloned()
                    .ok_or_else(|| "列表下标越界".into())
            }
            "range" => {
                if !(2..=3).contains(&v.len()) {
                    return Err("range需要开始、结束和可选步长".into());
                }
                let step = if v.len() == 3 { a(2)?.integer()? } else { 1 };
                if step == 0 || step.unsigned_abs() > 100000 {
                    return Err("范围步长无效".into());
                }
                let mut item = a(0)?.clone();
                let end = a(1)?;
                let mut out = vec![];
                loop {
                    self.tick()?;
                    let cmp = compare(&item, end)?;
                    if (step > 0 && cmp.is_gt()) || (step < 0 && cmp.is_lt()) {
                        break;
                    }
                    if out.len() >= 4096 {
                        return Err("列表最多4096项，请缩小范围或使用find_dates".into());
                    }
                    out.push(item.clone());
                    if cmp.is_eq() {
                        break;
                    }
                    item = match item {
                        Datum::Date(d) => Datum::Date(add_days(d, step)?),
                        Datum::Number(n) => Datum::Number(n.add(&Number::integer(step))?),
                        _ => return Err("range仅支持日期或数值".into()),
                    };
                }
                Ok(Datum::List(out))
            }
            "date" => {
                arity(v.len(), 3)?;
                let y = i32::try_from(a(0)?.integer()?).map_err(|_| "年份无效")?;
                let m = u32::try_from(a(1)?.integer()?).map_err(|_| "月份无效")?;
                let d = u32::try_from(a(2)?.integer()?).map_err(|_| "日无效")?;
                Ok(Datum::Date(valid_date(
                    NaiveDate::from_ymd_opt(y, m, d).ok_or("日期不存在")?,
                )?))
            }
            "date_diff" => {
                arity(v.len(), 2)?;
                Ok(Datum::Number(Number::integer(
                    (a(1)?.date()? - a(0)?.date()?).num_days() as i128,
                )))
            }
            "date_add" => {
                if !(3..=4).contains(&v.len()) {
                    return Err("date_add需要日期、整数偏移、单位和可选月末规则".into());
                }
                let d = a(0)?.date()?;
                let n = a(1)?.integer()?;
                if n.unsigned_abs() > 100000 {
                    return Err("日期偏移超出上限".into());
                }
                let rule = if v.len() == 4 {
                    a(3)?.symbol()?
                } else {
                    "strict"
                };
                if !matches!(rule, "strict" | "clamp" | "carry") {
                    return Err("月末规则无效".into());
                }
                let result =
                    match a(2)?.symbol()? {
                        "days" => add_days(d, n)?,
                        "weeks" => add_days(d, n * 7)?,
                        "months" | "years" => {
                            let months = Months::new(
                                n.unsigned_abs() as u32
                                    * if a(2)?.symbol()? == "years" { 12 } else { 1 },
                            );
                            let next = if n >= 0 {
                                d.checked_add_months(months)
                            } else {
                                d.checked_sub_months(months)
                            }
                            .ok_or("日期溢出")?;
                            if next.day() != d.day() {
                                match rule {
                                    "strict" => return Err(
                                        "目标月份不存在该日，必须先clarify确认使用月末还是溢出顺延"
                                            .into(),
                                    ),
                                    "carry" => add_days(next, (d.day() - next.day()) as i128)?,
                                    _ => next,
                                }
                            } else {
                                next
                            }
                        }
                        _ => return Err("日期偏移单位无效".into()),
                    };
                Ok(Datum::Date(valid_date(result)?))
            }
            "year" | "month" | "day" | "weekday" | "is_leap" | "is_month_end" | "month_end" => {
                arity(v.len(), 1)?;
                let d = a(0)?.date()?;
                Ok(match name {
                    "year" => Datum::Number(Number::integer(d.year() as i128)),
                    "month" => Datum::Number(Number::integer(d.month() as i128)),
                    "day" => Datum::Number(Number::integer(d.day() as i128)),
                    "weekday" => {
                        Datum::Number(Number::integer(d.weekday().number_from_monday() as i128))
                    }
                    "is_leap" => Datum::Bool(d.leap_year()),
                    "is_month_end" => {
                        Datum::Bool(d.succ_opt().is_some_and(|next| next.month() != d.month()))
                    }
                    _ => {
                        let mut end = d;
                        while let Some(next) = end.succ_opt() {
                            if next.month() != d.month() {
                                break;
                            }
                            end = next;
                        }
                        Datum::Date(end)
                    }
                })
            }
            "find_dates" => self.find_dates(&v),
            "dates" => {
                arity(v.len(), 1)?;
                if let Datum::Search { dates, evidence } = a(0)? {
                    if evidence["tie_at_limit"] == true {
                        return Err("最近日期存在并列，请先clarify确认取舍规则；可以直接输出搜索结果展示所有并列候选".into());
                    }
                    Ok(Datum::List(dates.clone()))
                } else {
                    Err("dates需要日期搜索结果".into())
                }
            }
            _ => Err(format!("不支持函数 {name}")),
        }
    }
    fn find_dates(&mut self, v: &[Datum]) -> Result<Datum, String> {
        arity(v.len(), 10)?;
        let anchor = v[0].date()?;
        let direction = v[1].symbol()?;
        let limit = usize::try_from(v[2].integer()?).map_err(|_| "数量无效")?;
        let within = u64::try_from(v[3].integer()?).map_err(|_| "范围无效")?;
        if !matches!(direction, "past" | "future" | "nearest")
            || !(1..=16).contains(&limit)
            || !(1..=146097).contains(&within)
        {
            return Err("搜索方向、数量或范围无效".into());
        }
        let list = |i: usize, max: i128| -> Result<Vec<i128>, String> {
            v[i].list()?
                .iter()
                .map(|x| {
                    let n = x.integer()?;
                    if (1..=max).contains(&n) {
                        Ok(n)
                    } else {
                        Err("日期筛选条件超出范围".into())
                    }
                })
                .collect()
        };
        let (months, days, weekdays) = (list(4, 12)?, list(5, 31)?, list(6, 7)?);
        let condition = |i: usize| -> Result<Option<bool>, String> {
            if matches!(v[i], Datum::Any) {
                Ok(None)
            } else {
                v[i].boolean().map(Some)
            }
        };
        let (leap, end, include) = (condition(7)?, condition(8)?, v[9].boolean()?);
        let mut dates = vec![];
        let mut checked = 0;
        let mut through = 0;
        for distance in u64::from(!include)..=within {
            self.tick()?;
            through = distance;
            let delta = Days::new(distance);
            let candidates = if distance == 0 {
                [Some(anchor), None]
            } else {
                [
                    (direction != "future")
                        .then(|| anchor.checked_sub_days(delta))
                        .flatten(),
                    (direction != "past")
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
                if (months.is_empty() || months.contains(&(d.month() as i128)))
                    && (days.is_empty() || days.contains(&(d.day() as i128)))
                    && (weekdays.is_empty()
                        || weekdays.contains(&(d.weekday().number_from_monday() as i128)))
                    && leap.is_none_or(|l| l == d.leap_year())
                    && end.is_none_or(|e| e == d.succ_opt().is_some_and(|n| n.month() != d.month()))
                {
                    dates.push(Datum::Date(d));
                }
            }
            if dates.len() >= limit {
                break;
            }
        }
        let evidence = json!({"anchor":anchor.to_string(),"direction":direction,"requested_limit":limit,"within_days":within,"include_anchor":include,"tie_at_limit":dates.len()>limit,"range_exhausted":dates.len()<limit,"checked_dates":checked,"checked_through_distance_days":through,"calendar":"Gregorian","distance":"calendar_days"});
        self.search_evidence.push(evidence.clone());
        Ok(Datum::Search { dates, evidence })
    }
}
fn arity(actual: usize, expected: usize) -> Result<(), String> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("函数需要 {expected} 个参数"))
    }
}
fn add_days(d: NaiveDate, n: i128) -> Result<NaiveDate, String> {
    let n = i64::try_from(n).map_err(|_| "日期偏移过大")?;
    let days = Days::new(n.unsigned_abs());
    valid_date(
        if n >= 0 {
            d.checked_add_days(days)
        } else {
            d.checked_sub_days(days)
        }
        .ok_or("日期溢出")?,
    )
}
fn isqrt(n: i128) -> i128 {
    if n < 2 {
        return n;
    }
    let (mut lo, mut hi) = (1, n / 2 + 1);
    while lo < hi {
        let mid = lo + ((hi - lo) / 2 + (hi - lo) % 2);
        if mid <= n / mid {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    lo
}
