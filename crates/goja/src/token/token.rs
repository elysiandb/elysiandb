use super::token_const::*;
use std::fmt;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Debug)]
pub struct Token(pub i64);

impl Token {
    pub fn string(self) -> String {
        if self.0 == 0 {
            return "UNKNOWN".to_string();
        }
        if self.0 < TOKEN2STRING.len() as i64 {
            return TOKEN2STRING[self.0 as usize].to_string();
        }
        format!("token({})", self.0)
    }

    pub fn precedence(self, in_: bool) -> i64 {
        match self {
            LOGICAL_OR => 1,
            LOGICAL_AND => 2,
            OR | OR_ASSIGN => 3,
            EXCLUSIVE_OR => 4,
            AND | AND_ASSIGN => 5,
            EQUAL | NOT_EQUAL | STRICT_EQUAL | STRICT_NOT_EQUAL => 6,
            LESS | GREATER | LESS_OR_EQUAL | GREATER_OR_EQUAL | INSTANCEOF => 7,
            IN => {
                if in_ {
                    7
                } else {
                    0
                }
            }
            SHIFT_LEFT
            | SHIFT_RIGHT
            | UNSIGNED_SHIFT_RIGHT
            | SHIFT_LEFT_ASSIGN
            | SHIFT_RIGHT_ASSIGN
            | UNSIGNED_SHIFT_RIGHT_ASSIGN => 8,
            PLUS | MINUS | ADD_ASSIGN | SUBTRACT_ASSIGN => 9,
            MULTIPLY | SLASH | REMAINDER | MULTIPLY_ASSIGN | QUOTIENT_ASSIGN | REMAINDER_ASSIGN => {
                11
            }
            _ => 0,
        }
    }
}

impl fmt::Display for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

struct Keyword {
    token: Token,
    future_keyword: bool,
    strict: bool,
}

const fn kw(token: Token) -> Keyword {
    Keyword {
        token,
        future_keyword: false,
        strict: false,
    }
}

const fn future(token: Token) -> Keyword {
    Keyword {
        token,
        future_keyword: true,
        strict: false,
    }
}

const fn strict(token: Token) -> Keyword {
    Keyword {
        token,
        future_keyword: false,
        strict: true,
    }
}

fn keyword(literal: &[u8]) -> Option<Keyword> {
    Some(match literal {
        b"if" => kw(IF),
        b"in" => kw(IN),
        b"do" => kw(DO),
        b"var" => kw(VAR),
        b"for" => kw(FOR),
        b"new" => kw(NEW),
        b"try" => kw(TRY),
        b"this" => kw(THIS),
        b"else" => kw(ELSE),
        b"case" => kw(CASE),
        b"void" => kw(VOID),
        b"with" => kw(WITH),
        b"async" => kw(ASYNC),
        b"while" => kw(WHILE),
        b"break" => kw(BREAK),
        b"catch" => kw(CATCH),
        b"throw" => kw(THROW),
        b"return" => kw(RETURN),
        b"typeof" => kw(TYPEOF),
        b"delete" => kw(DELETE),
        b"switch" => kw(SWITCH),
        b"default" => kw(DEFAULT),
        b"finally" => kw(FINALLY),
        b"function" => kw(FUNCTION),
        b"continue" => kw(CONTINUE),
        b"debugger" => kw(DEBUGGER),
        b"instanceof" => kw(INSTANCEOF),
        b"const" => kw(CONST),
        b"class" => kw(CLASS),
        b"enum" => future(KEYWORD),
        b"export" => future(KEYWORD),
        b"extends" => kw(EXTENDS),
        b"import" => future(KEYWORD),
        b"super" => kw(SUPER),
        b"let" => strict(LET),
        b"static" => strict(STATIC),
        b"await" => kw(AWAIT),
        b"yield" => kw(YIELD),
        b"false" => kw(BOOLEAN),
        b"true" => kw(BOOLEAN),
        b"null" => kw(NULL),
        _ => return None,
    })
}

pub fn is_keyword(literal: &[u8]) -> (Token, bool) {
    if let Some(keyword) = keyword(literal) {
        if keyword.future_keyword {
            return (KEYWORD, keyword.strict);
        }
        return (keyword.token, false);
    }
    (Token(0), false)
}

pub fn is_id(tkn: Token) -> bool {
    tkn >= IDENTIFIER
}

pub fn is_unreserved_word(tkn: Token) -> bool {
    tkn > ESCAPED_RESERVED_WORD
}
