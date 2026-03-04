use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use crate::value::Value;

impl std::fmt::Debug for Env {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Env")
            .field("vars", &self.vars.read().unwrap().keys())
            .field("has_parent", &self.parent.is_some())
            .field("is_hard", &self.is_hard)
            .field("verbose", &self.verbose)
            .field("verbose_trace", &self.verbose_trace)
            .finish()
    }
}

#[derive(Clone)]
pub struct Env {
    vars: Arc<RwLock<HashMap<String, Value>>>,
    pub history: Arc<RwLock<Vec<(String, Value)>>>,
    parent: Option<Box<Env>>,
    pub verbose: bool,
    pub verbose_trace: bool,
    pub is_hard: bool,
}

impl Env {
    pub fn with_verbose(verbose: bool, verbose_trace: bool) -> Self {
        Self {
            vars: Arc::new(RwLock::new(HashMap::new())),
            history: Arc::new(RwLock::new(Vec::new())),
            parent: None,
            verbose,
            verbose_trace,
            is_hard: true,
        }
    }

    pub fn with_parent(parent: Env, is_hard: bool) -> Self {
        let verbose = parent.verbose;
        let verbose_trace = parent.verbose_trace;
        let history = parent.history.clone();
        Self {
            vars: Arc::new(RwLock::new(HashMap::new())),
            history,
            parent: Some(Box::new(parent)),
            verbose,
            verbose_trace,
            is_hard,
        }
    }

    pub fn set(&self, name: String, val: Value) {
        match self.vars.write() {
            Ok(mut vars) => { vars.insert(name.clone(), val.clone()); }
            Err(e) => { log::error!("Environment write lock poisoned: {}", e); }
        }
        match self.history.write() {
            Ok(mut history) => {
                history.push((name, val));
                if history.len() > 100 { history.remove(0); }
            }
            Err(_) => {}
        }
    }

    pub fn define(&self, name: String, val: Value) {
        if !self.is_hard {
            if let Some(parent) = &self.parent {
                return parent.define(name, val);
            }
        }
        self.set(name, val);
    }

    pub fn update(&self, name: String, val: Value) -> Result<(), String> {
        let has_key = match self.vars.read() {
            Ok(vars) => vars.contains_key(&name),
            Err(e) => {
                log::error!("Environment read lock poisoned: {}", e);
                return Err(format!("Environment lock poisoned: {}", e));
            }
        };

        if has_key {
            match self.vars.write() {
                Ok(mut vars) => { vars.insert(name.clone(), val.clone()); }
                Err(e) => {
                    log::error!("Environment write lock poisoned: {}", e);
                    return Err(format!("Environment lock poisoned: {}", e));
                }
            }
            match self.history.write() {
                Ok(mut history) => {
                    history.push((name, val));
                    if history.len() > 100 { history.remove(0); }
                }
                Err(_) => {}
            }
            return Ok(());
        }

        if let Some(parent) = &self.parent {
            return parent.update(name, val);
        }
        Err(format!("Undefined variable for assignment: {}", name))
    }

    pub fn get(&self, name: &str) -> Option<Value> {
        let val = match self.vars.read() {
            Ok(vars) => vars.get(name).cloned(),
            Err(e) => {
                log::error!("Environment read lock poisoned: {}", e);
                None
            }
        };

        if val.is_some() {
            return val;
        }

        if let Some(parent) = &self.parent {
            return parent.get(name);
        }
        None
    }

    pub fn all_vars(&self) -> HashMap<String, Value> {
        let mut vars = if let Some(parent) = &self.parent {
            parent.all_vars()
        } else {
            HashMap::new()
        };
        match self.vars.read() {
            Ok(self_vars) => {
                for (k, v) in self_vars.iter() {
                    vars.insert(k.clone(), v.clone());
                }
            }
            Err(e) => { log::error!("Environment read lock poisoned: {}", e); }
        }
        vars
    }
}
