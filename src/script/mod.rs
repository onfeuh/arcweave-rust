//! Script parsing and evaluation 

pub mod ast;
pub mod parser;

use core::fmt;

use rand::RngExt;

use crate::RuntimeValue::{self, Boolean, Float, Integer, NaN, String as RString};
use crate::project::ElementRef;
use crate::script::ast::*;
use crate::{Content, RuntimeError, RuntimeState};

pub struct Environment<'a> {
    pub state: RuntimeState<'a>,
}

impl<'a> Environment<'a> {
    pub fn new(state: &RuntimeState<'a>) -> Self {
        Self {
            state: state.clone(),
        }
    }

    pub fn into_state(self) -> RuntimeState<'a> {
        self.state
    }

    pub fn eval_branch(&mut self, raw: &str) -> Result<bool, RuntimeError> {
        let decoded = html_escape::decode_html_entities(raw);
        let (remaining, input) = parser::input(&decoded)
            .map_err(|e| RuntimeError::ParsingError { err: e.to_string() })?;

        if !remaining.is_empty() {
            return Err(RuntimeError::ParsingError {
                err: format!("unexpected trailing input: {:?}", remaining),
            });
        }

        match input {
            Input::Script(_) => Err(RuntimeError::ParsingError {
                err: "unexpected input, got content script instead of branch condition".to_string(),
            }),
            Input::Branch(expr) => Ok(self.eval_expr(&expr)?.to_bool()),
        }
    }

    pub fn build_content(&mut self, raw: &str) -> Result<Option<Content>, RuntimeError> {
        let decoded = html_escape::decode_html_entities(raw);
        let (remaining, input) = parser::input(&decoded)
            .map_err(|e| RuntimeError::ParsingError { err: e.to_string() })?;

        if !remaining.is_empty() {
            return Err(RuntimeError::ParsingError {
                err: format!("unexpected trailing input: {:?}", remaining),
            });
        }

        match input {
            Input::Script(stmt) => Ok(self.build_statement(&stmt)?),
            Input::Branch(_) => Err(RuntimeError::ParsingError {
                err: "unexpected input, got branch condition instead of content script".to_string(),
            }),
        }
    }

    fn build_statement(&mut self, stmt: &Statement) -> Result<Option<Content>, RuntimeError> {
        match stmt {
            Statement::Func(call) => {
                match self.eval_func(call)? {
                    RString(s) => Ok(Some(Content::Inline(s))),
                    _ => Ok(None), // i should probably find a better way to do this
                }
            }

            Statement::Paragraph(text) => Ok(Some(Content::Paragraph(text.to_string()))),

            Statement::Quote(stmts) => {
                let output: Vec<Content> = stmts
                    .iter()
                    .filter_map(|stmt| self.build_statement(stmt).ok()?)
                    .collect();
                Ok(Some(Content::Quote(output)))
            }

            Statement::Block(stmts) => {
                let output: Vec<Content> = stmts
                    .iter()
                    .filter_map(|stmt| self.build_statement(stmt).ok()?)
                    .collect();
                Ok(Some(Content::Block(output)))
            }

            Statement::Assign { ty, var, expr } => {
                let r = self.eval_expr(expr)?;
                let Variable(name) = var;

                let new_value = match ty {
                    AssignTy::Assign => r,
                    op => {
                        let l = self.state.get_var(name)?.clone();
                        match op {
                            AssignTy::AssignAdd => l.op_add(r)?,
                            AssignTy::AssignSub => l.op_sub(r)?,
                            AssignTy::AssignMul => l.op_mul(r)?,
                            AssignTy::AssignDiv => l.op_div(r)?,
                            AssignTy::AssignMod => l.op_mod(r)?,
                            AssignTy::Assign => unreachable!("already matched"),
                        }
                    }
                };
                self.state.set_var(name, new_value)?;
                Ok(None)
            }

            Statement::Condition { cond, then, alt } => match self.eval_expr(cond)?.to_bool() {
                true => {
                    let then = self.build_statement(then)?;
                    Ok(then)
                }
                false => match alt.clone().map(|t| self.build_statement(&t)) {
                    Some(t) => Ok(t?),
                    None => Ok(None),
                },
            },
        }
    }

    fn eval_expr(&mut self, expr: &Expression) -> Result<RuntimeValue, RuntimeError> {
        match expr {
            Expression::Value(v) => self.eval_value(v),
            Expression::Numeric(op) => self.eval_numeric(op),
        }
    }

    fn eval_value(&mut self, val: &ast::Value) -> Result<RuntimeValue, RuntimeError> {
        match val {
            ast::Value::Integer(i) => Ok(RuntimeValue::Integer(*i)),
            ast::Value::Float(f) => Ok(RuntimeValue::Float(*f)),
            ast::Value::Boolean(b) => Ok(RuntimeValue::Boolean(*b)),
            ast::Value::String(s) => Ok(RuntimeValue::String(s.clone())),
            ast::Value::Var(Variable(name)) => self.state.get_var(name).cloned(),
            ast::Value::Func(call) => self.eval_func(call),
            ast::Value::Mention(m) => Ok(RuntimeValue::String(m.label.clone().unwrap_or_default())),
        }
    }

    fn eval_numeric(&mut self, op: &NumericOp) -> Result<RuntimeValue, RuntimeError> {
        match op {
            NumericOp::UnaryOp { op, expr } => {
                let v = self.eval_expr(expr)?;
                match op {
                    UnaryOpTy::Plus => Ok(v),
                    UnaryOpTy::Minus => Ok(v.negate()),
                    UnaryOpTy::Not => Ok(Boolean(!v.to_bool())),
                }
            }

            NumericOp::BinaryOp { op, lhs, rhs } => {
                let l = self.eval_expr(lhs)?;
                let r = self.eval_expr(rhs)?;
                l.binary_op(r, op)
            }
        }
    }

    fn eval_func(&mut self, call: &FuncCall) -> Result<RuntimeValue, RuntimeError> {
        let eval_args = |evaluator: &mut Self| -> Result<Vec<RuntimeValue>, RuntimeError> {
            call.args.iter().map(|a| evaluator.eval_expr(a)).collect()
        };

        match call.func {
            FuncTy::Abs => {
                let args = eval_args(self)?;
                let [v] = args.as_slice() else {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Abs,
                        details: "expected one argument".to_string(),
                    });
                };
                Ok(v.clone().abs()?)
            }

            FuncTy::Max => eval_args(self)?
                .into_iter()
                .map(|v| v.clone().to_float().map(|f| (f, v)))
                .try_fold(None::<(f32, RuntimeValue)>, |acc, item| {
                    let (f, v) = item.map_err(|err| RuntimeError::InvalidArgs {
                        func: FuncTy::Max,
                        details: format!("{}", err),
                    })?;
                    Ok(Some(match acc {
                        None => (f, v),
                        Some((best_f, _)) if f > best_f => (f, v),
                        Some(prev) => prev,
                    }))
                })
                .and_then(|opt| {
                    opt.map(|(_, v)| v).ok_or(RuntimeError::InvalidArgs {
                        func: FuncTy::Max,
                        details: "expected at least one argument".to_string(),
                    })
                }),

            FuncTy::Min => eval_args(self)?
                .into_iter()
                .map(|v| v.clone().to_float().map(|f| (f, v)))
                .try_fold(None::<(f32, RuntimeValue)>, |acc, item| {
                    let (f, v) = item.map_err(|err| RuntimeError::InvalidArgs {
                        func: FuncTy::Min,
                        details: format!("{}", err),
                    })?;
                    Ok(Some(match acc {
                        None => (f, v),
                        Some((best_f, _)) if f < best_f => (f, v),
                        Some(prev) => prev,
                    }))
                })
                .and_then(|opt| {
                    opt.map(|(_, v)| v).ok_or(RuntimeError::InvalidArgs {
                        func: FuncTy::Min,
                        details: "expected at least one argument".to_string(),
                    })
                }),

            FuncTy::Round => {
                let args = eval_args(self)?;
                let [v] = args.as_slice() else {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Round,
                        details: "expected one argument".to_string(),
                    });
                };
                Ok(v.clone().round()?)
            }

            FuncTy::Sqr => {
                let args = eval_args(self)?;
                let [v] = args.as_slice() else {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Sqr,
                        details: "expected one argument".to_string(),
                    });
                };
                Ok(v.clone().sqr()?)
            }

            FuncTy::Sqrt => {
                let args = eval_args(self)?;
                let [v] = args.as_slice() else {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Sqrt,
                        details: "expected one argument".to_string(),
                    });
                };
                Ok(v.clone().sqrt()?)
            }

            FuncTy::Rand => match eval_args(self)?.is_empty() {
                true => Ok(RuntimeValue::randf()),
                false => Err(RuntimeError::InvalidArgs {
                    func: FuncTy::Rand,
                    details: "unexpected argument(s)".to_string(),
                }),
            },

            FuncTy::Roll => {
                let (sides, amount) = match eval_args(self)?.as_slice() {
                    [sides] => (sides.clone().to_float()? as i32, 1),
                    [sides, amount] => (
                        sides.clone().to_float()? as i32,
                        amount.clone().to_float()? as i32,
                    ),
                    _ => {
                        return Err(RuntimeError::InvalidArgs {
                            func: FuncTy::Roll,
                            details: "expected one or two arguments".to_string(),
                        });
                    }
                };

                if sides < 1 {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Roll,
                        details: format!("expected a positive number of sides, got {sides}"),
                    });
                }

                if amount < 1 {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Roll,
                        details: format!("expected a positive number of dices, got {amount}"),
                    });
                }

                let mut rng = rand::rng();
                let total = (0..amount).map(|_| rng.random_range(1..=sides)).sum();
                Ok(RuntimeValue::Integer(total))
            }

            FuncTy::Visits => {
                let args = &call.args;
                let [m] = args.as_slice() else {
                    return Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Visits,
                        details: "expected one argument".to_string(),
                    });
                };

                let m = match m {
                    Expression::Value(Value::Mention(mention)) => Ok(mention),
                    _ => Err(RuntimeError::InvalidArgs {
                        func: FuncTy::Visits,
                        details: "expected element mention".to_string(),
                    }),
                }?;

                let data_id = m.attrs.get("data-id").and_then(|v| v.as_deref()).ok_or(
                    RuntimeError::InvalidArgs {
                        func: FuncTy::Visits,
                        details: "mention must have a data-id attribute".to_string(),
                    },
                )?;

                Ok(RuntimeValue::Integer(
                    *self
                        .state
                        .visits
                        .get(&ElementRef::from(data_id))
                        .ok_or(RuntimeError::InvalidArgs {
                            func: FuncTy::Visits,
                            details: format!("no element found with id {}", data_id),
                        })
                        .unwrap_or(&0),
                ))
            }

            FuncTy::Show => {
                let result = call
                    .args
                    .iter()
                    .map(|arg| self.eval_expr(arg).map(|v| v.to_string()))
                    .collect::<Result<Vec<_>, _>>()?
                    .join("");
                Ok(RuntimeValue::String(result))
            }

            FuncTy::Reset => {
                let vars = call
                    .args
                    .iter()
                    .map(|arg| match arg {
                        Expression::Value(Value::Var(Variable(name))) => Ok(name.as_str()),
                        _ => Err(RuntimeError::InvalidArgs {
                            func: FuncTy::ResetAll,
                            details: "all arguments must be variables".to_string(),
                        }),
                    })
                    .collect::<Result<Vec<&str>, _>>()?;

                self.state.reset(vars)?;
                Ok(RuntimeValue::Boolean(false))
            }

            FuncTy::ResetAll => {
                let excludes = call
                    .args
                    .iter()
                    .map(|arg| match arg {
                        Expression::Value(Value::Var(Variable(name))) => Ok(name.as_str()),
                        _ => Err(RuntimeError::InvalidArgs {
                            func: FuncTy::ResetAll,
                            details: "all arguments must be variables".to_string(),
                        }),
                    })
                    .collect::<Result<Vec<&str>, _>>()?;

                excludes
                    .iter()
                    .try_for_each(|name| self.state.get_var(name).map(|_| ()))?;

                self.state.reset_all(excludes)?;
                Ok(RuntimeValue::Boolean(false))
            }

            FuncTy::ResetVisits => {
                self.state.reset_visits();
                Ok(RuntimeValue::Boolean(false))
            }
        }
    }
}

