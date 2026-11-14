//! 「随手敲出来的数学」→ LaTeX（旧版 `_lumina/lumina/math_input.py` 的 Rust 版）。
//!
//! 用户 2026-11-14 的待办 J2-11 里剩下的两块之一：**AsciiMath 风格的线性输入**。
//! 支持 `a/b`、`x^2`、`sqrt(2)`、`sum_(i=1)^n a_i`、`|x|`、`[[1,2],[3,4]]`（矩阵）、
//! `alpha` / `λ`、`<=` `->` `oo` 这些写法；输入里一旦出现反斜杠就当成已经是 LaTeX，原样返回。
//!
//! 移植时**逐条对齐 Python**（词法、运算符优先级、隐式乘法、`_join` 的空格规则都一样），
//! 免得同一个输入两边给出不同结果。

// ------------------------------------------------------------------ 符号表

const SYMBOLS: &[(&str, &str)] = &[
    ("alpha", r"\alpha"),
    ("beta", r"\beta"),
    ("gamma", r"\gamma"),
    ("delta", r"\delta"),
    ("epsilon", r"\epsilon"),
    ("varepsilon", r"\varepsilon"),
    ("zeta", r"\zeta"),
    ("eta", r"\eta"),
    ("theta", r"\theta"),
    ("vartheta", r"\vartheta"),
    ("iota", r"\iota"),
    ("kappa", r"\kappa"),
    ("lambda", r"\lambda"),
    ("mu", r"\mu"),
    ("nu", r"\nu"),
    ("xi", r"\xi"),
    ("pi", r"\pi"),
    ("rho", r"\rho"),
    ("sigma", r"\sigma"),
    ("tau", r"\tau"),
    ("upsilon", r"\upsilon"),
    ("phi", r"\phi"),
    ("varphi", r"\varphi"),
    ("chi", r"\chi"),
    ("psi", r"\psi"),
    ("omega", r"\omega"),
    ("Gamma", r"\Gamma"),
    ("Delta", r"\Delta"),
    ("Theta", r"\Theta"),
    ("Lambda", r"\Lambda"),
    ("Xi", r"\Xi"),
    ("Pi", r"\Pi"),
    ("Sigma", r"\Sigma"),
    ("Phi", r"\Phi"),
    ("Psi", r"\Psi"),
    ("Omega", r"\Omega"),
    ("oo", r"\infty"),
    ("infty", r"\infty"),
    ("le", r"\leq"),
    ("leq", r"\leq"),
    ("ge", r"\geq"),
    ("geq", r"\geq"),
    ("ne", r"\neq"),
    ("neq", r"\neq"),
    ("lt", "<"),
    ("gt", ">"),
    ("to", r"\to"),
    ("rightarrow", r"\rightarrow"),
    ("leftarrow", r"\leftarrow"),
    ("Rightarrow", r"\Rightarrow"),
    ("times", r"\times"),
    ("div", r"\div"),
    ("pm", r"\pm"),
    ("mp", r"\mp"),
    ("cdot", r"\cdot"),
    ("cdots", r"\cdots"),
    ("dots", r"\dots"),
    ("ldots", r"\ldots"),
    ("in", r"\in"),
    ("notin", r"\notin"),
    ("sub", r"\subset"),
    ("sube", r"\subseteq"),
    ("supe", r"\supseteq"),
    ("cup", r"\cup"),
    ("cap", r"\cap"),
    ("uu", r"\cup"),
    ("nn", r"\cap"),
    ("emptyset", r"\varnothing"),
    ("AA", r"\forall"),
    ("EE", r"\exists"),
    ("RR", r"\mathbb{R}"),
    ("NN", r"\mathbb{N}"),
    ("ZZ", r"\mathbb{Z}"),
    ("QQ", r"\mathbb{Q}"),
    ("CC", r"\mathbb{C}"),
    ("PP", r"\mathbb{P}"),
    ("perp", r"\perp"),
    ("parallel", r"\parallel"),
    ("angle", r"\angle"),
    ("degree", r"^{\circ}"),
    ("xx", r"\times"),
    ("cdots2", r"\cdots"),
];

