//! GNU R 4.6.2 operator precedence and associativity table (shared with `deparse`).

/// Operator associativity.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Assoc {
    /// Left-associative (`a + b + c == (a + b) + c`).
    Left,
    /// Right-associative (`a ^ b ^ c == a ^ (b ^ c)`).
    Right,
    /// Non-associative (unary or relational chain guard).
    NonAssoc,
}

/// Precedence levels from lowest (`1`) to highest (`19`), matching R's `?Syntax`.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Prec(pub u8);

/// Precedence constants matching GNU R 4.6.2 `?Syntax`.
pub mod prec {
    use super::Prec;

    /// `?` help operator.
    pub const HELP: Prec = Prec(1);
    /// `=` assignment (lower than `<-`).
    pub const EQ_ASSIGN: Prec = Prec(2);
    /// `<-`, `<<-`, `:=` left assignment (right-associative).
    pub const LEFT_ASSIGN: Prec = Prec(3);
    /// `->`, `->>` right assignment (left-associative).
    pub const RIGHT_ASSIGN: Prec = Prec(4);
    /// `~` formula tilde.
    pub const TILDE: Prec = Prec(5);
    /// `|`, `||` logical OR.
    pub const OR: Prec = Prec(6);
    /// `&`, `&&` logical AND.
    pub const AND: Prec = Prec(7);
    /// `!` unary NOT.
    pub const NOT: Prec = Prec(8);
    /// `<`, `<=`, `>`, `>=`, `==`, `!=` comparisons.
    pub const REL: Prec = Prec(9);
    /// `+`, `-` binary addition/subtraction.
    pub const ADD: Prec = Prec(10);
    /// `*`, `/` binary multiplication/division.
    pub const MUL: Prec = Prec(11);
    /// `%any%`, `%||%`, `|>` special infix & native pipe.
    pub const SPECIAL: Prec = Prec(12);
    /// `:` sequence operator.
    pub const SEQ: Prec = Prec(13);
    /// `+`, `-` unary sign (tighter than `:`, looser than `^`).
    pub const UNARY: Prec = Prec(14);
    /// `^`, `**` exponentiation (right-associative; tighter than unary `-`!).
    pub const POW: Prec = Prec(15);
    /// `$`, `@` extraction.
    pub const EXTRACT: Prec = Prec(16);
    /// `[`, `[[` indexing.
    pub const SUBSET: Prec = Prec(17);
    /// `::`, `:::` namespace access.
    pub const NS: Prec = Prec(18);
}

/// Return `(precedence, associativity)` for a binary R operator symbol.
#[must_use]
pub fn binary(op: &str) -> Option<(Prec, Assoc)> {
    match op {
        "?" => Some((prec::HELP, Assoc::Left)),
        "=" => Some((prec::EQ_ASSIGN, Assoc::Right)),
        "<-" | "<<-" | ":=" => Some((prec::LEFT_ASSIGN, Assoc::Right)),
        "->" | "->>" => Some((prec::RIGHT_ASSIGN, Assoc::Left)),
        "~" => Some((prec::TILDE, Assoc::Right)),
        "|" | "||" => Some((prec::OR, Assoc::Left)),
        "&" | "&&" => Some((prec::AND, Assoc::Left)),
        "<" | "<=" | ">" | ">=" | "==" | "!=" => Some((prec::REL, Assoc::NonAssoc)),
        "+" | "-" => Some((prec::ADD, Assoc::Left)),
        "*" | "/" => Some((prec::MUL, Assoc::Left)),
        "|>" => Some((prec::SPECIAL, Assoc::Left)),
        ":" => Some((prec::SEQ, Assoc::Left)),
        "^" | "**" => Some((prec::POW, Assoc::Right)),
        "$" | "@" => Some((prec::EXTRACT, Assoc::Left)),
        "::" | ":::" => Some((prec::NS, Assoc::Left)),
        s if s.len() >= 2 && s.starts_with('%') && s.ends_with('%') => {
            Some((prec::SPECIAL, Assoc::Left))
        }
        _ => None,
    }
}

/// Return precedence for a prefix unary R operator symbol.
#[must_use]
pub fn unary(op: &str) -> Option<Prec> {
    match op {
        "?" => Some(prec::HELP),
        "~" => Some(prec::TILDE),
        "!" => Some(prec::NOT),
        "+" | "-" => Some(prec::UNARY),
        _ => None,
    }
}
