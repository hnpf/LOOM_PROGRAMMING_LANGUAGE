use crate::lexer::Token;
use crate::ast::{Expr, Literal, WhenArm, Pattern};
use crate::error::{LoomError, Result};

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or(&Token::EOF)
    }

    fn peek_n(&self, n: usize) -> &Token {
        self.tokens.get(self.pos + n).unwrap_or(&Token::EOF)
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if tok != Token::EOF {
            self.pos += 1;
        }
        tok
    }

    fn consume(&mut self, expected: Token) -> Result<()> {
        let tok = self.advance();
        if tok != expected {
            Err(LoomError::new(format!("Expected {:?}, got {:?} at pos {}", expected, tok, self.pos)))
        } else {
            Ok(())
        }
    }

    pub fn parse(&mut self) -> Result<Expr> {
        let mut exprs = Vec::new();
        while self.peek() != &Token::EOF {
            exprs.push(self.parse_expr()?);
        }
        Ok(Expr::Block(exprs))
    }

    fn parse_expr(&mut self) -> Result<Expr> {
        self.parse_assignment()
    }

    fn parse_assignment(&mut self) -> Result<Expr> {
        if let Token::Frame = self.peek() {
            self.advance();
            let name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected frame name")); };
            self.consume(Token::LBrace)?;
            let mut fields = Vec::new();
            while self.peek() != &Token::RBrace {
                let field_name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected field name")); };
                self.consume(Token::Colon)?;
                let field_type = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected field type")); };
                fields.push((field_name, field_type));
                if self.peek() == &Token::Comma { self.advance(); }
            }
            self.consume(Token::RBrace)?;
            return Ok(Expr::Frame { name, fields });
        }
        if let Token::Bind = self.peek() {
            self.advance();
            let name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected frame name for bind")); };
            self.consume(Token::LBrace)?;
            let mut methods = Vec::new();
            while self.peek() != &Token::RBrace {
                methods.push(self.parse_expr()?);
            }
            self.consume(Token::RBrace)?;
            return Ok(Expr::Bind { name, methods });
        }
        if let Token::Trait = self.peek() {
            self.advance();
            let name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected trait name")); };
            self.consume(Token::LBrace)?;
            let mut methods = Vec::new();
            while self.peek() != &Token::RBrace {
                if let Token::Act = self.peek() {
                    self.advance();
                    let m_name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected method name in trait")); };
                    self.consume(Token::LParen)?;
                    let mut params = Vec::new();
                    if self.peek() != &Token::RParen {
                        loop {
                            match self.advance() {
                                Token::Ident(p) => params.push(p),
                                Token::Self_ => params.push("self".to_string()),
                                _ => return Err(LoomError::new("Expected identifier or self in params")),
                            }
                            if self.peek() == &Token::Colon { self.advance(); self.parse_type()?; }
                            if self.peek() == &Token::Comma { self.advance(); } else { break; }
                        }
                    }
                    self.consume(Token::RParen)?;
                    let mut return_type = None;
                    if self.peek() == &Token::Arrow {
                        self.advance();
                        if let Token::Ident(t) = self.advance() { return_type = Some(t); }
                    }
                    methods.push((m_name, params, return_type));
                    if self.peek() == &Token::Semicolon { self.advance(); }
                } else {
                    return Err(LoomError::new("Only 'act' is allowed in trait"));
                }
            }
            self.consume(Token::RBrace)?;
            return Ok(Expr::Trait { name, methods });
        }
        if let Token::Weave = self.peek() {
            self.advance();
            let trait_name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected trait name for weave")); };
            if let Token::Ident(i) = self.advance() {
                if i != "into" { return Err(LoomError::new("Expected 'into' after trait name in weave")); }
            } else {
                return Err(LoomError::new("Expected 'into' after trait name in weave"));
            }
            let frame_name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected frame name for weave")); };
            self.consume(Token::LBrace)?;
            let mut methods = Vec::new();
            while self.peek() != &Token::RBrace {
                methods.push(self.parse_expr()?);
            }
            self.consume(Token::RBrace)?;
            return Ok(Expr::Weave { trait_name, frame_name, methods });
        }
        if let Token::Pull = self.peek() {
            self.advance();
            if let Token::Str(s) = self.peek().clone() {
                self.advance();
                return Ok(Expr::Pull(s));
            }
            let mut path = String::new();
            loop {
                if let Token::Ident(s) = self.advance() {
                    path.push_str(&s);
                } else {
                    return Err(LoomError::new("Expected identifier in pull path"));
                }
                if self.peek() == &Token::DoubleColon {
                    self.advance();
                    path.push_str("::");
                } else {
                    break;
                }
            }
            return Ok(Expr::Pull(path));
        }
        if let Token::Safety = self.peek() {
            self.advance();
            self.consume(Token::LParen)?;
            let mut limit = String::new();
            while self.peek() != &Token::RParen {
                match self.advance() {
                    Token::Ident(s) => { limit.push_str(&s); limit.push(' '); }
                    Token::Int(i) => { limit.push_str(&i.to_string()); limit.push(' '); }
                    Token::Colon => { limit.push(':'); limit.push(' '); }
                    _ => return Err(LoomError::new("Unexpected token in safety limit")),
                }
            }
            self.consume(Token::RParen)?;
            let body = Box::new(self.parse_expr()?);
            return Ok(Expr::Safety { limit: limit.trim().to_string(), body });
        }
        if let Token::Let = self.peek() {
            self.advance();
            let name = if let Token::Ident(name) = self.advance() {
                name
            } else {
                return Err(LoomError::new("Expected identifier after let"));
            };
            let type_hint = None;
            if self.peek() == &Token::Colon {
                self.advance();
                self.parse_type()?;
            }
            self.consume(Token::Eq)?;
            let val = Box::new(self.parse_expr()?);
            return Ok(Expr::Let { name, val, type_hint });
        }
        if let Token::Const = self.peek() {
            self.advance();
            let name = if let Token::Ident(name) = self.advance() {
                name
            } else {
                return Err(LoomError::new("Expected identifier after const"));
            };
            let type_hint = None;
            if self.peek() == &Token::Colon {
                self.advance();
                self.parse_type()?;
            }
            self.consume(Token::Eq)?;
            let val = Box::new(self.parse_expr()?);
            return Ok(Expr::Const { name, val, type_hint });
        }
        
        let left = self.parse_range()?;
        match left {
            Expr::Ident(ref name) => {
                if self.peek() == &Token::Eq {
                    self.advance();
                    let val = Box::new(self.parse_expr()?);
                    return Ok(Expr::Assign { name: name.clone(), val });
                }
            }
            Expr::FieldAccess { ref obj, ref field } => {
                if self.peek() == &Token::Eq {
                    self.advance();
                    let val = Box::new(self.parse_expr()?);
                    return Ok(Expr::FieldAssign { obj: obj.clone(), field: field.clone(), val });
                }
            }
            Expr::IndexAccess { ref obj, ref index } => {
                if self.peek() == &Token::Eq {
                    self.advance();
                    let val = Box::new(self.parse_expr()?);
                    return Ok(Expr::IndexAssign { obj: obj.clone(), index: index.clone(), val });
                }
            }
            _ => {}
        }
        Ok(left)
    }

    fn parse_range(&mut self) -> Result<Expr> {
        let left = self.parse_binary_op()?;
        if self.peek() == &Token::DoubleDot {
            self.advance();
            let right = self.parse_binary_op()?;
            return Ok(Expr::Range(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn parse_binary_op(&mut self) -> Result<Expr> {
        self.parse_comparison()
    }

    fn parse_comparison(&mut self) -> Result<Expr> {
        let mut left = self.parse_additive()?;
        while matches!(self.peek(), Token::EqEq | Token::Neq | Token::Lt | Token::Gt | Token::Le | Token::Ge) {
            let op = match self.advance() {
                Token::EqEq => "==".to_string(),
                Token::Neq => "!=".to_string(),
                Token::Lt => "<".to_string(),
                Token::Gt => ">".to_string(),
                Token::Le => "<=".to_string(),
                Token::Ge => ">=".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_additive()?;
            left = Expr::BinOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_additive(&mut self) -> Result<Expr> {
        let mut left = self.parse_multiplicative()?;
        while matches!(self.peek(), Token::Plus | Token::Minus) {
            let op = match self.advance() {
                Token::Plus => "+".to_string(),
                Token::Minus => "-".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_multiplicative()?;
            left = Expr::BinOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr> {
        let mut left = self.parse_call()?;
        while matches!(self.peek(), Token::Star | Token::Slash | Token::Percent) {
            let op = match self.advance() {
                Token::Star => "*".to_string(),
                Token::Slash => "/".to_string(),
                Token::Percent => "%".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_call()?;
            left = Expr::BinOp { left: Box::new(left), op, right: Box::new(right) };
        }
        Ok(left)
    }

    fn parse_call(&mut self) -> Result<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek() {
                Token::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    if self.peek() != &Token::RParen {
                        loop {
                            args.push(self.parse_expr()?);
                            if self.peek() == &Token::Comma {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                    }
                    self.consume(Token::RParen)?;
                    expr = Expr::Call { callee: Box::new(expr), args };
                }
                Token::Dot => {
                    self.advance();
                    if self.peek() == &Token::Await {
                        self.advance();
                        self.consume(Token::LParen)?;
                        self.consume(Token::RParen)?;
                        expr = Expr::Await(Box::new(expr));
                    } else {
                        let field = if let Token::Ident(f) = self.advance() {
                            f
                        } else {
                            return Err(LoomError::new("Expected field name after ."));
                        };
                        expr = Expr::FieldAccess { obj: Box::new(expr), field };
                    }
                }
                Token::LBracket => {
                    self.advance();
                    let index = self.parse_expr()?;
                    self.consume(Token::RBracket)?;
                    expr = Expr::IndexAccess { obj: Box::new(expr), index: Box::new(index) };
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr> {
        match self.advance() {
            Token::Int(i) => Ok(Expr::Literal(Literal::Int(i))),
            Token::Float(f) => Ok(Expr::Literal(Literal::Float(f))),
            Token::Minus => {
                let expr = self.parse_primary()?;
                Ok(Expr::BinOp {
                    left: Box::new(Expr::Literal(Literal::Int(0))),
                    op: "-".to_string(),
                    right: Box::new(expr),
                })
            }
            Token::Str(s) => Ok(Expr::Literal(Literal::Str(s))),
            Token::Char(c) => Ok(Expr::Literal(Literal::Char(c))),
            Token::Bool(b) => Ok(Expr::Literal(Literal::Bool(b))),
            Token::Ident(s) => {
                let is_frame_inst = self.peek() == &Token::LBrace && (
                    matches!(self.peek_n(1), Token::Ident(_)) && self.peek_n(2) == &Token::Colon ||
                    self.peek_n(1) == &Token::RBrace
                );
                
                if is_frame_inst {
                    self.advance();
                    let mut fields = Vec::new();
                    while self.peek() != &Token::RBrace {
                        let field_name = if let Token::Ident(n) = self.advance() { n } else { return Err(LoomError::new("Expected field name in instantiation")); };
                        self.consume(Token::Colon)?;
                        let val = self.parse_expr()?;
                        fields.push((field_name, val));
                        if self.peek() == &Token::Comma { self.advance(); }
                    }
                    self.consume(Token::RBrace)?;
                    Ok(Expr::FrameInst { name: s, fields })
                } else {
                    Ok(Expr::Ident(s))
                }
            }
            Token::Self_ => Ok(Expr::Ident("self".to_string())),
            Token::LBracket => {
                let mut items = Vec::new();
                if self.peek() != &Token::RBracket {
                    loop {
                        items.push(self.parse_expr()?);
                        if self.peek() == &Token::Comma {
                            self.advance();
                            if self.peek() == &Token::RBracket {
                                break;
                            }
                        } else {
                            break;
                        }
                    }
                }
                self.consume(Token::RBracket)?;
                Ok(Expr::List(items))
            }
            Token::LParen => {
                let expr = self.parse_expr()?;
                self.consume(Token::RParen)?;
                Ok(expr)
            }
            Token::LBrace => {
                // Peek ahead to see if it's a map literal (key: value)
                // If the block is empty, it can be an empty block or an empty map. 
                // We'll treat {} as an empty block for now, but { k: v } as a map.
                if self.peek() == &Token::RBrace {
                    self.advance();
                    return Ok(Expr::Block(Vec::new()));
                }

                // Parse the first expression
                let first = self.parse_expr()?;
                if self.peek() == &Token::Colon {
                    self.advance();
                    let first_val = self.parse_expr()?;
                    let mut entries = vec![(first, first_val)];
                    if self.peek() == &Token::Comma { self.advance(); }
                    while self.peek() != &Token::RBrace {
                        let key = self.parse_expr()?;
                        self.consume(Token::Colon)?;
                        let val = self.parse_expr()?;
                        entries.push((key, val));
                        if self.peek() == &Token::Comma { self.advance(); }
                    }
                    self.consume(Token::RBrace)?;
                    Ok(Expr::Map(entries))
                } else {
                    let mut exprs = vec![first];
                    while self.peek() != &Token::RBrace {
                        exprs.push(self.parse_expr()?);
                    }
                    self.consume(Token::RBrace)?;
                    Ok(Expr::Block(exprs))
                }
            }
            Token::Act => {
                let name = if let Token::Ident(n) = self.peek() {
                    let name = n.clone();
                    self.advance();
                    Some(name)
                } else {
                    None
                };
                self.consume(Token::LParen)?;
                let mut params = Vec::new();
                if self.peek() != &Token::RParen {
                    loop {
                        match self.advance() {
                            Token::Ident(p) => params.push(p),
                            Token::Self_ => params.push("self".to_string()),
                            _ => return Err(LoomError::new("Expected identifier or self in params")),
                        }
                        if self.peek() == &Token::Colon {
                            self.advance();
                            self.parse_type()?;
                        }
                        if self.peek() == &Token::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                }
                self.consume(Token::RParen)?;
                let return_type = None;
                if self.peek() == &Token::Arrow {
                    self.advance();
                    self.parse_type()?;
                }
                let body = Box::new(self.parse_expr()?);
                Ok(Expr::Act { name, params, body, return_type })
            }
            Token::If => {
                if self.peek() == &Token::Let {
                    self.advance(); // consume let
                    let pattern = self.parse_pattern()?;
                    self.consume(Token::Eq)?;
                    let val = Box::new(self.parse_expr()?);
                    let then_branch = Box::new(self.parse_expr()?);
                    let mut else_branch = None;
                    if self.peek() == &Token::Else {
                        self.advance();
                        else_branch = Some(Box::new(self.parse_expr()?));
                    }
                    Ok(Expr::IfLet { pattern, val, then_branch, else_branch })
                } else {
                    let cond = Box::new(self.parse_expr()?);
                    let then_branch = Box::new(self.parse_expr()?);
                    let mut else_branch = None;
                    if self.peek() == &Token::Else {
                        self.advance();
                        else_branch = Some(Box::new(self.parse_expr()?));
                    }
                    Ok(Expr::If { cond, then_branch, else_branch })
                }
            }
            Token::While => {
                let cond = Box::new(self.parse_expr()?);
                let body = Box::new(self.parse_expr()?);
                Ok(Expr::While { cond, body })
            }
            Token::Loop => {
                let body = Box::new(self.parse_expr()?);
                Ok(Expr::Loop(body))
            }
            Token::For => {
                let var = if let Token::Ident(v) = self.advance() {
                    v
                } else {
                    return Err(LoomError::new("Expected identifier after for"));
                };
                if let Token::Ident(i) = self.advance() {
                    if i != "in" {
                        return Err(LoomError::new("Expected 'in' after variable in for loop"));
                    }
                } else {
                    return Err(LoomError::new("Expected 'in' after variable in for loop"));
                }
                let iter = Box::new(self.parse_expr()?);
                let body = Box::new(self.parse_expr()?);
                Ok(Expr::For { var, iter, body })
            }
            Token::Spawn => Ok(Expr::Spawn(Box::new(self.parse_expr()?))),
            Token::When => {
                let val = Box::new(self.parse_expr()?);
                self.consume(Token::LBrace)?;
                let mut arms = Vec::new();
                while self.peek() != &Token::RBrace {
                    let pattern = self.parse_pattern()?;
                    self.consume(Token::FatArrow)?;
                    let body = self.parse_expr()?;
                    arms.push(WhenArm { pattern, body });
                    if self.peek() == &Token::Comma {
                        self.advance();
                    }
                }
                self.consume(Token::RBrace)?;
                Ok(Expr::When { val, arms })
            }
            Token::Return => {
                let val = if !matches!(self.peek(), Token::RBrace | Token::EOF | Token::Semicolon | Token::Comma) {
                    Some(Box::new(self.parse_expr()?))
                } else {
                    None
                };
                Ok(Expr::Return(val))
            }
            Token::Break => Ok(Expr::Break),
            Token::Dollar => {
                let cmd = Box::new(self.parse_expr()?);
                Ok(Expr::Shell(cmd))
            }
            Token::Comment(s) => Ok(Expr::TraceComment(s)),
            tok => Err(LoomError::new(format!("Unexpected token in primary: {:?}", tok))),
        }
    }

    fn parse_pattern(&mut self) -> Result<Pattern> {
        match self.advance() {
            Token::Int(i) => {
                if self.peek() == &Token::DoubleDot {
                    self.advance();
                    if let Token::Int(end) = self.advance() {
                        Ok(Pattern::Range(i, end))
                    } else {
                        Err(LoomError::new("Expected int after .."))
                    }
                } else {
                    Ok(Pattern::Literal(Literal::Int(i)))
                }
            }
            Token::Str(s) => Ok(Pattern::Literal(Literal::Str(s))),
            Token::Bool(b) => Ok(Pattern::Literal(Literal::Bool(b))),
            Token::Ident(s) if s == "_" => Ok(Pattern::CatchAll),
            Token::Ident(s) => {
                if self.peek() == &Token::LParen {
                    self.advance();
                    let binding = if let Token::Ident(b) = self.advance() {
                        Some(b)
                    } else {
                        None
                    };
                    self.consume(Token::RParen)?;
                    Ok(Pattern::Variant(s, binding))
                } else {
                    Ok(Pattern::Variant(s, None))
                }
            }
            tok => Err(LoomError::new(format!("Unexpected token in pattern: {:?}", tok))),
        }
    }

    fn parse_type(&mut self) -> Result<()> {
        if self.peek() == &Token::Tilde {
            self.advance();
        }
        match self.advance() {
            Token::Ident(_) | Token::Self_ => {
                if self.peek() == &Token::Lt {
                    self.advance();
                    loop {
                        self.parse_type()?;
                        if self.peek() == &Token::Comma {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    self.consume(Token::Gt)?;
                }
            }
            _ => {} // lock in
        }
        Ok(())
    }
}
