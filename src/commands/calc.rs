//! The palette's calculator: a typed sum such as `12*(3+4)` or `sqrt(2)` becomes a result row.
//!
//! Pure and bounded; never evaluates anything but numbers. `ui/src/calc.ts` is the same grammar and
//! the same output rules for the HTML palette; both are checked against
//! `ui/test/fixtures/calc-cases.json`.
//!
//! Grammar: `+ - * / % ^ ( )`, unary `+` and `-`, `×` and `÷`, decimals, the constants `pi` and
//! `e`, and the functions `sqrt abs round floor ceil sin cos tan ln log exp`. `^` is right
//! associative and binds tighter than unary minus (`-2^2` is `-4`); `%` is the remainder. Text only
//! counts as a calculation when it holds at least one operator or function call, so `42` is not one.

const MAX_LEN: usize = 120;
const MAX_DEPTH: u32 = 24;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Num(f64),
    Ident(String),
    Op(char),
    Open,
    Close,
}

fn tokenize(text: &str) -> Option<Vec<Token>> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut at = 0;
    while at < chars.len() {
        let c = chars[at];
        match c {
            ' ' | '\t' => at += 1,
            '0'..='9' | '.' => {
                let start = at;
                let mut dots = 0;
                while at < chars.len() && (chars[at].is_ascii_digit() || chars[at] == '.') {
                    if chars[at] == '.' {
                        dots += 1;
                    }
                    at += 1;
                }
                let literal: String = chars[start..at].iter().collect();
                if dots > 1 || literal == "." {
                    return None;
                }
                tokens.push(Token::Num(literal.parse().ok()?));
            }
            'a'..='z' | 'A'..='Z' => {
                let start = at;
                while at < chars.len() && chars[at].is_ascii_alphabetic() {
                    at += 1;
                }
                tokens.push(Token::Ident(chars[start..at].iter().collect::<String>().to_ascii_lowercase()));
            }
            '+' | '-' | '*' | '/' | '%' | '^' => {
                tokens.push(Token::Op(c));
                at += 1;
            }
            '×' => {
                tokens.push(Token::Op('*'));
                at += 1;
            }
            '÷' => {
                tokens.push(Token::Op('/'));
                at += 1;
            }
            '(' => {
                tokens.push(Token::Open);
                at += 1;
            }
            ')' => {
                tokens.push(Token::Close);
                at += 1;
            }
            _ => return None,
        }
    }
    Some(tokens)
}

struct Parser {
    tokens: Vec<Token>,
    at: usize,
    /// Whether an operator or a function call was seen: a bare number is not a calculation.
    active: bool,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    fn expr(&mut self, depth: u32) -> Option<f64> {
        if depth > MAX_DEPTH {
            return None;
        }
        let mut value = self.term(depth)?;
        while let Some(Token::Op(op @ ('+' | '-'))) = self.peek().cloned() {
            self.at += 1;
            self.active = true;
            let right = self.term(depth)?;
            value = if op == '+' { value + right } else { value - right };
        }
        Some(value)
    }

    fn term(&mut self, depth: u32) -> Option<f64> {
        let mut value = self.unary(depth)?;
        while let Some(Token::Op(op @ ('*' | '/' | '%'))) = self.peek().cloned() {
            self.at += 1;
            self.active = true;
            let right = self.unary(depth)?;
            value = match op {
                '*' => value * right,
                '/' => value / right,
                _ => value % right,
            };
        }
        Some(value)
    }

    fn unary(&mut self, depth: u32) -> Option<f64> {
        if depth > MAX_DEPTH {
            return None;
        }
        match self.peek() {
            Some(Token::Op('-')) => {
                self.at += 1;
                self.active = true;
                Some(-self.unary(depth + 1)?)
            }
            Some(Token::Op('+')) => {
                self.at += 1;
                self.active = true;
                self.unary(depth + 1)
            }
            _ => self.power(depth),
        }
    }

    fn power(&mut self, depth: u32) -> Option<f64> {
        let base = self.atom(depth)?;
        if let Some(Token::Op('^')) = self.peek() {
            self.at += 1;
            self.active = true;
            let exponent = self.unary(depth + 1)?;
            return Some(base.powf(exponent));
        }
        Some(base)
    }

