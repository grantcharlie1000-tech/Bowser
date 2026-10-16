//! The address bar's local calculator: a small, pure expression evaluator behind the drop-down's
//! "calculator" suggestion. Nothing here touches the network or the disk.
//!
//! Supported: `+ - * / % ^` (also `**`, `x`, `×`, `÷`), unary signs, parentheses, `2(3+4)` implicit
//! multiplication, `pi`, `e`, the functions in [`function`], and a percent suffix (`200*10%`).
//! Anything else (words, addresses, dates, a bare number) is not a calculation.

const MAX_CHARS: usize = 256;
const MAX_DEPTH: usize = 64;

/// The formatted result when `input` is a calculation, else `None`.
///
/// A calculation holds at least one operator, function or percent sign: `42` and `pi` are not
/// offered, and neither are addresses (`10.0.0.1`) or dates (`2024-10-16`). Division by zero and
/// any other non-finite result also give `None`.
pub fn evaluate(input: &str) -> Option<String> {
    let trimmed = input.trim().trim_end_matches('=').trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_CHARS || looks_like_date(trimmed) {
        return None;
    }
    let tokens = tokenize(trimmed)?;
    let interesting = tokens.iter().any(|token| {
        matches!(token, Token::Op(_) | Token::Func(_) | Token::Percent)
    });
    if !interesting {
        return None;
    }
    let mut parser = Parser { tokens: &tokens, at: 0, depth: 0 };
    let value = parser.expression()?;
    if parser.at != tokens.len() || !value.is_finite() {
        return None;
    }
    Some(format(value))
}

/// `2024-10-16`, `16/10/2024`, `1.2.2024`: three short numbers split by one repeated separator.
fn looks_like_date(text: &str) -> bool {
    for separator in ['-', '/', '.'] {
        let parts: Vec<&str> = text.split(separator).collect();
        if parts.len() == 3
            && parts.iter().all(|part| (1..=4).contains(&part.len()) && part.bytes().all(|b| b.is_ascii_digit()))
        {
            return true;
        }
    }
    false
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Token {
    Num(f64),
    Op(char),
    Open,
    Close,
    Func(&'static str),
    Percent,
}

fn function(name: &str) -> Option<&'static str> {
    ["sqrt", "abs", "sin", "cos", "tan", "ln", "log", "floor", "ceil", "round", "exp"].into_iter().find(|&known| known == name)
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let ch = chars[at];
        match ch {
            c if c.is_whitespace() => at += 1,
            '0'..='9' | '.' => {
                let start = at;
                let mut dots = 0;
                while at < chars.len() && (chars[at].is_ascii_digit() || chars[at] == '.' || chars[at] == '_') {
                    if chars[at] == '.' {
                        dots += 1;
                    }
                    at += 1;
                }
                if dots > 1 {
                    return None;
                }
                let digits: String = chars[start..at].iter().filter(|c| **c != '_').collect();
                let value: f64 = digits.parse().ok()?;
                // `2(3+4)`: a number straight before `(` multiplies.
                tokens.push(Token::Num(value));
                if chars.get(at) == Some(&'(') {
                    tokens.push(Token::Op('*'));
                }
            }
            '+' | '-' | '/' | '÷' | '×' | '^' | '%' => {
                // `%` after a number or `)` with nothing numeric after it is a percent suffix;
                // otherwise it is the modulo operator.
                if ch == '%' && percent_suffix(&tokens, &chars, at) {
                    tokens.push(Token::Percent);
                } else {
                    tokens.push(Token::Op(match ch {
                        '÷' => '/',
                        '×' => '*',
                        other => other,
                    }));
                }
                at += 1;
            }
            '*' => {
                if chars.get(at + 1) == Some(&'*') {
                    tokens.push(Token::Op('^'));
                    at += 2;
                } else {
                    tokens.push(Token::Op('*'));
                    at += 1;
                }
            }
            '(' => {
                tokens.push(Token::Open);
                at += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                at += 1;
            }
            c if c.is_alphabetic() => {
                let start = at;
                while at < chars.len() && chars[at].is_alphabetic() {
                    at += 1;
                }
                let word: String = chars[start..at].iter().collect::<String>().to_lowercase();
                match word.as_str() {
                    "pi" => tokens.push(Token::Num(std::f64::consts::PI)),
                    "e" => tokens.push(Token::Num(std::f64::consts::E)),
                    "x" => {
                        // Multiplication only between two operands: `3x4`.
                        if !matches!(tokens.last(), Some(Token::Num(_) | Token::Close))
                            || !chars.get(at).is_some_and(|c| c.is_ascii_digit() || *c == '.' || *c == '(' || c.is_whitespace())
                        {
                            return None;
                        }
                        tokens.push(Token::Op('*'));
                    }
                    other => tokens.push(Token::Func(function(other)?)),
                }
            }
            _ => return None,
        }
    }
    Some(tokens)
}

