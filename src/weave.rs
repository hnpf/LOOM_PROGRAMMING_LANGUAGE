use crate::ast::{Expr, Literal};
use std::fs;
use std::process::Command;
use std::path::Path;

pub fn weave(ast: Expr, out_file: &str) -> anyhow::Result<()> {
    log::info!("Preparing weave project directory...");
    let weave_dir = Path::new("target/weave_project");
    if weave_dir.exists() {
        fs::remove_dir_all(weave_dir)?;
    }
    fs::create_dir_all(weave_dir.join("src"))?;

    log::debug!("Creating Cargo.toml...");
    let cargo_toml = r#"[package]
name = "weaved_loom"
version = "0.1.0"
edition = "2021"

[dependencies]
tokio = { version = "1", features = ["full"] }
futures = "0.3"
"#;
    fs::write(weave_dir.join("Cargo.toml"), cargo_toml)?;

    let mut defs = String::new();
    let mut main_body = String::new();

    log::debug!("Generating Rust code from AST...");
    // add a robust runtime
    defs.push_str(r#"
use std::sync::{Arc, Mutex};
use std::pin::Pin;
use std::future::Future;

#[derive(Clone, Debug)]
enum LoomResult<T, E> { Ok(T), Err(E) }
#[derive(Clone, Debug)]
enum LoomOption<T> { Some(T), None }

#[derive(Clone, Debug)]
struct LoomTask<T>(Arc<Mutex<Option<tokio::task::JoinHandle<T>>>>);

impl<T: Send + 'static> LoomTask<T> {
    async fn l_await(self) -> LoomResult<T, String> {
        let handle = self.0.lock().unwrap().take();
        if let Some(h) = handle {
            match h.await {
                Ok(v) => LoomResult::Ok(v),
                Err(e) => LoomResult::Err(e.to_string()),
            }
        } else {
            LoomResult::Err("Task already awaited".to_string())
        }
    }
}

#[derive(Clone)]
enum AnyValue {
    Int(i64),
    Float(f64),
    Str(String),
    Bool(bool),
    List(Arc<Mutex<Vec<AnyValue>>>),
    Task(LoomTask<AnyValue>),
    Result(Box<LoomResult<AnyValue, AnyValue>>),
    Option(Box<LoomOption<AnyValue>>),
    ShellOutput { stdout: String, stderr: String, status: i32 },
    Func(Arc<dyn Fn(Vec<AnyValue>) -> Pin<Box<dyn Future<Output = AnyValue> + Send>> + Send + Sync>),
    Frame(String, Arc<Mutex<std::collections::HashMap<String, AnyValue>>>),
    None,
}

impl std::fmt::Debug for AnyValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnyValue::Int(i) => write!(f, "{}", i),
            AnyValue::Float(fl) => write!(f, "{}", fl),
            AnyValue::Str(s) => write!(f, "{}", s),
            AnyValue::Bool(b) => write!(f, "{}", b),
            AnyValue::List(l) => write!(f, "{:?}", l.lock().unwrap()),
            AnyValue::Result(r) => match &**r {
                LoomResult::Ok(v) => write!(f, "Ok({:?})", v),
                LoomResult::Err(e) => write!(f, "Err({:?})", e),
            },
            AnyValue::ShellOutput { stdout, .. } => write!(f, "{}", stdout),
            _ => write!(f, "Value"),
        }
    }
}

impl From<()> for AnyValue { fn from(_: ()) -> Self { AnyValue::None } }
impl From<i64> for AnyValue { fn from(v: i64) -> Self { AnyValue::Int(v) } }
impl From<f64> for AnyValue { fn from(v: f64) -> Self { AnyValue::Float(v) } }
impl From<String> for AnyValue { fn from(v: String) -> Self { AnyValue::Str(v) } }
impl From<bool> for AnyValue { fn from(v: bool) -> Self { AnyValue::Bool(v) } }
impl From<Vec<AnyValue>> for AnyValue { fn from(v: Vec<AnyValue>) -> Self { AnyValue::List(Arc::new(Mutex::new(v))) } }

