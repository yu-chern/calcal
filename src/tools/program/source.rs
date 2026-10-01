use super::{Datum, Engine, Input, number::Number, syntax};
use chrono::NaiveDate;

/// Source text is supplied by the server, never by model arguments.
pub fn bind(input: &Input, ctx: &super::ToolContext, engine: &mut Engine) -> Result<Datum, String> {
    let text = ctx.sources.get(input.source).ok_or("输入 source 不存在")?;
    if input.quote.trim() != input.quote || input.quote.is_empty() || input.quote.len() > 2048 {
        return Err("输入 quote 必须是用户原文中的非空完整片段".into());
    }
    // Prevent selecting 14 from 2014, or .5 / 5 from 18.5.
    let numeric_boundary = |c: char| c.is_ascii_digit() || c == '.';
    let found = text.match_indices(&input.quote).any(|(start, _)| {
        let end = start + input.quote.len();
        if matches!(input.kind.as_str(), "number" | "numbers" | "expression") {
            let before = text[..start].chars().next_back();
            let after = text[end..].chars().next();
            if input.quote.starts_with(numeric_boundary) {
                if before.is_some_and(|c| matches!(c, 'e' | 'E')) {
                    return false;
                }
                if before.is_some_and(|c| matches!(c, '-' | '+')) {
                    let prefix = &text[..start - 1];
                    let binary = prefix
                        .trim_end()
                        .chars()
                        .next_back()
                        .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, ')' | ']'));
                    if !binary {
                        return false;
                    }
                }
            }
            if input.quote.ends_with(numeric_boundary)
                && after.is_some_and(|c| matches!(c, '%' | '％' | 'e' | 'E'))
            {
                return false;
            }
        }
        !(input.quote.starts_with(numeric_boundary)
            && text[..start]
                .chars()
                .next_back()
                .is_some_and(numeric_boundary)
            || input.quote.ends_with(numeric_boundary)
                && text[end..].chars().next().is_some_and(numeric_boundary))
    });
    if !found {
        return Err("输入 quote 无法匹配用户原文或截断了原始数字；不能绑定模型推导值".into());
    }
    match input.kind.as_str() {
        "direction" | "month_rule" => {
            let selected = ctx
                .selections
                .get(&input.source)
                .filter(|_| input.quote == text.trim());
            let q = selected.map_or(input.quote.as_str(), String::as_str);
            let rule = if input.kind == "direction" {
                match q {
                    "past" | "过去" | "之前" | "以前" | "往前" | "只向过去找" | "向过去" => {
                        "past"
                    }
                    "future" | "未来" | "之后" | "以后" | "往后" | "只向未来找" | "向未来" => {
                        "future"
                    }
                    "nearest" | "双向" | "前后比较" | "前后" | "两边" => "nearest",
                    _ => {
                        return Err(
                            "搜索方向必须绑定用户明确的过去、未来或双向选择；仅说最近必须先clarify"
                                .into(),
                        );
                    }
                }
            } else {
                let q = q
                    .trim_start_matches("若该日不存在就用")
                    .trim_start_matches("不存在时用")
                    .trim_start_matches("不存在就用")
                    .trim_start_matches("使用")
                    .trim_start_matches('用');
                match q {
                    "目标月末"
                    | "目标月份的最后一天"
                    | "目标月份最后一天"
                    | "clamp"
                    | "月末"
                    | "目标月末日期"
                    | "最后一天"
                    | "使用目标月份的最后一天" => "clamp",
                    "carry" | "顺延" | "向后顺延" | "超出的天数向后顺延" => "carry",
                    "strict" | "不调整" | "保留原日期" => "strict",
                    _ => return Err("月末处理规则必须来自用户确认，请先clarify".into()),
                }
            };
            Ok(Datum::Symbol(rule.into()))
        }
        "numbers" => {
            let text = input.quote.trim_matches(['[', ']']);
            let values = text
                .split([',', '，', '、'])
                .map(|s| number(s.trim()).map(Datum::Number))
                .collect::<Result<Vec<_>, _>>()?;
            if values.is_empty() || values.len() > 256 {
                return Err("数值列表需要1至256项".into());
            }
            Ok(Datum::List(values))
        }
        "number" => Ok(Datum::Number(number(&input.quote)?)),
        "date" => {
            let normalized = input
                .quote
                .replace(['年', '月'], "-")
                .trim_end_matches(['日', '号'])
                .to_owned();
            let d = NaiveDate::parse_from_str(&normalized, "%Y-%m-%d")
                .map_err(|_| "日期输入必须含明确年月日且有效；相对日期请通过程序运算")?;
            super::valid_date(d).map(Datum::Date)
        }
        "weekday" => {
            let word = input
                .quote
                .trim_start_matches("星期")
                .trim_start_matches("周")
                .trim_start_matches("礼拜");
            let n = if matches!(word, "日" | "天") {
                7
            } else {
                number(word)?.int()?
            };
            if !(1..=7).contains(&n) {
                return Err("星期必须为周一至周日".into());
            }
            Ok(Datum::Number(Number::integer(n)))
        }
        "month" => {
            let n = number(input.quote.trim_end_matches('月'))?.int()?;
            if !(1..=12).contains(&n) {
                return Err("月份必须为一月至十二月".into());
            }
            Ok(Datum::Number(Number::integer(n)))
        }
        "expression" => {
            let ast = syntax::parse(&input.quote, true)?;
            // Only mathematical builtins; no other input or prior step can be imported.
            let mut clean = Engine::new(engine.today);
            clean.deadline = engine.deadline;
            clean.fuel = engine.fuel;
            let result = clean.evaluate(&ast);
            engine.fuel = clean.fuel;
            result
        }
        _ => Err("未知输入类型".into()),
    }
}
fn number(s: &str) -> Result<Number, String> {
    if s.len() > 128 {
        return Err("数值输入过长".into());
    }
    if let Some(s) = s.strip_suffix('%').or_else(|| s.strip_suffix('％')) {
        return atom(s)?.div(&Number::integer(100));
    }
    if let Some(s) = s.strip_prefix("百分之") {
        return atom(s)?.div(&Number::integer(100));
    }
    if let Some(s) = s.strip_suffix('折') {
        return atom(s)?.div(&Number::integer(10));
    }
    let s = s
        .trim_end_matches("个月")
        .trim_end_matches("小时")
        .trim_end_matches("分钟")
        .trim_end_matches(['元', '天', '周', '月', '年', '个', '秒']);
    atom(s)
}
fn digit(c: char) -> Option<i128> {
    match c {
        '零' | '〇' => Some(0),
        '一' => Some(1),
        '二' | '两' => Some(2),
        '三' => Some(3),
        '四' => Some(4),
        '五' => Some(5),
        '六' => Some(6),
        '七' => Some(7),
        '八' => Some(8),
        '九' => Some(9),
        _ => None,
    }
}
fn atom(s: &str) -> Result<Number, String> {
    if s == "半" {
        return Number::rational(1, 2);
    }
    if let Ok(n) = Number::parse(s) {
        return Ok(n);
    }
    let negative = s.starts_with('负');
    let s = s.strip_prefix('负').unwrap_or(s);
    if s.is_empty() || s.chars().count() > 24 {
        return Err("不能解析输入数值".into());
    }
    let n = if let Some((a, b)) = s.split_once('点') {
        if b.is_empty() || b.chars().any(|c| digit(c).is_none()) {
            return Err("中文小数格式无效".into());
        }
        let fraction = b
            .chars()
            .map(|c| digit(c).unwrap().to_string())
            .collect::<String>();
        Number::parse(&format!("{}.{}", chinese_integer(a)?, fraction))?
    } else {
        Number::integer(chinese_integer(s)?)
    };
    if negative { n.neg() } else { Ok(n) }
}
fn chinese_integer(s: &str) -> Result<i128, String> {
    // Large sections are parsed by place value, so 一万亿 is not mistaken for 一亿零一万.
    if let Some((a, b)) = s.split_once('亿') {
        if b.contains('亿') || a.is_empty() {
            return Err("中文数词格式不明确，请使用阿拉伯数字".into());
        }
        let hi = chinese_wan(a)?;
        let lo = if b.is_empty() { 0 } else { chinese_wan(b)? };
        if lo >= 100000000 {
            return Err("中文数词分节无效".into());
        }
        return Ok(hi * 100000000 + lo);
    }
    chinese_wan(s)
}
fn chinese_wan(s: &str) -> Result<i128, String> {
    if let Some((a, b)) = s.split_once('万') {
        if b.contains('万') || a.is_empty() {
            return Err("中文数词格式不明确，请使用阿拉伯数字".into());
        }
        let hi = chinese_small(a)?;
        let lo = if b.is_empty() { 0 } else { chinese_small(b)? };
        if lo >= 10000 {
            return Err("中文数词分节无效".into());
        }
        // 一万二 is colloquial and may mean 12000, not 10002.
        if !b.is_empty() && !b.starts_with(['零', '〇']) && b.chars().all(|c| digit(c).is_some())
        {
            return Err("中文省略数词有歧义，请用完整数词或阿拉伯数字".into());
        }
        return Ok(hi * 10000 + lo);
    }
    chinese_small(s)
}
fn chinese_small(s: &str) -> Result<i128, String> {
    if s.is_empty() {
        return Err("中文数词为空".into());
    }
    if s.chars().all(|c| digit(c).is_some()) {
        return s.chars().try_fold(0i128, |n, c| {
            n.checked_mul(10)
                .and_then(|n| n.checked_add(digit(c).unwrap()))
                .ok_or_else(|| "数值溢出".into())
        });
    }
    let (mut total, mut n, mut last, mut zero) = (0i128, 0i128, 10000i128, false);
    for c in s.chars() {
        if let Some(d) = digit(c) {
            if d == 0 {
                zero = true;
            } else {
                if n != 0 {
                    return Err("中文数词格式不明确".into());
                }
                n = d;
            }
        } else {
            let k = match c {
                '十' => 10,
                '百' => 100,
                '千' => 1000,
                _ => return Err("不能解析输入数值，请使用阿拉伯数字或完整中文数词".into()),
            };
            if k >= last {
                return Err("中文数词单位顺序无效".into());
            }
            total += n.max(1) * k;
            n = 0;
            last = k;
            zero = false;
        }
    }
    if n != 0 && last >= 100 && !zero {
        return Err("中文省略数词有歧义，请用完整数词或阿拉伯数字".into());
    }
    Ok(total + n)
}
