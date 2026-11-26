//! Search-query language for filtering cars and tracks.
//!
//! - `a b` and `a & b` are AND. `a | b` and `a, b` are OR. AND binds tighter
//!   than OR, so `a b | c` means `(a AND b) OR c`.
//! - `!a` negates a term. Repeated `!` toggle the negation. A `!` in the middle
//!   of a word is literal.
//! - `( ... )` groups. Nesting is limited to [`MAX_DEPTH`] levels.
//! - `field:value` is a case-insensitive substring match on one field.
//! - `field:` with nothing after the colon matches records that have the field,
//!   whatever its value.
//! - `field<value`, `field<=value`, `field>value`, `field>=value` and
//!   `field=value` compare the field numerically. `=>` and `=<` are accepted
//!   as aliases of `>=` and `<=`.
//! - `*` matches any run of characters and `?` matches exactly one character.
//! - `"phrase"` matches as a substring. `'text'` must equal the whole value.
//! - `\x` makes `x` a literal character.
//!
//! A plain word with no field matches when it occurs in any of the record's
//! text fields. Empty or whitespace-only input matches every record.
//!
//! [`sort_by_field`] orders records by one field, for use with a parsed query's results.

use std::cmp::Ordering;
use std::error::Error;
use std::fmt;

/// Maximum number of nested parenthesis groups accepted by [`parse`].
pub const MAX_DEPTH: usize = 64;

/// Something that can be searched.
///
/// Field names passed to [`Record::field`] are always lowercase, because the
/// query lowercases field names before matching.
pub trait Record {
    /// Values searched by plain words that have no field name.
    fn text_fields(&self) -> Vec<String>;
    /// Value of the named field, or `None` if the record has no such field.
    fn field(&self, name: &str) -> Option<String>;
}

/// A parsed search query.
#[derive(Debug, Clone, PartialEq)]
pub enum Query {
    /// Empty input: matches every record.
    All,
    /// Text match. `field: None` searches every text field.
    Text {
        field: Option<String>,
        pattern: Vec<PatternChar>,
        /// True for `'...'`: the whole value must equal the text, ignoring case.
        exact: bool,
    },
    /// Numeric comparison, e.g. `year>2000`.
    Compare {
        field: String,
        op: CompareOp,
        value: f64,
    },
    /// Field existence check, written `field:` with no value.
    Exists { field: String },
    Not(Box<Query>),
    And(Vec<Query>),
    Or(Vec<Query>),
}

/// Numeric comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareOp {
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Equal,
}

/// One element of a text pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PatternChar {
    /// A literal character, stored lowercase.
    Literal(char),
    /// `*`: any run of characters, including none.
    AnyRun,
    /// `?`: exactly one character.
    AnyOne,
}

/// A query that could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryError {
    pub message: String,
    /// 0-based character offset of the problem in the input.
    pub position: usize,
}

impl QueryError {
    fn new(message: impl Into<String>, position: usize) -> Self {
        Self {
            message: message.into(),
            position,
        }
    }
}

impl fmt::Display for QueryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (column {})", self.message, self.position + 1)
    }
}

impl Error for QueryError {}

/// Parses `text` into a [`Query`]. Empty or whitespace-only input gives [`Query::All`].
pub fn parse(text: &str) -> Result<Query, QueryError> {
    let tokens = tokenize(text)?;
    if tokens.is_empty() {
        return Ok(Query::All);
    }

    let mut parser = Parser {
        tokens,
        index: 0,
        depth: 0,
        end: text.chars().count(),
    };
    let query = parser.parse_or()?;

    match parser.tokens.get(parser.index) {
        None => Ok(query),
        Some((Token::Close, pos)) => Err(QueryError::new(
            "unexpected ')' without a matching '('",
            *pos,
        )),
        Some((_, pos)) => Err(QueryError::new("unexpected token", *pos)),
    }
}

/// True if `record` satisfies `query`.
pub fn matches(query: &Query, record: &impl Record) -> bool {
    match query {
        Query::All => true,
        Query::Text {
            field,
            pattern,
            exact,
        } => match field {
            Some(name) => record
                .field(name)
                .is_some_and(|value| text_matches(pattern, *exact, &value)),
            None => record
                .text_fields()
                .iter()
                .any(|value| text_matches(pattern, *exact, value)),
        },
        Query::Compare { field, op, value } => record
            .field(field)
            .and_then(|text| first_number(&text))
            .is_some_and(|actual| compare(*op, actual, *value)),
        Query::Exists { field } => record.field(field).is_some(),
        Query::Not(inner) => !matches(inner, record),
        Query::And(items) => items.iter().all(|item| matches(item, record)),
        Query::Or(items) => items.iter().any(|item| matches(item, record)),
    }
}

/// Returns the items that satisfy `query`, in their original order.
pub fn filter<'a, T: Record>(items: &'a [T], query: &Query) -> Vec<&'a T> {
    items.iter().filter(|item| matches(query, *item)).collect()
}

