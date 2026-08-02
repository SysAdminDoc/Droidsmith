//! Bounded boolean predicates over package attributes (profile schema v3).
//!
//! A v2 profile is an ordered list of concrete package names, which makes it
//! effectively device-specific: the same handset model from two carriers does
//! not ship the same bloat, so a fleet run has to be a fleet of identical
//! devices. A predicate resolves against the inventory Droidsmith already
//! enumerates, so one profile can describe intent ("every disabled carrier app
//! installed by the vendor store") rather than a list of names.
//!
//! Three properties are load-bearing here, and each is a deliberate constraint
//! rather than an implementation detail:
//!
//! 1. **Bounded and non-backtracking.** The grammar is LL(1) and parsed by
//!    recursive descent with an explicit depth counter, over a length-capped
//!    input with a capped atom count. There is no regex anywhere in this file.
//!    A profile is a file the user can be handed by someone else, so a
//!    predicate must not be able to cost more than linear time to evaluate.
//!
//! 2. **Total evaluation.** Every expression returns a decision or names the
//!    attribute it could not resolve. It never panics and never silently
//!    treats "unknown" as "matched".
//!
//! 3. **Unresolvable excludes.** An attribute the device did not report
//!    (`installer` is the one that genuinely happens) propagates through the
//!    whole expression and excludes the package, which is then reported. A
//!    predicate that cannot be decided must never quietly select a package for
//!    an irreversible action.
//!
//! Grammar:
//!
//! ```text
//! expr   := term ( '|' term )*
//! term   := factor ( '&' factor )*
//! factor := '!' factor | '(' expr ')' | atom
//! atom   := flag | attribute '==' value
//! value  := '"' … '"' | '\'' … '\'' | bare
//! ```

use crate::adb::packages::AppPackage;

/// Predicates are authored by hand and stored in a profile, so the cap is
/// generous for a readable expression and far below anything pathological.
pub const MAX_FILTER_BYTES: usize = 512;
const MAX_DEPTH: usize = 16;
const MAX_ATOMS: usize = 64;
const MAX_VALUE_BYTES: usize = 255;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FilterExpr {
    Flag(FilterFlag),
    Equals {
        attribute: FilterAttribute,
        value: String,
    },
    Not(Box<FilterExpr>),
    And(Box<FilterExpr>, Box<FilterExpr>),
    Or(Box<FilterExpr>, Box<FilterExpr>),
}

/// Attributes that are always present on an enumerated package, so they read
/// as bare words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterFlag {
    System,
    UserInstalled,
    Enabled,
    Disabled,
    Archived,
}

/// Attributes compared against a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterAttribute {
    /// The installing package. The one attribute a device genuinely may not
    /// report, which is why unresolvable is a real case and not a formality.
    Installer,
    /// The Android user the profile resolved to. Constant across packages; it
    /// exists so a predicate can guard against running on the wrong user.
    AndroidUser,
}

impl FilterAttribute {
    fn name(self) -> &'static str {
        match self {
            Self::Installer => "installer",
            Self::AndroidUser => "android_user",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterError {
    pub message: String,
    /// Byte offset the parser stopped at, so a message can point at the token.
    pub offset: usize,
}

impl std::fmt::Display for FilterError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{} (at byte {})", self.message, self.offset)
    }
}

/// Everything a predicate is allowed to see. Nothing here requires a device
/// round trip: it is all already in the inventory listing.
#[derive(Debug, Clone, Copy)]
pub struct FilterContext<'a> {
    pub package: &'a AppPackage,
    pub android_user: u32,
}

