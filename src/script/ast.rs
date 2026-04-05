use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Input {
    Script(Statement),
    Branch(Expression),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Statement {
    Func(FuncCall),
    Paragraph(String),
    Quote(Vec<Statement>),
    Block(Vec<Statement>),
    Assign {
        ty: AssignTy,
        var: Variable,
        expr: Expression,
    },
    Condition {
        cond: Expression,
        then: Box<Statement>,
        alt: Option<Box<Statement>>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expression {
    Value(Value),
    Numeric(NumericOp),
}

#[derive(Debug, Clone, PartialEq)]
pub enum NumericOp {
    UnaryOp {
        op: UnaryOpTy,
        expr: Box<Expression>,
    },
    BinaryOp {
        op: BinaryOpTy,
        lhs: Box<Expression>,
        rhs: Box<Expression>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Float(f32),
    Integer(i32),
    String(String),
    Boolean(bool),
    Var(Variable),
    Func(FuncCall),
    Mention(Mention),
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOpTy {
    Plus,
    Minus,
    Not,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOpTy {
    Add,
    Sub,
    Mod,
    Div,
    Mul,
    And,
    Or,
    GreaterThan,
    GreaterThanEqual,
    LessThan,
    LessThanEqual,
    Equal,
    NotEqual,
    Is,
    IsNot,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FuncCall {
    pub func: FuncTy,
    pub args: Vec<Expression>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FuncTy {
    Abs,
    Max,
    Min,
    Rand,
    Roll,
    Round,
    Sqr,
    Sqrt,
    Visits,
    // Void
    Show,
    Reset,
    ResetAll,
    ResetVisits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Mention {
    pub label: Option<String>,
    pub attrs: HashMap<String, Option<String>>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AssignTy {
    Assign,
    AssignAdd,
    AssignSub,
    AssignMul,
    AssignDiv,
    AssignMod,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Variable(pub String);
