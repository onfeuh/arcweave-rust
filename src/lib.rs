//! `arcweave-rust` is a rust implementation of the arcweave runtime
//! the crate tries to stay as close as possible to the behavior of the arcweave browser "[Play mode]"
//! and the [official implementation] through an extensive test suite
//! 
//! This crate features a custom parser made using [nom], full branch and segment code evaluation, 
//! useful and verbose errors and full safe access to all project items.
//!
//! [Play mode]: https://docs.arcweave.com/introduction/quick-tour/play-mode
//! [official implementation]: https://github.com/arcweave/arcscript-interpreters/tree/main
//! [nom]: https://docs.rs/nom/latest/nom/

#![warn(missing_docs)]

pub mod project;
pub mod script;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::{
    project::{
        BoardRef, Build, ConnRef, Connection, Element, ElementRef, Project, Resolve, VarRef,
    },
    script::{Environment, ast::FuncTy},
};

/// Errors that can occur during runtime evaluation with added context
#[derive(Debug, thiserror::Error)]
pub enum RuntimeErrorWithContext {
    #[error("failed to render {id}: {err}")]
    RenderError { id: String, err: RuntimeError },
}

/// Errors that can occur during runtime evaluation
#[derive(Debug, thiserror::Error)]
pub enum RuntimeError {
    /// A variable was referenced by name but not found in the given scope
    #[error("undefined variable: no variable `{name}` in scope `{scope}`")]
    UndefinedVariable { name: String, scope: String },

    /// A ref (element, connection, branch, etc.) could not be resolved in the project
    #[error("invalid reference: unable to resolve `{s}`")]
    InvalidRef { s: String },

    /// A script or condition string failed to parse
    #[error("parsing error: error {err}")]
    ParsingError { err: String },

    /// A function was called with the wrong number or type of arguments
    #[error("invalid function call: {func:?} {details}")]
    InvalidArgs { func: FuncTy, details: String },

    /// A numeric operation produced NaN where a valid number was required
    #[error("unexpected NaN")]
    UnexpectedNaN,

    /// A division or modulo operation was attempted with a zero divisor
    #[error("division by zero")]
    DivisionByZero,
}

/// A runtime value produced by evaluating an expression or reading an Arcweave project variable.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RuntimeValue {
    Integer(i32),
    Float(f32),
    Boolean(bool),
    String(String),
    /// Result of an invalid numeric operation
    NaN,
}

impl From<project::Value> for RuntimeValue {
    fn from(v: project::Value) -> Self {
        match v {
            project::Value::Integer(i) => RuntimeValue::Integer(i),
            project::Value::Float(f) => RuntimeValue::Float(f),
            project::Value::Boolean(b) => RuntimeValue::Boolean(b),
            project::Value::String(s) => RuntimeValue::String(s),
        }
    }
}

/// A project variable with its current and initial value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeVariable {
    name: String,
    scope: Option<BoardRef>,
    value: RuntimeValue,
    init_value: RuntimeValue,
}

impl TryFrom<project::Variable> for RuntimeVariable {
    type Error = String;

    fn try_from(v: project::Variable) -> Result<Self, Self::Error> {
        match v {
            project::Variable::Root { .. } => {
                Err("can't convert root variables to runtime variable".to_string())
            }
            project::Variable::Board {
                name,
                board_id,
                value,
            } => Ok(RuntimeVariable {
                name,
                scope: Some(board_id),
                value: value.clone().into(),
                init_value: value.into(),
            }),
            project::Variable::Global { name, value } => Ok(RuntimeVariable {
                name,
                scope: None,
                value: value.clone().into(),
                init_value: value.into(),
            }),
        }
    }
}

/// Rendered output from an element's content script.
#[derive(Debug, Clone)]
pub enum Content {
    /// A `<p>` block of plain text.
    Paragraph(String),
    /// An inline string produced by a `show()` call or a mention.
    Inline(String),
    /// A sequence of content nodes, produced by a multi-statement script block.
    Block(Vec<Content>),
    /// A `<blockquote>` block containing nested content nodes.
    Quote(Vec<Content>),
}


