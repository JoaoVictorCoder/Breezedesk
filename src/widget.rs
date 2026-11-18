//! Platform-independent widget runtime: variables, Lua script, element state
//! (graph history, hit areas) and the expression environment.

use crate::config::{Kind, WidgetConfig};
use crate::expr::{Env, Template, Value};
use crate::metrics::Metrics;
use crate::script::{self, Script};
use crate::web::{self, Web};
use serde_json::Value as Json;
use std::collections::{HashMap, VecDeque};
use std::time::Duration;

/// A variable: a plain value, or structured data (Lua table) reachable with dotted paths.
#[derive(Clone, Debug)]
pub enum Var {
    V(Value),
    J(Json),
}

enum VarDef {
    Lit(Value),
    Tpl(Template),
}

#[derive(Default)]
pub struct ElState {
    pub history: VecDeque<f32>,
    /// Area drawn in the last frame (for mouse hit tests).
    pub bounds: Option<tiny_skia::Rect>,
    /// Animation of bar/ring values: from → to, started at `anim_start`.
    pub anim_from: f32,
    pub anim_to: f32,
    pub anim_start: Option<std::time::Instant>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mouse {
    Left,
    Right,
    Middle,
    ScrollUp,
    ScrollDown,
}

pub struct Widget {
    pub cfg: WidgetConfig,
    pub states: Vec<ElState>,
    pub vars: HashMap<String, Var>,
    /// Values set with `!set` or `plume.set` (win over `[variables]`).
    pub overrides: HashMap<String, Var>,
    defs: Vec<(String, VarDef)>,
    pub script: Option<Script>,
    script_ready: bool,
    pub hovered: Option<usize>,
    pub mouse_over: bool,
    /// Actions requested by the script, to be run by the host.
    pub pending_actions: Vec<String>,
}

/// Expression environment for one widget.
pub struct Ctx<'a> {
    pub widget: &'a str,
    pub vars: &'a HashMap<String, Var>,
    pub overrides: &'a HashMap<String, Var>,
    pub cfg: &'a WidgetConfig,
    pub script: Option<&'a Script>,
    pub m: &'a Metrics,
    pub web: &'a Web,
}

pub fn json_to_value(j: &Json) -> Value {
    match j {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::bool(*b),
        Json::Number(n) => {
            let f = n.as_f64().unwrap_or(0.0);
            Value::Num(f, script::decimals(f))
        }
        Json::String(s) => Value::Str(s.clone()),
        other => Value::Str(other.to_string()),
    }
}

impl Env for Ctx<'_> {
    fn var(&self, name: &str) -> Option<Value> {
        let local = |n: &str| self.overrides.get(n).or_else(|| self.vars.get(n));
        if let Some(v) = local(name) {
            return Some(match v {
                Var::V(v) => v.clone(),
                Var::J(j) => json_to_value(j),
            });
        }
        if let Some((first, rest)) = name.split_once('.') {
            let path: Vec<&str> = rest.split('.').collect();
            if let Some(Var::J(j)) = local(first) {
                return Some(web::json_path(j, &path).map_or(Value::Null, json_to_value));
            }
            if self.cfg.web.iter().any(|w| w.name == first) {
                let key = format!("{}/{first}", self.widget);
                return Some(self.web.lookup(&key, &path).map_or(Value::Null, |j| json_to_value(&j)));
            }
        }
        if let Some(wcfg) = self.cfg.web.iter().find(|w| w.name == name) {
            let key = format!("{}/{}", self.widget, wcfg.name);
            return Some(self.web.lookup(&key, &[]).map_or(Value::Null, |j| json_to_value(&j)));
        }
        self.m.var(name)
    }

    fn call(&self, name: &str, args: &[Value]) -> Option<Value> {
        if name == "lua" {
            let Some(script) = self.script else {
                crate::log::log(format!("[{}] lua() used but the widget has no script", self.widget));
                return Some(Value::Null);
            };
            let fname = args.first().map(|v| v.to_string()).unwrap_or_default();
            return Some(script.call(&fname, args.get(1..).unwrap_or(&[]), self).unwrap_or_else(|e| {
                crate::log::log(format!("[{}] {e}", self.widget));
                Value::Null
            }));
        }
        if name == "get" {
            // Dynamic lookup: get('news.items.' + i + '.title')
            let n = args.first().map(|v| v.to_string()).unwrap_or_default();
            return Some(self.var(&n).unwrap_or(Value::Null));
        }
        if name == "web_ready" {
            let n = args.first().map(|v| v.to_string()).unwrap_or_default();
            return Some(Value::bool(self.web.has(&format!("{}/{n}", self.widget))));
        }
        self.m.call(name, args)
    }
}