/// Sorts `items` by the named field. The sort is stable, so records with equal
/// keys keep their order.
///
/// A value that is a plain number sorts by its number. Any other value sorts as
/// text, ignoring case. Ascending, numbers come before text. `descending` reverses
/// that order. Records without the field always go last, in both directions.
pub fn sort_by_field<T: Record>(items: &mut [T], field: &str, descending: bool) {
    let field = field.to_ascii_lowercase();
    items.sort_by(|a, b| match (sort_key(a, &field), sort_key(b, &field)) {
        (Some(x), Some(y)) => {
            let order = compare_keys(&x, &y);
            if descending { order.reverse() } else { order }
        }
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    });
}

/// A sortable form of a field value.
#[derive(Debug, Clone, PartialEq)]
enum SortKey {
    Number(f64),
    Text(String),
}

fn sort_key(record: &impl Record, field: &str) -> Option<SortKey> {
    let value = record.field(field)?;
    let trimmed = value.trim();
    Some(match trimmed.parse::<f64>() {
        Ok(number) if number.is_finite() => SortKey::Number(number),
        _ => SortKey::Text(trimmed.chars().map(fold).collect()),
    })
}

fn compare_keys(a: &SortKey, b: &SortKey) -> Ordering {
    match (a, b) {
        (SortKey::Number(x), SortKey::Number(y)) => x.total_cmp(y),
        (SortKey::Number(_), SortKey::Text(_)) => Ordering::Less,
        (SortKey::Text(_), SortKey::Number(_)) => Ordering::Greater,
        (SortKey::Text(x), SortKey::Text(y)) => x.cmp(y),
    }
}

// ---------------------------------------------------------------------------
// Tokenizer
// ---------------------------------------------------------------------------

/// How a character was written in the input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    /// Unquoted and unescaped: may be an operator, `*` or `?`.
    Bare,
    /// Escaped with a backslash: always literal.
    Escaped,
    /// Inside quotes: always literal. `single` marks `'...'`.
    Quoted { single: bool },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Piece {
    ch: char,
    kind: Kind,
}

#[derive(Debug, Clone)]
enum Token {
    Open,
    Close,
    And,
    Or,
    Not,
    /// A word, possibly with a field prefix.
    Term(Vec<Piece>),
}

fn is_term_break(c: char) -> bool {
    c.is_whitespace() || matches!(c, '(' | ')' | '&' | '|' | ',')
}

/// Lowercase form of a character, used for case-insensitive comparison.
fn fold(c: char) -> char {
    c.to_lowercase().next().unwrap_or(c)
}

/// Splits the input into tokens, each paired with its character offset.
fn tokenize(text: &str) -> Result<Vec<(Token, usize)>, QueryError> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        let single = match c {
            '(' => Some(Token::Open),
            ')' => Some(Token::Close),
            '&' => Some(Token::And),
            '|' | ',' => Some(Token::Or),
            '!' => Some(Token::Not),
            _ => None,
        };
        if let Some(token) = single {
            tokens.push((token, i));
            i += 1;
            continue;
        }

        let start = i;
        let mut pieces = Vec::new();
        while i < chars.len() && !is_term_break(chars[i]) {
            let c = chars[i];
            if c == '\\' {
                let (ch, step) = match chars.get(i + 1) {
                    Some(&next) => (next, 2),
                    None => ('\\', 1),
                };
                pieces.push(Piece {
                    ch,
                    kind: Kind::Escaped,
                });
                i += step;
            } else if c == '"' || c == '\'' {
                let quote_at = i;
                let single = c == '\'';
                i += 1;
                loop {
                    match chars.get(i).copied() {
                        None => return Err(QueryError::new("unterminated quote", quote_at)),
                        Some(d) if d == c => {
                            i += 1;
                            break;
                        }
                        Some('\\') => {
                            let (ch, step) = match chars.get(i + 1) {
                                Some(&next) => (next, 2),
                                None => ('\\', 1),
                            };
                            pieces.push(Piece {
                                ch,
                                kind: Kind::Quoted { single },
                            });
                            i += step;
                        }
                        Some(d) => {
                            pieces.push(Piece {
                                ch: d,
                                kind: Kind::Quoted { single },
                            });
                            i += 1;
                        }
                    }
                }
            } else {
                pieces.push(Piece {
                    ch: c,
                    kind: Kind::Bare,
                });
                i += 1;
            }
        }
        tokens.push((Token::Term(pieces), start));
    }

    Ok(tokens)
}

// ---------------------------------------------------------------------------
// Parser
// ---------------------------------------------------------------------------

