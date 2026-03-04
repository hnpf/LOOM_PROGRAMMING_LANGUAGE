use crate::error::{LoomError, Result};

#[derive(Debug, PartialEq, Clone)]
pub enum Token {
    Act, Bind, Weave, Pull, When, Spawn, Trait, Frame, Loop, While, For, Let, Const, Return, Break, Self_, Await,
    If, Else, Safety,
    Ident(String),
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    Plus, Minus, Star, Slash, Percent, Eq, EqEq, Neq, Lt, Gt, Le, Ge,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket,
    Comma, Colon, DoubleColon, Arrow, FatArrow, DoubleDot, Semicolon, Dot, Dollar, Tilde,
    Comment(String),
    EOF,
}

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    col: usize,
    pub verbose: bool,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Self { input, pos: 0, line: 1, col: 1, verbose: false }
    }

    pub fn with_verbose(input: &'a str, verbose: bool) -> Self {
        Self { input, pos: 0, line: 1, col: 1, verbose }
    }

    fn peek(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn advance(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        if c == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        Some(c)
    }

    fn consume_char(&mut self, expected: char) -> Result<()> {
        if self.peek() == Some(expected) {
            self.advance();
            Ok(())
        } else {
            Err(LoomError::new(format!("Expected character '{}'", expected)).with_location(self.line, self.col))
        }
    }

    pub fn next_token(&mut self) -> Result<Token> {
        self.skip_whitespace();
        let c = match self.advance() {
            Some(c) => c,
            None => return Ok(Token::EOF),
        };

        match c {
            '(' => Ok(Token::LParen),
            ')' => Ok(Token::RParen),
            '{' => Ok(Token::LBrace),
            '}' => Ok(Token::RBrace),
            '[' => Ok(Token::LBracket),
            ']' => Ok(Token::RBracket),
            ',' => Ok(Token::Comma),
            ':' => {
                if self.peek() == Some(':') {
                    self.advance();
                    Ok(Token::DoubleColon)
                } else {
                    Ok(Token::Colon)
                }
            }
            ';' => Ok(Token::Semicolon),
            '.' => {
                if self.peek() == Some('.') {
                    self.advance();
                    Ok(Token::DoubleDot)
                } else {
                    Ok(Token::Dot)
                }
            }
            '+' => Ok(Token::Plus),
            '-' => {
                if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token::Arrow)
                } else {
                    Ok(Token::Minus)
                }
            }
            '*' => Ok(Token::Star),
            '%' => Ok(Token::Percent),
            '/' => {
                if self.peek() == Some('/') {
                    self.advance();
                    if self.peek() == Some('?') {
                        self.advance();
                        let start = self.pos;
                        while let Some(nc) = self.peek() {
                            if nc == '\n' { break; }
                            self.advance();
                        }
                        let comment = self.input[start..self.pos].to_string();
                        Ok(Token::Comment(comment))
                    } else {
                        self.skip_comment();
                        self.next_token()
                    }
                } else {
                    Ok(Token::Slash)
                }
            }
            '=' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::EqEq)
                } else if self.peek() == Some('>') {
                    self.advance();
                    Ok(Token::FatArrow)
                } else {
                    Ok(Token::Eq)
                }
            }
            '!' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Neq)
                } else {
                    Err(LoomError::new("Unexpected token '!'").with_location(self.line, self.col))
                }
            }
            '<' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Le)
                } else {
                    Ok(Token::Lt)
                }
            }
            '>' => {
                if self.peek() == Some('=') {
                    self.advance();
                    Ok(Token::Ge)
                } else {
                    Ok(Token::Gt)
                }
            }
            '~' => Ok(Token::Tilde),
            '$' => Ok(Token::Dollar),
            '"' => Ok(self.read_string()),
            '\'' => {
                let c = self.advance().ok_or_else(|| LoomError::new("Unexpected EOF in char literal").with_location(self.line, self.col))?;
                self.consume_char('\'')?;
                Ok(Token::Char(c))
            }
            c if c.is_alphabetic() || c == '_' => Ok(self.read_ident(c)),
            c if c.is_digit(10) => Ok(self.read_number(c)),
            _ => Err(LoomError::new(format!("Unexpected character: '{}'", c)).with_location(self.line, self.col)),
        }
    }

    fn skip_whitespace(&mut self) {
        while let Some(c) = self.peek() {
            if c.is_whitespace() {
                self.advance();
            } else {
                break;
            }
        }
    }

    fn skip_comment(&mut self) {
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == '\n' {
                break;
            }
            self.advance();
        }
        let comment = &self.input[start..self.pos];
        if self.verbose {
            if comment.starts_with('*') {
                println!("[LOOM] Section: {}", comment[1..].trim());
            } else if comment.starts_with('!') {
                println!("[LOOM] URGENT: {}", comment[1..].trim());
            } else if comment.starts_with('?') {
                println!("[LOOM] Debug: {}", comment[1..].trim());
            } else if comment.starts_with('.') {
                println!("[LOOM] Metadata: {}", comment[1..].trim());
            } else if comment.starts_with(';') {
                println!("[LOOM] Legal: {}", comment[1..].trim());
            }
        }
    }

    fn read_ident(&mut self, first: char) -> Token {
        let mut ident = String::from(first);
        while let Some(c) = self.peek() {
            if c.is_alphanumeric() || c == '_' {
                ident.push(self.advance().unwrap());
            } else {
                break;
            }
        }

        match ident.as_str() {
            "act" => Token::Act,
            "bind" => Token::Bind,
            "weave" => Token::Weave,
            "pull" => Token::Pull,
            "when" => Token::When,
            "spawn" => Token::Spawn,
            "trait" => Token::Trait,
            "frame" => Token::Frame,
            "loop" => Token::Loop,
            "while" => Token::While,
            "for" => Token::For,
            "let" => Token::Let,
            "const" => Token::Const,
            "return" => Token::Return,
            "break" => Token::Break,
            "self" => Token::Self_,
            "await" => Token::Await,
            "if" => Token::If,
            "else" => Token::Else,
            "safety" => Token::Safety,
            "true" => Token::Bool(true),
            "false" => Token::Bool(false),
            _ => Token::Ident(ident),
        }
    }

    fn read_number(&mut self, first: char) -> Token {
        let mut num = String::from(first);
        let mut is_float = false;
        while let Some(c) = self.peek() {
            if c.is_digit(10) {
                num.push(self.advance().unwrap());
            } else if c == '.' && !is_float {
                // check if next is also a dot (range operator ..)
                let saved_pos = self.pos;
                self.advance();
                if self.peek() == Some('.') {
                    self.pos = saved_pos;
                    break;
                }
                is_float = true;
                num.push('.');
            } else {
                break;
            }
        }

        if is_float {
            Token::Float(num.parse().unwrap())
        } else {
            Token::Int(num.parse().unwrap())
        }
    }

    fn read_string(&mut self) -> Token {
        let mut s = String::new();
        while let Some(c) = self.advance() {
            if c == '"' {
                break;
            }
            if c == '\\' {
                if let Some(next) = self.advance() {
                    match next {
                        'n' => s.push('\n'),
                        'r' => s.push('\r'),
                        't' => s.push('\t'),
                        '\\' => s.push('\\'),
                        '"' => s.push('"'),
                        _ => {
                            s.push('\\');
                            s.push(next);
                        }
                    }
                }
            } else {
                s.push(c);
            }
        }
        Token::Str(s)
    }

    pub fn tokenize(&mut self) -> Result<Vec<Token>> {
        let mut tokens = Vec::new();
        loop {
            let tok = self.next_token()?;
            if tok == Token::EOF {
                break;
            }
            tokens.push(tok);
        }
        Ok(tokens)
    }
}