    fn atom(&mut self, depth: u32) -> Option<f64> {
        let token = self.peek()?.clone();
        self.at += 1;
        match token {
            Token::Num(value) => Some(value),
            Token::Open => {
                let value = self.expr(depth + 1)?;
                matches!(self.peek(), Some(Token::Close)).then_some(())?;
                self.at += 1;
                Some(value)
            }
            Token::Ident(name) => {
                if matches!(self.peek(), Some(Token::Open)) {
                    self.at += 1;
                    let argument = self.expr(depth + 1)?;
                    matches!(self.peek(), Some(Token::Close)).then_some(())?;
                    self.at += 1;
                    self.active = true;
                    return match name.as_str() {
                        "sqrt" => Some(argument.sqrt()),
                        "abs" => Some(argument.abs()),
                        "round" => Some(argument.round()),
                        "floor" => Some(argument.floor()),
                        "ceil" => Some(argument.ceil()),
                        "sin" => Some(argument.sin()),
                        "cos" => Some(argument.cos()),
                        "tan" => Some(argument.tan()),
                        "ln" => Some(argument.ln()),
                        "log" => Some(argument.log10()),
                        "exp" => Some(argument.exp()),
                        _ => None,
                    };
                }
                match name.as_str() {
                    "pi" => Some(std::f64::consts::PI),
                    "e" => Some(std::f64::consts::E),
                    _ => None,
                }
            }
            _ => None,
        }
    }
}

/// The value of `text` when it is a calculation, else `None`.
pub fn evaluate(text: &str) -> Option<f64> {
    let text = text.trim();
    if text.is_empty() || text.chars().count() > MAX_LEN {
        return None;
    }
    let mut parser = Parser { tokens: tokenize(text)?, at: 0, active: false };
    let value = parser.expr(0)?;
    (parser.at == parser.tokens.len() && parser.active).then_some(value)
}

/// `text` evaluated and written the way the palette shows it: twelve significant digits, no trailing
/// zeros, no exponent. `None` for anything that is not a calculation or whose result is not finite,
/// is 1e15 or larger, or is a non-zero value below 1e-6.
pub fn result_text(text: &str) -> Option<String> {
    let value = evaluate(text)?;
    if !value.is_finite() || value.abs() >= 1e15 {
        return None;
    }
    let rounded: f64 = format!("{value:.11e}").parse().ok()?;
    if rounded != 0.0 && rounded.abs() < 1e-6 {
        return None;
    }
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    Some(format!("{rounded}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_cases_give_the_documented_results() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("ui/test/fixtures/calc-cases.json");
        let text = std::fs::read_to_string(path).expect("calc fixture");
        // One case per line: ["expr", "result"] or ["expr", null]; no JSON library needed.
        let mut checked = 0;
        for line in text.lines().map(str::trim).filter(|l| l.starts_with("[\"")) {
            let line = line.trim_end_matches(',');
            let (expr, rest) = line[2..].split_once("\", ").expect("case shape");
            let expected = if rest.starts_with("null") { None } else { Some(rest.trim_start_matches('"').trim_end_matches(']').trim_end_matches('"')) };
            let expr = expr.replace("\\\\", "\\");
            assert_eq!(result_text(&expr).as_deref(), expected, "{expr}");
            checked += 1;
        }
        assert!(checked >= 30, "{checked}");
    }

    #[test]
    fn precedence_associativity_and_unary_minus() {
        assert_eq!(result_text("2+3*4").as_deref(), Some("14"));
        assert_eq!(result_text("2^3^2").as_deref(), Some("512"));
        assert_eq!(result_text("-2^2").as_deref(), Some("-4"));
        assert_eq!(result_text("(1+2)*3").as_deref(), Some("9"));
        assert_eq!(result_text("10 % 4").as_deref(), Some("2"));
        assert_eq!(result_text("0.1+0.2").as_deref(), Some("0.3"));
    }

    #[test]
    fn a_bare_number_or_word_is_not_a_calculation() {
        for text in ["42", " 7 ", "pi", "(3)", "hello", "", "  "] {
            assert_eq!(result_text(text), None, "{text:?}");
        }
    }

    #[test]
    fn bad_input_and_runaway_results_are_refused() {
        for text in ["1/0", "0/0", "2^10000", "sqrt(-1)", "1+", "(1+2", "1+2)", "1..2", "foo(2)", "2 $ 3", "1e5+1", &"(".repeat(80), &"1+".repeat(100)] {
            assert_eq!(result_text(text), None, "{text:?}");
        }
    }
}