/// 直接敲的 Unicode 符号（一个字符 → 命令）
const UNICODE_MAP: &[(&str, &str)] = &[
    ("α", r"\alpha"),
    ("β", r"\beta"),
    ("γ", r"\gamma"),
    ("δ", r"\delta"),
    ("ε", r"\varepsilon"),
    ("ζ", r"\zeta"),
    ("η", r"\eta"),
    ("θ", r"\theta"),
    ("ι", r"\iota"),
    ("κ", r"\kappa"),
    ("λ", r"\lambda"),
    ("μ", r"\mu"),
    ("ν", r"\nu"),
    ("ξ", r"\xi"),
    ("π", r"\pi"),
    ("ρ", r"\rho"),
    ("σ", r"\sigma"),
    ("τ", r"\tau"),
    ("υ", r"\upsilon"),
    ("φ", r"\varphi"),
    ("χ", r"\chi"),
    ("ψ", r"\psi"),
    ("ω", r"\omega"),
    ("Γ", r"\Gamma"),
    ("Δ", r"\Delta"),
    ("Θ", r"\Theta"),
    ("Λ", r"\Lambda"),
    ("Ξ", r"\Xi"),
    ("Π", r"\Pi"),
    ("Σ", r"\Sigma"),
    ("Φ", r"\Phi"),
    ("Ψ", r"\Psi"),
    ("Ω", r"\Omega"),
    ("∞", r"\infty"),
    ("≤", r"\leq"),
    ("≥", r"\geq"),
    ("≠", r"\neq"),
    ("≈", r"\approx"),
    ("→", r"\to"),
    ("←", r"\leftarrow"),
    ("⇒", r"\Rightarrow"),
    ("⇔", r"\Leftrightarrow"),
    ("±", r"\pm"),
    ("∓", r"\mp"),
    ("×", r"\times"),
    ("÷", r"\div"),
    ("·", r"\cdot"),
    ("∈", r"\in"),
    ("∉", r"\notin"),
    ("⊂", r"\subset"),
    ("⊆", r"\subseteq"),
    ("∪", r"\cup"),
    ("∩", r"\cap"),
    ("∅", r"\varnothing"),
    ("∀", r"\forall"),
    ("∃", r"\exists"),
    ("√", r"\sqrt"),
    ("∑", r"\sum"),
    ("∏", r"\prod"),
    ("∫", r"\int"),
    ("∂", r"\partial"),
    ("∇", r"\nabla"),
    ("°", r"^{\circ}"),
    ("′", "'"),
    ("⋅", r"\cdot"),
    ("∼", r"\sim"),
    ("≅", r"\cong"),
];

const COMMAND_WORDS: &[&str] = &["sqrt", "root", "abs", "norm", "frac"];

const FUNCTIONS: &[&str] = &[
    "sin", "cos", "tan", "cot", "sec", "csc", "arcsin", "arccos", "arctan", "sinh", "cosh",
    "tanh", "log", "ln", "lg", "exp", "det", "dim", "ker", "max", "min", "sup", "inf", "lim",
    "gcd", "deg", "arg",
];

const BIG_OPERATORS: &[(&str, &str)] = &[
    ("sum", r"\sum"),
    ("prod", r"\prod"),
    ("int", r"\int"),
    ("iint", r"\iint"),
    ("oint", r"\oint"),
    ("bigcup", r"\bigcup"),
    ("bigcap", r"\bigcap"),
];

const ACCENTS: &[(&str, &str)] = &[
    ("vec", r"\vec"),
    ("bar", r"\overline"),
    ("hat", r"\hat"),
    ("tilde", r"\tilde"),
    ("dot", r"\dot"),
    ("ddot", r"\ddot"),
    ("underline", r"\underline"),
];

/// 多字符运算符：**先长后短**（不然 `<=` 会被拆成 `<` + `=`）
const OPERATORS: &[&str] = &[
    "<=>", "<->", "+-", "-+", "=>", "->", "<=", ">=", "!=", "==", ":=", "**", "+", "-", "*", "/",
    "=", "<", ">", ",", "|",
];

fn symbol_of(name: &str) -> Option<&'static str> {
    SYMBOLS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

fn big_operator_of(name: &str) -> Option<&'static str> {
    BIG_OPERATORS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