/// The one non-boolean outcome: the device did not report an attribute the
/// expression needs, so no decision is possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unresolvable {
    pub attribute: &'static str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Token<'a> {
    And,
    Or,
    Not,
    Open,
    Close,
    Equals,
    Word(&'a str),
    Quoted(&'a str),
}

struct Lexer<'a> {
    source: &'a str,
    bytes: &'a [u8],
    position: usize,
}

impl<'a> Lexer<'a> {
    fn new(source: &'a str) -> Self {
        Self {
            source,
            bytes: source.as_bytes(),
            position: 0,
        }
    }

    fn skip_whitespace(&mut self) {
        while self.position < self.bytes.len() && self.bytes[self.position].is_ascii_whitespace() {
            self.position += 1;
        }
    }

    fn next_token(&mut self) -> Result<Option<(Token<'a>, usize)>, FilterError> {
        self.skip_whitespace();
        if self.position >= self.bytes.len() {
            return Ok(None);
        }
        let start = self.position;
        let byte = self.bytes[start];
        let token = match byte {
            b'&' => {
                self.position += 1;
                // Accept the doubled form too; `&&` reads more naturally to
                // anyone coming from a shell, and rejecting it would be a
                // gratuitous papercut.
                if self.peek() == Some(b'&') {
                    self.position += 1;
                }
                Token::And
            }
            b'|' => {
                self.position += 1;
                if self.peek() == Some(b'|') {
                    self.position += 1;
                }
                Token::Or
            }
            b'!' => {
                self.position += 1;
                Token::Not
            }
            b'(' => {
                self.position += 1;
                Token::Open
            }
            b')' => {
                self.position += 1;
                Token::Close
            }
            b'=' => {
                self.position += 1;
                if self.peek() != Some(b'=') {
                    return Err(self.error(start, "expected `==`"));
                }
                self.position += 1;
                Token::Equals
            }
            b'"' | b'\'' => {
                let quote = byte;
                self.position += 1;
                let value_start = self.position;
                while self.position < self.bytes.len() && self.bytes[self.position] != quote {
                    // No escape sequences: a package or installer id never
                    // needs one, and not having them keeps the lexer a single
                    // forward pass with no state.
                    if self.bytes[self.position] == b'\\' {
                        return Err(self.error(
                            self.position,
                            "escape sequences are not supported inside a quoted value",
                        ));
                    }
                    self.position += 1;
                }
                if self.position >= self.bytes.len() {
                    return Err(self.error(start, "unterminated quoted value"));
                }
                let value = &self.source[value_start..self.position];
                self.position += 1;
                Token::Quoted(value)
            }
            byte if is_word_byte(byte) => {
                while self.position < self.bytes.len() && is_word_byte(self.bytes[self.position]) {
                    self.position += 1;
                }
                Token::Word(&self.source[start..self.position])
            }
            _ => {
                return Err(self.error(start, "unexpected character"));
            }
        };
        Ok(Some((token, start)))
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.position).copied()
    }

    fn error(&self, offset: usize, message: &str) -> FilterError {
        FilterError {
            message: message.to_string(),
            offset,
        }
    }
}

fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'.' | b'-' | b':')
}

struct Parser<'a> {
    tokens: Vec<(Token<'a>, usize)>,
    index: usize,
    atoms: usize,
    end: usize,
}

