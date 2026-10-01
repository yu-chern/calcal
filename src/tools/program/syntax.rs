//! Small expression grammar; no strings, attributes, imports, assignment or I/O.
#[derive(Clone, Debug)]
pub enum Expr {
    Name(String),
    Number(String),
    List(Vec<Expr>),
    Call(String, Vec<Expr>),
    Unary(String, Box<Expr>),
    Binary(String, Box<Expr>, Box<Expr>),
}
#[derive(Clone, Debug, PartialEq)]
enum Token {
    Name(String),
    Number(String),
    Op(String),
    End,
}
pub fn parse(source: &str, literals: bool) -> Result<Expr, String> {
    if source.is_empty() || source.len() > 2048 {
        return Err("表达式为空或过长".into());
    }
    let mut tokens = Vec::new();
    let b = source.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_alphabetic() || c == b'_' {
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            tokens.push(Token::Name(source[start..i].into()));
        } else if c.is_ascii_digit() || c == b'.' {
            if !literals {
                return Err("程序禁止数值字面量；请绑定用户原文输入或引用此前工具结果".into());
            }
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'.') {
                i += 1;
            }
            if i < b.len() && matches!(b[i], b'e' | b'E') {
                i += 1;
                if i < b.len() && matches!(b[i], b'+' | b'-') {
                    i += 1;
                }
                while i < b.len() && b[i].is_ascii_digit() {
                    i += 1;
                }
            }
            tokens.push(Token::Number(source[start..i].into()));
        } else if b"+-*/%^<>=!&|(),[]".contains(&c) {
            i += 1;
            if i < b.len()
                && matches!(
                    &source[start..i + 1],
                    "<=" | ">=" | "==" | "!=" | "&&" | "||"
                )
            {
                i += 1;
            }
            tokens.push(Token::Op(source[start..i].into()));
        } else {
            return Err("表达式含不支持的字符；仅支持受限计算语言".into());
        }
        if tokens.len() > 512 {
            return Err("表达式过于复杂".into());
        }
    }
    tokens.push(Token::End);
    let mut parser = Parser { tokens, at: 0 };
    let expr = parser.expr(0, 0)?;
    if parser.peek() != &Token::End {
        return Err("表达式有多余内容".into());
    }
    Ok(expr)
}
struct Parser {
    tokens: Vec<Token>,
    at: usize,
}
impl Parser {
    fn peek(&self) -> &Token {
        &self.tokens[self.at]
    }
    fn next(&mut self) -> Token {
        let v = self.tokens[self.at].clone();
        if v != Token::End {
            self.at += 1;
        }
        v
    }
    fn eat(&mut self, s: &str) -> bool {
        if self.peek() == &Token::Op(s.into()) {
            self.at += 1;
            true
        } else {
            false
        }
    }
    fn expect(&mut self, s: &str) -> Result<(), String> {
        if self.eat(s) {
            Ok(())
        } else {
            Err(format!("表达式缺少 {s}"))
        }
    }
    fn items(&mut self, end: &str, depth: usize) -> Result<Vec<Expr>, String> {
        let mut out = vec![];
        if self.eat(end) {
            return Ok(out);
        }
        loop {
            out.push(self.expr(0, depth + 1)?);
            if self.eat(end) {
                return Ok(out);
            }
            self.expect(",")?;
        }
    }
    fn expr(&mut self, min: u8, depth: usize) -> Result<Expr, String> {
        if depth > 32 {
            return Err("表达式嵌套过深".into());
        }
        let mut left = match self.next() {
            Token::Number(v) => Expr::Number(v),
            Token::Name(v) => {
                if self.eat("(") {
                    Expr::Call(v, self.items(")", depth)?)
                } else {
                    Expr::Name(v)
                }
            }
            Token::Op(v) if v == "[" => Expr::List(self.items("]", depth)?),
            Token::Op(v) if v == "(" => {
                let e = self.expr(0, depth + 1)?;
                self.expect(")")?;
                e
            }
            Token::Op(v) if matches!(v.as_str(), "-" | "+" | "!") => {
                Expr::Unary(v, Box::new(self.expr(6, depth + 1)?))
            }
            _ => return Err("表达式缺少操作数".into()),
        };
        while let Token::Op(op) = self.peek() {
            let (l, r) = match op.as_str() {
                "||" => (1, 2),
                "&&" => (2, 3),
                "==" | "!=" | "<" | ">" | "<=" | ">=" => (3, 4),
                "+" | "-" => (4, 5),
                "*" | "/" | "%" => (5, 6),
                "^" => (7, 7),
                _ => (0, 0),
            };
            if l == 0 || l < min {
                break;
            }
            let op = op.clone();
            self.next();
            left = Expr::Binary(op, Box::new(left), Box::new(self.expr(r, depth + 1)?));
        }
        Ok(left)
    }
}