fn accent_of(name: &str) -> Option<&'static str> {
    ACCENTS
        .iter()
        .find(|(key, _)| *key == name)
        .map(|(_, value)| *value)
}

fn unicode_of(ch: char) -> Option<&'static str> {
    let mut buffer = [0u8; 4];
    let text = ch.encode_utf8(&mut buffer);
    UNICODE_MAP
        .iter()
        .find(|(key, _)| *key == text)
        .map(|(_, value)| *value)
}

fn is_function(name: &str) -> bool {
    FUNCTIONS.contains(&name)
}

/// 词法里「整体识别」的名字：符号表 + 函数 + 大型运算符 + 重音 + 命令词
fn known_names() -> Vec<&'static str> {
    let mut names: Vec<&'static str> = Vec::new();
    names.extend(SYMBOLS.iter().map(|(key, _)| *key));
    names.extend(FUNCTIONS.iter().copied());
    names.extend(BIG_OPERATORS.iter().map(|(key, _)| *key));
    names.extend(ACCENTS.iter().map(|(key, _)| *key));
    names.extend(COMMAND_WORDS.iter().copied());
    names.sort_by_key(|name| std::cmp::Reverse(name.len()));
    names
}

// ------------------------------------------------------------------ 词法

fn tokenize(text: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = text.chars().collect();
    let names = known_names();
    let mut tokens: Vec<String> = Vec::new();
    let mut index = 0usize;
    while index < chars.len() {
        let ch = chars[index];
        if ch.is_whitespace() {
            index += 1;
            continue;
        }
        if "()[]{}^_".contains(ch) {
            tokens.push(ch.to_string());
            index += 1;
            continue;
        }
        // 多字符运算符
        let rest: String = chars[index..].iter().collect();
        if let Some(op) = OPERATORS.iter().find(|op| rest.starts_with(*op)) {
            tokens.push((*op).to_string());
            index += op.chars().count();
            continue;
        }
        // 数字（`\d+(\.\d+)?`）：必须以数字开头，`.5` 在 Python 那边是「看不懂」
        if ch.is_ascii_digit() {
            let mut end = index;
            let mut seen_dot = false;
            while end < chars.len() {
                let one = chars[end];
                if one.is_ascii_digit() {
                    end += 1;
                    continue;
                }
                if one == '.' && !seen_dot {
                    // 小数点后面必须还有数字，否则不算进数字
                    if end + 1 < chars.len() && chars[end + 1].is_ascii_digit() {
                        seen_dot = true;
                        end += 1;
                        continue;
                    }
                }
                break;
            }
            tokens.push(chars[index..end].iter().collect());
            index = end;
            continue;
        }
        if ch == '.' && index + 1 < chars.len() && chars[index + 1].is_ascii_digit() {
            // 与 Python 一致：`.5` 这种开头不是数字的写法直接报错
            return Err(format!("看不懂这个字符：{ch}"));
        }
        if let Some(mapped) = unicode_of(ch) {
            tokens.push(mapped.to_string());
            index += 1;
            continue;
        }
        if ch.is_alphabetic() {
            // Python 那边是 `re.match(r"[A-Za-z]+")`：ASCII 字母连着取，别的一律按**单个字符**处理
            // （所以「中文」在旧版是原样透传的，这里也照做，不然同一个输入两边给的结果不一样）。
            let mut end = index;
            while end < chars.len() && chars[end].is_ascii_alphabetic() {
                end += 1;
            }
            let word: String = if end > index {
                chars[index..end].iter().collect()
            } else {
                ch.to_string()
            };
            // 优先整体识别符号 / 函数名，否则按单字母切开（`xy` → `x y`）
            match names.iter().find(|name| word.starts_with(**name)) {
                Some(name) => {
                    tokens.push((*name).to_string());
                    index += name.len();
                }
                None => {
                    tokens.push(word.chars().next().unwrap_or_default().to_string());
                    index += 1;
                }
            }
            continue;
        }
        return Err(format!("看不懂这个字符：{ch}"));
    }
    Ok(tokens)
}

// ------------------------------------------------------------------ 语法

struct Parser {
    tokens: Vec<String>,
    pos: usize,
}

impl Parser {
    fn new(text: &str) -> Result<Self, String> {
        Ok(Parser {
            tokens: tokenize(text)?,
            pos: 0,
        })
    }