impl AnyValue {
    fn l_len(&self) -> AnyValue {
        match self {
            AnyValue::List(l) => AnyValue::Int(l.lock().unwrap().len() as i64),
            AnyValue::Str(s) => AnyValue::Int(s.len() as i64),
            _ => AnyValue::Int(0),
        }
    }
    fn l_replace(&self, old: AnyValue, new: AnyValue) -> AnyValue {
        if let (AnyValue::Str(s), AnyValue::Str(o), AnyValue::Str(n)) = (self, old, new) {
            AnyValue::Str(s.replace(&o, &n))
        } else { AnyValue::None }
    }
    fn l_ends_with(&self, suffix: AnyValue) -> AnyValue {
        if let (AnyValue::Str(s), AnyValue::Str(su)) = (self, suffix) {
            AnyValue::Bool(s.ends_with(&su))
        } else { AnyValue::Bool(false) }
    }
    fn l_starts_with(&self, prefix: AnyValue) -> AnyValue {
        if let (AnyValue::Str(s), AnyValue::Str(p)) = (self, prefix) {
            AnyValue::Bool(s.starts_with(&p))
        } else { AnyValue::Bool(false) }
    }
    fn l_split(&self, delim: AnyValue) -> AnyValue {
        if let AnyValue::Str(s) = self {
            let d = if let AnyValue::Str(ds) = delim { ds } else { "".to_string() };
            let parts: Vec<AnyValue> = if d.is_empty() {
                s.chars().map(|c| AnyValue::Str(c.to_string())).collect()
            } else {
                s.split(&d).map(|p| AnyValue::Str(p.to_string())).collect()
            };
            AnyValue::List(Arc::new(Mutex::new(parts)))
        } else { AnyValue::None }
    }
    async fn l_await(self) -> AnyValue {
        if let AnyValue::Task(t) = self {
            match t.l_await().await {
                LoomResult::Ok(v) => AnyValue::Result(Box::new(LoomResult::Ok(v))),
                LoomResult::Err(e) => AnyValue::Result(Box::new(LoomResult::Err(AnyValue::Str(e)))),
            }
        } else { AnyValue::None }
    }
    async fn l_map(self, f: AnyValue) -> AnyValue {
        if let (AnyValue::List(items_lock), AnyValue::Func(func)) = (self, f) {
            let items = items_lock.lock().unwrap().clone();
            let mut new_items = Vec::new();
            for item in items {
                new_items.push(func(vec![item]).await);
            }
            AnyValue::List(Arc::new(Mutex::new(new_items)))
        } else { AnyValue::None }
    }
    fn l_push(&self, item: AnyValue) -> AnyValue {
        if let AnyValue::List(l) = self {
            l.lock().unwrap().push(item.clone());
            item
        } else { AnyValue::None }
    }
    fn get_field(&self, name: &str) -> AnyValue {
        match self {
            AnyValue::Frame(_, fields) => fields.lock().unwrap().get(name).cloned().unwrap_or(AnyValue::None),
            AnyValue::Result(res) => match &**res {
                LoomResult::Ok(v) if name == "Ok" => v.clone(),
                LoomResult::Err(e) if name == "Err" => e.clone(),
                _ => AnyValue::None,
            },
            AnyValue::Option(opt) => match &**opt {
                LoomOption::Some(v) if name == "Some" => v.clone(),
                _ => AnyValue::None,
            },
            AnyValue::ShellOutput { stdout, stderr, status } => {
                match name {
                    "stdout" => AnyValue::Str(stdout.clone()),
                    "stderr" => AnyValue::Str(stderr.clone()),
                    "status" => AnyValue::Int(*status as i64),
                    _ => AnyValue::None,
                }
            }
            _ => AnyValue::None,
        }
    }
    fn set_field(&self, name: &str, val: AnyValue) {
        if let AnyValue::Frame(_, fields) = self {
            fields.lock().unwrap().insert(name.to_string(), val);
        }
    }