impl RuntimeValue {
    fn randf() -> Self {
        RuntimeValue::Float(rand::random::<f32>())
    }

    fn binary_op(self, r: RuntimeValue, op: &BinaryOpTy) -> Result<RuntimeValue, RuntimeError> {
        use BinaryOpTy::*;
        match op {
            Add => self.op_add(r),
            Sub => self.op_sub(r),
            Mul => self.op_mul(r),
            Div => self.op_div(r),
            Mod => self.op_mod(r),
            GreaterThan => self.compare(r, |o| o.is_gt()),
            GreaterThanEqual => self.compare(r, |o| o.is_ge()),
            LessThan => self.compare(r, |o| o.is_lt()),
            LessThanEqual => self.compare(r, |o| o.is_le()),
            Equal | Is => Ok(self.equal(r, true)),
            NotEqual | IsNot => Ok(self.equal(r, false)),
            And => Ok(Boolean(self.to_bool() && r.to_bool())),
            Or => Ok(Boolean(self.to_bool() || r.to_bool())),
        }
    }

    fn compare(
        self,
        r: RuntimeValue,
        predicate: impl Fn(std::cmp::Ordering) -> bool,
    ) -> Result<RuntimeValue, RuntimeError> {
        let ord = match (&self, &r) {
            (Integer(a), Integer(b)) => a.cmp(b),
            (RString(a), RString(b)) => a.cmp(b),
            _ => self
                .to_float()?
                .partial_cmp(&r.to_float()?)
                .ok_or(RuntimeError::UnexpectedNaN)?,
        };
        Ok(Boolean(predicate(ord)))
    }

