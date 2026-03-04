#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Expr {
    Literal(Literal),
    Ident(String),
    Block(Vec<Expr>),
    Let { name: String, val: Box<Expr>, type_hint: Option<String> },
    Const { name: String, val: Box<Expr>, type_hint: Option<String> },
    Act { name: Option<String>, params: Vec<String>, body: Box<Expr>, return_type: Option<String> },
    Call { callee: Box<Expr>, args: Vec<Expr> },
    If { cond: Box<Expr>, then_branch: Box<Expr>, else_branch: Option<Box<Expr>> },
    IfLet { pattern: Pattern, val: Box<Expr>, then_branch: Box<Expr>, else_branch: Option<Box<Expr>> },
    While { cond: Box<Expr>, body: Box<Expr> },
    Loop(Box<Expr>),
    For { var: String, iter: Box<Expr>, body: Box<Expr> },
    When { val: Box<Expr>, arms: Vec<WhenArm> },
    Spawn(Box<Expr>),
    BinOp { left: Box<Expr>, op: String, right: Box<Expr> },
    Return(Option<Box<Expr>>),
    Break,
    Assign { name: String, val: Box<Expr> },
    Frame { name: String, fields: Vec<(String, String)> },
    FrameInst { name: String, fields: Vec<(String, Expr)> },
    Bind { name: String, methods: Vec<Expr> }, // methods are Act exprs
    FieldAccess { obj: Box<Expr>, field: String },
    FieldAssign { obj: Box<Expr>, field: String, val: Box<Expr> },
    Await(Box<Expr>),
    List(Vec<Expr>),
    IndexAccess { obj: Box<Expr>, index: Box<Expr> },
    IndexAssign { obj: Box<Expr>, index: Box<Expr>, val: Box<Expr> },
    Shell(Box<Expr>),
    Map(Vec<(Expr, Expr)>),
    Pull(String),
    Trait { name: String, methods: Vec<(String, Vec<String>, Option<String>)> }, // method name, params, return type
    Weave { trait_name: String, frame_name: String, methods: Vec<Expr> }, // methods are Act exprs
    Range(Box<Expr>, Box<Expr>),
    Safety { limit: String, body: Box<Expr> },
    TraceComment(String),
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Literal {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    None,
}

#[derive(Debug, Clone)]
pub struct WhenArm {
    pub pattern: Pattern,
    pub body: Expr,
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Pattern {
    Literal(Literal),
    Range(i64, i64),
    Variant(String, Option<String>), // variant name w optional binding
    CatchAll,
}