impl Widget {
    pub fn new(cfg: WidgetConfig, m: &Metrics) -> (Widget, Vec<String>) {
        let mut errors = Vec::new();
        let defs = cfg
            .variables
            .iter()
            .map(|(k, v)| {
                let def = match v {
                    toml::Value::Integer(i) => VarDef::Lit(Value::Num(*i as f64, 0)),
                    toml::Value::Float(f) => VarDef::Lit(Value::Num(*f, script::decimals(*f))),
                    toml::Value::Boolean(b) => VarDef::Lit(Value::bool(*b)),
                    toml::Value::String(s) if s.contains('{') => match Template::parse(s) {
                        Ok(t) => VarDef::Tpl(t),
                        Err(e) => {
                            errors.push(format!("{}: variable '{k}': {e}", cfg.name));
                            VarDef::Lit(Value::Null)
                        }
                    },
                    toml::Value::String(s) => VarDef::Lit(Value::Str(s.clone())),
                    other => VarDef::Lit(Value::Str(other.to_string())),
                };
                (k.clone(), def)
            })
            .collect::<Vec<_>>();

        // Tell the metrics what this widget reads, so it is measured from the start.
        for el in &cfg.elements {
            el.names(&mut |n| m.mark(n));
        }
        for (_, d) in &defs {
            if let VarDef::Tpl(t) = d {
                t.names(&mut |n| m.mark(n));
            }
        }
        if let Some(bg) = &cfg.background {
            bg.color.names(&mut |n| m.mark(n));
        }

        let script = cfg.script.as_ref().and_then(|p| match Script::load(&cfg.resolve(p), &cfg.name) {
            Ok(s) => Some(s),
            Err(e) => {
                errors.push(format!("{}: script: {e}", cfg.name));
                None
            }
        });

        let states = cfg.elements.iter().map(|_| ElState::default()).collect();
        let w = Widget {
            cfg,
            states,
            vars: HashMap::new(),
            overrides: HashMap::new(),
            defs,
            script,
            script_ready: false,
            hovered: None,
            mouse_over: false,
            pending_actions: Vec::new(),
        };
        (w, errors)
    }