    fn equal(self, r: RuntimeValue, expect_equal: bool) -> RuntimeValue {
        let eq = match (&self, &r) {
            (Integer(a), Integer(b)) => a == b,
            (Float(a), Float(b)) => a == b,
            (Boolean(a), Boolean(b)) => a == b,
            (RString(a), RString(b)) => a == b,
            (Integer(a), Float(b)) => (*a as f32) == *b,
            (Float(a), Integer(b)) => *a == (*b as f32),
            _ => false,
        };
        Boolean(if expect_equal { eq } else { !eq })
    }

    fn op_add(self, r: RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        match (&self, &r) {
            (Integer(a), Integer(b)) => Ok(Integer(a + b)),
            (RString(a), RString(b)) => Ok(RString(format!("{a}{b}"))),
            _ => Ok(Float(self.to_float()? + r.to_float()?)),
        }
    }

    fn op_sub(self, r: RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        match (&self, &r) {
            (Integer(a), Integer(b)) => Ok(Integer(a - b)),
            _ => Ok(Float(self.to_float()? - r.to_float()?)),
        }
    }

    fn op_mul(self, r: RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        match (&self, &r) {
            (Integer(a), Integer(b)) => Ok(Integer(a * b)),
            _ => Ok(Float(self.to_float()? * r.to_float()?)),
        }
    }