    fn loom_add(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Int(a + b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Float(a + b),
            (AnyValue::Str(a), AnyValue::Str(b)) => AnyValue::Str(a + &b),
            (AnyValue::Str(a), AnyValue::Int(b)) => AnyValue::Str(a + &b.to_string()),
            (AnyValue::Int(a), AnyValue::Str(b)) => AnyValue::Str(a.to_string() + &b),
            _ => AnyValue::None,
        }
    }
    fn loom_sub(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Int(a - b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Float(a - b),
            _ => AnyValue::None,
        }
    }
    fn loom_mul(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Int(a * b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Float(a * b),
            _ => AnyValue::None,
        }
    }
    fn loom_div(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Int(a / b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Float(a / b),
            _ => AnyValue::None,
        }
    }
    fn loom_eq(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Bool(a == b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Bool(a == b),
            (AnyValue::Str(a), AnyValue::Str(b)) => AnyValue::Bool(a == b),
            (AnyValue::Bool(a), AnyValue::Bool(b)) => AnyValue::Bool(a == b),
            (AnyValue::None, AnyValue::None) => AnyValue::Bool(true),
            _ => AnyValue::Bool(false),
        }
    }
    fn loom_ne(self, other: AnyValue) -> AnyValue {
        if let AnyValue::Bool(b) = self.loom_eq(other) { AnyValue::Bool(!b) } else { AnyValue::Bool(true) }
    }
    fn loom_lt(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Bool(a < b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Bool(a < b),
            _ => AnyValue::Bool(false),
        }
    }
    fn loom_gt(self, other: AnyValue) -> AnyValue {
        match (self, other) {
            (AnyValue::Int(a), AnyValue::Int(b)) => AnyValue::Bool(a > b),
            (AnyValue::Float(a), AnyValue::Float(b)) => AnyValue::Bool(a > b),
            _ => AnyValue::Bool(false),
        }
    }
}