/// A snapshot of the runtime at a given point in execution.
#[derive(Debug, Clone)]
pub struct RuntimeState<'a> {
    /// Current values of all variables, keyed by their project ref.
    pub variables: HashMap<VarRef, RuntimeVariable>,
    /// Number of times each element has been visited in this playthrough.
    pub visits: HashMap<&'a ElementRef, i32>,
    /// The element currently being presented to the player.
    pub current_element: &'a ElementRef,
}

/// An owned version of [RuntimeState] that can be serialized and serialized
#[derive(Debug, Serialize, Deserialize)]
struct State {
    pub variables: HashMap<VarRef, RuntimeVariable>,
    pub visits: HashMap<ElementRef, i32>,
    pub current_element: ElementRef,
}

impl<'a> RuntimeState<'a> {
    /// Turns this runtime state into an owned, serializable version of itself
    fn to_data(&self) -> State {
        State {
            variables: self.variables.clone(),
            visits: self
                .visits
                .iter()
                .map(|(k, v)| ((*k).clone(), *v))
                .collect(),
            current_element: self.current_element.clone(),
        }
    }

    /// Create a runtime state referencing the local project from an owned, serializable state
    fn from_data(data: State, project: &'a Project) -> Result<Self, RuntimeError> {
        let current_element = project
            .elements
            .get_key_value(&data.current_element)
            .map(|(k, _)| k)
            .ok_or_else(|| RuntimeError::InvalidRef {
                s: data.current_element.as_str().to_owned(),
            })?;

        let visits = data
            .visits
            .into_iter()
            .map(|(k, v)| {
                project
                    .elements
                    .get_key_value(&k)
                    .map(|(key, _)| (key, v))
                    .ok_or_else(|| RuntimeError::InvalidRef {
                        s: k.as_str().to_owned(),
                    })
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        Ok(RuntimeState {
            variables: data.variables,
            visits,
            current_element,
        })
    }

    /// Deletes all visits records
    pub fn reset_visits(&mut self) {
        // self.visits.values_mut().for_each(|v| *v = 0);
        self.visits = HashMap::new()
    }

    /// Reset all variables but the ones provided to their initial values
    pub fn reset_all(&mut self, exclude: Vec<&str>) -> Result<(), RuntimeError> {
        self.variables
            .values_mut()
            .filter(|var| {
                let full_name = match &var.scope {
                    Some(board_id) => format!("{}.{}", board_id.as_str(), var.name),
                    None => var.name.clone(),
                };
                !exclude.contains(&full_name.as_str())
            })
            .for_each(|var| var.value = var.init_value.clone());
        Ok(())
    }

    /// Reset the given variables to their initial values
    pub fn reset(&mut self, vars: Vec<&str>) -> Result<(), RuntimeError> {
        vars.iter().try_for_each(|var_name| {
            let (board_id, name) = match var_name.split_once('.') {
                Some((board_id, v)) => (Some(board_id), v),
                None => (None, *var_name),
            };

            let var = self
                .variables
                .values_mut()
                .find(|var| {
                    var.name == name
                        && match (&var.scope, board_id) {
                            (Some(bid), Some(id)) => bid.as_str() == id,
                            (None, None) => true,
                            _ => false,
                        }
                })
                .ok_or_else(|| RuntimeError::UndefinedVariable {
                    name: name.to_string(),
                    scope: board_id.unwrap_or("global").to_string(),
                })?;

            var.value = var.init_value.clone();
            Ok(())
        })
    }

    /// Get the value for a given variable
    pub fn get_var(&self, name: &str) -> Result<&RuntimeValue, RuntimeError> {
        let (board_id, var_name) = match name.split_once('.') {
            Some((board_id, var_name)) => (Some(board_id), var_name),
            None => (None, name),
        };

        self.variables
            .values()
            .find(|var| {
                var.name == var_name
                    && match (&var.scope, board_id) {
                        (Some(bid), Some(id)) => bid.as_str() == id,
                        (None, None) => true,
                        _ => false,
                    }
            })
            .map(|var| &var.value)
            .ok_or_else(|| RuntimeError::UndefinedVariable {
                name: var_name.to_string(),
                scope: board_id.unwrap_or("global").to_string(),
            })
    }

    /// Set a new value for a given variable. Throws an error if the variable does not exist
    pub fn set_var(
        &mut self,
        name: &str,
        new_value: RuntimeValue,
    ) -> Result<&RuntimeVariable, RuntimeError> {
        let (board_id, var_name) = match name.split_once('.') {
            Some((board_id, var_name)) => (Some(board_id), var_name),
            None => (None, name),
        };

        let var = self
            .variables
            .values_mut()
            .find(|var| {
                var.name == var_name
                    && match (&var.scope, board_id) {
                        (Some(bid), Some(id)) => bid.as_str() == id,
                        (None, None) => true,
                        _ => false,
                    }
            })
            .ok_or_else(|| RuntimeError::UndefinedVariable {
                name: var_name.to_string(),
                scope: board_id.unwrap_or("global").to_string(),
            })?;

        var.value = new_value;
        Ok(var)
    }
}

/// The main entry point for executing an Arcweave project.
///
/// Holds a reference to the [`Project`] and a stack of [`RuntimeState`] snapshots,
/// one per [`follow`](Runtime::follow) or [`set_current_element`](Runtime::set_current_element)
/// call. The stack can be used for undo or save/load via [`Runtime::save`] and [`Runtime::load`].
///
/// # Example
/// ```rust
/// let project = Project::from_file("story.json")?;
/// let mut runtime = Runtime::new(&project);
/// let current_content = runtime.render_current_content()?;
/// ```
#[derive(Debug)]
pub struct Runtime<'a> {
    states: Vec<RuntimeState<'a>>,
    project: &'a Project,
}

impl<'a> Runtime<'a> {
    /// Initializes a new runtime from a deserialized Arcweave project
    pub fn new(project: &'a Project) -> Self {
        let init_state = RuntimeState {
            current_element: &project.starting_element,
            variables: project
                .variables
                .iter()
                .filter_map(|(k, v)| {
                    RuntimeVariable::try_from(v.clone())
                        .ok()
                        .map(|rv| (k.clone(), rv))
                })
                .collect(),
            visits: HashMap::new(), // visits: project.elements.keys().map(|id| (id, 0)).collect(),
        };

        Self {
            states: vec![init_state],
            project,
        }
    }

