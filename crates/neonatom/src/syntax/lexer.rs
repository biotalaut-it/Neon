use super::{Diagnostic, DiagnosticKind, Span};
use rust_decimal::Decimal;

#[derive(Debug, Clone, PartialEq)]
pub(super) enum TokenKind {
    Int(i64),
    Decimal(Decimal),
    Ident(String),
    Str(String),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Eq,
    EqEq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    And,
    Or,
    Arrow,
    LParen,
    RParen,
    Comma,
    Semi,
    Colon,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Eof,
}
#[derive(Debug, Clone)]
pub(super) struct Token {
    pub(super) kind: TokenKind,
    pub(super) span: Span,
}

pub(super) fn lex(source: &str) -> Result<Vec<Token>, Diagnostic> {
    if source.len() > 1024 * 1024 {
        return Err(Diagnostic {
            kind: DiagnosticKind::Limit,
            message: "source exceeds 1 MiB limit".into(),
            span: Span {
                start: 0,
                end: source.len(),
            },
        });
    }
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut out = Vec::new();
    while i < bytes.len() {
        let c = bytes[i];
        if c.is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let simple = match c {
            b'+' => Some(TokenKind::Plus),
            b'-' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Arrow,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'-' => Some(TokenKind::Minus),
            b'*' => Some(TokenKind::Star),
            b'/' => Some(TokenKind::Slash),
            b'%' => Some(TokenKind::Percent),
            b'(' => Some(TokenKind::LParen),
            b')' => Some(TokenKind::RParen),
            b',' => Some(TokenKind::Comma),
            b';' => Some(TokenKind::Semi),
            b':' => Some(TokenKind::Colon),
            b'[' => Some(TokenKind::LBracket),
            b']' => Some(TokenKind::RBracket),
            b'{' => Some(TokenKind::LBrace),
            b'}' => Some(TokenKind::RBrace),
            b'=' if bytes.get(i + 1) == Some(&b'>') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Arrow,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'=' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::EqEq,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'=' => Some(TokenKind::Eq),
            b'!' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Ne,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'<' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Le,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'<' => Some(TokenKind::Lt),
            b'>' if bytes.get(i + 1) == Some(&b'=') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Ge,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'>' => Some(TokenKind::Gt),
            b'&' if bytes.get(i + 1) == Some(&b'&') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::And,
                    span: Span { start, end: i },
                });
                continue;
            }
            b'|' if bytes.get(i + 1) == Some(&b'|') => {
                i += 2;
                out.push(Token {
                    kind: TokenKind::Or,
                    span: Span { start, end: i },
                });
                continue;
            }
            _ => None,
        };
        if let Some(kind) = simple {
            i += 1;
            out.push(Token {
                kind,
                span: Span { start, end: i },
            });
            continue;
        }
        if c == b'"' {
            i += 1;
            let mut s = String::new();
            let mut closed = false;
            while i < bytes.len() {
                match bytes[i] {
                    b'"' => {
                        i += 1;
                        closed = true;
                        break;
                    }
                    b'\\' if i + 1 < bytes.len() => {
                        i += 1;
                        let ch = match bytes[i] {
                            b'n' => '\n',
                            b'r' => '\r',
                            b't' => '\t',
                            b'"' => '"',
                            b'\\' => '\\',
                            _ => {
                                return Err(Diagnostic {
                                    kind: DiagnosticKind::Lex,
                                    message: "unsupported string escape".into(),
                                    span: Span {
                                        start: i - 1,
                                        end: i + 1,
                                    },
                                })
                            }
                        };
                        s.push(ch);
                        i += 1;
                    }
                    _ => {
                        let rest = std::str::from_utf8(&bytes[i..]).unwrap_or("");
                        let ch = rest.chars().next().ok_or_else(|| Diagnostic {
                            kind: DiagnosticKind::Lex,
                            message: "invalid UTF-8".into(),
                            span: Span {
                                start: i,
                                end: i + 1,
                            },
                        })?;
                        s.push(ch);
                        i += ch.len_utf8();
                    }
                }
            }
            if !closed {
                return Err(Diagnostic {
                    kind: DiagnosticKind::Lex,
                    message: "unterminated string".into(),
                    span: Span {
                        start,
                        end: bytes.len(),
                    },
                });
            }
            out.push(Token {
                kind: TokenKind::Str(s),
                span: Span { start, end: i },
            });
            continue;
        }
        if c.is_ascii_digit() {
            i += 1;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if bytes.get(i) == Some(&b'.') && bytes.get(i + 1).is_some_and(u8::is_ascii_digit) {
                i += 1;
                while i < bytes.len() && bytes[i].is_ascii_digit() {
                    i += 1;
                }
                let value = Decimal::from_str_exact(&source[start..i]).map_err(|_| Diagnostic {
                    kind: DiagnosticKind::Lex,
                    message: "invalid fixed-point decimal literal".into(),
                    span: Span { start, end: i },
                })?;
                out.push(Token {
                    kind: TokenKind::Decimal(value),
                    span: Span { start, end: i },
                });
                continue;
            }
            let n = source[start..i].parse::<i64>().map_err(|_| Diagnostic {
                kind: DiagnosticKind::Lex,
                message: "integer literal outside signed 64-bit range".into(),
                span: Span { start, end: i },
            })?;
            out.push(Token {
                kind: TokenKind::Int(n),
                span: Span { start, end: i },
            });
            continue;
        }
        if c.is_ascii_alphabetic() || c == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            out.push(Token {
                kind: TokenKind::Ident(source[start..i].to_string()),
                span: Span { start, end: i },
            });
            continue;
        }
        return Err(Diagnostic {
            kind: DiagnosticKind::Lex,
            message: format!("unexpected character {:?}", c as char),
            span: Span {
                start,
                end: start + 1,
            },
        });
    }
    out.push(Token {
        kind: TokenKind::Eof,
        span: Span {
            start: bytes.len(),
            end: bytes.len(),
        },
    });
    Ok(out)
}
