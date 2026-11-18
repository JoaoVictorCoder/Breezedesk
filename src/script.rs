//! Lua scripting (LuaJIT, embedded in the executable).
//!
//! A widget may set `script = "file.lua"`. The script can define:
//! - `init()`   called once after loading
//! - `update()` called on every widget update; the table it returns becomes
//!   variables usable in `{...}` (tables are reachable with dotted paths)
//! - any function, callable from expressions with `lua("name", args...)` or
//!   from actions with `!lua:name`
//!
//! The `breezedesk` table gives access to the host:
//! `breezedesk.eval(expr)`, `breezedesk.get(name)`, `breezedesk.set(name, value)`,
//! `breezedesk.run(action)`, `breezedesk.log(msg)` and `breezedesk.widget` (the widget name).
//!
//! Scripts are sandboxed: no `io`, no `require`, no file loading and only the
//! harmless parts of `os` (time, date, clock).

use crate::expr::{self, Env, Value};
use crate::widget::Var;
use mlua::{Function, Lua, LuaOptions, LuaSerdeExt, StdLib, Table, Value as LV};
use std::cell::{Cell, RefCell};
use std::path::Path;

pub struct Script {
    lua: Lua,
    depth: Cell<u32>,
    /// Values given to `breezedesk.set` (applied by the widget after each call).
    pub pending_vars: RefCell<Vec<(String, Var)>>,
    /// Actions given to `breezedesk.run`.
    pub pending_actions: RefCell<Vec<String>>,
}

impl Script {
    pub fn load(path: &Path, widget: &str) -> Result<Script, String> {
        let src = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let libs = StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::BIT | StdLib::OS | StdLib::JIT;
        let lua = Lua::new_with(libs, LuaOptions::default()).map_err(|e| e.to_string())?;
        let setup = || -> mlua::Result<()> {
            let g = lua.globals();
            for name in ["dofile", "loadfile"] {
                g.set(name, LV::Nil)?;
            }
            let os: Table = g.get("os")?;
            for name in ["execute", "exit", "remove", "rename", "tmpname", "getenv", "setlocale"] {
                os.set(name, LV::Nil)?;
            }
            let widget_name = widget.to_string();
            let log = lua.create_function(move |_, args: mlua::Variadic<LV>| {
                let parts: Vec<String> =
                    args.iter().map(|v| v.to_string().unwrap_or_else(|_| format!("{v:?}"))).collect();
                crate::log::log(format!("[{widget_name}] {}", parts.join(" ")));
                Ok(())
            })?;
            g.set("print", log.clone())?;
            let breezedesk = lua.create_table()?;
            breezedesk.set("log", log)?;
            breezedesk.set("widget", widget)?;
            g.set("breezedesk", breezedesk.clone())?;
            // Scripts written before the rename (Plume) keep working.
            g.set("plume", breezedesk)?;
            let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            lua.load(&src).set_name(format!("@{name}")).exec()
        };
        setup().map_err(|e| e.to_string())?;
        Ok(Script {
            lua,
            depth: Cell::new(0),
            pending_vars: RefCell::default(),
            pending_actions: RefCell::default(),
        })
    }

    /// Runs `f` with the `breezedesk.*` host functions bound to `env`.
    fn with_env<R>(&self, env: &dyn Env, f: impl FnOnce(&Lua) -> mlua::Result<R>) -> mlua::Result<R> {
        if self.depth.get() > 0 {
            // Re-entered (e.g. lua() called from breezedesk.eval): functions are already bound.
            return f(&self.lua);
        }
        self.depth.set(1);
        let result = self.lua.scope(|scope| {
            let breezedesk: Table = self.lua.globals().get("breezedesk")?;
            breezedesk.set(
                "eval",
                scope.create_function(|lua, src: String| {
                    let v = expr::parse(&src).map_err(mlua::Error::runtime)?.eval(env);
                    to_lua(lua, &v)
                })?,
            )?;
            breezedesk.set(
                "get",
                scope.create_function(|lua, name: String| to_lua(lua, &env.var(&name).unwrap_or(Value::Null)))?,
            )?;
            breezedesk.set(
                "set",
                scope.create_function(|lua, (name, v): (String, LV)| {
                    self.pending_vars.borrow_mut().push((name, from_lua(lua, v)));
                    Ok(())
                })?,
            )?;
            breezedesk.set(
                "run",
                scope.create_function(|_, action: String| {
                    self.pending_actions.borrow_mut().push(action);
                    Ok(())
                })?,
            )?;
            f(&self.lua)
        });
        self.depth.set(0);
        result
    }

    fn function(&self, name: &str) -> Option<Function> {
        self.lua.globals().get::<Option<Function>>(name).ok().flatten()
    }

    pub fn init(&self, env: &dyn Env) -> Result<(), String> {
        let Some(f) = self.function("init") else { return Ok(()) };
        self.with_env(env, |_| f.call::<()>(())).map_err(|e| e.to_string())
    }

