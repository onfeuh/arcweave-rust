//! Core data structures for project serialization, deserialization and key resolving

use std::{collections::HashMap, fs::read_to_string};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{Content, RuntimeError, script::Environment};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub name: String,
    #[serde(rename = "startingElement")]
    pub starting_element: ElementRef,
    pub cover: Option<Cover>,
    #[serde(default)]
    pub boards: HashMap<BoardRef, Board>,
    #[serde(default)]
    pub notes: HashMap<NoteRef, Note>,
    #[serde(default)]
    pub elements: HashMap<ElementRef, Element>,
    #[serde(default)]
    pub jumpers: HashMap<JumperRef, Jumper>,
    #[serde(default)]
    pub connections: HashMap<ConnRef, Connection>,
    #[serde(default)]
    pub branches: HashMap<BranchRef, Branch>,
    #[serde(default)]
    pub components: HashMap<CompRef, Component>,
    #[serde(default)]
    pub attributes: HashMap<AttrRef, Attribute>,
    #[serde(default)]
    pub assets: HashMap<AssetRef, Asset>,
    #[serde(default)]
    pub variables: HashMap<VarRef, Variable>,
    #[serde(default)]
    pub conditions: HashMap<CondRef, Condition>,
}

#[derive(Error, Debug)]
pub enum ProjectInitError {
    #[error("failed to open file: `{0}`")]
    FileError(#[from] std::io::Error),
    #[error("failed to parse: `{0}`")]
    ParsingError(#[from] serde_json::Error),
}

impl Project {
    pub fn from_file(path: &str) -> Result<Self, ProjectInitError> {
        let s = read_to_string(path)?;
        Project::from_str(&s)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn from_str(s: &str) -> Result<Self, ProjectInitError> {
        serde_json::from_str(s).map_err(|e| e.into())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Board {
    Root {
        name: String,
        root: bool,
        #[serde(default)]
        children: Vec<BoardRef>,
    },
    Node {
        name: String,
        #[serde(default)]
        notes: Vec<NoteRef>,
        #[serde(default)]
        jumpers: Vec<JumperRef>,
        #[serde(default)]
        branches: Vec<BranchRef>,
        #[serde(default, rename = "customId")]
        custom_id: Option<String>,
        #[serde(default)]
        elements: Vec<ElementRef>,
        #[serde(default)]
        connections: Vec<ConnRef>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Element {
    pub theme: String,
    #[serde(default)]
    pub outputs: Vec<ConnRef>,
    #[serde(default)]
    pub attributes: Vec<AttrRef>,
    #[serde(default)]
    pub components: Vec<CompRef>,
    #[serde(default)]
    pub content: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Jumper {
    // pub width: u32,
    // pub height: u32,
    #[serde(rename = "elementId")]
    pub element_id: ElementRef,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "sourceType", content = "sourceid")]
pub enum SourceRef {
    #[serde(rename = "elements")]
    Element(ElementRef),
    #[serde(rename = "jumpers")]
    Jumper(JumperRef),
    #[serde(rename = "conditions")]
    Condition(CondRef),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "targetType", content = "targetid")]
pub enum TargetRef {
    #[serde(rename = "elements")]
    Element(ElementRef),
    #[serde(rename = "jumpers")]
    Jumper(JumperRef),
    #[serde(rename = "branches")]
    Branch(BranchRef),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Connection {
    #[serde(rename = "type")]
    pub ty: String,
    pub theme: String,
    #[serde(flatten)]
    pub source: SourceRef,
    #[serde(flatten)]
    pub target: TargetRef,
    #[serde(default, rename = "targetFace")]
    pub target_face: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Branch {
    pub theme: String,
    pub conditions: BranchConditions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BranchConditions {
    #[serde(rename = "ifCondition")]
    pub if_condition: CondRef,
    #[serde(rename = "elseCondition")]
    pub else_condition: Option<CondRef>,
    #[serde(default, rename = "elseIfConditions")]
    pub else_if_conditions: Vec<CondRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Component {
    Root {
        name: String,
        root: bool,
        #[serde(default)]
        children: Vec<CompRef>,
    },
    Node {
        name: String,
        #[serde(default)]
        assets: Option<ComponentAssets>,
        #[serde(default)]
        attributes: Vec<AttrRef>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentAssets {
    pub cover: Option<AssetSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attribute {
    #[serde(rename = "cId")]
    pub comp_id: CompRef,
    pub name: String,
    #[serde(rename = "cType")]
    pub ty: String,
    pub value: AttributeValue,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeValue {
    pub data: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default)]
    pub plain: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Asset {
    Root {
        root: bool,
        #[serde(default)]
        children: Vec<AssetRef>,
    },
    Node {
        name: String,
        #[serde(default, rename = "type")]
        ty: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssetSource {
    ById {
        id: AssetRef,
    },
    ByFile {
        file: String,
        #[serde(rename = "type")]
        ty: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Variable {
    Root {
        root: bool,
        #[serde(default)]
        children: Vec<VarRef>,
    },
    Board {
        name: String,
        #[serde(rename = "cId")]
        board_id: BoardRef,
        #[serde(flatten)]
        value: Value,
    },
    Global {
        name: String,
        #[serde(flatten)]
        value: Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum Value {
    #[serde(rename = "integer")]
    Integer(i32),
    #[serde(rename = "float")]
    Float(f32),
    #[serde(rename = "boolean")]
    Boolean(bool),
    #[serde(rename = "string")]
    String(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    pub output: ConnRef,
    pub script: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cover {
    pub file: String,
    #[serde(rename = "type")]
    pub ty: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    #[serde(default)]
    pub content: Option<String>,
}

pub trait Resolve {
    type Output;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Self::Output, RuntimeError>;
}

pub trait Build {
    fn build(&self, env: &mut Environment) -> Result<Option<Content>, RuntimeError>;
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct ElementRef(String);

impl Resolve for ElementRef {
    type Output = Element;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Element, RuntimeError> {
        project.elements.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for ElementRef {
    fn from(s: &str) -> Self {
        ElementRef(s.to_owned())
    }
}

impl ElementRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Build for Element {
    fn build(&self, env: &mut Environment) -> Result<Option<Content>, RuntimeError> {
        match &self.content {
            Some(string) => env.build_content(string),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct ConnRef(String);

impl Resolve for ConnRef {
    type Output = Connection;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Connection, RuntimeError> {
        project
            .connections
            .get(self)
            .ok_or(RuntimeError::InvalidRef {
                s: self.as_str().to_owned(),
            })
    }
}

impl From<&str> for ConnRef {
    fn from(s: &str) -> Self {
        ConnRef(s.to_owned())
    }
}

impl ConnRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Build for Connection {
    fn build(&self, env: &mut Environment) -> Result<Option<Content>, RuntimeError> {
        match &self.label {
            Some(string) => env.build_content(string),
            None => Ok(None),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct BoardRef(String);

impl Resolve for BoardRef {
    type Output = Board;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Board, RuntimeError> {
        project.boards.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for BoardRef {
    fn from(s: &str) -> Self {
        BoardRef(s.to_owned())
    }
}

impl BoardRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct NoteRef(String);

impl Resolve for NoteRef {
    type Output = Note;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Note, RuntimeError> {
        project.notes.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for NoteRef {
    fn from(s: &str) -> Self {
        NoteRef(s.to_owned())
    }
}

impl NoteRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct JumperRef(String);

impl Resolve for JumperRef {
    type Output = Jumper;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Jumper, RuntimeError> {
        project.jumpers.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for JumperRef {
    fn from(s: &str) -> Self {
        JumperRef(s.to_owned())
    }
}

impl JumperRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct BranchRef(String);

impl Resolve for BranchRef {
    type Output = Branch;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Branch, RuntimeError> {
        project.branches.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for BranchRef {
    fn from(s: &str) -> Self {
        BranchRef(s.to_owned())
    }
}

impl BranchRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct CompRef(String);

impl Resolve for CompRef {
    type Output = Component;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Component, RuntimeError> {
        project
            .components
            .get(self)
            .ok_or(RuntimeError::InvalidRef {
                s: self.as_str().to_owned(),
            })
    }
}

impl From<&str> for CompRef {
    fn from(s: &str) -> Self {
        CompRef(s.to_owned())
    }
}

impl CompRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct AttrRef(String);

impl Resolve for AttrRef {
    type Output = Attribute;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Attribute, RuntimeError> {
        project
            .attributes
            .get(self)
            .ok_or(RuntimeError::InvalidRef {
                s: self.as_str().to_owned(),
            })
    }
}

impl From<&str> for AttrRef {
    fn from(s: &str) -> Self {
        AttrRef(s.to_owned())
    }
}

impl AttrRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct AssetRef(String);

impl Resolve for AssetRef {
    type Output = Asset;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Asset, RuntimeError> {
        project.assets.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for AssetRef {
    fn from(s: &str) -> Self {
        AssetRef(s.to_owned())
    }
}

impl AssetRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct VarRef(String);

impl Resolve for VarRef {
    type Output = Variable;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Variable, RuntimeError> {
        project.variables.get(self).ok_or(RuntimeError::InvalidRef {
            s: self.as_str().to_owned(),
        })
    }
}

impl From<&str> for VarRef {
    fn from(s: &str) -> Self {
        VarRef(s.to_owned())
    }
}

impl VarRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
pub struct CondRef(String);

impl Resolve for CondRef {
    type Output = Condition;
    fn resolve<'a>(&self, project: &'a Project) -> Result<&'a Condition, RuntimeError> {
        project
            .conditions
            .get(self)
            .ok_or(RuntimeError::InvalidRef {
                s: self.as_str().to_owned(),
            })
    }
}

impl From<&str> for CondRef {
    fn from(s: &str) -> Self {
        CondRef(s.to_owned())
    }
}

impl CondRef {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use crate::project::Project;

    #[test]
    fn test_sample_project_parsing() {
        let p = Project::from_file("tests/game-engine-example-2026-03.json").unwrap();
    }
}
