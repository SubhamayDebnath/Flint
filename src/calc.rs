//! Tiny safe math evaluator: + - * / % ^ ( ) and decimals.
//! Returns `None` when the text is not a (non-trivial) expression.

pub fn eval(src: &str) -> Option<String> {
    let s: Vec<u8> = src
        .chars()
        .filter(|c| !c.is_whitespace())
        .map(|c| match c {
            '×' => '*',
            '÷' => '/',
            c => c,
        })
        .map(|c| if c.is_ascii() { c as u8 } else { 0 })
        .collect();
    if s.is_empty()
        || !s
            .iter()
            .all(|c| c.is_ascii_digit() || b"+-*/%^().".contains(c))
        || !s.iter().any(u8::is_ascii_digit)
    {
        return None;
    }
    // needs an operator (a leading sign alone does not count)
    let body = if matches!(s[0], b'+' | b'-') {
        &s[1..]
    } else {
        &s[..]
    };
    if !body.iter().any(|c| b"+-*/%^".contains(c)) {
        return None;
    }
    let mut p = Parser {
        s: &s,
        i: 0,
        depth: 0,
    };
    let v = p.expr()?;
    if p.i != s.len() || !v.is_finite() {
        return None;
    }
    Some(format_num(v))
}

fn format_num(v: f64) -> String {
    // 12 significant digits, trailing zeros removed (mirrors Number(r.toPrecision(12)))
    let v = if v == 0.0 { 0.0 } else { v };
    let abs = v.abs();
    if abs != 0.0 && !(1e-7..1e21).contains(&abs) {
        let t = format!("{v:.11e}");
        let (m, e) = t.split_once('e').unwrap_or((&t, "0"));
        let m = if m.contains('.') {
            m.trim_end_matches('0').trim_end_matches('.')
        } else {
            m
        };
        let e: i32 = e.parse().unwrap_or(0);
        return format!("{m}e{}{}", if e < 0 { "-" } else { "+" }, e.abs());
    }
    let digits = 11 - abs.max(1e-300).log10().floor() as i32;
    let decimals = digits.clamp(0, 20) as usize;
    let t = format!("{v:.decimals$}");
    if t.contains('.') {
        t.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        t
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
    depth: u32,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn expr(&mut self) -> Option<f64> {
        let mut v = self.term()?;
        while let Some(op @ (b'+' | b'-')) = self.peek() {
            self.i += 1;
            let r = self.term()?;
            v = if op == b'+' { v + r } else { v - r };
        }
        Some(v)
    }

    fn term(&mut self) -> Option<f64> {
        let mut v = self.pow()?;
        while let Some(op @ (b'*' | b'/' | b'%')) = self.peek() {
            self.i += 1;
            let r = self.pow()?;
            v = match op {
                b'*' => v * r,
                b'/' => v / r,
                _ => v % r,
            };
        }
        Some(v)
    }

    fn pow(&mut self) -> Option<f64> {
        let b = self.unary()?;
        if self.peek() == Some(b'^') {
            self.i += 1;
            self.depth += 1;
            if self.depth > 64 {
                return None;
            }
            let e = self.pow()?;
            self.depth -= 1;
            return Some(b.powf(e));
        }
        Some(b)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek()? {
            b'-' => {
                self.i += 1;
                self.unary().map(|v| -v)
            }
            b'+' => {
                self.i += 1;
                self.unary()
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Option<f64> {
        if self.peek()? == b'(' {
            self.i += 1;
            self.depth += 1;
            if self.depth > 64 {
                return None; // no stack overflow on "((((((..."
            }
            let v = self.expr()?;
            self.depth -= 1;
            if self.peek()? != b')' {
                return None;
            }
            self.i += 1;
            return Some(v);
        }
        let start = self.i;
        while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'.') {
            self.i += 1;
        }
        std::str::from_utf8(&self.s[start..self.i])
            .ok()?
            .parse()
            .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::eval;

    #[test]
    fn basics() {
        assert_eq!(eval("2*(3+4)").as_deref(), Some("14"));
        assert_eq!(eval("1/3").as_deref(), Some("0.333333333333"));
        assert_eq!(eval("2^10").as_deref(), Some("1024"));
        assert_eq!(eval("0.1+0.2").as_deref(), Some("0.3"));
        assert_eq!(eval("-5").as_deref(), None);
        assert_eq!(eval("chrome"), None);
        assert_eq!(eval("1/0"), None);
        assert_eq!(eval("((((1+"), None);
    }
}