    fn peek(&self) -> Option<&str> {
        self.tokens.get(self.pos).map(|one| one.as_str())
    }

    fn next(&mut self) -> Option<String> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn expect(&mut self, token: &str) -> Result<(), String> {
        let got = self.next();
        match got {
            Some(one) if one == token => Ok(()),
            other => Err(format!(
                "缺一个 {token}，拿到的是 {}",
                other.unwrap_or_else(|| "空".to_string())
            )),
        }
    }

    fn at_end(&self) -> bool {
        self.pos >= self.tokens.len()
    }

    fn peek_in(&self, set: &[&str]) -> Option<String> {
        let token = self.peek()?;
        if set.contains(&token) {
            Some(token.to_string())
        } else {
            None
        }
    }

    fn parse(&mut self) -> Result<String, String> {
        self.expr()
    }

    fn expr(&mut self) -> Result<String, String> {
        let mut parts: Vec<String> = vec![self.term()?];
        while let Some(op) = self.peek_in(&[
            "+", "-", "+-", "-+", "=", "<", ">", "<=", ">=", "!=", "==", "->", "=>", "<=>",
        ]) {
            self.next();
            parts.push(operator_latex(&op));
            parts.push(self.term()?);
        }
        Ok(parts.join(" "))
    }

    fn term(&mut self) -> Result<String, String> {
        let mut parts: Vec<String> = vec![self.unary()?];
        loop {
            let Some(token) = self.peek().map(|one| one.to_string()) else {
                break;
            };
            if token == "*" || token == "/" || token == "**" {
                self.next();
                let right = self.unary()?;
                if token == "/" {
                    parts = vec![format!(r"\frac{{{}}}{{{}}}", join(&parts), right)];
                } else if token == "**" {
                    parts = vec![attach_script(&join(&parts), "^", &right)];
                } else {
                    parts.push(format!(r"\cdot {right}"));
                }
                continue;
            }
            if token == "|" {
                break;
            }
            if self.starts_atom(&token) {
                // 隐式乘法：`2x`、`2(x+1)`、`(a+b)(c+d)`
                parts.push(self.unary()?);
                continue;
            }
            break;
        }
        Ok(join(&parts))
    }

    fn unary(&mut self) -> Result<String, String> {
        match self.peek() {
            Some("-") => {
                self.next();
                Ok(format!("-{}", self.unary()?))
            }
            Some("+") => {
                self.next();
                self.unary()
            }
            _ => self.postfix(),
        }
    }

    fn postfix(&mut self) -> Result<String, String> {
        let mut base = self.primary()?;
        while let Some(symbol) = self.peek_in(&["^", "_"]) {
            self.next();
            let operand = self.script_operand()?;
            base = attach_script(&base, &symbol, &operand);
        }
        Ok(base)
    }

