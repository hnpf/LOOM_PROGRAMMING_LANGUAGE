use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use tokio::task::JoinHandle;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum Value {
    Int(i64),
    Float(f64),
    Str(String),
    Char(char),
    Bool(bool),
    List(Arc<RwLock<Vec<Value>>>),
    Map(HashMap<Value, Value>),
    Frame(String, Arc<RwLock<HashMap<String, Value>>>),
    Task(Arc<Mutex<Option<JoinHandle<Result<Value, String>>>>>),
    Ok(Box<Value>),
    Err(String),
    Some(Box<Value>),
    ShellOutput { stdout: String, stderr: String, status: i32 },
    None,
    Func(Vec<String>, Box<crate::ast::Expr>, crate::env::Env),
    Builtin(String, Option<Box<Value>>),
    Range(i64, i64),
}

impl std::cmp::PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Int(a), Value::Int(b)) => a == b,
            (Value::Float(a), Value::Float(b)) => a == b,
            (Value::Str(a), Value::Str(b)) => a == b,
            (Value::Char(a), Value::Char(b)) => a == b,
            (Value::Bool(a), Value::Bool(b)) => a == b,
            (Value::List(a), Value::List(b)) => {
                let a_items = match a.read() {
                    Ok(l) => l,
                    Err(_) => return false,
                };
                let b_items = match b.read() {
                    Ok(l) => l,
                    Err(_) => return false,
                };
                *a_items == *b_items
            }
            (Value::Map(a), Value::Map(b)) => a == b,
            (Value::Frame(n1, f1), Value::Frame(n2, f2)) => {
                if n1 != n2 { return false; }
                let f1_fields = match f1.read() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                let f2_fields = match f2.read() {
                    Ok(f) => f,
                    Err(_) => return false,
                };
                *f1_fields == *f2_fields
            }
            (Value::Ok(a), Value::Ok(b)) => a == b,
            (Value::Err(a), Value::Err(b)) => a == b,
            (Value::Some(a), Value::Some(b)) => a == b,
            (Value::ShellOutput { stdout: s1, stderr: e1, status: st1 }, Value::ShellOutput { stdout: s2, stderr: e2, status: st2 }) => {
                s1 == s2 && e1 == e2 && st1 == st2
            }
            (Value::None, Value::None) => true,
            (Value::Range(s1, e1), Value::Range(s2, e2)) => s1 == s2 && e1 == e2,
            _ => false,
        }
    }
}

impl std::cmp::Eq for Value {}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        match self {
            Value::Int(i) => i.hash(state),
            Value::Str(s) => s.hash(state),
            Value::Char(c) => c.hash(state),
            Value::Bool(b) => b.hash(state),
            _ => {} // float and others are trickier to hash
        }
    }
}