fn percent_suffix(tokens: &[Token], chars: &[char], at: usize) -> bool {
    if !matches!(tokens.last(), Some(Token::Num(_) | Token::Close | Token::Percent)) {
        return false;
    }
    // Skip spaces; a following operand means modulo (`10 % 3`), anything else a suffix (`50%`).
    let next = chars[at + 1..].iter().find(|c| !c.is_whitespace());
    !next.is_some_and(|c| c.is_ascii_digit() || *c == '.' || *c == '(' || c.is_alphabetic() || matches!(c, '-' | '+'))
}

struct Parser<'a> {
    tokens: &'a [Token],
    at: usize,
    depth: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<Token> {
        self.tokens.get(self.at).copied()
    }

    fn enter(&mut self) -> Option<()> {
        self.depth += 1;
        (self.depth <= MAX_DEPTH).then_some(())
    }

    fn expression(&mut self) -> Option<f64> {
        self.enter()?;
        let mut left = self.term()?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek() {
            self.at += 1;
            let right = self.term()?;
            left = if op == '+' { left + right } else { left - right };
        }
        self.depth -= 1;
        Some(left)
    }

    fn term(&mut self) -> Option<f64> {
        let mut left = self.unary()?;
        while let Some(Token::Op(op @ ('*' | '/' | '%'))) = self.peek() {
            self.at += 1;
            let right = self.unary()?;
            left = match op {
                '*' => left * right,
                '/' => left / right,
                _ => left % right,
            };
        }
        Some(left)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.peek() {
            Some(Token::Op('-')) => {
                self.at += 1;
                self.enter()?;
                let value = self.unary()?;
                self.depth -= 1;
                Some(-value)
            }
            Some(Token::Op('+')) => {
                self.at += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.postfix()?;
        if let Some(Token::Op('^')) = self.peek() {
            self.at += 1;
            self.enter()?;
            let exponent = self.unary()?; // right associative, and `2^-1` works
            self.depth -= 1;
            return Some(base.powf(exponent));
        }
        Some(base)
    }

    fn postfix(&mut self) -> Option<f64> {
        let mut value = self.atom()?;
        while self.peek() == Some(Token::Percent) {
            self.at += 1;
            value /= 100.0;
        }
        Some(value)
    }

    fn atom(&mut self) -> Option<f64> {
        match self.peek()? {
            Token::Num(value) => {
                self.at += 1;
                Some(value)
            }
            Token::Open => {
                self.at += 1;
                let value = self.expression()?;
                (self.peek() == Some(Token::Close)).then_some(())?;
                self.at += 1;
                Some(value)
            }
            Token::Func(name) => {
                self.at += 1;
                (self.peek() == Some(Token::Open)).then_some(())?;
                self.at += 1;
                let argument = self.expression()?;
                (self.peek() == Some(Token::Close)).then_some(())?;
                self.at += 1;
                apply(name, argument)
            }
            _ => None,
        }
    }
}

fn apply(name: &str, x: f64) -> Option<f64> {
    let value = match name {
        "sqrt" => x.sqrt(),
        "abs" => x.abs(),
        "sin" => x.sin(),
        "cos" => x.cos(),
        "tan" => x.tan(),
        "ln" => x.ln(),
        "log" => x.log10(),
        "floor" => x.floor(),
        "ceil" => x.ceil(),
        "round" => x.round(),
        "exp" => x.exp(),
        _ => return None,
    };
    value.is_finite().then_some(value)
}

fn format(value: f64) -> String {
    let value = if value == 0.0 { 0.0 } else { value };
    if value.fract() == 0.0 && value.abs() < 1e15 {
        return format!("{value:.0}");
    }
    let magnitude = value.abs();
    if !(1e-5..1e15).contains(&magnitude) {
        let text = format!("{value:.4e}");
        // `1.2345e20`: trim zeros of the mantissa.
        if let Some((mantissa, exponent)) = text.split_once('e') {
            let mantissa = if mantissa.contains('.') { mantissa.trim_end_matches('0').trim_end_matches('.') } else { mantissa };
            return format!("{mantissa}e{exponent}");
        }
        return text;
    }
    // Ten significant digits, then trailing zeros trimmed.
    let digits_before = if magnitude >= 1.0 { magnitude.log10().floor() as i32 + 1 } else { 0 };
    let decimals = (10 - digits_before).clamp(0, 15) as usize;
    let text = format!("{value:.decimals$}");
    if text.contains('.') { text.trim_end_matches('0').trim_end_matches('.').to_owned() } else { text }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calc(text: &str) -> Option<String> {
        evaluate(text)
    }

    #[test]
    fn basic_arithmetic_and_precedence() {
        assert_eq!(calc("2+2").as_deref(), Some("4"));
        assert_eq!(calc("2+3*4").as_deref(), Some("14"));
        assert_eq!(calc("(2+3)*4").as_deref(), Some("20"));
        assert_eq!(calc("10/4").as_deref(), Some("2.5"));
        assert_eq!(calc("10 % 3").as_deref(), Some("1"));
        assert_eq!(calc("2^3^2").as_deref(), Some("512"), "power is right associative");
        assert_eq!(calc("2**10").as_deref(), Some("1024"));
        assert_eq!(calc("-3+5").as_deref(), Some("2"));
        assert_eq!(calc("2^-1").as_deref(), Some("0.5"));
        assert_eq!(calc("1/3").as_deref(), Some("0.3333333333"));
        assert_eq!(calc("1_000*3").as_deref(), Some("3000"));
    }

    #[test]
    fn alternative_operators_functions_and_constants() {
        assert_eq!(calc("6×7").as_deref(), Some("42"));
        assert_eq!(calc("84÷2").as_deref(), Some("42"));
        assert_eq!(calc("3x4").as_deref(), Some("12"));
        assert_eq!(calc("3 x 4").as_deref(), Some("12"));
        assert_eq!(calc("2(3+4)").as_deref(), Some("14"));
        assert_eq!(calc("sqrt(16)").as_deref(), Some("4"));
        assert_eq!(calc("abs(-7.5)").as_deref(), Some("7.5"));
        assert_eq!(calc("round(2.6)+floor(1.9)+ceil(0.1)").as_deref(), Some("5"));
        assert_eq!(calc("2*pi").as_deref(), Some("6.283185307"));
        assert_eq!(calc("log(1000)").as_deref(), Some("3"));
        assert_eq!(calc("ln(e)").as_deref(), Some("1"));
        assert_eq!(calc("2+2=").as_deref(), Some("4"), "a trailing = is ignored");
    }

    #[test]
    fn percent_suffix_and_modulo() {
        assert_eq!(calc("200*10%").as_deref(), Some("20"));
        assert_eq!(calc("50%").as_deref(), Some("0.5"));
        assert_eq!(calc("17 % 5").as_deref(), Some("2"));
    }

    #[test]
    fn things_that_are_not_calculations() {
        for text in [
            "", "   ", "42", "3.5", "pi", "e", "rust 2024", "example.com", "1.5.2", "10.0.0.1", "2024-10-16",
            "16/10/2024", "hello world", "2+", "(2+3", "2+3)", "sqrt", "sqrt(", "foo(2)", "1/0", "0/0", "sqrt(-1)",
            "x4", "3x", "2 +* 3", "ln(0)", "javascript:alert(1)", "https://a.example/?q=1+1",
        ] {
            assert_eq!(calc(text), None, "{text:?}");
        }
        assert_eq!(calc(&"1+".repeat(200)), None, "longer than 256 characters");
    }

    #[test]
    fn deep_nesting_is_refused_not_overflowed() {
        let deep = format!("{}1+1{}", "(".repeat(100), ")".repeat(100));
        assert_eq!(calc(&deep), None);
        let ok = format!("{}1+1{}", "(".repeat(20), ")".repeat(20));
        assert_eq!(calc(&ok).as_deref(), Some("2"));
        assert_eq!(calc(&format!("{}1", "-".repeat(100))), None);
    }

    #[test]
    fn output_formatting() {
        assert_eq!(calc("0.1+0.2").as_deref(), Some("0.3"));
        assert_eq!(calc("-0*5").as_deref(), Some("0"));
        assert_eq!(calc("10^20").as_deref(), Some("1e20"));
        assert_eq!(calc("12345*10^17").as_deref(), Some("1.2345e21"));
        assert_eq!(calc("1/1000000").as_deref(), Some("1e-6"));
        assert_eq!(calc("123456789*1000").as_deref(), Some("123456789000"));
        assert_eq!(calc("1000000/7").as_deref(), Some("142857.1429"));
    }

    #[test]
    fn arbitrary_text_never_panics() {
        let alphabet: Vec<char> = "0123456789.+-*/%^()=_ xXeE×÷πabsqrtlogpi,;:\\\"'#@!?<>".chars().collect();
        let mut state = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..3000 {
            let mut text = String::new();
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let length = (state >> 59) as usize + 1;
            for _ in 0..length {
                state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                text.push(alphabet[(state >> 33) as usize % alphabet.len()]);
            }
            let _ = evaluate(&text);
        }
    }
}