struct Parser {
    tokens: Vec<(Token, usize)>,
    index: usize,
    /// Current parenthesis nesting depth.
    depth: usize,
    /// Character length of the input, for end-of-input errors.
    end: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.index).map(|(token, _)| token)
    }

    /// `or := and ( ('|' | ',') and )*`
    fn parse_or(&mut self) -> Result<Query, QueryError> {
        let mut items = vec![self.parse_and()?];
        while matches!(self.peek(), Some(Token::Or)) {
            self.index += 1;
            items.push(self.parse_and()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Query::Or(items)
        })
    }

    /// `and := unary ( '&' unary | unary )*`. Juxtaposed terms are an implicit AND.
    fn parse_and(&mut self) -> Result<Query, QueryError> {
        let mut items = vec![self.parse_unary()?];
        loop {
            if matches!(self.peek(), Some(Token::And)) {
                self.index += 1;
            } else if !matches!(self.peek(), Some(Token::Open | Token::Not | Token::Term(_))) {
                break;
            }
            items.push(self.parse_unary()?);
        }
        Ok(if items.len() == 1 {
            items.remove(0)
        } else {
            Query::And(items)
        })
    }

    /// `unary := '!'* primary`. An even number of `!` cancels out.
    fn parse_unary(&mut self) -> Result<Query, QueryError> {
        let mut negated = false;
        while matches!(self.peek(), Some(Token::Not)) {
            negated = !negated;
            self.index += 1;
        }
        let query = self.parse_primary()?;
        Ok(if negated {
            Query::Not(Box::new(query))
        } else {
            query
        })
    }

    /// `primary := '(' or ')' | term`
    fn parse_primary(&mut self) -> Result<Query, QueryError> {
        let Some((token, pos)) = self.tokens.get(self.index).cloned() else {
            return Err(QueryError::new(
                "expected a term but reached the end of the query",
                self.end,
            ));
        };

        match token {
            Token::Term(pieces) => {
                self.index += 1;
                build_term(&pieces, pos)
            }
            Token::Open => {
                if self.depth >= MAX_DEPTH {
                    return Err(QueryError::new(
                        format!("parentheses nested more than {MAX_DEPTH} levels deep"),
                        pos,
                    ));
                }
                self.index += 1;
                let missing = || QueryError::new("missing ')' to close '('", pos);
                match self.peek() {
                    None => return Err(missing()),
                    Some(Token::Close) => {
                        return Err(QueryError::new("empty parentheses", pos));
                    }
                    Some(_) => {}
                }

                self.depth += 1;
                let inner = self.parse_or()?;
                self.depth -= 1;

                if matches!(self.peek(), Some(Token::Close)) {
                    self.index += 1;
                    Ok(inner)
                } else {
                    Err(missing())
                }
            }
            Token::Close => Err(QueryError::new(
                "unexpected ')' without a matching '('",
                pos,
            )),
            Token::And => Err(QueryError::new("expected a term but found '&'", pos)),
            Token::Or => Err(QueryError::new("expected a term but found '|'", pos)),
            Token::Not => Err(QueryError::new("expected a term but found '!'", pos)),
        }
    }
}

/// Operator that can follow a field name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FieldOp {
    /// `:` substring match.
    Text,
    /// Numeric comparison.
    Compare(CompareOp),
}

/// Finds a field operator starting at `at`. Returns it with its length in pieces.
fn operator_at(pieces: &[Piece], at: usize) -> Option<(FieldOp, usize)> {
    let is = |k: usize, want: char| {
        pieces
            .get(k)
            .is_some_and(|p| p.kind == Kind::Bare && p.ch == want)
    };

    if is(at, ':') {
        return Some((FieldOp::Text, 1));
    }
    if is(at, '<') {
        return Some(if is(at + 1, '=') {
            (FieldOp::Compare(CompareOp::LessEqual), 2)
        } else {
            (FieldOp::Compare(CompareOp::Less), 1)
        });
    }
    if is(at, '>') {
        return Some(if is(at + 1, '=') {
            (FieldOp::Compare(CompareOp::GreaterEqual), 2)
        } else {
            (FieldOp::Compare(CompareOp::Greater), 1)
        });
    }
    if is(at, '=') {
        if is(at + 1, '<') {
            return Some((FieldOp::Compare(CompareOp::LessEqual), 2));
        }
        if is(at + 1, '>') {
            return Some((FieldOp::Compare(CompareOp::GreaterEqual), 2));
        }
        return Some((FieldOp::Compare(CompareOp::Equal), 1));
    }
    None
}

/// Builds a query from one word. A word is `field<op>value` when it starts
/// with ASCII letters followed by an operator; otherwise it is plain text.
fn build_term(pieces: &[Piece], pos: usize) -> Result<Query, QueryError> {
    let letters = pieces
        .iter()
        .take_while(|p| p.kind == Kind::Bare && p.ch.is_ascii_alphabetic())
        .count();

    if letters > 0 && let Some((op, len)) = operator_at(pieces, letters) {
        let field: String = pieces[..letters]
            .iter()
            .map(|p| p.ch.to_ascii_lowercase())
            .collect();
        let op_text: String = pieces[letters..letters + len]
            .iter()
            .map(|p| p.ch)
            .collect();
        let value = &pieces[letters + len..];

        // `field:` with nothing after it is an existence check. Comparisons need a number.
        if value.is_empty() && op == FieldOp::Text {
            return Ok(Query::Exists { field });
        }
        if value.is_empty() {
            return Err(QueryError::new(
                format!("missing value after '{field}{op_text}'"),
                pos,
            ));
        }

        return match op {
            FieldOp::Text => Ok(Query::Text {
                field: Some(field),
                pattern: compile_pattern(value),
                exact: is_exact(value),
            }),
            FieldOp::Compare(op) => {
                let text: String = value.iter().map(|p| p.ch).collect();
                let value = text
                    .trim()
                    .parse::<f64>()
                    .ok()
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| {
                        QueryError::new(format!("expected a number after '{field}{op_text}'"), pos)
                    })?;
                Ok(Query::Compare { field, op, value })
            }
        };
    }

    Ok(Query::Text {
        field: None,
        pattern: compile_pattern(pieces),
        exact: is_exact(pieces),
    })
}