    /// Serializes the current states into a json string
    pub fn save(&self) -> Result<String, serde_json::Error> {
        let data: Vec<State> = self.states.iter().map(|s| s.to_data()).collect();
        serde_json::to_string(&data)
    }

   /// Updates the runtime with the provided serialized states
   /// This will fail if the given data references items that don't exist in the current project
    pub fn load(&mut self, saved: &str) -> Result<(), RuntimeError> {
        let data: Vec<State> = serde_json::from_str(saved)
            .map_err(|e| RuntimeError::ParsingError { err: e.to_string() })?;

        self.states = data
            .into_iter()
            .map(|d| RuntimeState::from_data(d, self.project))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(())
    }

    fn current_state(&self) -> &RuntimeState<'a> {
        &self.states[self.states.len() - 1]
    }

    /// Clears the states buffer, leaving only the current state
    pub fn flush(&mut self) {
        self.states = vec![self.states.pop().expect("states should never be empty")]
    }

    fn commit(&mut self, state: RuntimeState<'a>) {
        self.states.push(state);
    }

    /// Resolves and returns the current element from the project
    pub fn get_current_element(&self) -> Result<&Element, RuntimeError> {
        self.current_state().current_element.resolve(self.project)
    }

    /// Sets the current element, builds its content, and commits the new state
    pub fn set_current_element(
        &mut self,
        el: &'a ElementRef,
    ) -> Result<(), RuntimeErrorWithContext> {
        if el == self.current_state().current_element {
            return Ok(());
        }

        let mut env = Environment::new(self.current_state());
        env.state.current_element = el;
        *env.state.visits.entry(el).or_insert(0) += 1;
        el.resolve(self.project)
            .and_then(|e| e.build(&mut env))
            .map_err(|e| RuntimeErrorWithContext::RenderError {
                id: el.as_str().to_owned(),
                err: e,
            })?;

        self.commit(env.into_state());
        Ok(())
    }