async fn l_print(args: Vec<AnyValue>) -> AnyValue {
    for (i, v) in args.iter().enumerate() {
        if i > 0 { print!(" "); }
        match v {
            AnyValue::Str(ref s) => print!("{}", s),
            _ => print!("{:?}", v),
        }
    }
    println!();
    AnyValue::None
}
async fn l_print_raw(args: Vec<AnyValue>) -> AnyValue {
    for v in args.iter() {
        match v {
            AnyValue::Str(ref s) => print!("{}", s),
            _ => print!("{:?}", v),
        }
    }
    use std::io::Write;
    std::io::stdout().flush().unwrap();
    AnyValue::None
}
async fn l_time_ms() -> AnyValue {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    AnyValue::Int(now as i64)
}
async fn l_vec(size: AnyValue, val: AnyValue) -> AnyValue {
    if let AnyValue::Int(s) = size {
        let items = vec![val; s as usize];
        AnyValue::List(Arc::new(Mutex::new(items)))
    } else { AnyValue::None }
}
async fn l_Ok(v: AnyValue) -> AnyValue { AnyValue::Result(Box::new(LoomResult::Ok(v))) }
async fn l_Err(v: AnyValue) -> AnyValue { AnyValue::Result(Box::new(LoomResult::Err(v))) }
async fn l_Some(v: AnyValue) -> AnyValue { AnyValue::Option(Box::new(LoomOption::Some(v))) }
async fn l_read(v: AnyValue) -> AnyValue {
    if let AnyValue::Str(s) = v {
        match std::fs::read_to_string(s) {
            Ok(content) => AnyValue::Result(Box::new(LoomResult::Ok(AnyValue::Str(content)))),
            Err(e) => AnyValue::Result(Box::new(LoomResult::Err(AnyValue::Str(e.to_string())))),
        }
    } else { AnyValue::None }
}
async fn l_write(f: AnyValue, c: AnyValue) -> AnyValue {
    if let (AnyValue::Str(filename), AnyValue::Str(content)) = (f, c) {
        match std::fs::write(filename, content) {
            Ok(_) => AnyValue::Result(Box::new(LoomResult::Ok(AnyValue::None))),
            Err(e) => AnyValue::Result(Box::new(LoomResult::Err(AnyValue::Str(e.to_string())))),
        }
    } else { AnyValue::None }
}
async fn l_ls(v: AnyValue) -> AnyValue {
    let path = if let AnyValue::Str(s) = v { s } else { ".".to_string() };
    match std::fs::read_dir(path) {
        Ok(entries) => {
            let mut files = Vec::new();
            for entry in entries {
                if let Ok(e) = entry {
                    if let Some(s) = e.file_name().to_str() {
                        files.push(AnyValue::Str(s.to_string()));
                    }
                }
            }
            AnyValue::List(Arc::new(Mutex::new(files)))
        }
        Err(e) => AnyValue::Result(Box::new(LoomResult::Err(AnyValue::Str(e.to_string())))),
    }
}
async fn l_sleep(v: AnyValue) -> AnyValue {
    if let AnyValue::Int(ms) = v {
        tokio::time::sleep(std::time::Duration::from_millis(ms as u64)).await;
    }
    AnyValue::None
}
async fn l_ord(v: AnyValue) -> AnyValue {
    if let AnyValue::Str(s) = v {
        if let Some(c) = s.chars().next() { AnyValue::Int(c as i64) } else { AnyValue::None }
    } else { AnyValue::None }
}
async fn l_chr(v: AnyValue) -> AnyValue {
    if let AnyValue::Int(i) = v {
        if let Some(c) = char::from_u32(i as u32) { AnyValue::Str(c.to_string()) } else { AnyValue::None }
    } else { AnyValue::None }
}
async fn l_xor(a: AnyValue, b: AnyValue) -> AnyValue {
    if let (AnyValue::Int(av), AnyValue::Int(bv)) = (a, b) {
        AnyValue::Int(av ^ bv)
    } else { AnyValue::None }
}
async fn l_split_st(s: AnyValue, d: AnyValue) -> AnyValue {
    if let AnyValue::Str(sv) = s {
        let dv = if let AnyValue::Str(ds) = d { ds } else { "".to_string() };
        let parts: Vec<AnyValue> = if dv.is_empty() {
            sv.chars().map(|c| AnyValue::Str(c.to_string())).collect()
        } else {
            sv.split(&dv).map(|p| AnyValue::Str(p.to_string())).collect()
        };
        AnyValue::List(Arc::new(Mutex::new(parts)))
    } else { AnyValue::None }
}
"#);

    if let Expr::Block(exprs) = ast {
        for expr in exprs {
            match expr {
                Expr::Frame { name, .. } => {
                    defs.push_str(&format!("// Frame {}\n", name));
                }
                Expr::Bind { name, methods } => {
                    for m in methods {
                        if let Expr::Act { name: Some(m_name), params, body, .. } = m {
                            let mut p_list = Vec::new();
                            for p in params { p_list.push(format!("l_{}: AnyValue", p)); }
                            defs.push_str(&format!("fn l_{}_{}({}) -> Pin<Box<dyn Future<Output = AnyValue> + Send>> {{\n", name, m_name, p_list.join(", ")));
                            defs.push_str(&format!("Box::pin(async move {{ {} }})", gen_expr_async(*body)));
                            defs.push_str("\n}\n");
                        }
                    }
                }
                Expr::Act { name: Some(a_name), params, body, .. } => {
                    let mut p_list = Vec::new();
                    for p in params { p_list.push(format!("l_{}: AnyValue", p)); }
                    let r_name = if a_name == "main" { "loom_main".to_string() } else { format!("l_{}", a_name) };
                    defs.push_str(&format!("fn {}({}) -> Pin<Box<dyn Future<Output = AnyValue> + Send>> {{\n", r_name, p_list.join(", ")));
                    defs.push_str(&format!("Box::pin(async move {{ {} }})", gen_expr_async(*body)));
                    defs.push_str("\n}\n");
                }
                _ => { main_body.push_str(&gen_expr_async(expr)); main_body.push_str(";\n"); }
            }
        }
    }

    let mut rust_code = String::new();
    rust_code.push_str("#[allow(dead_code, unused_variables, unused_mut, non_snake_case, unreachable_code, unused_parens)]\n");
    rust_code.push_str(&defs);
    rust_code.push_str("#[tokio::main] async fn main() {\n");
    rust_code.push_str("let l_args = AnyValue::from(std::env::args().skip(1).map(AnyValue::Str).collect::<Vec<AnyValue>>());\n");
    rust_code.push_str(&main_body);
    rust_code.push_str("}\n");

    fs::write(weave_dir.join("src/main.rs"), rust_code)?;
    log::info!("Compiling weaved code with cargo...");
    let status = Command::new("cargo").arg("build").arg("--release").current_dir(weave_dir).status()?;
    if status.success() {
        let bin = if cfg!(windows) { "target/release/weaved_loom.exe" } else { "target/release/weaved_loom" };
        fs::copy(weave_dir.join(bin), out_file)?;
        log::info!("Successfully weaved to {}", out_file);
    } else { 
        log::error!("Cargo compilation failed.");
        anyhow::bail!("Compilation failed"); 
    }
    Ok(())
}