    fn script_operand(&mut self) -> Result<String, String> {
        match self.peek().map(|one| one.to_string()) {
            Some(token) if token == "{" => {
                self.expect("{")?;
                let mut parts: Vec<String> = Vec::new();
                while self.peek().is_some() && self.peek() != Some("}") {
                    parts.push(self.expr()?);
                }
                self.expect("}")?;
                Ok(join(&parts))
            }
            Some(token) if token == "(" => {
                self.expect("(")?;
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(token) if token == "-" || token == "+" => {
                self.next();
                Ok(format!("{token}{}", self.script_operand()?))
            }
            _ => self.single_atom(),
        }
    }

    fn single_atom(&mut self) -> Result<String, String> {
        let Some(token) = self.next() else {
            return Err("公式在这里断了，缺少内容".to_string());
        };
        if is_number(&token) {
            return Ok(token);
        }
        if token.starts_with('\\') {
            return Ok(token);
        }
        if let Some(value) = symbol_of(&token) {
            return Ok(value.to_string());
        }
        if let Some(value) = big_operator_of(&token) {
            return Ok(value.to_string());
        }
        if accent_of(&token).is_some() {
            return Ok(token);
        }
        if token.chars().all(|one| one.is_alphabetic()) && !token.is_empty() {
            return Ok(token);
        }
        if token == "(" {
            let inner = self.expr()?;
            self.expect(")")?;
            return Ok(format!(r"\left({inner}\right)"));
        }
        Err(format!("这里不该出现 {token}"))
    }

    fn starts_atom(&self, token: &str) -> bool {
        ![
            "+", "-", "+-", "-+", "*", "/", "=", "<", ">", "<=", ">=", "!=", "==", "->", "=>",
            "<=>", "|", ")", "]", "}", ",", "**",
        ]
        .contains(&token)
    }

    fn primary(&mut self) -> Result<String, String> {
        let Some(token) = self.peek().map(|one| one.to_string()) else {
            return Err("公式在这里断了".to_string());
        };
        match token.as_str() {
            "(" => {
                self.next();
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(format!(r"\left({inner}\right)"))
            }
            "[" => {
                // 可能是矩阵 `[[a,b],[c,d]]`，也可能只是方括号
                if self.looks_like_matrix() {
                    return self.matrix();
                }
                self.next();
                let inner = self.expr()?;
                self.expect("]")?;
                Ok(format!(r"\left[{inner}\right]"))
            }
            "{" => {
                self.next();
                let inner = self.expr()?;
                self.expect("}")?;
                Ok(inner)
            }
            "|" => {
                self.next();
                let inner = self.expr()?;
                self.expect("|")?;
                Ok(format!(r"\left|{inner}\right|"))
            }
            "sqrt" => {
                self.next();
                Ok(format!(r"\sqrt{{{}}}", self.paren_or_atom()?))
            }
            "root" => {
                self.next();
                let degree = self.paren_or_atom()?;
                Ok(format!(r"\sqrt[{degree}]{{{}}}", self.paren_or_atom()?))
            }
            "abs" => {
                self.next();
                Ok(format!(r"\left|{}\right|", self.paren_or_atom()?))
            }
            "norm" => {
                self.next();
                Ok(format!(r"\left\|{}\right\|", self.paren_or_atom()?))
            }
            _ => {
                if let Some(value) = accent_of(&token) {
                    self.next();
                    return Ok(format!("{value}{{{}}}", self.paren_or_atom()?));
                }
                if let Some(value) = big_operator_of(&token) {
                    self.next();
                    return Ok(value.to_string());
                }
                if is_function(&token) {
                    self.next();
                    return Ok(format!("\\{token} "));
                }
                if let Some(value) = symbol_of(&token) {
                    self.next();
                    return Ok(value.to_string());
                }
                if is_number(&token) {
                    self.next();
                    return Ok(token);
                }
                if token.starts_with('\\') {
                    self.next();
                    return Ok(token);
                }
                if token.chars().all(|one| one.is_alphabetic()) && !token.is_empty() {
                    self.next();
                    return Ok(token);
                }
                Err(format!("这里不该出现 {token}"))
            }
        }
    }

    fn paren_or_atom(&mut self) -> Result<String, String> {
        match self.peek().map(|one| one.to_string()) {
            Some(token) if token == "(" => {
                self.next();
                let inner = self.expr()?;
                self.expect(")")?;
                Ok(inner)
            }
            Some(token) if token == "{" => {
                self.next();
                let inner = self.expr()?;
                self.expect("}")?;
                Ok(inner)
            }
            _ => self.postfix(),
        }
    }

    fn looks_like_matrix(&self) -> bool {
        if self.peek() != Some("[") {
            return false;
        }
        let mut depth = 0i32;
        for token in self.tokens.iter().skip(self.pos) {
            match token.as_str() {
                "[" => depth += 1,
                "]" => depth -= 1,
                "," if depth >= 2 => return true,
                _ => {}
            }
        }
        false
    }

    fn matrix(&mut self) -> Result<String, String> {
        let mut rows: Vec<Vec<String>> = Vec::new();
        self.expect("[")?;
        loop {
            self.expect("[")?;
            let mut row: Vec<String> = Vec::new();
            loop {
                row.push(self.expr()?);
                if self.peek() == Some(",") {
                    self.next();
                    continue;
                }
                break;
            }
            self.expect("]")?;
            rows.push(row);
            if self.peek() == Some(",") {
                self.next();
                continue;
            }
            break;
        }
        self.expect("]")?;
        let body = rows
            .iter()
            .map(|row| row.join(" & "))
            .collect::<Vec<String>>()
            .join(r" \\ ");
        Ok(format!(r"\begin{{pmatrix}}{body}\end{{pmatrix}}"))
    }
}

fn is_number(token: &str) -> bool {
    let mut seen_dot = false;
    let mut digits = 0usize;
    for (index, ch) in token.chars().enumerate() {
        if ch.is_ascii_digit() {
            digits += 1;
            continue;
        }
        if ch == '.' && !seen_dot && index > 0 && digits > 0 {
            seen_dot = true;
            continue;
        }
        return false;
    }
    digits > 0
}

fn operator_latex(op: &str) -> String {
    match op {
        "<=" => r"\leq".to_string(),
        ">=" => r"\geq".to_string(),
        "!=" => r"\neq".to_string(),
        "==" => "=".to_string(),
        "->" => r"\to".to_string(),
        "=>" => r"\Rightarrow".to_string(),
        "<=>" => r"\Leftrightarrow".to_string(),
        "+-" => r"\pm".to_string(),
        "-+" => r"\mp".to_string(),
        other => other.to_string(),
    }
}

/// `out` 是否以 `\[A-Za-z]+` 结尾（`\lambda` 后面直接跟字母会粘成不存在的命令）
fn ends_with_command(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut index = chars.len();
    while index > 0 && chars[index - 1].is_ascii_alphabetic() {
        index -= 1;
    }
    index < chars.len() && index > 0 && chars[index - 1] == '\\'
}

fn join(parts: &[String]) -> String {
    let mut out = String::new();
    for part in parts {
        if part.is_empty() {
            continue;
        }
        if out.is_empty() {
            out = part.clone();
            continue;
        }
        let starts_alpha = part.chars().next().map(|one| one.is_ascii_alphabetic()).unwrap_or(false);
        if ends_with_command(&out) && starts_alpha {
            out.push(' ');
            out.push_str(part);
        } else {
            out.push_str(part);
        }
    }
    out
}

fn attach_script(base: &str, symbol: &str, operand: &str) -> String {
    let operand = operand.trim();
    if operand.is_empty() {
        return base.to_string();
    }
    let inner = if operand.starts_with('{')
        && operand.ends_with('}')
        && operand.matches('{').count() == 1
    {
        &operand[1..operand.len() - 1]
    } else {
        operand
    };
    format!("{base}{symbol}{{{inner}}}")
}

// ------------------------------------------------------------------ 对外接口

pub fn looks_like_latex(text: &str) -> bool {
    text.contains('\\')
}

/// 把随手写的数学转成 LaTeX；已经是 LaTeX 的原样返回
pub fn to_latex(text: &str) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(String::new());
    }
    if looks_like_latex(text) {
        return Ok(text.to_string());
    }
    let mut parser = Parser::new(text)?;
    let result = parser.parse()?;
    if !parser.at_end() {
        let rest = parser.tokens[parser.pos..].join(" ");
        return Err(format!("公式末尾没看懂：{rest}"));
    }
    Ok(result.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(text: &str) -> String {
        to_latex(text).unwrap_or_else(|exc| panic!("{text} 转换失败：{exc}"))
    }

    /// 旧版 `math_input.py` 里写的那些例子，逐条对齐
    #[test]
    fn ported_docstring_examples() {
        assert_eq!(conv("1/2"), r"\frac{1}{2}");
        assert_eq!(conv("a/b"), r"\frac{a}{b}");
        assert_eq!(conv("(a+b)/2"), r"\frac{\left(a + b\right)}{2}");
        assert_eq!(conv("x^2"), "x^{2}");
        assert_eq!(conv("x_i^2"), "x_{i}^{2}");
        assert_eq!(conv("A^T"), "A^{T}");
        assert_eq!(conv("A^(-1)"), "A^{-1}");
        assert_eq!(conv("sqrt(2)"), r"\sqrt{2}");
        assert_eq!(conv("sqrt(x+1)"), r"\sqrt{x + 1}");
        assert_eq!(conv("root(3)(8)"), r"\sqrt[3]{8}");
        assert_eq!(conv("|x|"), r"\left|x\right|");
        assert_eq!(conv("norm(x)"), r"\left\|x\right\|");
        assert_eq!(conv("abs(-2)"), r"\left|-2\right|");
        assert_eq!(conv("vec(a)"), r"\vec{a}");
        assert_eq!(conv("bar(x)"), r"\overline{x}");
    }

    /// 大型运算符 + 上下标 + 隐式乘法
    #[test]
    fn sums_and_implicit_products() {
        assert_eq!(conv("sum_(i=1)^n a_i"), r"\sum_{i = 1}^{n}a_{i}");
        assert_eq!(conv("int_0^1 f(x) dx"), r"\int_{0}^{1}f\left(x\right)dx");
        assert_eq!(conv("2x"), "2x");
        assert_eq!(conv("2(x+1)"), r"2\left(x + 1\right)");
        assert_eq!(conv("(a+b)(c+d)"), r"\left(a + b\right)\left(c + d\right)");
        assert_eq!(conv("a*b"), r"a\cdot b");
    }

    /// 矩阵、希腊字母、Unicode 直接敲
    #[test]
    fn matrix_greek_and_unicode() {
        assert_eq!(
            conv("[[1,2],[3,4]]"),
            r"\begin{pmatrix}1 & 2 \\ 3 & 4\end{pmatrix}"
        );
        assert_eq!(conv("alpha"), r"\alpha");
        assert_eq!(conv("λ^2-4λ+3=0"), r"\lambda^{2} - 4\lambda + 3 = 0");
        assert_eq!(conv("Ω"), r"\Omega");
        assert_eq!(conv("RR"), r"\mathbb{R}");
    }

    /// 符号与运算符：先长后短，`<=` 不能拆成 `<` `=`
    #[test]
    fn operators_map_to_latex() {
        assert_eq!(conv("x <= 3"), r"x \leq 3");
        assert_eq!(conv("a != b"), r"a \neq b");
        assert_eq!(conv("x -> 0"), r"x \to 0");
        assert_eq!(conv("p => q"), r"p \Rightarrow q");
        assert_eq!(conv("oo"), r"\infty");
        assert_eq!(conv("x >= 1"), r"x \geq 1");
    }

    /// 已经是 LaTeX 的原样返回；空串给空串
    #[test]
    fn latex_passthrough_and_empty() {
        assert_eq!(conv(r"\frac{1}{2}"), r"\frac{1}{2}");
        assert_eq!(conv("   "), "");
        assert!(looks_like_latex(r"\alpha"));
        assert!(!looks_like_latex("alpha"));
    }

    /// 看不懂的输入要报错（不能悄悄给出错的结果）
    #[test]
    fn unknown_input_reports_error() {
        assert!(to_latex("a +").is_err(), "断在运算符后面要报错");
        assert!(to_latex("(a+b").is_err(), "少了右括号要报错");
        assert!(to_latex(".5").is_err(), "和 Python 一样：`.5` 这种不认");
        assert!(to_latex("a_1 + a_2 + ... + a_n").is_err(), "`...` 旧版也不认");
    }

    /// 中文在旧版里是**原样透传**的（`str.isalpha()` 对汉字也是真），这里跟着一致
    #[test]
    fn chinese_passes_through_like_python() {
        assert_eq!(conv("中文"), "中文");
    }

    /// 和 Python 跑同样的输入，结果要对得上（这份期望值就是把 Python 的输出抄过来的）
    #[test]
    fn matches_python_reference_output() {
        let cases = [
            ("x_(n+1)", r"x_{n + 1}"),
            ("frac(a)(b)", r"frac\left(a\right)\left(b\right)"),
            ("lim_(x->0) (sin x)/x", r"\frac{\lim _{x \to 0}\left(\sin x\right)}{x}"),
            ("2^10", "2^{10}"),
            ("e^(-x^2)", r"e^{-x^{2}}"),
            ("1<=x<=2", r"1 \leq x \leq 2"),
            ("hat(b)", r"\hat{b}"),
        ];
        for (input, expect) in cases {
            assert_eq!(conv(input), expect, "输入 {input}");
        }
    }

    /// `\lambda` 后面紧跟字母要补空格，否则粘成不存在的命令
    #[test]
    fn command_followed_by_letter_gets_a_space() {
        assert_eq!(conv("lambda x"), r"\lambda x");
        assert_eq!(conv("alphabeta"), r"\alpha\beta");
    }
}