/// Parse a predicate. Rejects anything outside the bounded grammar, and does
/// so before a device is ever contacted.
pub fn parse(source: &str) -> Result<FilterExpr, FilterError> {
    if source.len() > MAX_FILTER_BYTES {
        return Err(FilterError {
            message: format!("filter exceeds {MAX_FILTER_BYTES} bytes"),
            offset: MAX_FILTER_BYTES,
        });
    }
    if source.trim().is_empty() {
        return Err(FilterError {
            message: "filter is empty".to_string(),
            offset: 0,
        });
    }
    let mut lexer = Lexer::new(source);
    let mut tokens = Vec::new();
    while let Some(token) = lexer.next_token()? {
        tokens.push(token);
    }
    let mut parser = Parser {
        tokens,
        index: 0,
        atoms: 0,
        end: source.len(),
    };
    let expr = parser.expr(0)?;
    if parser.index != parser.tokens.len() {
        let offset = parser.tokens[parser.index].1;
        return Err(FilterError {
            message: "unexpected trailing input".to_string(),
            offset,
        });
    }
    Ok(expr)
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<Token<'a>> {
        self.tokens.get(self.index).map(|(token, _)| *token)
    }

    fn offset(&self) -> usize {
        self.tokens
            .get(self.index)
            .map(|(_, offset)| *offset)
            .unwrap_or(self.end)
    }

    fn error(&self, message: &str) -> FilterError {
        FilterError {
            message: message.to_string(),
            offset: self.offset(),
        }
    }

    fn expr(&mut self, depth: usize) -> Result<FilterExpr, FilterError> {
        let mut left = self.term(depth)?;
        while matches!(self.peek(), Some(Token::Or)) {
            self.index += 1;
            let right = self.term(depth)?;
            left = FilterExpr::Or(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn term(&mut self, depth: usize) -> Result<FilterExpr, FilterError> {
        let mut left = self.factor(depth)?;
        while matches!(self.peek(), Some(Token::And)) {
            self.index += 1;
            let right = self.factor(depth)?;
            left = FilterExpr::And(Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn factor(&mut self, depth: usize) -> Result<FilterExpr, FilterError> {
        if depth >= MAX_DEPTH {
            return Err(self.error("filter nests deeper than the supported limit"));
        }
        match self.peek() {
            Some(Token::Not) => {
                self.index += 1;
                Ok(FilterExpr::Not(Box::new(self.factor(depth + 1)?)))
            }
            Some(Token::Open) => {
                self.index += 1;
                let inner = self.expr(depth + 1)?;
                if !matches!(self.peek(), Some(Token::Close)) {
                    return Err(self.error("expected `)`"));
                }
                self.index += 1;
                Ok(inner)
            }
            Some(Token::Word(word)) => {
                self.atoms += 1;
                if self.atoms > MAX_ATOMS {
                    return Err(self.error("filter has more terms than the supported limit"));
                }
                self.index += 1;
                if matches!(self.peek(), Some(Token::Equals)) {
                    self.index += 1;
                    let attribute = match word {
                        "installer" => FilterAttribute::Installer,
                        "android_user" => FilterAttribute::AndroidUser,
                        other => {
                            return Err(FilterError {
                                message: format!(
                                    "unknown comparable attribute {other:?}; supported: installer, android_user"
                                ),
                                offset: self.offset(),
                            })
                        }
                    };
                    let value = match self.peek() {
                        Some(Token::Word(value)) | Some(Token::Quoted(value)) => {
                            self.index += 1;
                            value.to_string()
                        }
                        _ => return Err(self.error("expected a value after `==`")),
                    };
                    if value.is_empty() || value.len() > MAX_VALUE_BYTES {
                        return Err(self.error("comparison value is empty or too long"));
                    }
                    if attribute == FilterAttribute::AndroidUser && value.parse::<u32>().is_err() {
                        return Err(FilterError {
                            message: format!("android_user expects a number, got {value:?}"),
                            offset: self.offset(),
                        });
                    }
                    return Ok(FilterExpr::Equals { attribute, value });
                }
                let flag = match word {
                    "system" => FilterFlag::System,
                    "user_installed" => FilterFlag::UserInstalled,
                    "enabled" => FilterFlag::Enabled,
                    "disabled" => FilterFlag::Disabled,
                    "archived" => FilterFlag::Archived,
                    other => {
                        return Err(FilterError {
                            message: format!(
                                "unknown attribute {other:?}; supported: system, user_installed, enabled, disabled, archived, installer, android_user"
                            ),
                            offset: self.offset(),
                        })
                    }
                };
                Ok(FilterExpr::Flag(flag))
            }
            Some(Token::Quoted(_)) => Err(self.error("a bare value is not a condition")),
            _ => Err(self.error("expected a condition")),
        }
    }
}

/// Decide one package against a parsed predicate.
///
/// `Err` means the expression needed an attribute the device did not report.
/// Callers must exclude the package and report it — the whole point of the
/// three-valued result is that "cannot decide" is not "matches".
pub fn evaluate(expr: &FilterExpr, context: &FilterContext<'_>) -> Result<bool, Unresolvable> {
    match expr {
        FilterExpr::Flag(flag) => Ok(match flag {
            FilterFlag::System => context.package.system,
            FilterFlag::UserInstalled => !context.package.system,
            FilterFlag::Enabled => context.package.enabled,
            FilterFlag::Disabled => !context.package.enabled,
            FilterFlag::Archived => context.package.archived,
        }),
        FilterExpr::Equals { attribute, value } => match attribute {
            FilterAttribute::Installer => match context.package.installer.as_deref() {
                Some(installer) => Ok(installer == value),
                None => Err(Unresolvable {
                    attribute: attribute.name(),
                }),
            },
            FilterAttribute::AndroidUser => Ok(value
                .parse::<u32>()
                .map(|wanted| wanted == context.android_user)
                .unwrap_or(false)),
        },
        // Strict propagation, including through `!` and short-circuit
        // positions: an expression containing an undecidable term is itself
        // undecidable. `!installer == "x"` on a package with no installer must
        // not become "true".
        FilterExpr::Not(inner) => evaluate(inner, context).map(|value| !value),
        FilterExpr::And(left, right) => {
            let left = evaluate(left, context)?;
            let right = evaluate(right, context)?;
            Ok(left && right)
        }
        FilterExpr::Or(left, right) => {
            let left = evaluate(left, context)?;
            let right = evaluate(right, context)?;
            Ok(left || right)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(name: &str, system: bool, enabled: bool, installer: Option<&str>) -> AppPackage {
        AppPackage {
            package: name.to_string(),
            enabled,
            system,
            apk_path: None,
            uid: None,
            installer: installer.map(str::to_string),
            archived: false,
            retained: false,
        }
    }

    fn decide(source: &str, package: &AppPackage) -> Result<bool, Unresolvable> {
        let expr = parse(source).expect("parses");
        evaluate(
            &expr,
            &FilterContext {
                package,
                android_user: 0,
            },
        )
    }

    #[test]
    fn flags_read_the_attributes_the_inventory_already_carries() {
        let vendor = package("com.vendor.app", true, false, Some("com.android.vending"));
        assert_eq!(decide("system", &vendor), Ok(true));
        assert_eq!(decide("user_installed", &vendor), Ok(false));
        assert_eq!(decide("disabled", &vendor), Ok(true));
        assert_eq!(decide("enabled", &vendor), Ok(false));
        assert_eq!(decide("archived", &vendor), Ok(false));
        assert_eq!(
            decide("installer == \"com.android.vending\"", &vendor),
            Ok(true)
        );
        assert_eq!(decide("installer == com.other.store", &vendor), Ok(false));
        assert_eq!(decide("android_user == 0", &vendor), Ok(true));
        assert_eq!(decide("android_user == 10", &vendor), Ok(false));
    }

    #[test]
    fn boolean_operators_bind_and_group_the_usual_way() {
        let vendor = package("com.vendor.app", true, false, Some("com.android.vending"));
        // `&` binds tighter than `|`, so this is `(system & disabled) | archived`.
        assert_eq!(decide("system & disabled | archived", &vendor), Ok(true));
        assert_eq!(decide("system & (disabled | archived)", &vendor), Ok(true));
        assert_eq!(decide("!system", &vendor), Ok(false));
        assert_eq!(decide("!(system & enabled)", &vendor), Ok(true));
        // Doubled forms are accepted so a shell-shaped expression still parses.
        assert_eq!(decide("system && disabled", &vendor), Ok(true));
        assert_eq!(decide("enabled || archived", &vendor), Ok(false));
    }

    #[test]
    fn an_unreported_attribute_is_undecidable_rather_than_false() {
        // The device reported no installer. Every expression that touches it
        // must refuse to decide, including through negation and through the
        // positions a short-circuit evaluator would skip.
        let unknown = package("com.vendor.app", true, true, None);
        let expected = Err(Unresolvable {
            attribute: "installer",
        });
        assert_eq!(
            decide("installer == com.android.vending", &unknown),
            expected
        );
        assert_eq!(
            decide("!(installer == com.android.vending)", &unknown),
            expected
        );
        // `system` is true here, so a short-circuiting OR would answer "true"
        // and select this package for an irreversible action on the strength
        // of a term it never evaluated.
        assert_eq!(
            decide("system | installer == com.android.vending", &unknown),
            expected
        );
        // ...and a short-circuiting AND would answer "false" off a term it
        // never evaluated, which is the same bug pointing the other way.
        assert_eq!(
            decide(
                "user_installed & installer == com.android.vending",
                &unknown
            ),
            expected
        );
        // Attributes that are always present still decide normally.
        assert_eq!(decide("system & enabled", &unknown), Ok(true));
    }

    #[test]
    fn the_grammar_is_bounded_in_length_depth_and_term_count() {
        let long = format!("installer == \"{}\"", "a".repeat(MAX_FILTER_BYTES));
        assert!(parse(&long).unwrap_err().message.contains("exceeds"));

        let deep = format!(
            "{}system{}",
            "(".repeat(MAX_DEPTH + 2),
            ")".repeat(MAX_DEPTH + 2)
        );
        assert!(parse(&deep).unwrap_err().message.contains("nests deeper"));

        // Under the limits, a deeply nested expression still parses.
        let shallow = format!("{}system{}", "(".repeat(4), ")".repeat(4));
        assert!(parse(&shallow).is_ok());

        // Packed with no spaces so the atom cap, not the length cap, is what
        // fires — otherwise this would pass for the wrong reason.
        let many = (0..MAX_ATOMS + 5)
            .map(|_| "system")
            .collect::<Vec<_>>()
            .join("|");
        assert!(
            many.len() < MAX_FILTER_BYTES,
            "the length cap would fire first"
        );
        assert!(parse(&many).unwrap_err().message.contains("more terms"));
    }

    #[test]
    fn malformed_input_is_rejected_with_an_offset_instead_of_being_guessed_at() {
        for (source, fragment) in [
            ("", "empty"),
            ("system &", "expected a condition"),
            ("(system", "expected `)`"),
            ("system)", "trailing"),
            ("nonsense", "unknown attribute"),
            ("nonsense == 1", "unknown comparable attribute"),
            ("installer ==", "expected a value"),
            ("installer = x", "expected `==`"),
            ("android_user == later", "expects a number"),
            ("\"bare\"", "not a condition"),
            ("installer == \"unterminated", "unterminated"),
            ("installer == \"back\\slash\"", "escape sequences"),
            ("system # comment", "unexpected character"),
        ] {
            let error = parse(source).unwrap_err();
            assert!(
                error.message.contains(fragment),
                "{source:?} produced {error:?}, expected {fragment:?}"
            );
        }
    }

    #[test]
    fn parsing_is_linear_and_never_backtracks_on_adversarial_nesting() {
        // The shape that kills a backtracking matcher: alternation under
        // repetition. This is a recursive-descent LL(1) parser over a token
        // vector, so it is linear by construction; the assertion is that the
        // bounded grammar refuses it outright rather than trying.
        let hostile = format!("{}(system|system)", "!".repeat(200));
        let error = parse(&hostile).unwrap_err();
        assert!(error.message.contains("nests deeper"), "{error:?}");
    }
}