    /// Renders the content of the current element
    pub fn render_current_content(&self) -> Result<Option<Content>, RuntimeErrorWithContext> {
        let current_el =
            self.get_current_element()
                .map_err(|e| RuntimeErrorWithContext::RenderError {
                    id: self.current_state().current_element.as_str().to_owned(),
                    err: e,
                })?;
        let mut env = Environment::new(self.current_state());
        current_el
            .clone()
            .build(&mut env)
            .map_err(|e| RuntimeErrorWithContext::RenderError {
                id: self.current_state().current_element.as_str().to_owned(),
                err: e,
            })
    }

    /// Renders the available paths from the current element
    pub fn render_current_options(
        &mut self,
    ) -> Result<HashMap<ConnRef, Option<Content>>, RuntimeErrorWithContext> {
        let current_el =
            self.get_current_element()
                .map_err(|e| RuntimeErrorWithContext::RenderError {
                    id: self.current_state().current_element.as_str().to_owned(),
                    err: e,
                })?;
        let connections: HashMap<ConnRef, Connection> = current_el
            .outputs
            .iter()
            .map(|conn_ref| {
                let conn = conn_ref
                    .resolve(self.project)
                    .map_err(|e| RuntimeErrorWithContext::RenderError {
                        id: conn_ref.as_str().to_owned(),
                        err: e,
                    })?
                    .clone();
                Ok((conn_ref.clone(), conn.clone()))
            })
            .collect::<Result<HashMap<_, _>, _>>()?;

        connections
            .into_iter()
            .map(|(r, c)| {
                let mut env = Environment::new(self.current_state());
                Ok((
                    r.clone(),
                    c.build(&mut env)
                        .map_err(|e| RuntimeErrorWithContext::RenderError {
                            id: r.as_str().to_owned(),
                            err: e,
                        })?,
                ))
            })
            .collect()
    }

    /// Follows a given connection and moves to the next element, updating the current state according to its content.
    pub fn follow(&mut self, path: &ConnRef) -> Result<(), RuntimeErrorWithContext> {
        let mut env = Environment::new(self.current_state());
        let mut current_path = path.clone();

        loop {
            let conn = current_path.resolve(self.project).map_err(|e| {
                RuntimeErrorWithContext::RenderError {
                    id: current_path.as_str().to_owned(),
                    err: e,
                }
            })?;

            let _ = conn.build(&mut env);

            match &conn.target {
                project::TargetRef::Element(element_ref) => {
                    env.state.current_element = element_ref;
                    *env.state.visits.entry(element_ref).or_insert(0) += 1;
                    element_ref
                        .resolve(self.project)
                        .and_then(|e| e.build(&mut env))
                        .map_err(|e| RuntimeErrorWithContext::RenderError {
                            id: element_ref.as_str().to_owned(),
                            err: e,
                        })?;
                    break;
                }
                project::TargetRef::Jumper(jumper_ref) => {
                    let element_ref = jumper_ref
                        .resolve(self.project)
                        .map(|j| &j.element_id)
                        .map_err(|e| RuntimeErrorWithContext::RenderError {
                            id: jumper_ref.as_str().to_owned(),
                            err: e,
                        })?;
                    env.state.current_element = element_ref;
                    *env.state.visits.entry(element_ref).or_insert(0) += 1;
                    element_ref
                        .resolve(self.project)
                        .and_then(|e| e.build(&mut env))
                        .map_err(|e| RuntimeErrorWithContext::RenderError {
                            id: element_ref.as_str().to_owned(),
                            err: e,
                        })?;
                    break;
                }
                project::TargetRef::Branch(branch_ref) => {
                    let branch = branch_ref.resolve(self.project).map_err(|e| {
                        RuntimeErrorWithContext::RenderError {
                            id: branch_ref.as_str().to_owned(),
                            err: e,
                        }
                    })?;

                    let winning_output = std::iter::once((&branch.conditions.if_condition, false))
                    .chain(branch.conditions.else_if_conditions.iter().map(|c| (c, false)))
                    .chain(branch.conditions.else_condition.iter().map(|c| (c, true)))
                    .find_map(|(cond_ref, is_else)| -> Option<Result<ConnRef, RuntimeErrorWithContext>> {
                        let cond = match cond_ref.resolve(self.project) {
                            Ok(c) => c,
                            Err(e) => return Some(Err(RuntimeErrorWithContext::RenderError {
                                id: cond_ref.as_str().to_owned(),
                                err: e,
                            })),
                        };
                        let passes = match &cond.script {
                            None => is_else,
                            Some(script) => match env.eval_branch(script) {
                                Ok(b) => b,
                                Err(e) => return Some(Err(RuntimeErrorWithContext::RenderError {
                                    id: cond_ref.as_str().to_owned(),
                                    err: e,
                                })),
                            },
                        };
                        passes.then(|| Ok(cond.output.clone()))
                    })
                    .transpose()?;

                    match winning_output {
                        Some(conn_ref) => current_path = conn_ref,
                        None => break,
                    }
                }
            }
        }

        self.commit(env.into_state());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        Runtime, RuntimeValue,
        project::{ConnRef, ElementRef, Project},
    };

