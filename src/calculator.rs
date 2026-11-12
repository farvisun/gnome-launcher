use anyhow::{Result, bail};

pub fn calculate(input: &str) -> Result<Option<String>> {
    let input = input.trim().trim_start_matches('=');
    if input.len() > 512 {
        bail!("Expression is too long");
    }
    if input.is_empty() || !input.chars().any(|c| c.is_ascii_digit()) {
        return Ok(None);
    }
    if input
        .chars()
        .any(|c| !(c.is_ascii_alphanumeric() || c.is_whitespace() || ".+-*/%^(),π×÷".contains(c)))
    {
        return Ok(None);
    }
    let mut parser = Parser {
        chars: input.chars().collect(),
        pos: 0,
        depth: 0,
    };
    let value = parser.expression(0)?;
    parser.space();
    if parser.pos != parser.chars.len() {
        bail!("Complete the expression");
    }
    if !value.is_finite() {
        bail!("Result is undefined");
    }
    let result = format!("{value:.10}");
    Ok(Some(
        result
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string(),
    ))
}
struct Parser {
    chars: Vec<char>,
    pos: usize,
    depth: usize,
}
impl Parser {
    fn space(&mut self) {
        while self.chars.get(self.pos).is_some_and(|c| c.is_whitespace()) {
            self.pos += 1;
        }
    }
    fn take(&mut self, ch: char) -> bool {
        self.space();
        if self.chars.get(self.pos) == Some(&ch) {
            self.pos += 1;
            true
        } else {
            false
        }
    }
    fn expression(&mut self, min: u8) -> Result<f64> {
        self.depth += 1;
        if self.depth > 64 {
            bail!("Expression is too deeply nested");
        }
        self.space();
        let mut lhs = if self.take('-') {
            -self.expression(3)?
        } else if self.take('+') {
            self.expression(3)?
        } else if self.take('(') {
            let n = self.expression(0)?;
            if !self.take(')') {
                bail!("Missing closing parenthesis");
            }
            n
        } else if self.chars.get(self.pos).is_some_and(|c| c.is_alphabetic()) {
            let start = self.pos;
            while self.chars.get(self.pos).is_some_and(|c| c.is_alphabetic()) {
                self.pos += 1;
            }
            let name: String = self.chars[start..self.pos].iter().collect();
            if name == "pi" || name == "π" {
                std::f64::consts::PI
            } else if name == "e" {
                std::f64::consts::E
            } else {
                if !self.take('(') {
                    bail!("Expected function argument");
                }
                let n = self.expression(0)?;
                if !self.take(')') {
                    bail!("Missing closing parenthesis");
                }
                match name.as_str() {
                    "sqrt" => n.sqrt(),
                    "sin" => n.sin(),
                    "cos" => n.cos(),
                    "tan" => n.tan(),
                    "ln" => n.ln(),
                    "log" => n.log10(),
                    "abs" => n.abs(),
                    "round" => n.round(),
                    "floor" => n.floor(),
                    "ceil" => n.ceil(),
                    _ => bail!("Unknown function"),
                }
            }
        } else {
            let start = self.pos;
            while self
                .chars
                .get(self.pos)
                .is_some_and(|c| c.is_ascii_digit() || *c == '.')
            {
                self.pos += 1;
            }
            if start == self.pos {
                bail!("Enter a number");
            }
            self.chars[start..self.pos]
                .iter()
                .collect::<String>()
                .parse::<f64>()?
        };
        loop {
            self.space();
            if self.chars.get(self.pos) == Some(&'%') {
                lhs /= 100.;
                self.pos += 1;
                continue;
            }
            let op = self.chars.get(self.pos).copied().unwrap_or('\0');
            let (left, right) = match op {
                '+' | '-' => (1, 2),
                '*' | '/' | '×' | '÷' => (2, 3),
                '^' => (4, 4),
                _ => break,
            };
            if left < min {
                break;
            }
            self.pos += 1;
            let rhs = self.expression(right)?;
            lhs = match op {
                '+' => lhs + rhs,
                '-' => lhs - rhs,
                '*' | '×' => lhs * rhs,
                '/' | '÷' => lhs / rhs,
                '^' => lhs.powf(rhs),
                _ => unreachable!(),
            };
        }
        self.depth -= 1;
        Ok(lhs)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn precedence_and_functions() {
        for (expr, value) in [
            ("2+3*4", "14"),
            ("(2+3)*4", "20"),
            ("2^3^2", "512"),
            ("-2^2", "-4"),
            ("200*15%", "30"),
            ("sqrt(81)", "9"),
            ("0", "0"),
        ] {
            assert_eq!(calculate(expr).unwrap().as_deref(), Some(value), "{expr}");
        }
    }
    #[test]
    fn rejects_invalid_and_executable_input() {
        assert!(calculate("1/0").is_err());
        assert!(calculate("sqrt(-1)").is_err());
        assert!(calculate("2+").is_err());
        assert!(calculate("process.exit(1)").is_err());
        assert_eq!(calculate("Files").unwrap(), None);
    }
}