    fn op_div(self, r: RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        match &r {
            Integer(0) => return Err(RuntimeError::DivisionByZero),
            Float(f) if *f == 0.0 => return Err(RuntimeError::DivisionByZero),
            _ => {}
        }

        match (&self, &r) {
            (Integer(a), Integer(b)) => Ok(Integer(a / b)),
            _ => Ok(Float(self.to_float()? / r.to_float()?)),
        }
    }

    fn op_mod(self, r: RuntimeValue) -> Result<RuntimeValue, RuntimeError> {
        if matches!(&r, Integer(0)) {
            return Err(RuntimeError::DivisionByZero);
        }

        match (&self, &r) {
            (Integer(a), Integer(b)) => Ok(Integer(a % b)),
            _ => Ok(Float(self.to_float()? % r.to_float()?)),
        }
    }

    fn as_bool(&self) -> RuntimeValue {
        match self {
            Boolean(_) => self.clone(),
            Integer(i) => Boolean(*i != 0_i32),
            Float(f) => Boolean(*f != 0.0_f32),
            RString(s) => Boolean(!s.is_empty()),
            NaN => Boolean(false),
        }
    }

    fn to_bool(&self) -> bool {
        match self.clone().as_bool() {
            Boolean(b) => b,
            _ => false,
        }
    }

    fn as_num(&self) -> RuntimeValue {
        match self {
            Integer(_) => self.clone(),
            Float(_) => self.clone(),
            RString(_) => NaN,
            Boolean(b) => match b {
                true => Integer(1),
                false => Integer(0),
            },
            NaN => NaN,
        }
    }

    fn to_float(&self) -> Result<f32, RuntimeError> {
        match self.as_num() {
            Integer(i) => Ok(i as f32),
            Float(f) => Ok(f),
            _ => Err(RuntimeError::UnexpectedNaN),
        }
    }

    fn negate(self) -> RuntimeValue {
        match self.as_num() {
            Integer(i) => Integer(-i),
            Float(f) => Float(-f),
            _ => NaN,
        }
    }

    fn abs(self) -> Result<RuntimeValue, RuntimeError> {
        match self.as_num() {
            Integer(i) => Ok(Integer(i.abs())),
            Float(f) => Ok(Float(f.abs())),
            _ => Err(RuntimeError::UnexpectedNaN),
        }
    }

    fn round(self) -> Result<RuntimeValue, RuntimeError> {
        match self.as_num() {
            Integer(i) => Ok(Integer(i)),
            Float(f) => Ok(Integer(f.round() as i32)),
            _ => Err(RuntimeError::UnexpectedNaN),
        }
    }

    fn sqrt(self) -> Result<RuntimeValue, RuntimeError> {
        match self.as_num() {
            Integer(i) => Ok(Float((i as f32).sqrt())),
            Float(f) => Ok(Float(f.sqrt())),
            _ => Err(RuntimeError::UnexpectedNaN),
        }
    }

    fn sqr(self) -> Result<RuntimeValue, RuntimeError> {
        match self.as_num() {
            Integer(i) => Ok(Integer(i * i)),
            Float(f) => Ok(Float(f * f)),
            _ => Err(RuntimeError::UnexpectedNaN),
        }
    }
}

impl fmt::Display for RuntimeValue {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RuntimeValue::Integer(i) => write!(f, "{i}"),
            RuntimeValue::Float(fl) => write!(f, "{fl}"),
            RuntimeValue::Boolean(b) => write!(f, "{b}"),
            RuntimeValue::String(s) => write!(f, "{s}"),
            RuntimeValue::NaN => write!(f, "NaN"),
        }
    }
}
