use super::{
    lexer::{lex, Token, TokenKind},
    Diagnostic, DiagnosticKind, Expr, Op, Program,
};

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    max_nesting: usize,
}
impl Parser {
    fn current(&self) -> &Token {
        &self.tokens[self.pos]
    }
    fn bump(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if !matches!(t.kind, TokenKind::Eof) {
            self.pos += 1;
        }
        t
    }
    fn error(&self, message: &str) -> Diagnostic {
        Diagnostic {
            kind: DiagnosticKind::Parse,
            message: message.into(),
            span: self.current().span,
        }
    }
    fn eat(&mut self, kind: &TokenKind) -> bool {
        if std::mem::discriminant(&self.current().kind) == std::mem::discriminant(kind) {
            self.bump();
            true
        } else {
            false
        }
    }
    fn expect(&mut self, kind: &TokenKind, what: &str) -> Result<(), Diagnostic> {
        if self.eat(kind) {
            Ok(())
        } else {
            Err(self.error(&format!("expected {what}")))
        }
    }
    fn parse_program(&mut self) -> Result<Program, Diagnostic> {
        let mut expressions = Vec::new();
        while !matches!(self.current().kind, TokenKind::Eof) {
            expressions.push(self.expr(0, 0)?);
            if !self.eat(&TokenKind::Semi) && !matches!(self.current().kind, TokenKind::Eof) {
                return Err(self.error("expected ';' between expressions"));
            }
        }
        Ok(Program { expressions })
    }
    fn expr(&mut self, min_bp: u8, depth: usize) -> Result<Expr, Diagnostic> {
        if depth > self.max_nesting {
            return Err(self.error("maximum parser nesting exceeded"));
        }
        let token = self.bump();
        let mut lhs = match token.kind {
            TokenKind::Int(n) => Expr::Int(n),
            TokenKind::Decimal(n) => Expr::Decimal(n),
            TokenKind::Str(s) => Expr::Str(s),
            TokenKind::Ident(ref s) if s == "true" => Expr::Bool(true),
            TokenKind::Ident(ref s) if s == "false" => Expr::Bool(false),
            TokenKind::Ident(ref s) if s == "let" => {
                let name = match self.bump().kind {
                    TokenKind::Ident(n) => n,
                    _ => return Err(self.error("expected identifier after let")),
                };
                self.expect(&TokenKind::Eq, "'='")?;
                Expr::Let(name, Box::new(self.expr(0, depth + 1)?))
            }
            TokenKind::Ident(name) => Expr::Var(name),
            TokenKind::Minus => {
                let rhs = self.expr(11, depth + 1)?;
                Expr::Binary(Box::new(Expr::Int(0)), Op::Sub, Box::new(rhs))
            }
            TokenKind::LParen => {
                let e = self.expr(0, depth + 1)?;
                self.expect(&TokenKind::RParen, "')'")?;
                e
            }
            TokenKind::LBracket => {
                let mut items = Vec::new();
                if !self.eat(&TokenKind::RBracket) {
                    loop {
                        items.push(self.expr(0, depth + 1)?);
                        if self.eat(&TokenKind::RBracket) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "',' or ']' after list item")?;
                    }
                }
                Expr::List(items)
            }
            TokenKind::LBrace => {
                let mut fields = Vec::new();
                if !self.eat(&TokenKind::RBrace) {
                    loop {
                        let key = match self.bump().kind {
                            TokenKind::Ident(key) | TokenKind::Str(key) => key,
                            _ => return Err(self.error("record key must be text or an identifier")),
                        };
                        self.expect(&TokenKind::Colon, "':' after record key")?;
                        fields.push((key, self.expr(0, depth + 1)?));
                        if self.eat(&TokenKind::RBrace) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "',' or '}' after record field")?;
                    }
                }
                Expr::Record(fields)
            }
            _ => {
                return Err(Diagnostic {
                    kind: DiagnosticKind::Parse,
                    message: "expected expression".into(),
                    span: token.span,
                })
            }
        };
        loop {
            if matches!(self.current().kind, TokenKind::Arrow) {
                if min_bp > 1 {
                    break;
                }
                let param = match lhs {
                    Expr::Var(name) => name,
                    _ => return Err(self.error("lambda parameter must be an identifier")),
                };
                self.bump();
                let body = self.expr(0, depth + 1)?;
                lhs = Expr::Lambda(param, Box::new(body));
                continue;
            }
            if matches!(self.current().kind, TokenKind::LParen) {
                if min_bp > 13 {
                    break;
                }
                let callee = match lhs {
                    Expr::Var(name) => name,
                    _ => return Err(self.error("only named built-ins can be called")),
                };
                self.bump();
                let mut args = Vec::new();
                if !self.eat(&TokenKind::RParen) {
                    loop {
                        args.push(self.expr(0, depth + 1)?);
                        if self.eat(&TokenKind::RParen) {
                            break;
                        }
                        self.expect(&TokenKind::Comma, "','")?;
                    }
                }
                lhs = Expr::Call(callee, args);
                continue;
            }
            let Some((op, left_bp, right_bp)) = infix(&self.current().kind) else {
                break;
            };
            if left_bp < min_bp {
                break;
            }
            self.bump();
            let rhs = self.expr(right_bp, depth + 1)?;
            lhs = Expr::Binary(Box::new(lhs), op, Box::new(rhs));
        }
        Ok(lhs)
    }
}
fn infix(k: &TokenKind) -> Option<(Op, u8, u8)> {
    Some(match k {
        TokenKind::Or => (Op::Or, 1, 2),
        TokenKind::And => (Op::And, 3, 4),
        TokenKind::EqEq => (Op::Eq, 5, 6),
        TokenKind::Ne => (Op::Ne, 5, 6),
        TokenKind::Lt => (Op::Lt, 7, 8),
        TokenKind::Le => (Op::Le, 7, 8),
        TokenKind::Gt => (Op::Gt, 7, 8),
        TokenKind::Ge => (Op::Ge, 7, 8),
        TokenKind::Plus => (Op::Add, 9, 10),
        TokenKind::Minus => (Op::Sub, 9, 10),
        TokenKind::Star => (Op::Mul, 11, 12),
        TokenKind::Slash => (Op::Div, 11, 12),
        TokenKind::Percent => (Op::Rem, 11, 12),
        _ => return None,
    })
}

pub fn parse(source: &str) -> Result<Program, Diagnostic> {
    let tokens = lex(source)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        max_nesting: 128,
    };
    parser.parse_program()
}