/// True when the whole value is one `'...'` quoted string.
fn is_exact(pieces: &[Piece]) -> bool {
    !pieces.is_empty()
        && pieces
            .iter()
            .all(|p| p.kind == Kind::Quoted { single: true })
}

/// Converts pieces to a pattern. Only unescaped, unquoted `*` and `?` are wildcards.
fn compile_pattern(pieces: &[Piece]) -> Vec<PatternChar> {
    pieces
        .iter()
        .map(|p| match (p.kind, p.ch) {
            (Kind::Bare, '*') => PatternChar::AnyRun,
            (Kind::Bare, '?') => PatternChar::AnyOne,
            (_, c) => PatternChar::Literal(fold(c)),
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

/// Substring match (or whole-value match when `exact`), case-insensitive.
fn text_matches(pattern: &[PatternChar], exact: bool, text: &str) -> bool {
    let folded: Vec<char> = text.chars().map(fold).collect();
    if exact {
        glob_match(pattern, &folded)
    } else {
        let mut wrapped = Vec::with_capacity(pattern.len() + 2);
        wrapped.push(PatternChar::AnyRun);
        wrapped.extend_from_slice(pattern);
        wrapped.push(PatternChar::AnyRun);
        glob_match(&wrapped, &folded)
    }
}

/// Matches the whole of `text` against `pattern`. Runs in O(pattern * text).
fn glob_match(pattern: &[PatternChar], text: &[char]) -> bool {
    let (mut p, mut t) = (0usize, 0usize);
    // Position of the last `*` in the pattern, and the text position it last absorbed up to.
    let mut star: Option<usize> = None;
    let mut retry = 0usize;

    while t < text.len() {
        match pattern.get(p) {
            Some(PatternChar::AnyRun) => {
                star = Some(p);
                p += 1;
                retry = t;
                continue;
            }
            Some(PatternChar::AnyOne) => {
                p += 1;
                t += 1;
                continue;
            }
            Some(PatternChar::Literal(c)) if *c == text[t] => {
                p += 1;
                t += 1;
                continue;
            }
            _ => {}
        }

        match star {
            Some(s) => {
                p = s + 1;
                retry += 1;
                t = retry;
            }
            None => return false,
        }
    }

    while matches!(pattern.get(p), Some(PatternChar::AnyRun)) {
        p += 1;
    }
    p == pattern.len()
}

fn compare(op: CompareOp, actual: f64, target: f64) -> bool {
    match op {
        CompareOp::Less => actual < target,
        CompareOp::LessEqual => actual <= target,
        CompareOp::Greater => actual > target,
        CompareOp::GreaterEqual => actual >= target,
        CompareOp::Equal => (actual - target).abs() < 0.0001,
    }
}

/// Reads the first number in a field value, e.g. `"1990"` or `"1.5 s"`.
fn first_number(text: &str) -> Option<f64> {
    let chars: Vec<char> = text.chars().collect();
    let start = chars.iter().position(|c| c.is_ascii_digit())?;
    let begin = if start > 0 && chars[start - 1] == '-' {
        start - 1
    } else {
        start
    };

    let mut end = start;
    let mut seen_dot = false;
    while end < chars.len() {
        let c = chars[end];
        let dot_ok = c == '.'
            && !seen_dot
            && chars
                .get(end + 1)
                .is_some_and(|d| d.is_ascii_digit());
        if c.is_ascii_digit() || dot_ok {
            seen_dot |= c == '.';
            end += 1;
        } else {
            break;
        }
    }

    let number: String = chars[begin..end].iter().collect();
    number.parse::<f64>().ok().filter(|v| v.is_finite())
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

impl Record for crate::content::CarEntry {
    fn text_fields(&self) -> Vec<String> {
        let mut fields = vec![
            self.id.clone(),
            self.name.clone(),
            self.brand.clone(),
            self.car_class.clone(),
            self.year.clone(),
            self.country.clone(),
            self.author.clone(),
        ];
        if let Some(parent) = &self.parent {
            fields.push(parent.clone());
        }
        fields.extend(self.tags.iter().cloned());
        fields
    }

    fn field(&self, name: &str) -> Option<String> {
        let non_empty = |s: &str| (!s.is_empty()).then(|| s.to_string());
        match name {
            "id" => non_empty(&self.id),
            "name" => non_empty(&self.name),
            "brand" => non_empty(&self.brand),
            "class" | "car_class" => non_empty(&self.car_class),
            "year" => non_empty(&self.year),
            "tag" | "tags" => (!self.tags.is_empty()).then(|| self.tags.join(" ")),
            "parent" => self.parent.as_deref().and_then(non_empty),
            "country" => non_empty(&self.country),
            "author" => non_empty(&self.author),
            "bhp" | "power" => self.bhp.map(|v| format!("{v}")),
            "weight" | "mass" => self.weight_kg.map(|v| format!("{v}")),
            "pwratio" | "pw" => self.pwratio.map(|v| format!("{v}")),
            _ => None,
        }
    }
}

impl Record for crate::content::TrackEntry {
    fn text_fields(&self) -> Vec<String> {
        let mut fields = vec![
            self.id.clone(),
            self.name.clone(),
            self.country.clone(),
            self.city.clone(),
        ];
        fields.extend(self.layouts.iter().cloned());
        fields.extend(self.tags.iter().cloned());
        fields
    }

    fn field(&self, name: &str) -> Option<String> {
        let non_empty = |s: &str| (!s.is_empty()).then(|| s.to_string());
        match name {
            "id" => non_empty(&self.id),
            "name" => non_empty(&self.name),
            "country" => non_empty(&self.country),
            "city" => non_empty(&self.city),
            "length" | "length_m" => self.length_m.map(|v| format!("{v}")),
            "pitboxes" | "pits" => self.pitboxes.map(|v| v.to_string()),
            "layouts" => (!self.layouts.is_empty()).then(|| self.layouts.join(" ")),
            "tag" | "tags" => (!self.tags.is_empty()).then(|| self.tags.join(" ")),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BMW: &str = "BMW M3 E30";
    const FERRARI: &str = "Ferrari 458 Italia";
    const ALFA: &str = "Alfa Romeo 155 V6 TI";
    const LOTUS: &str = "Lotus Exige S1";
    const FORD: &str = "Ford Focus RS";

    struct Car {
        name: &'static str,
        brand: &'static str,
        class: &'static str,
        year: u32,
        tags: &'static [&'static str],
    }

    impl Record for Car {
        fn text_fields(&self) -> Vec<String> {
            let mut out = vec![
                self.name.to_string(),
                self.brand.to_string(),
                self.class.to_string(),
            ];
            out.extend(self.tags.iter().map(|t| t.to_string()));
            out
        }

        fn field(&self, name: &str) -> Option<String> {
            match name {
                "name" => Some(self.name.to_string()),
                "brand" => Some(self.brand.to_string()),
                "class" => Some(self.class.to_string()),
                "year" => Some(self.year.to_string()),
                "tag" => Some(self.tags.join(" ")),
                _ => None,
            }
        }
    }

    fn fleet() -> Vec<Car> {
        vec![
            Car {
                name: BMW,
                brand: "BMW",
                class: "Touring",
                year: 1990,
                tags: &["rwd", "classic"],
            },
            Car {
                name: FERRARI,
                brand: "Ferrari",
                class: "GT3",
                year: 2010,
                tags: &["mid-engine"],
            },
            Car {
                name: ALFA,
                brand: "Alfa Romeo",
                class: "Touring",
                year: 1994,
                tags: &["rwd", "touring"],
            },
            Car {
                name: LOTUS,
                brand: "Lotus",
                class: "Road",
                year: 2001,
                tags: &["drift"],
            },
            Car {
                name: FORD,
                brand: "Ford",
                class: "Rally",
                year: 2009,
                tags: &["awd", "drift"],
            },
        ]
    }

    /// Names of the cars matching `query`, in fleet order.
    fn names(query: &str) -> Vec<&'static str> {
        let fleet = fleet();
        let parsed = parse(query).unwrap_or_else(|e| panic!("{query:?} should parse: {e}"));
        filter(&fleet, &parsed)
            .into_iter()
            .map(|car| car.name)
            .collect()
    }

    fn error(query: &str) -> QueryError {
        parse(query).expect_err("query should fail to parse")
    }

    #[test]
    fn empty_input_matches_everything() {
        assert_eq!(parse("").unwrap(), Query::All);
        assert_eq!(parse("   \t ").unwrap(), Query::All);
        assert_eq!(names("").len(), 5);
        assert_eq!(names(" \n ").len(), 5);
    }

    #[test]
    fn plain_words_search_text_fields_case_insensitively() {
        assert_eq!(names("bmw"), [BMW]);
        assert_eq!(names("FERRARI"), [FERRARI]);
        assert_eq!(names("bmw | ferrari"), [BMW, FERRARI]);
        assert_eq!(names("drift"), [LOTUS, FORD]);
        assert_eq!(names("nonexistent"), Vec::<&str>::new());
        assert_eq!(names("*"), [BMW, FERRARI, ALFA, LOTUS, FORD]);
    }

    #[test]
    fn and_binds_tighter_than_or() {
        // (bmw AND ferrari) OR ford. The other grouping would match nothing.
        assert_eq!(names("bmw ferrari | ford"), [FORD]);
        assert_eq!(names("bmw & ferrari | ford"), [FORD]);
        // bmw AND touring matches the BMW, OR ford.
        assert_eq!(names("bmw touring | ford"), [BMW, FORD]);
        // ford OR (bmw AND touring).
        assert_eq!(names("ford | bmw touring"), [BMW, FORD]);
        assert_eq!(names("bmw & touring"), [BMW]);
        assert_eq!(names("bmw , ferrari"), [BMW, FERRARI]);
    }

    #[test]
    fn parentheses_group_terms() {
        assert_eq!(names("(bmw | ferrari) touring"), [BMW]);
        assert_eq!(names("bmw (ferrari | ford)"), Vec::<&str>::new());
        assert_eq!(names("(ferrari | ford) & !rally"), [FERRARI]);
        assert_eq!(names("bmw & (ferrari | ford | bmw)"), [BMW]);
    }

    #[test]
    fn negation() {
        assert_eq!(names("!touring"), [FERRARI, LOTUS, FORD]);
        assert_eq!(names("!!bmw"), [BMW]);
        assert_eq!(names("touring !bmw"), [ALFA]);
        assert_eq!(names("!(touring | road)"), [FERRARI, FORD]);
        assert_eq!(names("!bmw !ferrari"), [ALFA, LOTUS, FORD]);
        assert_eq!(names("!tag:drift"), [BMW, FERRARI, ALFA]);
    }

    #[test]
    fn wildcards() {
        assert_eq!(names("fer*ari"), [FERRARI]);
        assert_eq!(names("f?rrari"), [FERRARI]);
        assert_eq!(names("alfa*155"), [ALFA]);
        assert_eq!(names("ex?ge"), [LOTUS]);
        assert_eq!(names("brand:f*"), [FERRARI, ALFA, FORD]);
    }

    #[test]
    fn escapes_and_quotes_are_literal() {
        // Escaped or quoted wildcards and parentheses are plain characters.
        assert_eq!(names("\\*"), Vec::<&str>::new());
        assert_eq!(names("\"*\""), Vec::<&str>::new());
        assert_eq!(names("\\(bmw"), Vec::<&str>::new());
        assert_eq!(names("\"(bmw)\""), Vec::<&str>::new());
        assert_eq!(names("\\bmw"), [BMW]);
        assert_eq!(names("\"alfa romeo\""), [ALFA]);
        assert_eq!(names("\"alfa romeo\" | lotus"), [ALFA, LOTUS]);
    }

    #[test]
    fn single_quotes_require_an_exact_match() {
        assert_eq!(names("'alfa romeo'"), [ALFA]);
        assert_eq!(names("'alfa'"), Vec::<&str>::new());
        assert_eq!(names("'Alfa Romeo 155 V6 TI'"), [ALFA]);
    }

    #[test]
    fn field_matches() {
        assert_eq!(names("brand:bmw"), [BMW]);
        assert_eq!(names("brand:alfa"), [ALFA]);
        assert_eq!(names("class:gt3"), [FERRARI]);
        assert_eq!(names("tag:drift"), [LOTUS, FORD]);
        assert_eq!(names("name:\"alfa romeo\""), [ALFA]);
        assert_eq!(names("name:'alfa romeo'"), Vec::<&str>::new());
        assert_eq!(names("NAME:bmw"), [BMW]);
        assert_eq!(names("colour:red"), Vec::<&str>::new());
    }

    #[test]
    fn numeric_comparisons() {
        assert_eq!(names("year>2000"), [FERRARI, LOTUS, FORD]);
        assert_eq!(names("YEAR>2000"), [FERRARI, LOTUS, FORD]);
        assert_eq!(names("year<=1994"), [BMW, ALFA]);
        assert_eq!(names("year<1990"), Vec::<&str>::new());
        assert_eq!(names("year=1990"), [BMW]);
        assert_eq!(names("year>=2009"), [FERRARI, FORD]);
        assert_eq!(names("year=>2009"), [FERRARI, FORD]);
        assert_eq!(names("year=<1990"), [BMW]);
        assert_eq!(names("year>=1990 & year<2000"), [BMW, ALFA]);
        assert_eq!(names("year>1990.5"), [FERRARI, ALFA, LOTUS, FORD]);
        assert_eq!(names("year>2000 !tag:drift"), [FERRARI]);
        // A numeric comparison on a non-numeric field never matches.
        assert_eq!(names("brand>5"), Vec::<&str>::new());
    }

    #[test]
    fn filter_keeps_order_and_references() {
        let fleet = fleet();
        let query = parse("drift").unwrap();
        let found = filter(&fleet, &query);
        assert_eq!(found.len(), 2);
        assert!(std::ptr::eq(found[0], &fleet[3]));
        assert!(std::ptr::eq(found[1], &fleet[4]));
    }

    #[test]
    fn field_existence_check() {
        // Every car has a tag field, so the check passes for all of them.
        assert_eq!(names("tag:").len(), 5);
        assert_eq!(names("brand:").len(), 5);
        // No car has a colour field.
        assert_eq!(names("colour:"), Vec::<&str>::new());
        assert_eq!(names("!colour:").len(), 5);
        assert_eq!(names("(tag:) & touring"), [BMW, ALFA]);
        assert_eq!(names("year: & !class:touring"), [FERRARI, LOTUS, FORD]);
        // A value after the colon is still a substring match, not an existence check.
        assert_eq!(names("brand:bmw"), [BMW]);
        // Comparisons still need a value.
        assert!(error("year>").message.contains("missing value"));
        assert!(error("year<=").message.contains("missing value"));
    }

    /// A record whose one optional field may be missing.
    struct Item {
        name: &'static str,
        value: Option<&'static str>,
    }

    impl Record for Item {
        fn text_fields(&self) -> Vec<String> {
            vec![self.name.to_string()]
        }

        fn field(&self, name: &str) -> Option<String> {
            match name {
                "value" => self.value.map(str::to_string),
                _ => None,
            }
        }
    }

    fn sorted_names(cars: &[Car]) -> Vec<&'static str> {
        cars.iter().map(|car| car.name).collect()
    }

    #[test]
    fn sort_by_numeric_field() {
        let mut cars = fleet();
        sort_by_field(&mut cars, "year", false);
        assert_eq!(sorted_names(&cars), [BMW, ALFA, LOTUS, FORD, FERRARI]);

        sort_by_field(&mut cars, "YEAR", true);
        assert_eq!(sorted_names(&cars), [FERRARI, FORD, LOTUS, ALFA, BMW]);
    }

    #[test]
    fn sort_by_text_field_ignores_case() {
        let mut cars = fleet();
        sort_by_field(&mut cars, "name", false);
        assert_eq!(sorted_names(&cars), [ALFA, BMW, FERRARI, FORD, LOTUS]);

        sort_by_field(&mut cars, "name", true);
        assert_eq!(sorted_names(&cars), [LOTUS, FORD, FERRARI, BMW, ALFA]);
    }

    #[test]
    fn sort_by_unknown_field_keeps_order() {
        let mut cars = fleet();
        sort_by_field(&mut cars, "colour", false);
        assert_eq!(sorted_names(&cars), [BMW, FERRARI, ALFA, LOTUS, FORD]);
        sort_by_field(&mut cars, "colour", true);
        assert_eq!(sorted_names(&cars), [BMW, FERRARI, ALFA, LOTUS, FORD]);
    }

    #[test]
    fn sort_puts_missing_values_last_and_numbers_before_text() {
        let mut items = vec![
            Item { name: "missing", value: None },
            Item { name: "text", value: Some("beta") },
            Item { name: "ten", value: Some("10") },
            Item { name: "two", value: Some(" 2 ") },
            Item { name: "alpha", value: Some("Alpha") },
        ];

        sort_by_field(&mut items, "value", false);
        let order: Vec<&str> = items.iter().map(|i| i.name).collect();
        assert_eq!(order, ["two", "ten", "alpha", "text", "missing"]);

        // Descending is the exact reverse of the values' order; missing values stay last.
        sort_by_field(&mut items, "value", true);
        let order: Vec<&str> = items.iter().map(|i| i.name).collect();
        assert_eq!(order, ["text", "alpha", "ten", "two", "missing"]);
    }

    #[test]
    fn sort_is_stable_for_equal_keys() {
        let mut items = vec![
            Item { name: "first", value: Some("5") },
            Item { name: "second", value: Some("5.0") },
            Item { name: "third", value: Some("5") },
        ];
        sort_by_field(&mut items, "value", false);
        let order: Vec<&str> = items.iter().map(|i| i.name).collect();
        assert_eq!(order, ["first", "second", "third"]);
    }

    #[test]
    fn unbalanced_parentheses_are_errors() {
        let e = error("(bmw");
        assert!(e.message.contains("missing"), "{e}");
        assert_eq!(e.position, 0);

        let e = error("bmw)");
        assert!(e.message.contains("unexpected ')'"), "{e}");
        assert_eq!(e.position, 3);

        assert_eq!(error("((bmw)").position, 0);
        assert_eq!(error("(").position, 0);
        assert_eq!(error("()").position, 0);
        assert!(error("(bmw").to_string().contains("column 1"));
    }

    #[test]
    fn dangling_operators_and_bad_values_are_errors() {
        assert!(error("bmw &").message.contains("end of the query"));
        assert!(error("| bmw").message.contains("'|'"));
        assert!(error("bmw ||").message.contains("'|'"));
        assert!(error("!").message.contains("end of the query"));
        assert_eq!(error("year>abc").position, 0);
        assert!(error("year>").message.contains("missing value"));
        assert!(error("\"open").message.contains("unterminated"));
    }

    #[test]
    fn nesting_depth_is_limited() {
        let deep = |n: usize| format!("{}bmw{}", "(".repeat(n), ")".repeat(n));

        assert_eq!(names(&deep(MAX_DEPTH)), [BMW]);
        let e = parse(&deep(MAX_DEPTH + 1)).expect_err("too deep");
        assert!(e.message.contains("nested"), "{e}");
        assert!(parse(&"(".repeat(100_000)).is_err());
    }

    #[test]
    fn long_inputs_do_not_overflow() {
        // Repeated `!` toggle instead of nesting, so this stays shallow.
        assert_eq!(names(&format!("{}bmw", "!".repeat(100_000))), [BMW]);
        // A long implicit AND is a flat list, not a deep tree.
        assert!(parse(&"a ".repeat(10_000)).is_ok());
    }

    #[test]
    fn junk_input_never_panics() {
        let cars = fleet();
        let mut inputs: Vec<String> = [
            "", " ", "(", ")", "()", "((", "))", "&", "|", ",", "!", "!!", "&&", "||", "a&",
            "&a", "a|", "|a", "a!", "!a", "\\", "a\\", "\"", "'", "\"\"", "''", "\"a", "'a",
            "`", "*", "?", "**", "a*b*c", "::", ":", "a:", ":a", "<", ">", "=", "<=", "=<",
            ">=", "=>", "year<", "year=>", "year==2", "year>>2", "year<=<=3", "é", "日本語",
            "🏎️ ferrari", "\u{0}", "a\u{0}b", "\n\t", "name:\"", "name:'", "((a|b)&c",
            "a,,b", "!(!(!(", "year>1e308", "year>nan", "year>inf", "year>-", "year>1.2.3",
            "bmw ferrari | ", "\\)", "\\(", "year>99999999999999999999999999999999999999999999",
            "\"\\", "'\\'", "a\\\\\\", "::::", "<<<<", "====",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        // Deterministic pseudo-random junk from an operator-heavy alphabet (xorshift64).
        let alphabet: Vec<char> = "()&|,!\\\"'*?:<>=ab yz1.-\u{0}é🏎".chars().collect();
        let mut seed: u64 = 0x2545_F491_4F6C_DD1D;
        let mut next = move || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for _ in 0..2000 {
            let len = (next() % 24) as usize;
            let s: String = (0..len)
                .map(|_| alphabet[(next() % alphabet.len() as u64) as usize])
                .collect();
            inputs.push(s);
        }

        for input in &inputs {
            if let Ok(query) = parse(input) {
                let _ = filter(&cars, &query);
            }
        }
    }

    #[test]
    fn car_and_track_records_support_numeric_tag_and_boolean_queries() {
        use crate::content::{CarEntry, TrackEntry};
        use std::path::PathBuf;

        let cars = vec![
            CarEntry {
                id: "ks_ferrari_488_gt3".into(),
                name: "Ferrari 488 GT3".into(),
                brand: "Ferrari".into(),
                car_class: "race".into(),
                year: "2016".into(),
                tags: vec!["gt3".into(), "#italian".into()],
                parent: Some("ks_ferrari_488_gtb".into()),
                country: "Italy".into(),
                author: "Kunos".into(),
                bhp: Some(550.0),
                weight_kg: Some(1260.0),
                pwratio: Some(2.29),
                path: PathBuf::from("c1"),
            },
            CarEntry {
                id: "ks_mazda_mx5_nd".into(),
                name: "Mazda MX-5 ND".into(),
                brand: "Mazda".into(),
                car_class: "street".into(),
                year: "2015".into(),
                tags: vec!["street".into(), "jdm".into()],
                parent: None,
                country: "Japan".into(),
                author: "Kunos".into(),
                bhp: Some(160.0),
                weight_kg: Some(1050.0),
                pwratio: Some(6.56),
                path: PathBuf::from("c2"),
            },
        ];

        let q1 = parse("bhp>400 & tag:gt3 & country:italy").unwrap();
        let hits1 = filter(&cars, &q1);
        assert_eq!(hits1.len(), 1);
        assert_eq!(hits1[0].id, "ks_ferrari_488_gt3");

        let q2 = parse("!parent: & weight<1100").unwrap();
        let hits2 = filter(&cars, &q2);
        assert_eq!(hits2.len(), 1);
        assert_eq!(hits2[0].id, "ks_mazda_mx5_nd");

        let tracks = vec![
            TrackEntry {
                id: "spa".into(),
                name: "Spa-Francorchamps".into(),
                country: "Belgium".into(),
                city: "Stavelot".into(),
                length_m: Some(7004.0),
                pitboxes: Some(24),
                layouts: vec!["gp".into()],
                tags: vec!["circuit".into(), "f1".into()],
                path: PathBuf::from("t1"),
            },
            TrackEntry {
                id: "drift".into(),
                name: "Drift Playground".into(),
                country: "Italy".into(),
                city: "Vallelunga".into(),
                length_m: Some(1200.0),
                pitboxes: Some(8),
                layouts: vec![],
                tags: vec!["drift".into()],
                path: PathBuf::from("t2"),
            },
        ];

        let tq = parse("length>5000 & pitboxes>=20 & tag:f1").unwrap();
        let thits = filter(&tracks, &tq);
        assert_eq!(thits.len(), 1);
        assert_eq!(thits[0].id, "spa");
    }
}