fn gen_expr_async(expr: Expr) -> String {
    match expr {
        Expr::Literal(l) => match l {
            Literal::Int(i) => format!("AnyValue::Int({})", i),
            Literal::Float(f) => format!("AnyValue::Float({})", f),
            Literal::Str(s) => {
                if s.contains('{') {
                    let mut parts = Vec::new();
                    let mut chars = s.chars().peekable();
                    let mut current_literal = String::new();
                    
                    while let Some(c) = chars.next() {
                        if c == '{' {
                            if let Some(&'{') = chars.peek() {
                                chars.next();
                                current_literal.push('{');
                            } else {
                                if !current_literal.is_empty() {
                                    parts.push(format!("AnyValue::from(\"{}\".to_string())", current_literal));
                                    current_literal.clear();
                                }
                                let mut expr_str = String::new();
                                while let Some(nc) = chars.next() {
                                    if nc == '}' { break; }
                                    expr_str.push(nc);
                                }
                                
                                let is_valid_ident = !expr_str.is_empty() && expr_str.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '.');
                                if is_valid_ident {
                                    if expr_str == "self.label" {
                                        parts.push(format!("l_self.get_field(\"label\")"));
                                    } else {
                                        parts.push(format!("l_{}.clone()", expr_str.replace(".", "_")));
                                    }
                                } else {
                                    parts.push(format!("AnyValue::from(\"{{{{{}}}}}\".to_string())", expr_str));
                                }
                            }
                        } else if c == '}' {
                            if let Some(&'}') = chars.peek() {
                                chars.next();
                                current_literal.push('}');
                            } else {
                                current_literal.push('}');
                            }
                        } else {
                            current_literal.push(c);
                        }
                    }
                    
                    if !current_literal.is_empty() {
                        parts.push(format!("AnyValue::from(\"{}\".to_string())", current_literal));
                    }
                    
                    if parts.is_empty() {
                        "AnyValue::from(\"\".to_string())".to_string()
                    } else {
                        let mut res = parts[0].clone();
                        for p in &parts[1..] {
                            res = format!("({}).loom_add({})", res, p);
                        }
                        res
                    }
                } else {
                    format!("AnyValue::Str(\"{}\".to_string())", s)
                }
            },
            Literal::Bool(b) => format!("AnyValue::Bool({})", b),
            _ => "AnyValue::None".to_string(),
        },
        Expr::Ident(s) => {
            if s == "print" { "l_print".to_string() }
            else if s == "print_raw" { "l_print_raw".to_string() }
            else if s == "time_ms" { "l_time_ms".to_string() }
            else if s == "vec" { "l_vec".to_string() }
            else if s == "input" { "l_input".to_string() }
            else if s == "clear" { "l_clear".to_string() }
            else if s == "choose" { "l_choose".to_string() }
            else if s == "Ok" { "l_Ok".to_string() }
            else if s == "Err" { "l_Err".to_string() }
            else if s == "Some" { "l_Some".to_string() }
            else if s == "read" { "l_read".to_string() }
            else if s == "write" { "l_write".to_string() }
            else if s == "ls" { "l_ls".to_string() }
            else if s == "sleep" { "l_sleep".to_string() }
            else if s == "ord" { "l_ord".to_string() }
            else if s == "chr" { "l_chr".to_string() }
            else if s == "xor" { "l_xor".to_string() }
            else if s == "split" { "l_split_st".to_string() }
            else if s == "None" { "AnyValue::None".to_string() }
            else if s == "main" { "loom_main".to_string() }
            else { format!("l_{}", s) }
        },
        Expr::Call { callee, args } => {
            let s = gen_expr_async(*callee.clone());
            if s == "l_print" || s == "l_print_raw" || s == "l_time_ms" || s == "l_vec" || s == "l_input" || s == "l_clear" || s == "l_choose" || s == "l_Ok" || s == "l_Err" || s == "l_Some" || s == "l_read" || s == "l_write" || s == "l_ls" || s == "l_sleep" || s == "ord" || s == "chr" || s == "xor" || s == "split" {
                if s == "l_print" || s == "l_print_raw" {
                    let mut res = format!("{}(vec![", s);
                    for (i, a) in args.into_iter().enumerate() {
                        if i > 0 { res.push_str(", "); }
                        res.push_str(&format!("({}).clone().into()", gen_expr_async(a)));
                    }
                    res.push_str("]).await");
                    return res;
                }
                let mut res = format!("{}(", s);
                for (i, a) in args.into_iter().enumerate() {
                    if i > 0 { res.push_str(", "); }
                    res.push_str(&format!("({}).clone().into()", gen_expr_async(a)));
                }
                res.push_str(").await");
                return res;
            }
            
            match *callee {
                Expr::FieldAccess { ref obj, ref field } => {
                    let target = gen_expr_async(*obj.clone());
                    if field == "len" { format!("{}.l_len()", target) }
                    else if field == "replace" { format!("{}.l_replace({}, {})", target, gen_expr_async(args[0].clone()), gen_expr_async(args[1].clone())) }
                    else if field == "ends_with" { format!("{}.l_ends_with({})", target, gen_expr_async(args[0].clone())) }
                    else if field == "starts_with" { format!("{}.l_starts_with({})", target, gen_expr_async(args[0].clone())) }
                    else if field == "split" { format!("{}.l_split({})", target, if args.is_empty() { "AnyValue::None".to_string() } else { gen_expr_async(args[0].clone()) }) }
                    else if field == "map" { format!("{}.clone().l_map({}.into()).await", target, gen_expr_async(args[0].clone())) }
                    else if field == "push" { format!("{}.l_push({}.into())", target, gen_expr_async(args[0].clone())) }
                    else {
                        // guess if method call
                        let mut res = format!("l_Task_{}(", field);
                        res.push_str(&format!("{}.clone().into()", target));
                        for a in args {
                            res.push_str(", ");
                            res.push_str(&format!("{}.clone().into()", gen_expr_async(a)));
                        }
                        res.push_str(").await"); res
                    }
                }
                _ => {
                    let mut res = format!("{}(", s);
                    for (i, a) in args.into_iter().enumerate() {
                        if i > 0 { res.push_str(", "); }
                        res.push_str(&format!("{}.clone().into()", gen_expr_async(a)));
                    }
                    res.push_str(").await"); res
                }
            }
        }
        Expr::Block(v) => {
            let mut s = "{\n".to_string();
            let len = v.len();
            for (i, e) in v.iter().enumerate() {
                s.push_str(&gen_expr_async(e.clone()));
                if i < len - 1 {
                    s.push_str(";\n");
                }
            }
            if len == 0 { s.push_str("AnyValue::None"); }
            s.push_str("\n}"); s
        }
        Expr::Let { name, val, .. } => format!("let mut l_{} = AnyValue::from({})", name, gen_expr_async(*val)),
        Expr::List(v) => {
            let mut s = "AnyValue::List(vec![".to_string();
            for (i, e) in v.into_iter().enumerate() { if i > 0 { s.push_str(", "); } s.push_str(&format!("({}).into()", gen_expr_async(e))); }
            s.push_str("])"); s
        }
        Expr::FrameInst { name, fields } => {
            let mut s = format!("AnyValue::Frame(\"{}\".to_string(), Arc::new(Mutex::new(vec![", name);
            for (n, v) in fields { s.push_str(&format!("(\"{}\".to_string(), {}.into()), ", n, gen_expr_async(v))); }
            s.push_str("].into_iter().collect())))"); s
        }
        Expr::Spawn(e) => format!("AnyValue::Task(LoomTask(Arc::new(Mutex::new(Some(tokio::spawn(async move {{ {} }}))))))", gen_expr_async(*e)),
        Expr::Act { name: None, params, body, .. } => {
            let mut p_bindings = Vec::new();
            for (i, p) in params.iter().enumerate() {
                p_bindings.push(format!("let l_{} = args.get({}).cloned().unwrap_or(AnyValue::None);", p, i));
            }
            format!("AnyValue::Func(Arc::new(|args| Box::pin(async move {{ {} {} }})))", p_bindings.join(" "), gen_expr_async(*body))
        }
        Expr::BinOp { left, op, right } => {
            let op_str = match op.as_str() {
                "+" => "loom_add", "-" => "loom_sub", "*" => "loom_mul", "/" => "loom_div",
                "==" => "loom_eq", "!=" => "loom_ne", "<" => "loom_lt", ">" => "loom_gt",
                _ => "loom_add",
            };
            format!("({}.clone()).{}({}.clone())", gen_expr_async(*left), op_str, gen_expr_async(*right))
        },
        Expr::FieldAccess { obj, field } => format!("{}.get_field(\"{}\")", gen_expr_async(*obj), field),
        Expr::FieldAssign { obj, field, val } => format!("{{ let v = AnyValue::from({}); {}.set_field(\"{}\", v.clone()); v }}", gen_expr_async(*val), gen_expr_async(*obj), field),
        Expr::When { val, arms } => {
            let mut s = format!("match ({}).clone() {{\n", gen_expr_async(*val));
            for arm in arms {
                match arm.pattern {
                    crate::ast::Pattern::Variant(ref name, ref bind) => {
                        let b = bind.as_ref().map(|x| format!("l_{}", x)).unwrap_or("_".to_string());
                        if name == "Ok" || name == "Err" {
                            s.push_str(&format!("AnyValue::Result(box_res) if matches!(*box_res, LoomResult::{}(_)) => if let LoomResult::{}(val) = *box_res {{ let {} = val; {} }} else {{ AnyValue::None }},\n", name, name, b, gen_expr_async(arm.body)));
                        } else if name == "Some" {
                             s.push_str(&format!("AnyValue::Option(box_opt) if matches!(*box_opt, LoomOption::Some(_)) => if let LoomOption::Some(val) = *box_opt {{ let {} = val; {} }} else {{ AnyValue::None }},\n", b, gen_expr_async(arm.body)));
                        } else if name == "None" {
                             s.push_str(&format!("AnyValue::Option(box_opt) if matches!(*box_opt, LoomOption::None) => {{ {} }},\n", gen_expr_async(arm.body)));
                        } else { s.push_str("_ => AnyValue::None,\n"); }
                    }
                    crate::ast::Pattern::CatchAll => { s.push_str(&format!("_ => {},\n", gen_expr_async(arm.body))); }
                    _ => { s.push_str("_ => AnyValue::None,\n"); }
                }
            }
            s.push_str("_ => AnyValue::None,\n");
            s.push_str("}"); s
        }
        Expr::Assign { name, val } => {
            format!("{{ l_{} = AnyValue::from({}); l_{}.clone() }}", name, gen_expr_async(*val), name)
        },
        Expr::If { cond, then_branch, else_branch } => {
            let mut s = format!("if let AnyValue::Bool(true) = {} {{\n", gen_expr_async(*cond));
            s.push_str(&gen_expr_async(*then_branch));
            s.push_str("\n} else {\n");
            if let Some(eb) = else_branch {
                s.push_str(&gen_expr_async(*eb));
            } else {
                s.push_str("AnyValue::None");
            }
            s.push_str("\n}"); s
        }
        Expr::While { cond, body } => {
            format!("while let AnyValue::Bool(true) = {} {{ {} ; }} AnyValue::None", gen_expr_async(*cond), gen_expr_async(*body))
        }
        Expr::For { var, iter, body } => {
            format!("if let AnyValue::List(items_lock) = ({}).clone() {{ let items = items_lock.lock().unwrap().clone(); for item in items {{ let l_{} = item; {} ; }} }} AnyValue::None", gen_expr_async(*iter), var, gen_expr_async(*body))
        }
        Expr::Await(e) => format!("({}).clone().l_await().await", gen_expr_async(*e)),
        Expr::IndexAccess { obj, index } => {
            format!("if let AnyValue::List(items_lock) = ({}).clone() {{ if let AnyValue::Int(idx) = ({}) {{ items_lock.lock().unwrap().get(idx as usize).cloned().unwrap_or(AnyValue::None) }} else {{ AnyValue::None }} }} else {{ AnyValue::None }}", gen_expr_async(*obj), gen_expr_async(*index))
        }
        Expr::IndexAssign { obj, index, val } => {
            format!("{{ let v = AnyValue::from({}); if let AnyValue::List(items_lock) = ({}).clone() {{ if let AnyValue::Int(idx) = ({}) {{ let mut items = items_lock.lock().unwrap(); if (idx as usize) < items.len() {{ items[idx as usize] = v.clone(); }} }} }} v }}", gen_expr_async(*val), gen_expr_async(*obj), gen_expr_async(*index))
        }
        Expr::Return(val) => {
            let v = if let Some(e) = val { gen_expr_async(*e) } else { "AnyValue::None".to_string() };
            format!("return {}", v)
        }
        Expr::Break => "break AnyValue::None".to_string(),
        Expr::Shell(expr) => {
            let cmd = gen_expr_async(*expr);
            format!(r#"{{
                let cmd_val = {};
                let cmd_str = match cmd_val {{
                    AnyValue::Str(s) => s,
                    _ => format!("{{:?}}", cmd_val),
                }};
                let output = std::process::Command::new("sh").arg("-c").arg(&cmd_str).output().unwrap();
                AnyValue::ShellOutput {{
                    stdout: String::from_utf8_lossy(&output.stdout).trim().to_string(),
                    stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
                    status: output.status.code().unwrap_or(-1),
                }}
            }}"#, cmd)
        },
        _ => "AnyValue::None".to_string(),
    }
}