    // ── helpers ──────────────────────────────────────────────────────────────

    fn load_project() -> Project {
        Project::from_file("tests/game-engine-example-2026-03.json")
            .expect("failed to load project")
    }

    fn el<'a>(project: &'a Project, id: &str) -> &'a ElementRef {
        project
            .elements
            .get_key_value(&ElementRef::from(id))
            .map(|(k, _)| k)
            .unwrap_or_else(|| panic!("element not found: {id}"))
    }

    fn conn(id: &str) -> ConnRef {
        ConnRef::from(id)
    }

    fn heal_wanda<'a>(runtime: &mut Runtime<'a>, project: &'a Project) {
        runtime
            .set_current_element(el(project, HEALER_START))
            .unwrap();
        runtime.follow(&conn(HEALER_OPT_HELP)).unwrap(); // → HEALER_ASK_HELP
        runtime.follow(&conn(HEALER_ASK_HELP_CONN)).unwrap(); // → HEALER_GET_POTION (have_potion=true)
        runtime
            .set_current_element(el(project, WANDA_START))
            .unwrap();
        runtime.follow(&conn(WANDA_OPT_EXAMINE)).unwrap(); // → WANDA_HEALTH_BAD
        runtime.follow(&conn(WANDA_OPT_GIVE)).unwrap(); // → WANDA_GAVE_POTION (wanda_health=70)
        runtime.follow(&conn(WANDA_GAVE_POTION_CONN)).unwrap(); // → WANDA_GOT_POTION
    }

    // ── element ids ──────────────────────────────────────────────────────────

    const PLAY_MODE_START: &str = "5bb43181-eebd-40b8-9561-2e58223c6016";
    const WANDA_START: &str = "5265ceac-f13f-47fc-a02c-6eda7eea6b90";
    const WANDA_HEALTH_BAD: &str = "63bee155-1348-4b82-a52d-24c96143c9db";
    const WANDA_ILL_BE_BACK: &str = "61b54b64-7376-4b5e-ae0d-cc1b390e16b7";
    const WANDA_GAVE_POTION: &str = "045ab2b6-6d77-43f7-a7b4-e275f41667c3";
    const WANDA_GOT_POTION: &str = "dbbbec36-4a9c-4cad-8533-c9f421a5093d";
    const WANDA_HELPED: &str = "62d0eaea-ccd4-4020-8bf9-7b00c7397684";
    const WANDA_NO_POTION: &str = "4d7d2516-68e0-450c-a580-65c0b2616965";
    const WANDA_GAVE_POTION_CONN: &str = "9327955d-269b-4c52-8c93-3172f7b4f721";
    const WANDA_DONT_KNOW: &str = "be67a949-dd00-4460-98c9-45067f0d331f";
    const HEALER_START: &str = "ddc209cc-7832-4ff6-ae1f-4eea592d07d4";
    const HEALER_ASK_HELP: &str = "540d5fc7-aa78-4b1a-8c11-bc2d4dc24ba9";
    const HEALER_GET_POTION: &str = "d852a577-bd1f-44cf-8187-77a86f97baef";
    const HEALER_NO_WORRIES: &str = "3893f5e9-d85e-4e5b-9423-e02ac03eb34f";
    const HEALER_ANOTHER: &str = "a8965b1b-4841-427e-8cf5-d8eda2f59bf2";
    const HEALER_ONLY_ONE: &str = "e14fa106-d418-4935-ab44-0de013995605";
    const HEALER_WRONG_EL: &str = "9c3607e5-4fb7-49db-ac8d-ccd7a1bf6d56";

    // ── connection ids ───────────────────────────────────────────────────────

    const WANDA_OPT_GLAD: &str = "8a89cabd-7542-48db-b501-3c0019145989";
    const WANDA_OPT_EXAMINE: &str = "02dfbe78-ae86-4525-8af9-51a54d58193b";
    const WANDA_OPT_BACK: &str = "4a58bb7e-43b4-4cc9-9a84-9c05618ad8ac";
    const WANDA_OPT_GIVE: &str = "62a9e84c-9652-4636-a796-73a75488bb79";
    const WANDA_OPT_DUNNO: &str = "60df4786-8a65-46ee-a3e9-1df36a5e1f39";
    const HEALER_OPT_HELP: &str = "0879ddb5-e1a2-4b95-a28c-d4df8df845e9";
    const HEALER_OPT_WRONG: &str = "993d8d9a-ff84-48eb-b531-7b9b2d37cb91";
    const HEALER_ASK_HELP_CONN: &str = "90872a7c-0f63-472b-8330-a6b14d824aeb";
    const WANDA_DONT_KNOW_CONN: &str = "d334c938-ed9e-4869-8307-1dc3a2a1149f";
    const HEALER_ANOTHER_CONN: &str = "cff4ab38-b38b-4aca-981e-c1e04561421b";
    const HEALER_WRONG_EL_CONN: &str = "c6932ce2-0e49-4f25-8595-6432208338c2";

    // ── tests ────────────────────────────────────────────────────────────────

    #[test]
    fn test_initial_state() {
        let project = load_project();
        let runtime = Runtime::new(&project);
        let state = runtime.current_state();

        assert_eq!(state.current_element, el(&project, PLAY_MODE_START));

        assert!(matches!(
            state.get_var("wanda_health"),
            Ok(RuntimeValue::Integer(20))
        ));
        assert!(matches!(
            state.get_var("healer_health"),
            Ok(RuntimeValue::Integer(50))
        ));
        assert!(matches!(
            state.get_var("have_potion"),
            Ok(RuntimeValue::Boolean(false))
        ));
    }

    #[test]
    fn test_wanda_wounded_on_start() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, WANDA_START))
            .unwrap();

        let content = runtime.render_current_content().unwrap();
        assert!(content.is_some());

        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_START)
        );
        assert!(matches!(
            runtime.current_state().get_var("wanda_health"),
            Ok(RuntimeValue::Integer(20))
        ));
    }

    #[test]
    fn test_examine_wanda_bad_health() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, WANDA_START))
            .unwrap();

        runtime.follow(&conn(WANDA_OPT_EXAMINE)).unwrap();

        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_HEALTH_BAD)
        );
    }

    #[test]
    fn test_full_help_wanda_scenario() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, WANDA_START))
            .unwrap();
        assert!(matches!(
            runtime.current_state().get_var("wanda_health"),
            Ok(RuntimeValue::Integer(20))
        ));

        runtime.follow(&conn(WANDA_OPT_BACK)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_ILL_BE_BACK)
        );

        let options = runtime.render_current_options().unwrap();
        assert_eq!(options.len(), 1);

        runtime
            .set_current_element(el(&project, HEALER_START))
            .unwrap();

        runtime.follow(&conn(HEALER_OPT_HELP)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_ASK_HELP)
        );

        runtime.follow(&conn(HEALER_ASK_HELP_CONN)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_GET_POTION)
        );
        assert!(matches!(
            runtime.current_state().get_var("have_potion"),
            Ok(RuntimeValue::Boolean(true))
        ));

        runtime
            .set_current_element(el(&project, WANDA_START))
            .unwrap();
        runtime.follow(&conn(WANDA_OPT_EXAMINE)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_HEALTH_BAD)
        );

        runtime.follow(&conn(WANDA_OPT_GIVE)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_GAVE_POTION)
        );
        assert!(matches!(
            runtime.current_state().get_var("wanda_health"),
            Ok(RuntimeValue::Integer(70))
        ));
        assert!(matches!(
            runtime.current_state().get_var("have_potion"),
            Ok(RuntimeValue::Boolean(false))
        ));

        runtime.follow(&conn(WANDA_GAVE_POTION_CONN)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_GOT_POTION)
        );
    }

    #[test]
    fn test_give_potion_without_having_it() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, WANDA_START))
            .unwrap();
        runtime.follow(&conn(WANDA_OPT_EXAMINE)).unwrap();

        runtime.follow(&conn(WANDA_OPT_DUNNO)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_DONT_KNOW)
        );

        runtime.follow(&conn(WANDA_DONT_KNOW_CONN)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_NO_POTION)
        );
    }

    #[test]
    fn test_healer_refuses_second_potion() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, HEALER_START))
            .unwrap();
        runtime.follow(&conn(HEALER_OPT_HELP)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_ASK_HELP)
        );
        runtime.follow(&conn(HEALER_ASK_HELP_CONN)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_GET_POTION)
        );
        assert!(matches!(
            runtime.current_state().get_var("have_potion"),
            Ok(RuntimeValue::Boolean(true))
        ));

        runtime
            .set_current_element(el(&project, HEALER_START))
            .unwrap();
        runtime.follow(&conn(HEALER_OPT_HELP)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_ANOTHER)
        );

        runtime.follow(&conn(HEALER_ANOTHER_CONN)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_ONLY_ONE)
        );
    }

    #[test]
    fn test_healer_wrong_turn() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, HEALER_START))
            .unwrap();
        runtime.follow(&conn(HEALER_OPT_WRONG)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_WRONG_EL)
        );

        runtime.follow(&conn(HEALER_WRONG_EL_CONN)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, HEALER_NO_WORRIES)
        );
    }

    #[test]
    fn test_wanda_glad_branch_requires_healed() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        heal_wanda(&mut runtime, &project);
        assert!(matches!(
            runtime.current_state().get_var("wanda_health"),
            Ok(RuntimeValue::Integer(70))
        ));

        runtime
            .set_current_element(el(&project, WANDA_START))
            .unwrap();
        runtime.follow(&conn(WANDA_OPT_GLAD)).unwrap();
        assert_eq!(
            runtime.current_state().current_element,
            el(&project, WANDA_HELPED)
        );
    }

    #[test]
    fn test_save_load_roundtrip() {
        let project = load_project();
        let mut runtime = Runtime::new(&project);

        runtime
            .set_current_element(el(&project, HEALER_START))
            .unwrap();
        runtime.follow(&conn(HEALER_OPT_HELP)).unwrap();
        runtime.follow(&conn(HEALER_ASK_HELP_CONN)).unwrap();

        let saved = runtime.save().unwrap();

        let mut runtime2 = Runtime::new(&project);
        runtime2.load(&saved).unwrap();

        assert_eq!(
            runtime2.current_state().current_element,
            el(&project, HEALER_GET_POTION)
        );
        assert!(matches!(
            runtime2.current_state().get_var("have_potion"),
            Ok(RuntimeValue::Boolean(true))
        ));
    }
}
