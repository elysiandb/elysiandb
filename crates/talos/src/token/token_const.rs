use super::token::Token;

pub const ILLEGAL: Token = Token(1);
pub const EOF: Token = Token(2);
pub const COMMENT: Token = Token(3);

pub const STRING: Token = Token(4);
pub const NUMBER: Token = Token(5);

pub const PLUS: Token = Token(6);
pub const MINUS: Token = Token(7);
pub const MULTIPLY: Token = Token(8);
pub const EXPONENT: Token = Token(9);
pub const SLASH: Token = Token(10);
pub const REMAINDER: Token = Token(11);

pub const AND: Token = Token(12);
pub const OR: Token = Token(13);
pub const EXCLUSIVE_OR: Token = Token(14);
pub const SHIFT_LEFT: Token = Token(15);
pub const SHIFT_RIGHT: Token = Token(16);
pub const UNSIGNED_SHIFT_RIGHT: Token = Token(17);

pub const ADD_ASSIGN: Token = Token(18);
pub const SUBTRACT_ASSIGN: Token = Token(19);
pub const MULTIPLY_ASSIGN: Token = Token(20);
pub const EXPONENT_ASSIGN: Token = Token(21);
pub const QUOTIENT_ASSIGN: Token = Token(22);
pub const REMAINDER_ASSIGN: Token = Token(23);

pub const AND_ASSIGN: Token = Token(24);
pub const OR_ASSIGN: Token = Token(25);
pub const EXCLUSIVE_OR_ASSIGN: Token = Token(26);
pub const SHIFT_LEFT_ASSIGN: Token = Token(27);
pub const SHIFT_RIGHT_ASSIGN: Token = Token(28);
pub const UNSIGNED_SHIFT_RIGHT_ASSIGN: Token = Token(29);

pub const LOGICAL_AND: Token = Token(30);
pub const LOGICAL_OR: Token = Token(31);
pub const COALESCE: Token = Token(32);
pub const INCREMENT: Token = Token(33);
pub const DECREMENT: Token = Token(34);

pub const LOGICAL_AND_ASSIGN: Token = Token(35);
pub const LOGICAL_OR_ASSIGN: Token = Token(36);
pub const COALESCE_ASSIGN: Token = Token(37);

pub const EQUAL: Token = Token(38);
pub const STRICT_EQUAL: Token = Token(39);
pub const LESS: Token = Token(40);
pub const GREATER: Token = Token(41);
pub const ASSIGN: Token = Token(42);
pub const NOT: Token = Token(43);

pub const BITWISE_NOT: Token = Token(44);

pub const NOT_EQUAL: Token = Token(45);
pub const STRICT_NOT_EQUAL: Token = Token(46);
pub const LESS_OR_EQUAL: Token = Token(47);
pub const GREATER_OR_EQUAL: Token = Token(48);

pub const LEFT_PARENTHESIS: Token = Token(49);
pub const LEFT_BRACKET: Token = Token(50);
pub const LEFT_BRACE: Token = Token(51);
pub const COMMA: Token = Token(52);
pub const PERIOD: Token = Token(53);

pub const RIGHT_PARENTHESIS: Token = Token(54);
pub const RIGHT_BRACKET: Token = Token(55);
pub const RIGHT_BRACE: Token = Token(56);
pub const SEMICOLON: Token = Token(57);
pub const COLON: Token = Token(58);
pub const QUESTION_MARK: Token = Token(59);
pub const QUESTION_DOT: Token = Token(60);
pub const ARROW: Token = Token(61);
pub const ELLIPSIS: Token = Token(62);
pub const BACKTICK: Token = Token(63);

pub const PRIVATE_IDENTIFIER: Token = Token(64);

pub const IDENTIFIER: Token = Token(65);
pub const KEYWORD: Token = Token(66);
pub const BOOLEAN: Token = Token(67);
pub const NULL: Token = Token(68);

pub const IF: Token = Token(69);
pub const IN: Token = Token(70);
pub const OF: Token = Token(71);
pub const DO: Token = Token(72);

pub const VAR: Token = Token(73);
pub const FOR: Token = Token(74);
pub const NEW: Token = Token(75);
pub const TRY: Token = Token(76);

pub const THIS: Token = Token(77);
pub const ELSE: Token = Token(78);
pub const CASE: Token = Token(79);
pub const VOID: Token = Token(80);
pub const WITH: Token = Token(81);

pub const CONST: Token = Token(82);
pub const WHILE: Token = Token(83);
pub const BREAK: Token = Token(84);
pub const CATCH: Token = Token(85);
pub const THROW: Token = Token(86);
pub const CLASS: Token = Token(87);
pub const SUPER: Token = Token(88);

pub const RETURN: Token = Token(89);
pub const TYPEOF: Token = Token(90);
pub const DELETE: Token = Token(91);
pub const SWITCH: Token = Token(92);

pub const DEFAULT: Token = Token(93);
pub const FINALLY: Token = Token(94);
pub const EXTENDS: Token = Token(95);

pub const FUNCTION: Token = Token(96);
pub const CONTINUE: Token = Token(97);
pub const DEBUGGER: Token = Token(98);

pub const INSTANCEOF: Token = Token(99);

pub const ESCAPED_RESERVED_WORD: Token = Token(100);

pub const LET: Token = Token(101);
pub const STATIC: Token = Token(102);
pub const ASYNC: Token = Token(103);
pub const AWAIT: Token = Token(104);
pub const YIELD: Token = Token(105);

pub(super) static TOKEN2STRING: [&str; 106] = [
    "",
    "ILLEGAL",
    "EOF",
    "COMMENT",
    "STRING",
    "NUMBER",
    "+",
    "-",
    "*",
    "**",
    "/",
    "%",
    "&",
    "|",
    "^",
    "<<",
    ">>",
    ">>>",
    "+=",
    "-=",
    "*=",
    "**=",
    "/=",
    "%=",
    "&=",
    "|=",
    "^=",
    "<<=",
    ">>=",
    ">>>=",
    "&&",
    "||",
    "??",
    "++",
    "--",
    "&&=",
    "||=",
    "??=",
    "==",
    "===",
    "<",
    ">",
    "=",
    "!",
    "~",
    "!=",
    "!==",
    "<=",
    ">=",
    "(",
    "[",
    "{",
    ",",
    ".",
    ")",
    "]",
    "}",
    ";",
    ":",
    "?",
    "?.",
    "=>",
    "...",
    "`",
    "PRIVATE_IDENTIFIER",
    "IDENTIFIER",
    "KEYWORD",
    "BOOLEAN",
    "NULL",
    "if",
    "in",
    "of",
    "do",
    "var",
    "for",
    "new",
    "try",
    "this",
    "else",
    "case",
    "void",
    "with",
    "const",
    "while",
    "break",
    "catch",
    "throw",
    "class",
    "super",
    "return",
    "typeof",
    "delete",
    "switch",
    "default",
    "finally",
    "extends",
    "function",
    "continue",
    "debugger",
    "instanceof",
    "",
    "let",
    "static",
    "async",
    "await",
    "yield",
];
