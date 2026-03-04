use crate::ast::{Expr, Literal, Pattern};
use crate::value::Value;
use crate::env::Env;
use std::sync::{Arc, Mutex, RwLock};
use std::collections::HashMap;
use tokio::task;
use tokio::io::{self, AsyncBufReadExt};
use async_recursion::async_recursion;
use serde_json;

pub enum ControlFlow {
    Value(Value),
    Return(Value),
    Break,
}

#[async_recursion]
pub async fn eval(expr: Expr, env: &Env) -> Result<Value, String> {
    match eval_internal(expr, env).await? {
        ControlFlow::Value(v) => Ok(v),
        ControlFlow::Return(v) => Ok(v),
        ControlFlow::Break => Ok(Value::None),
    }
}

#[async_recursion]
async fn eval_internal(expr: Expr, env: &Env) -> Result<ControlFlow, String> {
    match expr {
        Expr::Literal(lit) => match lit {
            Literal::Int(i) => Ok(ControlFlow::Value(Value::Int(i))),
            Literal::Float(f) => Ok(ControlFlow::Value(Value::Float(f))),
            Literal::Str(s) => {
                let mut result = String::new();
                let mut chars = s.chars().peekable();
                
                while let Some(c) = chars.next() {
                    if c == '{' {
                        if let Some(&'{') = chars.peek() {
                            chars.next(); // consume second '{'
                            result.push('{');
                        } else {
                            // try to find matching '}'
                            let mut expr_str = String::new();
                            let mut found_end = false;
                            while let Some(nc) = chars.next() {
                                if nc == '}' {
                                    found_end = true;
                                    break;
                                }
                                expr_str.push(nc);
                            }
                            
                            if found_end {
                                let mut interpolated = false;
                                
                                // Try to resolve the expression within braces
                                // For now we support: {ident}, {obj.field}, {obj.method()}
                                if let Some(val) = env.get(&expr_str) {
                                    result.push_str(&value_to_string(&val));
                                    interpolated = true;
                                } else if expr_str.contains('.') {
                                    let parts: Vec<&str> = expr_str.split('.').collect();
                                    if parts.len() == 2 {
                                        let field_name = if parts[1].ends_with("()") {
                                            &parts[1][..parts[1].len()-2]
                                        } else {
                                            parts[1]
                                        };
                                        
                                        if let Some(obj) = env.get(parts[0]) {
                                            // Create a temporary FieldAccess expression to reuse eval_internal
                                            let _fa_expr = Expr::FieldAccess { 
                                                obj: Box::new(Expr::Literal(Literal::None)), // Dummy, we use the value directly
                                                field: field_name.to_string() 
                                            };
                                            
                                            // We need a way to evaluate this field access on an existing Value
                                            // For now, let's just do it manually for common types or 
                                            // refactor eval_internal to take a Value for FieldAccess
                                            
                                            match obj {
                                                Value::Frame(_frame_name, fields_lock) => {
                                                    let fields = fields_lock.read().unwrap();
                                                    if let Some(f_val) = fields.get(field_name) {
                                                        result.push_str(&value_to_string(f_val));
                                                        interpolated = true;
                                                    }
                                                },
                                                Value::Str(s) => {
                                                    if field_name == "len" {
                                                        result.push_str(&s.len().to_string());
                                                        interpolated = true;
                                                    }
                                                },
                                                Value::List(l) => {
                                                    if field_name == "len" {
                                                        result.push_str(&l.read().unwrap().len().to_string());
                                                        interpolated = true;
                                                    }
                                                },
                                                Value::Map(m) => {
                                                    if field_name == "len" {
                                                        result.push_str(&m.len().to_string());
                                                        interpolated = true;
                                                    } else if let Some(v) = m.get(&Value::Str(field_name.to_string())) {
                                                        result.push_str(&value_to_string(v));
                                                        interpolated = true;
                                                    }
                                                },
                                                Value::ShellOutput { ref stdout, ref stderr, status } => {
                                                    match field_name {
                                                        "stdout" => { result.push_str(stdout); interpolated = true; }
                                                        "stderr" => { result.push_str(stderr); interpolated = true; }
                                                        "status" => { result.push_str(&status.to_string()); interpolated = true; }
                                                        _ => {}
                                                    }
                                                },
                                                _ => {}
                                            }
                                        }
                                    }
                                }
                                
                                if !interpolated {
                                    result.push('{');
                                    result.push_str(&expr_str);
                                    result.push('}');
                                }
                            } else {
                                result.push('{');
                                result.push_str(&expr_str);
                            }
                        }
                    } else if c == '}' {
                        if let Some(&'}') = chars.peek() {
                            chars.next(); // consume second '}'
                            result.push('}');
                        } else {
                            result.push('}');
                        }
                    } else {
                        result.push(c);
                    }
                }
                Ok(ControlFlow::Value(Value::Str(result)))
            },
            Literal::Char(c) => Ok(ControlFlow::Value(Value::Char(c))),
            Literal::Bool(b) => Ok(ControlFlow::Value(Value::Bool(b))),
            Literal::None => Ok(ControlFlow::Value(Value::None)),
        },
        Expr::Ident(name) => {
            if let Some(val) = env.get(&name) {
                Ok(ControlFlow::Value(val))
            } else if name == "print" || name == "print_raw" || name == "time_ms" || name == "vec" || name == "input" || name == "clear" || name == "choose" || name == "Ok" || name == "Err" || name == "Some" || name == "read" || name == "write" || name == "ls" || name == "sleep" || name == "ord" || name == "chr" || name == "xor" || name == "split" || name == "env" || name == "set_env" || name == "exists" || name == "is_dir" || name == "is_file" || name == "json" || name == "str" || name == "int" || name == "float" || name == "bool" || name == "net" {
                Ok(ControlFlow::Value(Value::Builtin(name, None)))
            } else if name == "None" || name == "_" {
                Ok(ControlFlow::Value(Value::None))
            } else {
                Err(format!("Undefined variable: {}", name))
            }
        }
        Expr::Block(exprs) => {
            let mut last_val = Value::None;
            for e in exprs {
                match eval_internal(e, env).await? {
                    ControlFlow::Value(v) => last_val = v,
                    cf => return Ok(cf),
                }
            }
            Ok(ControlFlow::Value(last_val))
        }
        Expr::Let { name, val, .. } => {
            log::trace!("Evaluating let binding: {}", name);
            match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => {
                    log::debug!("Let binding: {} = {:?}", name, v);
                    env.define(name, v.clone());
                    Ok(ControlFlow::Value(v))
                }
                cf => Ok(cf),
            }
        }
        Expr::Const { name, val, .. } => {
            log::trace!("Evaluating const binding: {}", name);
            match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => {
                    log::debug!("Const binding: {} = {:?}", name, v);
                    env.define(name, v.clone());
                    Ok(ControlFlow::Value(v))
                }
                cf => Ok(cf),
            }
        }
        Expr::BinOp { left, op, right } => {
            let l = match eval_internal(*left, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            let r = match eval_internal(*right, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            log::trace!("BinOp: {:?} {} {:?}", l, op, r);
            match (l.clone(), op.as_str(), r.clone()) {
                (Value::Int(a), "+", Value::Int(b)) => Ok(ControlFlow::Value(Value::Int(a + b))),
                (Value::Int(a), "-", Value::Int(b)) => Ok(ControlFlow::Value(Value::Int(a - b))),
                (Value::Int(a), "*", Value::Int(b)) => Ok(ControlFlow::Value(Value::Int(a * b))),
                (Value::Int(a), "/", Value::Int(b)) => Ok(ControlFlow::Value(Value::Int(a / b))),
                (Value::Int(a), "%", Value::Int(b)) => Ok(ControlFlow::Value(Value::Int(a % b))),
                (Value::Int(a), "==", Value::Int(b)) => Ok(ControlFlow::Value(Value::Bool(a == b))),
                (Value::Int(a), "!=", Value::Int(b)) => Ok(ControlFlow::Value(Value::Bool(a != b))),
                (Value::Str(a), "==", Value::Str(b)) => Ok(ControlFlow::Value(Value::Bool(a == b))),
                (Value::Str(a), "!=", Value::Str(b)) => Ok(ControlFlow::Value(Value::Bool(a != b))),
                (Value::Bool(a), "==", Value::Bool(b)) => Ok(ControlFlow::Value(Value::Bool(a == b))),
                (Value::Bool(a), "!=", Value::Bool(b)) => Ok(ControlFlow::Value(Value::Bool(a != b))),
                (Value::None, "==", Value::None) => Ok(ControlFlow::Value(Value::Bool(true))),
                (Value::None, "!=", Value::None) => Ok(ControlFlow::Value(Value::Bool(false))),
                (Value::Int(a), "<", Value::Int(b)) => Ok(ControlFlow::Value(Value::Bool(a < b))),
                (Value::Int(a), ">", Value::Int(b)) => Ok(ControlFlow::Value(Value::Bool(a > b))),
                (Value::Str(a), "+", r_val) => {
                    let r_str = match r_val {
                        Value::Int(i) => i.to_string(),
                        Value::Float(f) => f.to_string(),
                        Value::Str(s) => s,
                        Value::Bool(b) => b.to_string(),
                        _ => format!("{:?}", r_val),
                    };
                    Ok(ControlFlow::Value(Value::Str(format!("{}{}", a, r_str))))
                }
                (l_val, "+", Value::Str(b)) => {
                    let l_str = match l_val {
                        Value::Int(i) => i.to_string(),
                        Value::Float(f) => f.to_string(),
                        Value::Bool(b) => b.to_string(),
                        _ => format!("{:?}", l_val),
                    };
                    Ok(ControlFlow::Value(Value::Str(format!("{}{}", l_str, b))))
                }
                _ => Err(format!("Unsupported binary operation: {:?} {} {:?}", l, op, r)),
            }
        }
        Expr::If { cond, then_branch, else_branch } => {
            let c = match eval_internal(*cond, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            if let Value::Bool(true) = c {
                eval_internal(*then_branch, env).await
            } else if let Some(eb) = else_branch {
                eval_internal(*eb, env).await
            } else {
                Ok(ControlFlow::Value(Value::None))
            }
        }
        Expr::IfLet { pattern, val, then_branch, else_branch } => {
            let v = match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            if let Some(bindings) = match_pattern(&v, &pattern) {
                let if_env = Env::with_parent(env.clone(), false);
                for (name, val) in bindings {
                    if_env.set(name, val);
                }
                eval_internal(*then_branch, &if_env).await
            } else if let Some(eb) = else_branch {
                eval_internal(*eb, env).await
            } else {
                Ok(ControlFlow::Value(Value::None))
            }
        }
        Expr::Act { name, params, body, .. } => {
            let func = Value::Func(params, body, env.clone());
            if let Some(n) = name {
                if env.verbose {
                    println!("[LOOM] Defined act: {}", n);
                }
                env.set(n, func.clone());
            }
            Ok(ControlFlow::Value(func))
        }
        Expr::Call { callee, args } => {
            if env.verbose {
                // Ffind a name for callee if possible
                if let Expr::Ident(ref name) = *callee {
                    println!("[LOOM] Calling act: {}", name);
                }
            }
            let func = match eval_internal(*callee, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            match func {
                Value::Func(params, body, closure_env) => {
                    let call_env = Env::with_parent(closure_env, true);
                    for (param, arg) in params.into_iter().zip(args) {
                        let arg_val = match eval_internal(arg, env).await? {
                            ControlFlow::Value(v) => v,
                            cf => return Ok(cf),
                        };
                        call_env.set(param, arg_val);
                    }
                    match eval_internal(*body, &call_env).await? {
                        ControlFlow::Return(v) => Ok(ControlFlow::Value(v)),
                        cf => Ok(cf),
                    }
                }
                Value::Builtin(name, bound) => {
                    let mut arg_vals = Vec::new();
                    for arg in args {
                        match eval_internal(arg, env).await? {
                            ControlFlow::Value(v) => arg_vals.push(v),
                            cf => return Ok(cf),
                        }
                    }
                    if env.verbose {
                        println!("[LOOM] Calling builtin: {}", name);
                    }
                    match name.as_str() {
                        "choose" => {
                            let prompt = match arg_vals.get(0) {
                                Some(Value::Str(p)) => p.clone(),
                                _ => return Err("choose() requires a prompt string".to_string()),
                            };
                            let options_val = match arg_vals.get(1) {
                                Some(v) => v,
                                _ => return Err("choose() requires a list of options".to_string()),
                            };
                            
                            let options = match options_val {
                                Value::List(items_lock) => {
                                    match items_lock.read() {
                                        Ok(items) => items.clone(),
                                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                                    }
                                }
                                _ => return Err("choose() requires a list of options".to_string()),
                            };
                            
                            use std::io::Write;
                            loop {
                                println!("{}", prompt);
                                for (i, opt) in options.iter().enumerate() {
                                    match opt {
                                        Value::Str(s) => println!("{}. {}", i + 1, s),
                                        _ => println!("{}. {:?}", i + 1, opt),
                                    }
                                }
                                print!("> ");
                                let _ = std::io::stdout().flush();
                                
                                let stdin = io::stdin();
                                let mut reader = io::BufReader::new(stdin);
                                let mut line = String::new();
                                match reader.read_line(&mut line).await {
                                    Ok(_) => {
                                        let input = line.trim();
                                        if let Ok(idx) = input.parse::<usize>() {
                                            if idx > 0 && idx <= options.len() {
                                                return Ok(ControlFlow::Value(options[idx - 1].clone()));
                                            }
                                        }
                                        println!("Invalid selection. Please try again.");
                                    }
                                    Err(e) => return Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                                }
                            }
                        }
                        "clear" => {
                            print!("\x1B[2J\x1B[1;1H");
                            use std::io::Write;
                            let _ = std::io::stdout().flush();
                            Ok(ControlFlow::Value(Value::None))
                        }
                        "vec" => {
                            let size = match arg_vals.get(0) {
                                Some(Value::Int(i)) => *i as usize,
                                _ => return Err("vec() requires a size integer".to_string()),
                            };
                            let val = arg_vals.get(1).cloned().unwrap_or(Value::None);
                            let items = vec![val; size];
                            Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(items)))))
                        }
                        "time_ms" => {
                            let now = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap()
                                .as_millis();
                            Ok(ControlFlow::Value(Value::Int(now as i64)))
                        }
                        "input" => {
                            if let Some(Value::Str(prompt)) = arg_vals.get(0) {
                                use std::io::Write;
                                print!("{}", prompt);
                                let _ = std::io::stdout().flush();
                            }
                            let stdin = io::stdin();
                            let mut reader = io::BufReader::new(stdin);
                            let mut line = String::new();
                            match reader.read_line(&mut line).await {
                                Ok(_) => {
                                    if line.ends_with('\n') { line.pop(); }
                                    if line.ends_with('\r') { line.pop(); }
                                    Ok(ControlFlow::Value(Value::Str(line)))
                                }
                                Err(e) => Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                            }
                        }
                        "print" => {
                            for (i, val) in arg_vals.iter().enumerate() {
                                if i > 0 { print!(" "); }
                                match val {
                                    Value::Int(i) => print!("{}", i),
                                    Value::Float(f) => print!("{}", f),
                                    Value::Str(s) => print!("{}", s),
                                    Value::Bool(b) => print!("{}", b),
                                    _ => print!("{:?}", val),
                                }
                            }
                            println!();
                            Ok(ControlFlow::Value(Value::None))
                        }
                        "print_raw" => {
                            for val in arg_vals.iter() {
                                match val {
                                    Value::Int(i) => print!("{}", i),
                                    Value::Float(f) => print!("{}", f),
                                    Value::Str(s) => print!("{}", s),
                                    Value::Bool(b) => print!("{}", b),
                                    _ => print!("{:?}", val),
                                }
                            }
                            use std::io::Write;
                            std::io::stdout().flush().unwrap();
                            Ok(ControlFlow::Value(Value::None))
                        }
                        "Ok" => Ok(ControlFlow::Value(Value::Ok(Box::new(arg_vals.get(0).cloned().unwrap_or(Value::None))))),
                        "Err" => {
                            let msg = match arg_vals.get(0) {
                                Some(Value::Str(s)) => s.clone(),
                                _ => "Unknown error".to_string(),
                            };
                            Ok(ControlFlow::Value(Value::Err(msg)))
                        } // AGHAGGBAHBHAANANAHAHAHAHFHAHFAFAFFAFNFANFGNVNVNVNVCNXAHBAHCHASH
                        "Some" => Ok(ControlFlow::Value(Value::Some(Box::new(arg_vals.get(0).cloned().unwrap_or(Value::None))))),
                        "len" => {
                            if let Some(bound_val) = bound {
                                if let Value::List(ref items_lock) = *bound_val {
                                    let items = match items_lock.read() {
                                        Ok(i) => i,
                                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                                    };
                                    Ok(ControlFlow::Value(Value::Int(items.len() as i64)))
                                } else if let Value::Str(ref s) = *bound_val {
                                    Ok(ControlFlow::Value(Value::Int(s.len() as i64)))
                                } else if let Value::Map(ref map) = *bound_val {
                                    Ok(ControlFlow::Value(Value::Int(map.len() as i64)))
                                } else {
                                    Err(format!("len() called on invalid type"))
                                }
                            } else {
                                Err(format!("len() called on nothing"))
                            }
                        }
                        "replace" => {
                            if let (Some(bound_box), Some(Value::Str(old)), Some(Value::Str(new))) = (&bound, arg_vals.get(0), arg_vals.get(1)) {
                                if let Value::Str(s) = &**bound_box {
                                    return Ok(ControlFlow::Value(Value::Str(s.replace(old, new))));
                                }
                            }
                            Err("replace() requires string bound value and two string arguments".to_string())
                        }
                        "ends_with" => {
                            if let (Some(bound_box), Some(Value::Str(suffix))) = (&bound, arg_vals.get(0)) {
                                if let Value::Str(s) = &**bound_box {
                                    return Ok(ControlFlow::Value(Value::Bool(s.ends_with(suffix))));
                                }
                            }
                            Err("ends_with() requires string bound value and one string argument".to_string())
                        }
                        "starts_with" => {
                            if let (Some(bound_box), Some(Value::Str(prefix))) = (&bound, arg_vals.get(0)) {
                                if let Value::Str(s) = &**bound_box {
                                    return Ok(ControlFlow::Value(Value::Bool(s.starts_with(prefix))));
                                }
                            }
                            Err("starts_with() requires string bound value and one string argument".to_string())
                        }
                        "map" => {
                            if let Some(bound_val) = bound {
                                if let Value::List(ref items_lock) = *bound_val {
                                    let f = arg_vals.get(0).ok_or("map() requires a function")?.clone();
                                    let mut new_items = Vec::new();
                                    let items: Vec<Value> = match items_lock.read() {
                                        Ok(i) => i.clone(),
                                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                                    };
                                    for item in items {
                                        match f.clone() {
                                            Value::Func(params, body, closure_env) => {
                                                let call_env = Env::with_parent(closure_env, true);
                                                if let Some(param) = params.get(0) {
                                                    call_env.set(param.clone(), item.clone());
                                                }
                                                match eval_internal(*body, &call_env).await? {
                                                    ControlFlow::Value(v) => new_items.push(v),
                                                    ControlFlow::Return(v) => new_items.push(v),
                                                    _ => new_items.push(Value::None),
                                                }
                                            }
                                            _ => return Err("map() argument must be a function".to_string()),
                                        }
                                    }
                                    Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(new_items)))))
                                } else {
                                    Err(format!("map() called on non-list"))
                                }
                            } else {
                                Err(format!("map() called on nothing"))
                            }
                        }
                        "filter" => {
                            if let Some(bound_val) = bound {
                                if let Value::List(ref items_lock) = *bound_val {
                                    let f = arg_vals.get(0).ok_or("filter() requires a function")?.clone();
                                    let mut new_items = Vec::new();
                                    let items: Vec<Value> = match items_lock.read() {
                                        Ok(i) => i.clone(),
                                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                                    };
                                    for item in items {
                                        match f.clone() {
                                            Value::Func(params, body, closure_env) => {
                                                let call_env = Env::with_parent(closure_env, true);
                                                if let Some(param) = params.get(0) {
                                                    call_env.set(param.clone(), item.clone());
                                                }
                                                match eval_internal(*body, &call_env).await? {
                                                    ControlFlow::Value(Value::Bool(true)) | ControlFlow::Return(Value::Bool(true)) => new_items.push(item),
                                                    _ => {}
                                                }
                                            }
                                            _ => return Err("filter() argument must be a function".to_string()),
                                        }
                                    }
                                    Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(new_items)))))
                                } else {
                                    Err(format!("filter() called on non-list"))
                                }
                            } else {
                                Err(format!("filter() called on nothing"))
                            }
                        }
                        "push" => {
                            if let Some(bound_val) = bound {
                                if let Value::List(ref items_lock) = *bound_val {
                                    let item = arg_vals.get(0).ok_or("push() requires an item")?.clone();
                                    match items_lock.write() {
                                        Ok(mut i) => { i.push(item); }
                                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                                    }
                                    Ok(ControlFlow::Value(Value::None))
                                } else {
                                    Err(format!("push() called on non-list"))
                                }
                            } else {
                                Err(format!("push() called on nothing"))
                            }
                        }
                        "keys" => {
                            if let Some(bound_val) = bound {
                                if let Value::Map(ref map) = *bound_val {
                                    let keys: Vec<Value> = map.keys().cloned().collect();
                                    Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(keys)))))
                                } else {
                                    Err(format!("keys() called on non-map"))
                                }
                            } else {
                                Err(format!("keys() called on nothing"))
                            }
                        }
                        "values" => {
                            if let Some(bound_val) = bound {
                                if let Value::Map(ref map) = *bound_val {
                                    let values: Vec<Value> = map.values().cloned().collect();
                                    Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(values)))))
                                } else {
                                    Err(format!("values() called on non-map"))
                                }
                            } else {
                                Err(format!("values() called on nothing"))
                            }
                        }
                        "read" => {
                            let filename = match arg_vals.get(0) {
                                Some(Value::Str(s)) => s.clone(),
                                _ => return Err("read() requires a filename string".to_string()),
                            };
                            match std::fs::read_to_string(filename) {
                                Ok(content) => Ok(ControlFlow::Value(Value::Ok(Box::new(Value::Str(content))))),
                                Err(e) => Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                            }
                        }
                        "write" => {
                            let filename = match arg_vals.get(0) {
                                Some(Value::Str(s)) => s.clone(),
                                _ => return Err("write() requires a filename string".to_string()),
                            };
                            let content = match arg_vals.get(1) {
                                Some(Value::Str(s)) => s.clone(),
                                _ => return Err("write() requires content as second argument".to_string()),
                            };
                            match std::fs::write(filename, content) {
                                Ok(_) => Ok(ControlFlow::Value(Value::Ok(Box::new(Value::None)))),
                                Err(e) => Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                            }
                        }
                        "ls" => {
                            let path = match arg_vals.get(0) {
                                Some(Value::Str(s)) => s.clone(),
                                Some(Value::None) | None => ".".to_string(),
                                _ => return Err("ls() requires a path string or none".to_string()),
                            };
                            match std::fs::read_dir(path) {
                                Ok(entries) => {
                                    let mut files = Vec::new();
                                    for entry in entries {
                                        if let Ok(e) = entry {
                                            if let Some(s) = e.file_name().to_str() {
                                                files.push(Value::Str(s.to_string()));
                                            }
                                        }
                                    }
                                    Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(files)))))
                                }
                                Err(e) => Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                            }
                        }
                        "sleep" => {
                            let ms = match arg_vals.get(0) {
                                Some(Value::Int(i)) => *i as u64,
                                _ => return Err("sleep() requires milliseconds as an integer".to_string()),
                            };
                            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
                            Ok(ControlFlow::Value(Value::None))
                        }
                        "ord" => {
                            match arg_vals.get(0) {
                                Some(Value::Str(s)) if !s.is_empty() => Ok(ControlFlow::Value(Value::Int(s.chars().next().unwrap() as i64))),
                                Some(Value::Char(c)) => Ok(ControlFlow::Value(Value::Int(*c as i64))),
                                _ => Err("ord() requires a non-empty string or character".to_string()),
                            }
                        }
                        "chr" => {
                            match arg_vals.get(0) {
                                Some(Value::Int(i)) => {
                                    if let Some(c) = char::from_u32(*i as u32) {
                                        Ok(ControlFlow::Value(Value::Str(c.to_string())))
                                    } else {
                                        Err("Invalid character code for chr()".to_string())
                                    }
                                }
                                _ => Err("chr() requires an integer".to_string()),
                            }
                        }
                        "str" => {
                            match arg_vals.get(0) {
                                Some(Value::Int(i)) => Ok(ControlFlow::Value(Value::Str(i.to_string()))),
                                Some(Value::Float(f)) => Ok(ControlFlow::Value(Value::Str(f.to_string()))),
                                Some(Value::Bool(b)) => Ok(ControlFlow::Value(Value::Str(b.to_string()))),
                                Some(Value::Str(s)) => Ok(ControlFlow::Value(Value::Str(s.clone()))),
                                Some(v) => Ok(ControlFlow::Value(Value::Str(format!("{:?}", v)))),
                                None => Ok(ControlFlow::Value(Value::Str("None".to_string()))),
                            }
                        }
                        "int" => {
                            match arg_vals.get(0) {
                                Some(Value::Int(i)) => Ok(ControlFlow::Value(Value::Int(*i))),
                                Some(Value::Float(f)) => Ok(ControlFlow::Value(Value::Int(*f as i64))),
                                Some(Value::Str(s)) => {
                                    match s.parse::<i64>() {
                                        Ok(i) => Ok(ControlFlow::Value(Value::Int(i))),
                                        Err(_) => Err(format!("Could not parse int: {}", s)),
                                    }
                                }
                                Some(Value::Bool(b)) => Ok(ControlFlow::Value(Value::Int(if *b { 1 } else { 0 }))),
                                _ => Err("int() requires a number, string, or boolean".to_string()),
                            }
                        }
                        "float" => {
                            match arg_vals.get(0) {
                                Some(Value::Float(f)) => Ok(ControlFlow::Value(Value::Float(*f))),
                                Some(Value::Int(i)) => Ok(ControlFlow::Value(Value::Float(*i as f64))),
                                Some(Value::Str(s)) => {
                                    match s.parse::<f64>() {
                                        Ok(f) => Ok(ControlFlow::Value(Value::Float(f))),
                                        Err(_) => Err(format!("Could not parse float: {}", s)),
                                    }
                                }
                                _ => Err("float() requires a number or string".to_string()),
                            }
                        }
                        "bool" => {
                            match arg_vals.get(0) {
                                Some(Value::Bool(b)) => Ok(ControlFlow::Value(Value::Bool(*b))),
                                Some(Value::Int(i)) => Ok(ControlFlow::Value(Value::Bool(*i != 0))),
                                Some(Value::Str(s)) => Ok(ControlFlow::Value(Value::Bool(!s.is_empty()))),
                                Some(Value::None) | None => Ok(ControlFlow::Value(Value::Bool(false))),
                                _ => Ok(ControlFlow::Value(Value::Bool(true))),
                            }
                        }
                        "is_ok" => {
                            if let Some(bound_val) = bound {
                                match &*bound_val {
                                    Value::Ok(_) => Ok(ControlFlow::Value(Value::Bool(true))),
                                    Value::Err(_) => Ok(ControlFlow::Value(Value::Bool(false))),
                                    _ => Err("is_ok() called on non-result".to_string()),
                                }
                            } else {
                                Err("is_ok() called on nothing".to_string())
                            }
                        }
                        "is_err" => {
                            if let Some(bound_val) = bound {
                                match &*bound_val {
                                    Value::Ok(_) => Ok(ControlFlow::Value(Value::Bool(false))),
                                    Value::Err(_) => Ok(ControlFlow::Value(Value::Bool(true))),
                                    _ => Err("is_err() called on non-result".to_string()),
                                }
                            } else {
                                Err("is_err() called on nothing".to_string())
                            }
                        }
                        "split" => {
                            if let Some(bound_box) = &bound {
                                if let Value::Str(s) = &**bound_box {
                                    let delim = match arg_vals.get(0) {
                                        Some(Value::Str(d)) => d.as_str(),
                                        _ => "",
                                    };
                                    let parts: Vec<Value> = if delim.is_empty() {
                                        s.chars().map(|c| Value::Str(c.to_string())).collect()
                                    } else {
                                        s.split(delim).map(|p| Value::Str(p.to_string())).collect()
                                    };
                                    return Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(parts)))));
                                }
                            }
                            // standalone split(str, delim)
                            match (arg_vals.get(0), arg_vals.get(1)) {
                                (Some(Value::Str(s)), Some(Value::Str(d))) => {
                                    let parts: Vec<Value> = s.split(d).map(|p| Value::Str(p.to_string())).collect();
                                    Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(parts)))))
                                }
                                _ => Err("split() requires a string and an optional/required delimiter".to_string()),
                            }
                        }
                        "xor" => {
                            match (arg_vals.get(0), arg_vals.get(1)) {
                                (Some(Value::Int(a)), Some(Value::Int(b))) => Ok(ControlFlow::Value(Value::Int(a ^ b))),
                                _ => Err("xor() requires two integers".to_string()),
                            }
                        }
                        "env" => {
                            match arg_vals.get(0) {
                                Some(Value::Str(key)) => {
                                    match std::env::var(key) {
                                        Ok(val) => Ok(ControlFlow::Value(Value::Str(val))),
                                        Err(_) => Ok(ControlFlow::Value(Value::None)),
                                    }
                                }
                                _ => Err("env() requires a key string".to_string()),
                            }
                        }
                        "set_env" => {
                            match (arg_vals.get(0), arg_vals.get(1)) {
                                (Some(Value::Str(key)), Some(Value::Str(val))) => {
                                    unsafe { std::env::set_var(key, val); }
                                    Ok(ControlFlow::Value(Value::None))
                                }
                                _ => Err("set_env() requires a key string and value string".to_string()),
                            }
                        }
                        "exists" => {
                            match arg_vals.get(0) {
                                Some(Value::Str(path)) => {
                                    Ok(ControlFlow::Value(Value::Bool(std::path::Path::new(path).exists())))
                                }
                                _ => Err("exists() requires a path string".to_string()),
                            }
                        }
                        "is_dir" => {
                            match arg_vals.get(0) {
                                Some(Value::Str(path)) => {
                                    Ok(ControlFlow::Value(Value::Bool(std::path::Path::new(path).is_dir())))
                                }
                                _ => Err("is_dir() requires a path string".to_string()),
                            }
                        }
                        "is_file" => {
                            match arg_vals.get(0) {
                                Some(Value::Str(path)) => {
                                    Ok(ControlFlow::Value(Value::Bool(std::path::Path::new(path).is_file())))
                                }
                                _ => Err("is_file() requires a path string".to_string()),
                            }
                        }
                        "json" => {
                            match arg_vals.get(0) {
                                Some(Value::Str(json_str)) => {
                                    match serde_json::from_str::<serde_json::Value>(json_str) {
                                        Ok(v) => Ok(ControlFlow::Value(json_to_loom(v))),
                                        Err(e) => Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                                    }
                                }
                                _ => Err("json() requires a JSON string".to_string()),
                            }
                        }
                        "net.connect" => {
                            let ip = match arg_vals.get(0) {
                                Some(Value::Str(s)) => s.clone(),
                                _ => return Err("net.connect() requires an IP string".to_string()),
                            };
                            let port = match arg_vals.get(1) {
                                Some(Value::Int(p)) => *p as u16,
                                _ => return Err("net.connect() requires a port integer".to_string()),
                            };
                            let addr = format!("{}:{}", ip, port);
                            match tokio::time::timeout(std::time::Duration::from_secs(1), tokio::net::TcpStream::connect(addr)).await {
                                Ok(Ok(_)) => Ok(ControlFlow::Value(Value::Ok(Box::new(Value::None)))),
                                Ok(Err(e)) => Ok(ControlFlow::Value(Value::Err(e.to_string()))),
                                Err(_) => Ok(ControlFlow::Value(Value::Err("Connection timeout".to_string()))),
                            }
                        }
                        _ => Err(format!("unknown builtin: {}", name)),
                    }
                }
                _ => Err(format!("not a function: {:?}", func)),
            }
        }
        Expr::Spawn(expr) => {
            log::debug!("Spawning task for expression: {:?}", expr);
            let inner_env = env.clone();
            let handle = task::spawn(async move {
                eval(*expr, &inner_env).await
            });
            Ok(ControlFlow::Value(Value::Task(Arc::new(Mutex::new(Some(handle))))))
        }
        Expr::Await(expr) => {
            let val = match eval_internal(*expr, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            if let Value::Task(handle_lock) = val {
                let handle = {
                    let mut lock = match handle_lock.lock() {
                        Ok(l) => l,
                        Err(e) => return Err(format!("Task lock poisoned: {}", e)),
                    };
                    lock.take()
                };
                if let Some(h) = handle {
                    match h.await {
                        Ok(Ok(v)) => {
                            log::debug!("Task finished successfully!");
                            Ok(ControlFlow::Value(Value::Ok(Box::new(v))))
                        },
                        Ok(Err(e)) => {
                            log::error!("Task failed with Loom error: {}", e);
                            Ok(ControlFlow::Value(Value::Err(format!("Task error: {}", e))))
                        },
                        Err(e) => {
                            log::error!("Task panic or cancellation: {}", e);
                            Ok(ControlFlow::Value(Value::Err(format!("Task panic: {}", e))))
                        },
                    }
                } else {
                    Ok(ControlFlow::Value(Value::Err("Task already awaited".to_string())))
                }
            } else {
                Err(format!("Cannot await non-task value"))
            }
        }
        Expr::When { val, arms } => {
            let v = match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            for arm in arms {
                if let Some(bindings) = match_pattern(&v, &arm.pattern) {
                    let when_env = Env::with_parent(env.clone(), false);
                    for (name, val) in bindings {
                        when_env.set(name, val);
                    }
                    return eval_internal(arm.body, &when_env).await;
                }
            }
            Err(format!("No match found for when expression"))
        }
        Expr::Assign { name, val } => {
            let v = match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            env.update(name, v.clone())?;
            Ok(ControlFlow::Value(v))
        }
        Expr::While { cond, body } => {
            let mut last_val = Value::None;
            while let ControlFlow::Value(Value::Bool(true)) = eval_internal(*cond.clone(), env).await? {
                match eval_internal(*body.clone(), env).await? {
                    ControlFlow::Value(v) => last_val = v,
                    ControlFlow::Break => break,
                    ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                }
            }
            Ok(ControlFlow::Value(last_val))
        }
        Expr::Loop(body) => {
            let mut last_val = Value::None;
            loop {
                match eval_internal(*body.clone(), env).await? {
                    ControlFlow::Value(v) => last_val = v,
                    ControlFlow::Break => break,
                    ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                }
            }
            Ok(ControlFlow::Value(last_val))
        }
        Expr::Range(start, end) => {
            let s = match eval_internal(*start, env).await? {
                ControlFlow::Value(Value::Int(i)) => i,
                _ => return Err("Range start must be an integer".to_string()),
            };
            let e = match eval_internal(*end, env).await? {
                ControlFlow::Value(Value::Int(i)) => i,
                _ => return Err("Range end must be an integer".to_string()),
            };
            Ok(ControlFlow::Value(Value::Range(s, e)))
        }
        Expr::For { var, iter, body } => {
            let iter_val = match eval_internal(*iter, env).await? {
                ControlFlow::Value(v) => v,
                _ => return Err(format!("For loop requires an iterable")),
            };
            
            let mut last_val = Value::None;
            
            match iter_val {
                Value::List(items_lock) => {
                    let items = match items_lock.read() {
                        Ok(i) => i.clone(),
                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                    };
                    for item in items {
                        let loop_env = Env::with_parent(env.clone(), false);
                        loop_env.set(var.clone(), item.clone());
                        match eval_internal(*body.clone(), &loop_env).await? {
                            ControlFlow::Value(v) => last_val = v,
                            ControlFlow::Break => break,
                            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        }
                    }
                }
                Value::Range(start, end) => {
                    for i in start..end {
                        let loop_env = Env::with_parent(env.clone(), false);
                        loop_env.set(var.clone(), Value::Int(i));
                        match eval_internal(*body.clone(), &loop_env).await? {
                            ControlFlow::Value(v) => last_val = v,
                            ControlFlow::Break => break,
                            ControlFlow::Return(v) => return Ok(ControlFlow::Return(v)),
                        }
                    }
                }
                _ => return Err(format!("For loop requires a list or range, got {:?}", iter_val)),
            }
            Ok(ControlFlow::Value(last_val))
        }
        Expr::Frame { name, .. } => {
            if env.verbose {
                println!("[LOOM] Defined frame: {}", name);
            }
            Ok(ControlFlow::Value(Value::None))
        }
        Expr::Pull(path) => {
            if env.verbose {
                println!("[LOOM] Pulling: {}", path);
            }
            if path.ends_with(".md") {
                match std::fs::read_to_string(&path) {
                    Ok(content) => {
                        let mut code = String::new();
                        let mut in_block = false;
                        for line in content.lines() {
                            if line.trim().starts_with("```") {
                                if in_block {
                                    in_block = false;
                                } else if line.trim().starts_with("```loom") {
                                    in_block = true;
                                }
                                continue;
                            }
                            if in_block {
                                code.push_str(line);
                                code.push('\n');
                            }
                        }
                        if !code.is_empty() {
                            let mut lexer = crate::lexer::Lexer::with_verbose(&code, env.verbose);
                            match lexer.tokenize() {
                                Ok(tokens) => {
                                    let mut parser = crate::parser::Parser::new(tokens);
                                    match parser.parse() {
                                        Ok(ast) => {
                                            eval(ast, env).await?;
                                        }
                                        Err(e) => return Err(format!("Error parsing pulled markdown code: {}", e)),
                                    }
                                }
                                Err(e) => return Err(format!("Error lexing pulled markdown code: {}", e)),
                            }
                        }
                    }
                    Err(e) => return Err(format!("Failed to read pull file {}: {}", path, e)),
                }
            }
            Ok(ControlFlow::Value(Value::None))
        }
        Expr::Safety { limit, body } => {
            let normalized_limit = limit.replace(" ", "");
            if env.verbose {
                println!("[LOOM] Safety block with limit: {}", normalized_limit);
            }
            if normalized_limit.contains("0MB") || normalized_limit.contains("mem:0") {
                return Err(format!("Safety limit exceeded: {}", normalized_limit));
            }
            // Mock memory limit: just log it for now
            eval_internal(*body, env).await
        }
        Expr::TraceComment(s) => {
            if env.verbose_trace {
                println!("[TRACE] {}", s.trim());
            }
            Ok(ControlFlow::Value(Value::None))
        }
        Expr::Trait { name, .. } => {
            if env.verbose {
                println!("[LOOM] Defined trait: {}", name);
            }
            Ok(ControlFlow::Value(Value::None))
        }
        Expr::Weave { trait_name, frame_name, methods } => {
            if env.verbose {
                println!("[LOOM] Weaving trait {} into frame {}", trait_name, frame_name);
            }
            for method in methods {
                if let Expr::Act { name: Some(m_name), params, body, .. } = method {
                    let full_method_name = format!("weave::{}::{}::{}", trait_name, frame_name, m_name);
                    let func = Value::Func(params, body, env.clone());
                    env.set(full_method_name, func);
                }
            }
            Ok(ControlFlow::Value(Value::None))
        }
        Expr::FrameInst { name, fields } => {
            if env.verbose {
                println!("[LOOM] Instantiating frame: {}", name);
            }
            let mut field_values = std::collections::HashMap::new();
            for (f_name, f_expr) in fields {
                match eval_internal(f_expr, env).await? {
                    ControlFlow::Value(v) => { field_values.insert(f_name, v); }
                    cf => return Ok(cf),
                }
            }
            Ok(ControlFlow::Value(Value::Frame(name, Arc::new(RwLock::new(field_values)))))
        }
        Expr::Bind { name, methods } => {
            if env.verbose {
                println!("[LOOM] Binding methods to frame: {}", name);
            }
            for method in methods {
                if let Expr::Act { name: Some(m_name), params, body, .. } = method {
                    let full_method_name = format!("{}::{}", name, m_name);
                    let func = Value::Func(params, body, env.clone());
                    env.set(full_method_name, func);
                }
            }
            Ok(ControlFlow::Value(Value::None))
        }
        Expr::FieldAccess { obj, field } => {
            let val = match eval_internal(*obj, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            match val {
                Value::Frame(frame_name, fields_lock) => {
                    let fields = match fields_lock.read() {
                        Ok(f) => f,
                        Err(e) => return Err(format!("Frame lock poisoned: {}", e)),
                    };
                    if let Some(f_val) = fields.get(&field) {
                        return Ok(ControlFlow::Value(f_val.clone()));
                    }
                    drop(fields);
                    let method_name = format!("{}::{}", frame_name, field);
                    if let Some(method) = env.get(&method_name) {
                        if let Value::Func(params, body, closure_env) = method {
                            if params.get(0) == Some(&"self".to_string()) {
                                let mut new_params = params.clone();
                                new_params.remove(0);
                                let bound_env = Env::with_parent(closure_env, true);
                                bound_env.set("self".to_string(), Value::Frame(frame_name, fields_lock.clone()));
                                return Ok(ControlFlow::Value(Value::Func(new_params, body, bound_env)));
                            }
                            return Ok(ControlFlow::Value(Value::Func(params, body, closure_env)));
                        }
                    }
                    // now we check weave methods
                    for (key, val) in env.all_vars() {
                        if key.starts_with("weave::") && key.ends_with(&format!("::{}::{}", frame_name, field)) {
                            if let Value::Func(params, body, closure_env) = val {
                                if params.get(0) == Some(&"self".to_string()) {
                                    let mut new_params = params.clone();
                                    new_params.remove(0);
                                    let bound_env = Env::with_parent(closure_env, true);
                                    bound_env.set("self".to_string(), Value::Frame(frame_name, fields_lock.clone()));
                                    return Ok(ControlFlow::Value(Value::Func(new_params, body, bound_env)));
                                }
                                return Ok(ControlFlow::Value(Value::Func(params, body, closure_env)));
                            }
                        }
                    }
                    Err(format!("Field or method {} not found on frame {}", field, frame_name))
                }
                Value::Str(ref _s) => {
                    if field == "len" || field == "replace" || field == "ends_with" || field == "starts_with" || field == "split" {
                        Ok(ControlFlow::Value(Value::Builtin(field, Some(Box::new(val)))))
                    } else {
                        Err(format!("Method {} not found on string", field))
                    }
                }
                Value::List(_) => {
                    if field == "len" || field == "map" || field == "filter" || field == "push" {
                        Ok(ControlFlow::Value(Value::Builtin(field, Some(Box::new(val)))))
                    } else {
                        Err(format!("Method {} not found on list", field))
                    }
                }
                Value::Map(ref map) => {
                    if field == "len" || field == "keys" || field == "values" {
                        Ok(ControlFlow::Value(Value::Builtin(field, Some(Box::new(val)))))
                    } else if let Some(v) = map.get(&Value::Str(field.clone())) {
                        Ok(ControlFlow::Value(v.clone()))
                    } else {
                        Ok(ControlFlow::Value(Value::None))
                    }
                }
                Value::Builtin(name, None) => {
                    Ok(ControlFlow::Value(Value::Builtin(format!("{}.{}", name, field), None)))
                }
                Value::ShellOutput { stdout, stderr, status } => {
                    match field.as_str() {
                        "stdout" => Ok(ControlFlow::Value(Value::Str(stdout))),
                        "stderr" => Ok(ControlFlow::Value(Value::Str(stderr))),
                        "status" => Ok(ControlFlow::Value(Value::Int(status as i64))),
                        _ => Err(format!("Field {} not found on ShellOutput", field)),
                    }
                }
                Value::Ok(ref v) => {
                    if field == "Ok" { Ok(ControlFlow::Value(*v.clone())) }
                    else if field == "is_ok" || field == "is_err" { Ok(ControlFlow::Value(Value::Builtin(field, Some(Box::new(val))))) }
                    else { Err(format!("Method or field {} not found on Ok variant", field)) }
                }
                Value::Err(ref m) => {
                    if field == "Err" { Ok(ControlFlow::Value(Value::Str(m.clone()))) }
                    else if field == "is_ok" || field == "is_err" { Ok(ControlFlow::Value(Value::Builtin(field, Some(Box::new(val))))) }
                    else { Err(format!("Method or field {} not found on Err variant", field)) }
                }
                Value::Some(v) => {
                    if field == "Some" { Ok(ControlFlow::Value(*v)) }
                    else { Err(format!("Method or field {} not found on Some variant", field)) }
                }
                _ => Err(format!("Field access on non-frame value")),
            }
        }
        Expr::FieldAssign { obj, field, val } => {
            let target_obj = match eval_internal(*obj, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            let value = match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            match target_obj {
                Value::Frame(_, fields_lock) => {
                    match fields_lock.write() {
                        Ok(mut f) => { f.insert(field, value.clone()); }
                        Err(e) => return Err(format!("Frame lock poisoned: {}", e)),
                    }
                    Ok(ControlFlow::Value(value))
                }
                _ => Err(format!("Field assignment on non-frame value")),
            }
        }
        Expr::Map(entries) => {
            let mut map = std::collections::HashMap::new();
            for (k_expr, v_expr) in entries {
                let k = match eval_internal(k_expr.clone(), env).await {
                    Ok(ControlFlow::Value(v)) => v,
                    Err(e) if e.starts_with("Undefined variable:") => {
                        if let Expr::Ident(name) = k_expr {
                            Value::Str(name)
                        } else {
                            return Err(e);
                        }
                    }
                    Ok(cf) => return Ok(cf),
                    Err(e) => return Err(e),
                };
                let v = match eval_internal(v_expr, env).await? {
                    ControlFlow::Value(v) => v,
                    cf => return Ok(cf),
                };
                map.insert(k, v);
            }
            Ok(ControlFlow::Value(Value::Map(map)))
        }
        Expr::List(exprs) => {
            let mut items = Vec::new();
            for e in exprs {
                match eval_internal(e, env).await? {
                    ControlFlow::Value(v) => items.push(v),
                    cf => return Ok(cf),
                }
            }
            Ok(ControlFlow::Value(Value::List(Arc::new(RwLock::new(items)))))
        }
        Expr::IndexAccess { obj, index } => {
            let target = match eval_internal(*obj, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            let idx_val = match eval_internal(*index, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            match target {
                Value::List(items_lock) => {
                    let idx = match idx_val {
                        Value::Int(i) => i as usize,
                        _ => return Err(format!("List index must be an integer")),
                    };
                    let items = match items_lock.read() {
                        Ok(i) => i,
                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                    };
                    if idx < items.len() {
                        Ok(ControlFlow::Value(items[idx].clone()))
                    } else {
                        Err(format!("Index out of bounds: {}", idx))
                    }
                }
                Value::Map(map) => {
                     if let Some(val) = map.get(&idx_val) {
                         Ok(ControlFlow::Value(val.clone()))
                     } else {
                         Ok(ControlFlow::Value(Value::None))
                     }
                }
                Value::Str(s) => {
                    let idx = match idx_val {
                        Value::Int(i) => i as usize,
                        _ => return Err(format!("String index must be an integer")),
                    };
                    if let Some(c) = s.chars().nth(idx) {
                        Ok(ControlFlow::Value(Value::Str(c.to_string())))
                    } else {
                         Err(format!("Index out of bounds: {}", idx))
                    }
                }
                _ => Err(format!("Indexing non-indexable value")),
            }
        }
        Expr::Return(val_expr) => {
            let val = if let Some(e) = val_expr {
                match eval_internal(*e, env).await? {
                    ControlFlow::Value(v) => v,
                    cf => return Ok(cf),
                }
            } else {
                Value::None
            };
            Ok(ControlFlow::Return(val))
        }
        Expr::Break => Ok(ControlFlow::Break),
        Expr::IndexAssign { obj, index, val } => {
            let target = match eval_internal(*obj, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            let idx_val = match eval_internal(*index, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            let v = match eval_internal(*val, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            match target {
                Value::List(items_lock) => {
                    let idx = match idx_val {
                        Value::Int(i) => i as usize,
                        _ => return Err(format!("List index must be an integer")),
                    };
                    let mut items = match items_lock.write() {
                        Ok(i) => i,
                        Err(e) => return Err(format!("List lock poisoned: {}", e)),
                    };
                    if idx < items.len() {
                        items[idx] = v.clone();
                        Ok(ControlFlow::Value(v))
                    } else {
                        Err(format!("Index out of bounds: {}", idx))
                    }
                }
                _ => Err(format!("Index assignment on non-list value")),
            }
        }
        Expr::Shell(expr) => {
            let cmd_val = match eval_internal(*expr, env).await? {
                ControlFlow::Value(v) => v,
                cf => return Ok(cf),
            };
            let cmd = match cmd_val {
                Value::Str(s) => s,
                _ => format!("{:?}", cmd_val),
            };
            let output = std::process::Command::new("sh")
                .arg("-c")
                .arg(&cmd)
                .output();
            match output {
                Ok(out) => {
                    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
                    let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
                    let status = out.status.code().unwrap_or(-1);
                    Ok(ControlFlow::Value(Value::ShellOutput { stdout, stderr, status }))
                }
                Err(e) => Err(format!("Failed to execute command: {}", e)),
            }
        }
    }
}

fn match_pattern(val: &Value, pattern: &Pattern) -> Option<Vec<(String, Value)>> {
    match (val, pattern) {
        (_, Pattern::CatchAll) => Some(vec![]),
        (Value::Int(v), Pattern::Literal(Literal::Int(p))) if v == p => Some(vec![]),
        (Value::Str(v), Pattern::Literal(Literal::Str(p))) if v == p => Some(vec![]),
        (Value::Char(v), Pattern::Literal(Literal::Char(p))) if v == p => Some(vec![]),
        (Value::Bool(v), Pattern::Literal(Literal::Bool(p))) if v == p => Some(vec![]),
        (Value::None, Pattern::Variant(name, _)) if name == "None" => Some(vec![]),
        (Value::Ok(v), Pattern::Variant(name, bind)) if name == "Ok" => {
            let mut bindings = vec![];
            if let Some(b) = bind {
                bindings.push((b.clone(), *v.clone()));
            }
            Some(bindings)
        }
        (Value::Err(m), Pattern::Variant(name, bind)) if name == "Err" => {
            let mut bindings = vec![];
            if let Some(b) = bind {
                bindings.push((b.clone(), Value::Str(m.clone())));
            }
            Some(bindings)
        }
        (Value::Some(v), Pattern::Variant(name, bind)) if name == "Some" => {
            let mut bindings = vec![];
            if let Some(b) = bind {
                bindings.push((b.clone(), *v.clone()));
            }
            Some(bindings)
        }
        (Value::Int(v), Pattern::Range(start, end)) if v >= start && v <= end => Some(vec![]),
        _ => None,
    }
}

fn value_to_string(v: &Value) -> String {
    match v {
        Value::Int(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Str(s) => s.clone(),
        Value::Char(c) => c.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::ShellOutput { stdout, .. } => stdout.clone(),
        Value::List(l) => format!("{:?}", l.read().unwrap()),
        Value::Map(m) => format!("{:?}", m),
        Value::None => "None".to_string(),
        _ => format!("{:?}", v),
    }
}

fn json_to_loom(v: serde_json::Value) -> Value {
    match v {
        serde_json::Value::Null => Value::None,
        serde_json::Value::Bool(b) => Value::Bool(b),
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Value::Int(i)
            } else if let Some(f) = n.as_f64() {
                Value::Float(f)
            } else {
                Value::None
            }
        },
        serde_json::Value::String(s) => Value::Str(s),
        serde_json::Value::Array(arr) => {
            let items: Vec<Value> = arr.into_iter().map(json_to_loom).collect();
            Value::List(Arc::new(RwLock::new(items)))
        },
        serde_json::Value::Object(obj) => {
            let mut map = HashMap::new();
            for (k, v) in obj {
                map.insert(Value::Str(k), json_to_loom(v));
            }
            Value::Map(map)
        }
    }
}