    pub fn ctx<'a>(&'a self, m: &'a Metrics, web: &'a Web) -> Ctx<'a> {
        Ctx {
            widget: &self.cfg.name,
            vars: &self.vars,
            overrides: &self.overrides,
            cfg: &self.cfg,
            script: self.script.as_ref(),
            m,
            web,
        }
    }

    /// Download jobs of this widget (URLs may use `[variables]`).
    pub fn web_jobs(&mut self, m: &Metrics, web: &Web) -> Vec<web::Job> {
        self.eval_vars(m, web);
        let ctx = self.ctx(m, web);
        self.cfg
            .web
            .iter()
            .map(|w| web::Job {
                key: format!("{}/{}", self.cfg.name, w.name),
                url: w.url.0.render(&ctx),
                interval: Duration::from_secs(w.interval.max(10)),
                format: w.format,
            })
            .collect()
    }

    fn eval_vars(&mut self, m: &Metrics, web: &Web) {
        let mut vars = HashMap::new();
        for (name, def) in &self.defs {
            if self.overrides.contains_key(name) {
                continue;
            }
            let v = match def {
                VarDef::Lit(v) => v.clone(),
                VarDef::Tpl(t) => {
                    let ctx = Ctx {
                        widget: &self.cfg.name,
                        vars: &vars,
                        overrides: &self.overrides,
                        cfg: &self.cfg,
                        script: self.script.as_ref(),
                        m,
                        web,
                    };
                    t.value(&ctx)
                }
            };
            vars.insert(name.clone(), Var::V(v));
        }
        self.vars = vars;
    }

    /// Recomputes variables, runs the Lua `update()` and records graph history.
    pub fn update(&mut self, m: &Metrics, web: &Web) {
        self.eval_vars(m, web);

        if let Some(script) = &self.script {
            let ctx = Ctx {
                widget: &self.cfg.name,
                vars: &self.vars,
                overrides: &self.overrides,
                cfg: &self.cfg,
                script: Some(script),
                m,
                web,
            };
            if !self.script_ready {
                self.script_ready = true;
                if let Err(e) = script.init(&ctx) {
                    crate::log::log(format!("[{}] init: {e}", self.cfg.name));
                }
            }
            match script.update(&ctx) {
                Ok(vals) => self.vars.extend(vals),
                Err(e) => crate::log::log(format!("[{}] update: {e}", self.cfg.name)),
            }
        }
        self.drain_script();

        let ctx = self.ctx(m, web);
        let samples: Vec<Option<f32>> = self
            .cfg
            .elements
            .iter()
            .map(|el| match &el.kind {
                Kind::Graph { value, .. } => Some(value.eval(&ctx).num() as f32),
                _ => None,
            })
            .collect();
        for ((st, s), el) in self.states.iter_mut().zip(samples).zip(&self.cfg.elements) {
            if let (Some(v), Kind::Graph { w, step, .. }) = (s, &el.kind) {
                st.history.push_back(v);
                let cap = (w / step.max(0.5)) as usize + 2;
                while st.history.len() > cap {
                    st.history.pop_front();
                }
            }
        }
    }

    fn drain_script(&mut self) {
        if let Some(s) = &self.script {
            self.overrides.extend(s.pending_vars.borrow_mut().drain(..));
            self.pending_actions.extend(s.pending_actions.borrow_mut().drain(..));
        }
    }

    /// Topmost element taking left/middle clicks under a point.
    pub fn clickable_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.cfg.elements.len()).rev().find(|&i| {
            self.cfg.elements[i].clickable()
                && self.states[i].bounds.is_some_and(|b| x >= b.left() && x < b.right() && y >= b.top() && y < b.bottom())
        })
    }

    /// Topmost interactive element under a point (widget coordinates).
    pub fn element_at(&self, x: f32, y: f32) -> Option<usize> {
        (0..self.cfg.elements.len()).rev().find(|&i| {
            self.cfg.elements[i].interactive()
                && self.states[i].bounds.is_some_and(|b| x >= b.left() && x < b.right() && y >= b.top() && y < b.bottom())
        })
    }

    /// The action strings (templates rendered) of an element for a mouse event.
    pub fn actions(&self, idx: usize, mouse: Mouse, m: &Metrics, web: &Web) -> Vec<String> {
        let Some(el) = self.cfg.elements.get(idx) else { return Vec::new() };
        let list = match mouse {
            Mouse::Left => &el.on_click,
            Mouse::Right => &el.on_right_click,
            Mouse::Middle => &el.on_middle_click,
            Mouse::ScrollUp => &el.on_scroll_up,
            Mouse::ScrollDown => &el.on_scroll_down,
        };
        let ctx = self.ctx(m, web);
        list.0.iter().map(|t| t.render(&ctx)).collect()
    }

    /// `!set:name=expression`
    pub fn set_var(&mut self, name: &str, src: &str, m: &Metrics, web: &Web) -> Result<(), String> {
        let v = crate::expr::parse(src)?.eval(&self.ctx(m, web));
        self.overrides.insert(name.to_string(), Var::V(v));
        Ok(())
    }

    /// `!lua:function`
    pub fn call_lua(&mut self, f: &str, m: &Metrics, web: &Web) -> Result<(), String> {
        let script = self.script.as_ref().ok_or("this widget has no script")?;
        script.call(f, &[], &self.ctx(m, web))?;
        self.drain_script();
        Ok(())
    }

    pub fn has_hover_effects(&self) -> bool {
        self.cfg.hover_opacity.is_some() || self.cfg.elements.iter().any(|e| e.interactive())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn widget(src: &str) -> Widget {
        let cfg: WidgetConfig = toml::from_str(src).unwrap();
        let (w, errors) = Widget::new(cfg, &Metrics::new());
        assert!(errors.is_empty(), "{errors:?}");
        w
    }

    #[test]
    fn variables_overrides_and_actions() {
        let mut w = widget(
            r##"
            name = "t"
            width = 100
            height = 50
            [variables]
            base = 10
            double = "{base * 2}"
            label = "Value: {double}"
            [[element]]
            type = "rect"
            x = 0
            y = 0
            w = 10
            h = 10
            color = "#FFFFFF"
            on_click = "!set:base=base+1"
            "##,
        );
        let (m, web) = (Metrics::new(), Web::default());
        w.update(&m, &web);
        let ctx = w.ctx(&m, &web);
        assert_eq!(ctx.var("label").unwrap().to_string(), "Value: 20");
        drop(ctx);
        let acts = w.actions(0, Mouse::Left, &m, &web);
        assert_eq!(acts, ["!set:base=base+1"]);
        w.set_var("base", "base+1", &m, &web).unwrap();
        w.update(&m, &web);
        assert_eq!(w.ctx(&m, &web).var("double").unwrap().to_string(), "22");
    }
}

#[cfg(test)]
mod drag_tests {
    use super::*;

    #[test]
    fn scroll_only_elements_do_not_block_dragging() {
        let cfg: WidgetConfig = toml::from_str(
            r##"
            name = "t"
            width = 100
            height = 100
            [[element]]
            type = "rect"
            x = 0
            y = 0
            w = 100
            h = 100
            color = "#FFFFFF"
            on_scroll_up = "!reload"
            [[element]]
            type = "rect"
            x = 0
            y = 0
            w = 20
            h = 20
            color = "#FFFFFF"
            on_click = "!reload"
            "##,
        )
        .unwrap();
        let m = Metrics::new();
        let (mut w, _) = Widget::new(cfg, &m);
        w.states[0].bounds = tiny_skia::Rect::from_xywh(0.0, 0.0, 100.0, 100.0);
        w.states[1].bounds = tiny_skia::Rect::from_xywh(0.0, 0.0, 20.0, 20.0);
        assert_eq!(w.clickable_at(50.0, 50.0), None); // drag works over the scroll area
        assert_eq!(w.element_at(50.0, 50.0), Some(0)); // ...and scrolling still reaches it
        assert_eq!(w.clickable_at(10.0, 10.0), Some(1)); // the button still clicks
    }
}
