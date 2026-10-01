//! Checked rational arithmetic. Exact values never silently fall back to floats.
use std::cmp::Ordering;

#[derive(Clone, Debug)]
pub enum Number {
    Exact(i128, i128),
    Approx(f64),
}
fn checked(v: Option<i128>) -> Result<i128, String> {
    v.ok_or_else(|| "精确数值超出128位有理数范围，请缩小数值或明确采用近似计算".into())
}
impl Number {
    pub fn integer(n: i128) -> Self {
        Self::Exact(n, 1)
    }
    pub fn rational(n: i128, d: i128) -> Result<Self, String> {
        if d == 0 {
            return Err("除数不能为零".into());
        }
        // Reserve MIN so abs, negation and normalization are always safe.
        if n == i128::MIN || d == i128::MIN {
            return Err("精确数值溢出".into());
        }
        let (mut a, mut b) = (n.abs(), d.abs());
        while b != 0 {
            (a, b) = (b, a % b);
        }
        let sign = if d < 0 { -1 } else { 1 };
        Ok(Self::Exact(n / a * sign, d / a * sign))
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        if s.len() > 100 {
            return Err("数值过长".into());
        }
        let (base, exponent) = if let Some(i) = s.find(['e', 'E']) {
            (&s[..i], s[i + 1..].parse::<i32>().map_err(|_| "指数无效")?)
        } else {
            (s, 0)
        };
        if exponent.unsigned_abs() > 36 {
            return Err("精确指数超出范围".into());
        }
        let decimals = base.split_once('.').map_or(0, |(_, f)| f.len());
        if decimals > 36 {
            return Err("小数位数超出范围".into());
        }
        let n = base
            .replace('.', "")
            .parse::<i128>()
            .map_err(|_| "数值格式错误或超出范围")?;
        let scale = decimals as i32 - exponent;
        if scale.unsigned_abs() > 36 {
            return Err("精确小数位数超出范围".into());
        }
        let factor = checked(10_i128.checked_pow(scale.unsigned_abs()))?;
        if scale >= 0 {
            Self::rational(n, factor)
        } else {
            Self::rational(checked(n.checked_mul(factor))?, 1)
        }
    }
    pub fn float(&self) -> f64 {
        match self {
            Self::Exact(n, d) => *n as f64 / *d as f64,
            Self::Approx(v) => *v,
        }
    }
    pub fn approx(v: f64) -> Result<Self, String> {
        if !v.is_finite() {
            Err("计算结果不是有限数值，请检查定义域或溢出".into())
        } else {
            Ok(Self::Approx(v))
        }
    }
    pub fn int(&self) -> Result<i128, String> {
        match self {
            Self::Exact(n, 1) => Ok(*n),
            _ => Err("此参数必须是精确整数，不自动舍入".into()),
        }
    }
    pub fn neg(&self) -> Result<Self, String> {
        self.mul(&Self::integer(-1))
    }
    pub fn add(&self, rhs: &Self) -> Result<Self, String> {
        match (self, rhs) {
            (Self::Exact(a, b), Self::Exact(c, d)) => {
                let g = gcd(*b, *d);
                Self::rational(
                    checked(
                        checked(a.checked_mul(d / g))?.checked_add(checked(c.checked_mul(b / g))?),
                    )?,
                    checked(b.checked_mul(d / g))?,
                )
            }
            _ => Self::approx(self.float() + rhs.float()),
        }
    }
    pub fn mul(&self, rhs: &Self) -> Result<Self, String> {
        match (self, rhs) {
            (Self::Exact(a, b), Self::Exact(c, d)) => {
                let g = gcd(a.abs(), *d);
                let h = gcd(c.abs(), *b);
                Self::rational(
                    checked((a / g).checked_mul(c / h))?,
                    checked((b / h).checked_mul(d / g))?,
                )
            }
            _ => Self::approx(self.float() * rhs.float()),
        }
    }
    pub fn div(&self, rhs: &Self) -> Result<Self, String> {
        match rhs {
            Self::Exact(0, _) => Err("除数不能为零".into()),
            Self::Exact(n, d) => self.mul(&Self::rational(*d, *n)?),
            _ => Self::approx(self.float() / rhs.float()),
        }
    }
    pub fn pow(&self, rhs: &Self) -> Result<Self, String> {
        if let Ok(n) = rhs.int() {
            if n.unsigned_abs() > 1024 {
                return Err("幂指数超出执行上限".into());
            }
            let mut value = Self::integer(1);
            let base = if n < 0 {
                Self::integer(1).div(self)?
            } else {
                self.clone()
            };
            for _ in 0..n.unsigned_abs() {
                value = value.mul(&base)?;
            }
            Ok(value)
        } else {
            Self::approx(self.float().powf(rhs.float()))
        }
    }
    pub fn cmp(&self, rhs: &Self) -> Result<Ordering, String> {
        match (self, rhs) {
            (Self::Exact(a, b), Self::Exact(c, d)) => {
                let g = gcd(*b, *d);
                Ok(checked(a.checked_mul(d / g))?.cmp(&checked(c.checked_mul(b / g))?))
            }
            _ => self
                .float()
                .partial_cmp(&rhs.float())
                .ok_or_else(|| "无法比较数值".into()),
        }
    }
    pub fn round(&self, mode: &str) -> Result<Self, String> {
        match self {
            Self::Exact(n, d) => {
                let q = n / d;
                let r = n % d;
                let offset = match mode {
                    "floor" if r < 0 => -1,
                    "ceil" if r > 0 => 1,
                    "round" if r.abs() >= d / 2 + d % 2 => n.signum(),
                    _ => 0,
                };
                Ok(Self::integer(checked(q.checked_add(offset))?))
            }
            Self::Approx(v) => Self::approx(match mode {
                "floor" => v.floor(),
                "ceil" => v.ceil(),
                _ => v.round(),
            }),
        }
    }
    pub fn text(&self) -> String {
        match self {
            Self::Approx(v) => format!("约 {v}"),
            Self::Exact(n, 1) => n.to_string(),
            Self::Exact(n, d) => {
                let mut remaining = *d;
                while remaining % 2 == 0 {
                    remaining /= 2;
                }
                while remaining % 5 == 0 {
                    remaining /= 5;
                }
                if remaining != 1 {
                    return format!("{n}/{d}");
                }
                let mut r = n.abs() % d;
                let mut text = format!("{}{}.", if *n < 0 { "-" } else { "" }, n.abs() / d);
                // Long division without overflowing r*10.
                while r != 0 {
                    let mut next = 0;
                    let mut digit = 0;
                    for _ in 0..10 {
                        if next >= d - r {
                            next -= d - r;
                            digit += 1;
                        } else {
                            next += r;
                        }
                    }
                    text.push(char::from(b'0' + digit));
                    r = next;
                }
                text
            }
        }
    }
}
fn gcd(mut a: i128, mut b: i128) -> i128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a.max(1)
}