    /// Calls `update()`; returns the variables from the table it returns.
    pub fn update(&self, env: &dyn Env) -> Result<Vec<(String, Var)>, String> {
        let Some(f) = self.function("update") else { return Ok(Vec::new()) };
        self.with_env(env, |lua| {
            let ret: LV = f.call(())?;
            let mut out = Vec::new();
            if let LV::Table(t) = ret {
                for pair in t.pairs::<String, LV>() {
                    let (k, v) = pair?;
                    out.push((k, from_lua(lua, v)));
                }
            }
            Ok(out)
        })
        .map_err(|e| e.to_string())
    }

    /// Calls a global function by name (for `lua("fn", ...)` and `!lua:fn`).
    pub fn call(&self, name: &str, args: &[Value], env: &dyn Env) -> Result<Value, String> {
        let f = self.function(name).ok_or_else(|| format!("Lua function '{name}' not found"))?;
        self.with_env(env, |lua| {
            let args: Vec<LV> = args.iter().map(|a| to_lua(lua, a)).collect::<mlua::Result<_>>()?;
            let ret: LV = f.call(mlua::MultiValue::from_iter(args))?;
            Ok(match from_lua(lua, ret) {
                Var::V(v) => v,
                Var::J(j) => Value::Str(j.to_string()),
            })
        })
        .map_err(|e| e.to_string())
    }
}

fn to_lua(lua: &Lua, v: &Value) -> mlua::Result<LV> {
    Ok(match v {
        Value::Num(n, _) => LV::Number(*n),
        Value::Str(s) => LV::String(lua.create_string(s)?),
        Value::Null => LV::Nil,
    })
}

/// Lua values become expression values; tables become JSON (dotted paths).
pub fn from_lua(lua: &Lua, v: LV) -> Var {
    match v {
        LV::Nil => Var::V(Value::Null),
        LV::Boolean(b) => Var::V(Value::bool(b)),
        LV::Integer(i) => Var::V(Value::Num(i as f64, 0)),
        LV::Number(n) => Var::V(Value::Num(n, decimals(n))),
        LV::String(s) => Var::V(Value::Str(s.to_string_lossy())),
        t @ LV::Table(_) => match lua.from_value::<serde_json::Value>(t) {
            Ok(j) => Var::J(j),
            Err(_) => Var::V(Value::Null),
        },
        other => Var::V(Value::Str(other.to_string().unwrap_or_default())),
    }
}

/// How many decimals a number "naturally" has (at most 2).
pub fn decimals(n: f64) -> u8 {
    if n.fract() == 0.0 || !n.is_finite() {
        return 0;
    }
    let s = format!("{n}");
    s.split_once('.').map_or(0, |(_, f)| f.len().min(2) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct E(HashMap<&'static str, Value>);
    impl Env for E {
        fn var(&self, n: &str) -> Option<Value> {
            self.0.get(n).cloned()
        }
        fn call(&self, _: &str, _: &[Value]) -> Option<Value> {
            None
        }
    }

    fn script(src: &str) -> Script {
        let dir = std::env::temp_dir().join(format!("breezedesk-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join(format!("{}.lua", src.len()));
        std::fs::write(&p, src).unwrap();
        Script::load(&p, "test").unwrap()
    }

    #[test]
    fn update_set_run_and_sandbox() {
        let s = script(
            r#"
            local n = 0
            function update()
              n = n + 1
              breezedesk.set("count", n)
              if breezedesk.get("cpu") > 50 then breezedesk.run("!reload") end
              return { status = "cpu " .. breezedesk.eval("cpu * 2"), info = { temp = 21.5 }, jit = jit.version }
            end
            function double(x) return x * 2 end
            function sandbox() return tostring(io) .. tostring(os.execute) .. tostring(require) end
            "#,
        );
        let env = E(HashMap::from([("cpu", Value::Num(60.0, 0))]));
        let vars = s.update(&env).unwrap();
        let get = |k: &str| vars.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        assert!(matches!(get("status"), Some(Var::V(Value::Str(ref t))) if t == "cpu 120"));
        assert!(matches!(get("info"), Some(Var::J(ref j)) if j["temp"] == 21.5));
        assert!(matches!(get("jit"), Some(Var::V(Value::Str(ref t))) if t.starts_with("LuaJIT")));
        assert_eq!(s.pending_actions.borrow().as_slice(), ["!reload"]);
        assert_eq!(s.pending_vars.borrow().len(), 1);
        assert_eq!(s.call("double", &[Value::Num(21.0, 0)], &env).unwrap().to_string(), "42");
        assert_eq!(s.call("sandbox", &[], &env).unwrap().to_string(), "nilnilnil");
        assert!(s.call("missing", &[], &env).is_err());
    }
}
