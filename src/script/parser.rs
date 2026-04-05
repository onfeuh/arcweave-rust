use std::collections::HashMap;

use nom::{
    IResult, Parser,
    branch::alt,
    bytes::complete::{tag, take_until, take_while, take_while1},
    character::complete::{char, digit0, digit1, multispace0, one_of, satisfy},
    combinator::{eof, map, map_res, opt, recognize, value},
    error::ParseError,
    multi::{fold_many0, many0, separated_list0},
    sequence::{delimited, pair, preceded},
};

use crate::script::ast::{
    AssignTy, BinaryOpTy, Expression, FuncCall, FuncTy, Input, Mention, NumericOp, Statement,
    UnaryOpTy, Value, Variable,
};

// arcweave/arcscript-interpreters/grammar/ArcscriptParser.g4 -- 29afa5a
// arcweave/arcscript-interpreters/grammar/ArcscriptLexer.g4 -- 0e247fa

// input: script EOF | codestart compound_condition_or codeend EOF;
pub fn input(input: &str) -> IResult<&str, Input> {
    alt((
        map(compound_condition_or, Input::Branch),
        map(pair(script, eof), |(stmt, _)| Input::Script(stmt)),
    ))
    .parse(input)
}

// script: script_section+;
// script_section:
// 	blockquote+?
//	| paragraph+?
//	| assignment_segment
//	| function_call_segment
//	| conditional_section;
fn script(input: &str) -> IResult<&str, Statement> {
    ws(map(
        many0(preceded(
            multispace0, // Trim/consume whitespace before each element
            alt((
                ws(assignment_segment),
                ws(function_call_segment),
                ws(conditional_section),
                ws(blockquote),
                ws(paragraph),
            )),
        )),
        Statement::Block,
    ))
    .parse(input)
}

// STRING_CONTENT: ~[\\\r\n'"] | '\\' [abfnrtv'"\\]
fn string_content(input: &str) -> IResult<&str, &str> {
    alt((
        take_while1(|c| !"\\\r\n'\"".contains(c)),
        recognize(pair(tag("\\"), one_of("abfnrtv'\"\\"))),
    ))
    .parse(input)
}

// STRING: '"' STRING_CONTENT* '"' | '\'' STRING_CONTENT* '\''
fn string(input: &str) -> IResult<&str, Expression> {
    alt((
        map(
            delimited(tag("\""), recognize(many0(string_content)), tag("\"")),
            |s: &str| Expression::Value(Value::String(s.to_owned())),
        ),
        map(
            delimited(tag("\\"), recognize(many0(string_content)), tag("\\")),
            |s: &str| Expression::Value(Value::String(s.to_owned())),
        ),
    ))
    .parse(input)
}

// BOOLEAN: 'true' | 'false';
fn boolean(input: &str) -> IResult<&str, Expression> {
    alt((
        value(Expression::Value(Value::Boolean(true)), tag("true")),
        value(Expression::Value(Value::Boolean(false)), tag("false")),
    ))
    .parse(input)
}

// blockquote: BLOCKQUOTESTART (paragraph | assignment_segment | function_call_segment)* BLOCKQUOTEEND;
fn blockquote(input: &str) -> IResult<&str, Statement> {
    delimited(
        (tag("<blockquote"), take_until(">"), tag(">")),
        map(
            many0(ws(alt((
                assignment_segment,
                function_call_segment,
                paragraph,
            )))),
            Statement::Quote,
        ),
        tag("</blockquote>"),
    )
    .parse(input)
}

// paragraph: paragraph_start PARAGRAPHEND { this.currentLine++;};
fn paragraph(input: &str) -> IResult<&str, Statement> {
    map(
        alt((
            delimited(
                (tag("<p "), take_until(">"), tag(">")),
                take_until("</p>"),
                tag("</p>"),
            ),
            delimited(tag("<p>"), take_until("</p>"), tag("</p>")),
        )),
        |s: &str| Statement::Paragraph(s.trim().to_string()),
    )
    .parse(input)
}

// CODESTART: '<pre' (~('>'))* '><code' (~('>'))* '>'
fn code_start(input: &str) -> IResult<&str, &str> {
    let (input, _) = multispace0(input)?;
    let (input, _) = tag("<pre")(input)?;
    let (input, _) = take_until(">")(input)?;
    let (input, _) = tag(">")(input)?;
    let (input, _) = take_while(|c: char| c.is_whitespace())(input)?;
    let (input, _) = tag("<code")(input)?;
    let (input, _) = take_until(">")(input)?;
    let (input, _) = tag(">")(input)?;
    let (input, _) = multispace0(input)?;
    Ok((input, ""))
}

fn code_end(input: &str) -> IResult<&str, &str> {
    ws(tag("</code></pre>")).parse(input)
}

// assignment_segment: codestart statement_assignment codeend;
fn assignment_segment(input: &str) -> IResult<&str, Statement> {
    delimited(code_start, ws(statement_assignment), code_end).parse(input)
}

// function_call_segment: codestart statement_function_call codeend;
fn function_call_segment(input: &str) -> IResult<&str, Statement> {
    delimited(code_start, ws(void_function_call), code_end).parse(input)
}

// conditional_section: if_section else_if_section* else_section? endif_segment;
fn conditional_section(input: &str) -> IResult<&str, Statement> {
    // if section
    let (input, cond_if) = pair(
        delimited(
            code_start,
            preceded(preceded(tag("if"), multispace0), compound_condition_or),
            code_end,
        ),
        script,
    )
    .parse(input)?;

    // else_if_section*
    let (input, cond_else_ifs) = many0(pair(
        delimited(
            code_start,
            preceded(preceded(tag("elseif"), multispace0), compound_condition_or),
            code_end,
        ),
        script,
    ))
    .parse(input)?;

    // else_section?
    let (input, cond_else) = opt(preceded(
        delimited(code_start, preceded(tag("else"), multispace0), code_end),
        script,
    ))
    .parse(input)?;

    // endif_segment
    let (input, _) = delimited(code_start, tag("endif"), code_end).parse(input)?;

    // folding
    let alt = cond_else_ifs
        .into_iter()
        .rev()
        .fold(cond_else.map(Box::new), |acc, (cond, then)| {
            Some(Box::new(Statement::Condition {
                cond,
                then: Box::new(then),
                alt: acc,
            }))
        });

    Ok((
        input,
        Statement::Condition {
            cond: cond_if.0,
            then: Box::new(cond_if.1),
            alt,
        },
    ))
}

/*
statement_assignment:
    VARIABLE (
        ASSIGNADD | ASSIGNSUB | ASSIGNMUL | ASSIGNDIV | ASSIGNMOD | ASSIGN
    ) compound_condition_or;
*/
fn statement_assignment(input: &str) -> IResult<&str, Statement> {
    let (input, var) = variable(input)?;
    let (input, ty) = ws(alt((
        value(AssignTy::AssignAdd, tag("+=")),
        value(AssignTy::AssignSub, tag("-=")),
        value(AssignTy::AssignMul, tag("*=")),
        value(AssignTy::AssignDiv, tag("/=")),
        value(AssignTy::AssignMod, tag("%=")),
        value(AssignTy::Assign, tag("=")),
    )))
    .parse(input)?;
    let (input, expr) = compound_condition_or(input)?;

    Ok((input, Statement::Assign { ty, var, expr }))
}

// argument: additive_numeric_expression | STRING | mention;
fn argument(input: &str) -> IResult<&str, Expression> {
    ws(alt((string, mention, additive_numeric_expression))).parse(input)
}

// mention_attributes: ATTR_NAME (TAG_EQUALS ATTR_VALUE)?;
fn mention_attributes(input: &str) -> IResult<&str, (String, Option<String>)> {
    let (input, name) = attr_name(input)?;
    let (input, value) = opt(preceded(tag("="), attr_value)).parse(input)?;
    Ok((input, (name, value)))
}

// ATTR_NAME: [:a-zA-Z] [:a-zA-Z0-9_.-]*;
fn attr_name(input: &str) -> IResult<&str, String> {
    map(
        recognize(pair(
            satisfy(|c| c == ':' || c.is_ascii_alphabetic()),
            take_while(|c: char| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'),
        )),
        |s: &str| s.to_string(),
    )
    .parse(input)
}

// ATTR_VALUE: DOUBLE_QUOTE_STRING | SINGLE_QUOTE_STRING | ATTCHARS | HEXCHARS | DECCHARS
fn attr_value(input: &str) -> IResult<&str, String> {
    let (input, _) = take_while(|c: char| c == ' ').parse(input)?;

    alt((
        // DOUBLE_QUOTE_STRING
        map(
            delimited(tag("\""), take_while(|c| c != '<' && c != '"'), tag("\"")),
            |s: &str| s.to_string(),
        ),
        // SINGLE_QUOTE_STRING
        map(
            delimited(tag("'"), take_while(|c| c != '<' && c != '\''), tag("'")),
            |s: &str| s.to_string(),
        ),
        // ATTCHARS, HEXCHARS, DECCHARS
        map(
            take_while1(|c: char| {
                c == '-'
                    || c == '_'
                    || c == '.'
                    || c == '/'
                    || c == '+'
                    || c == ','
                    || c == '?'
                    || c == '='
                    || c == ':'
                    || c == ';'
                    || c == '#'
                    || c.is_ascii_alphanumeric()
            }),
            |s: &str| s.to_string(),
        ),
    ))
    .parse(input)
}

// mention: MENTION_TAG_OPEN mention_attributes* '>' MENTION_LABEL? TAG_OPEN MENTION_TAG_CLOSE
fn mention(input: &str) -> IResult<&str, Expression> {
    let (input, _) = tag("<span")(input)?;
    let (input, attrs) = fold_many0(
        preceded(multispace0, mention_attributes),
        HashMap::new,
        |mut acc: HashMap<String, Option<String>>, (name, value)| {
            acc.insert(name, value);
            acc
        },
    )
    .parse(input)?;
    let (input, _) = multispace0(input)?;
    let (input, _) = tag(">")(input)?;
    let (input, label) = opt(take_while(|c| c != '<')).parse(input)?;
    let (input, _) = tag("<")(input)?;
    let (input, _) = tag("/span>")(input)?;

    Ok((
        input,
        Expression::Value(Value::Mention(Mention {
            label: label.map(|s| s.into()),
            attrs,
        })),
    ))
}

/*
additive_numeric_expression:
    multiplicative_numeric_expression
    | additive_numeric_expression (ADD | SUB) multiplicative_numeric_expression;
*/
fn additive_numeric_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = multiplicative_numeric_expression(input)?;

    let (input, pairs) = many0(pair(
        ws(alt((
            value(BinaryOpTy::Add, tag("+")),
            value(BinaryOpTy::Sub, tag("-")),
        ))),
        multiplicative_numeric_expression,
    ))
    .parse(input)?;

    let expr = pairs.into_iter().fold(first, |lhs, (op, rhs)| {
        Expression::Numeric(NumericOp::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    });

    Ok((input, expr))
}

/*
multiplicative_numeric_expression:
    signed_unary_numeric_expression
    | multiplicative_numeric_expression (MUL | DIV | MOD) signed_unary_numeric_expression;
*/
fn multiplicative_numeric_expression(input: &str) -> IResult<&str, Expression> {
    let (input, first) = signed_unary_numeric_expression(input)?;

    let (input, pairs) = many0(pair(
        ws(alt((
            value(BinaryOpTy::Mul, tag("*")),
            value(BinaryOpTy::Div, tag("/")),
            value(BinaryOpTy::Mod, tag("%")),
        ))),
        signed_unary_numeric_expression,
    ))
    .parse(input)?;

    let expr = pairs.into_iter().fold(first, |lhs, (op, rhs)| {
        Expression::Numeric(NumericOp::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    });

    Ok((input, expr))
}

/*
signed_unary_numeric_expression:
    sign unary_numeric_expression
    | unary_numeric_expression;
*/
fn signed_unary_numeric_expression(input: &str) -> IResult<&str, Expression> {
    let (input, sign) = opt(preceded(
        multispace0,
        alt((
            value(UnaryOpTy::Plus, tag("+")),
            value(UnaryOpTy::Minus, tag("-")),
        )),
    ))
    .parse(input)?;

    let (input, expr) = unary_numeric_expression(input)?;

    match sign {
        Some(op) => Ok((
            input,
            Expression::Numeric(NumericOp::UnaryOp {
                op,
                expr: Box::new(expr),
            }),
        )),
        None => Ok((input, expr)),
    }
}

/*
unary_numeric_expression:
    FLOAT
    | VARIABLE {this.assertVariable($VARIABLE);}
    | INTEGER
    | STRING
    | BOOLEAN
    | function_call
    | LPAREN compound_condition_or RPAREN;
*/
fn unary_numeric_expression(input: &str) -> IResult<&str, Expression> {
    alt((
        // FLOAT
        map(
            map_res(recognize((digit0, char('.'), digit1)), |s: &str| {
                s.parse::<f32>()
            }),
            |f: f32| Expression::Value(Value::Float(f)),
        ),
        // function_call
        function_call,
        // VARIABLE
        map(variable, |v| Expression::Value(Value::Var(v))),
        // INTEGER
        map(map_res(digit1, |s: &str| s.parse::<i32>()), |f: i32| {
            Expression::Value(Value::Integer(f))
        }),
        // STRING
        string,
        // BOOLEAN
        boolean,
        // LPAREN compound_condition_or RPAREN;
        delimited(tag("("), compound_condition_or, tag(")")),
    ))
    .parse(input)
}

// VARIABLE (helper)
fn variable(input: &str) -> IResult<&str, Variable> {
    map(
        recognize(pair(
            satisfy(|c| c.is_ascii_alphabetic() || c == '$' || c == '_'),
            take_while(|c: char| c.is_ascii_alphanumeric() || c == '$' || c == '_'),
        )),
        |s: &str| Variable(s.to_owned()),
    )
    .parse(input)

    // TODO => ASSERT VARIABLE !
}

/*
function_call:
    FNAME LPAREN argument_list? RPAREN {this.assertFunctionArguments($FNAME, $argument_list.ctx);};
*/
fn function_call(input: &str) -> IResult<&str, Expression> {
    let (input, name) = alt((
        value(FuncTy::Abs, tag("abs")),
        value(FuncTy::Sqr, tag("sqr")),
        value(FuncTy::Sqrt, tag("sqrt")),
        value(FuncTy::Min, tag("min")),
        value(FuncTy::Max, tag("max")),
        value(FuncTy::Rand, tag("random")),
        value(FuncTy::Roll, tag("roll")),
        value(FuncTy::Round, tag("round")),
        value(FuncTy::Visits, tag("visits")),
    ))
    .parse(input)?;

    let (input, args) = delimited(
        tag("("),
        opt(separated_list0(char(','), argument)),
        tag(")"),
    )
    .parse(input)?;

    Ok((
        input,
        Expression::Value(Value::Func(FuncCall {
            func: name,
            args: args.unwrap_or_default(),
        })),
    ))
}

/*
void_function_call:
    VFNAME LPAREN argument_list? RPAREN {this.assertFunctionArguments($VFNAME, $argument_list.ctx);}
    | VFNAMEVARS LPAREN variable_list? RPAREN {this.assertFunctionArguments($VFNAMEVARS, $variable_list.ctx);
        };
*/
fn void_function_call(input: &str) -> IResult<&str, Statement> {
    let (input, name) = alt((
        value(FuncTy::Show, tag("show")),
        value(FuncTy::Reset, tag("reset")),
        value(FuncTy::ResetAll, tag("resetAll")),
        value(FuncTy::ResetVisits, tag("resetVisits")),
    ))
    .parse(input)?;

    let (input, args) = delimited(
        tag("("),
        opt(separated_list0(char(','), argument)),
        tag(")"),
    )
    .parse(input)?;

    Ok((
        input,
        Statement::Func(FuncCall {
            func: name,
            args: args.unwrap_or_default(),
        }),
    ))
}

/*
compound_condition_or:
    compound_condition_and (
        ( OR | ORKEYWORD) compound_condition_or
    )?;
*/
fn compound_condition_or(input: &str) -> IResult<&str, Expression> {
    let (input, first) = compound_condition_and(input)?;

    let (input, pairs) = many0(pair(
        ws(alt((
            value(BinaryOpTy::Or, tag("||")),
            value(BinaryOpTy::Or, tag("or")),
        ))),
        compound_condition_and,
    ))
    .parse(input)?;

    let expr = pairs.into_iter().fold(first, |lhs, (op, rhs)| {
        Expression::Numeric(NumericOp::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    });

    Ok((input, expr))
}

/*
compound_condition_and:
    negated_unary_condition (
        (AND | ANDKEYWORD) compound_condition_and
    )?;
*/
fn compound_condition_and(input: &str) -> IResult<&str, Expression> {
    let (input, first) = negated_unary_condition(input)?;

    let (input, pairs) = many0(pair(
        ws(alt((
            value(BinaryOpTy::And, tag("&&")),
            value(BinaryOpTy::And, tag("and")),
        ))),
        negated_unary_condition,
    ))
    .parse(input)?;

    let expr = pairs.into_iter().fold(first, |lhs, (op, rhs)| {
        Expression::Numeric(NumericOp::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    });

    Ok((input, expr))
}

// // negated_unary_condition: (NEG | NOTKEYWORD)? unary_condition;
fn negated_unary_condition(input: &str) -> IResult<&str, Expression> {
    let (input, sign) = opt(alt((
        preceded(multispace0, value(UnaryOpTy::Not, tag("!"))),
        ws(value(UnaryOpTy::Not, tag("not"))),
    )))
    .parse(input)?;

    let (input, expr) = unary_condition(input)?;

    match sign {
        Some(op) => Ok((
            input,
            Expression::Numeric(NumericOp::UnaryOp {
                op,
                expr: Box::new(expr),
            }),
        )),
        None => Ok((input, expr)),
    }
}

// unary_condition: condition;
fn unary_condition(input: &str) -> IResult<&str, Expression> {
    condition.parse(input)
}

// condition: expression (conditional_operator expression)?;
fn condition(input: &str) -> IResult<&str, Expression> {
    let (input, lhs) = expression(input)?;

    let (input, maybe_rhs) = opt(pair(
        alt((
            value(BinaryOpTy::IsNot, tag("is not")),
            value(BinaryOpTy::Is, tag("is")),
            value(BinaryOpTy::Equal, tag("==")),
            value(BinaryOpTy::NotEqual, tag("!=")),
            value(BinaryOpTy::LessThanEqual, tag("<=")),
            value(BinaryOpTy::GreaterThanEqual, tag(">=")),
            value(BinaryOpTy::LessThan, tag("<")),
            value(BinaryOpTy::GreaterThan, tag(">")),
        )),
        expression,
    ))
    .parse(input)?;

    match maybe_rhs {
        Some((op, rhs)) => Ok((
            input,
            Expression::Numeric(NumericOp::BinaryOp {
                op,
                lhs: Box::new(lhs),
                rhs: Box::new(rhs),
            }),
        )),
        None => Ok((input, lhs)),
    }
}

// expression: STRING | BOOLEAN | additive_numeric_expression;
fn expression(input: &str) -> IResult<&str, Expression> {
    ws(alt((string, boolean, additive_numeric_expression))).parse(input)
}

// https://github.com/rust-bakery/nom/blob/main/doc/nom_recipes.md
/// A combinator that takes a parser `inner` and produces a parser that also consumes both leading and
/// trailing whitespace, returning the output of `inner`.
fn ws<'a, O, E: ParseError<&'a str>, F>(inner: F) -> impl Parser<&'a str, Output = O, Error = E>
where
    F: Parser<&'a str, Output = O, Error = E>,
{
    delimited(multispace0, inner, multispace0)
}

#[cfg(test)]
mod tests {
    use crate::script::{ast::*, parser::input};

    // ── helpers ──────────────────────────────────────────────────────────────

    fn parse_branch(s: &str) -> Expression {
        let (remaining, result) = input(s).expect("parse failed");
        assert!(remaining.is_empty(), "unconsumed input: {:?}", remaining);
        match result {
            Input::Branch(expr) => expr,
            Input::Script(_) => panic!("expected Branch, got Script"),
        }
    }

    fn parse_script(s: &str) -> Statement {
        let (remaining, result) = input(s).expect("parse failed");
        assert!(remaining.is_empty(), "unconsumed input: {:?}", remaining);
        match result {
            Input::Script(stmt) => stmt,
            Input::Branch(_) => panic!("expected Script, got Branch"),
        }
    }

    fn int(i: i32) -> Expression {
        Expression::Value(Value::Integer(i))
    }

    fn float(f: f32) -> Expression {
        Expression::Value(Value::Float(f))
    }

    fn boolean(b: bool) -> Expression {
        Expression::Value(Value::Boolean(b))
    }

    fn var(name: &str) -> Expression {
        Expression::Value(Value::Var(Variable(name.to_owned())))
    }

    fn string(s: &str) -> Expression {
        Expression::Value(Value::String(s.to_owned()))
    }

    fn binop(op: BinaryOpTy, lhs: Expression, rhs: Expression) -> Expression {
        Expression::Numeric(NumericOp::BinaryOp {
            op,
            lhs: Box::new(lhs),
            rhs: Box::new(rhs),
        })
    }

    fn unop(op: UnaryOpTy, expr: Expression) -> Expression {
        Expression::Numeric(NumericOp::UnaryOp {
            op,
            expr: Box::new(expr),
        })
    }

    // ── literals ─────────────────────────────────────────────────────────────

    #[test]
    fn test_integer() {
        assert_eq!(parse_branch("42"), int(42));
    }

    #[test]
    fn test_negative_integer() {
        assert_eq!(parse_branch("-5"), unop(UnaryOpTy::Minus, int(5)));
    }

    #[test]
    fn test_float() {
        assert_eq!(parse_branch("3.14"), float(3.14));
    }

    #[test]
    fn test_boolean_true() {
        assert_eq!(parse_branch("true"), boolean(true));
    }

    #[test]
    fn test_boolean_false() {
        assert_eq!(parse_branch("false"), boolean(false));
    }

    #[test]
    fn test_string_double_quote() {
        assert_eq!(parse_branch("\"hello\""), string("hello"));
    }

    // ── variables ────────────────────────────────────────────────────────────

    #[test]
    fn test_variable() {
        assert_eq!(parse_branch("my_var"), var("my_var"));
    }

    #[test]
    fn test_variable_with_dollar() {
        assert_eq!(parse_branch("$count"), var("$count"));
    }

    #[test]
    fn test_variable_with_numbers() {
        assert_eq!(parse_branch("var123"), var("var123"));
    }

    // ── comparison operators ──────────────────────────────────────────────────

    #[test]
    fn test_equal() {
        assert_eq!(
            parse_branch("x == 1"),
            binop(BinaryOpTy::Equal, var("x"), int(1))
        );
    }

    #[test]
    fn test_not_equal() {
        assert_eq!(
            parse_branch("x != 1"),
            binop(BinaryOpTy::NotEqual, var("x"), int(1))
        );
    }

    #[test]
    fn test_less_than() {
        assert_eq!(
            parse_branch("x < 10"),
            binop(BinaryOpTy::LessThan, var("x"), int(10))
        );
    }

    #[test]
    fn test_less_than_equal() {
        assert_eq!(
            parse_branch("x <= 10"),
            binop(BinaryOpTy::LessThanEqual, var("x"), int(10))
        );
    }

    #[test]
    fn test_greater_than() {
        assert_eq!(
            parse_branch("x > 5"),
            binop(BinaryOpTy::GreaterThan, var("x"), int(5))
        );
    }

    #[test]
    fn test_greater_than_equal() {
        assert_eq!(
            parse_branch("x >= 5"),
            binop(BinaryOpTy::GreaterThanEqual, var("x"), int(5))
        );
    }

    #[test]
    fn test_is_keyword() {
        assert_eq!(
            parse_branch("x is true"),
            binop(BinaryOpTy::Is, var("x"), boolean(true))
        );
    }

    #[test]
    fn test_is_not_keyword() {
        assert_eq!(
            parse_branch("x is not false"),
            binop(BinaryOpTy::IsNot, var("x"), boolean(false))
        );
    }

    // ── logical operators ─────────────────────────────────────────────────────

    #[test]
    fn test_and_symbol() {
        assert_eq!(
            parse_branch("a && b"),
            binop(BinaryOpTy::And, var("a"), var("b"))
        );
    }

    #[test]
    fn test_and_keyword() {
        assert_eq!(
            parse_branch("a and b"),
            binop(BinaryOpTy::And, var("a"), var("b"))
        );
    }

    #[test]
    fn test_or_symbol() {
        assert_eq!(
            parse_branch("a || b"),
            binop(BinaryOpTy::Or, var("a"), var("b"))
        );
    }

    #[test]
    fn test_or_keyword() {
        assert_eq!(
            parse_branch("a or b"),
            binop(BinaryOpTy::Or, var("a"), var("b"))
        );
    }

    #[test]
    fn test_not_symbol() {
        assert_eq!(
            parse_branch("!flag"),
            unop(UnaryOpTy::Not, var("flag"))
        );
    }

    #[test]
    fn test_not_keyword() {
        assert_eq!(
            parse_branch("not flag"),
            unop(UnaryOpTy::Not, var("flag"))
        );
    }

    // ── arithmetic ────────────────────────────────────────────────────────────

    #[test]
    fn test_add() {
        assert_eq!(
            parse_branch("1 + 2"),
            binop(BinaryOpTy::Add, int(1), int(2))
        );
    }

    #[test]
    fn test_sub() {
        assert_eq!(
            parse_branch("5 - 3"),
            binop(BinaryOpTy::Sub, int(5), int(3))
        );
    }

    #[test]
    fn test_mul() {
        assert_eq!(
            parse_branch("2 * 3"),
            binop(BinaryOpTy::Mul, int(2), int(3))
        );
    }

    #[test]
    fn test_div() {
        assert_eq!(
            parse_branch("6 / 2"),
            binop(BinaryOpTy::Div, int(6), int(2))
        );
    }

    #[test]
    fn test_mod() {
        assert_eq!(
            parse_branch("7 % 3"),
            binop(BinaryOpTy::Mod, int(7), int(3))
        );
    }

    #[test]
    fn test_mul_precedence_over_add() {
        assert_eq!(
            parse_branch("1 + 2 * 3"),
            binop(
                BinaryOpTy::Add,
                int(1),
                binop(BinaryOpTy::Mul, int(2), int(3))
            )
        );
    }

    #[test]
    fn test_parens_override_precedence() {
        assert_eq!(
            parse_branch("(1 + 2) * 3"),
            binop(
                BinaryOpTy::Mul,
                binop(BinaryOpTy::Add, int(1), int(2)),
                int(3)
            )
        );
    }

    // ── html-encoded operators (from Arcweave JSON) ───────────────────────────

    #[test]
    fn test_html_encoded_greater_than_equal() {
        use html_escape::decode_html_entities;
        let decoded = decode_html_entities("wanda_health &gt;= 40").into_owned();
        assert_eq!(
            parse_branch(&decoded),
            binop(BinaryOpTy::GreaterThanEqual, var("wanda_health"), int(40))
        );
    }

    #[test]
    fn test_html_encoded_less_than() {
        use html_escape::decode_html_entities;
        let decoded = decode_html_entities("wanda_health &lt; 40").into_owned();
        assert_eq!(
            parse_branch(&decoded),
            binop(BinaryOpTy::LessThan, var("wanda_health"), int(40))
        );
    }

    // ── complex conditions ────────────────────────────────────────────────────

    #[test]
    fn test_compound_and_or() {
        assert_eq!(
            parse_branch("a && b || c"),
            binop(
                BinaryOpTy::Or,
                binop(BinaryOpTy::And, var("a"), var("b")),
                var("c")
            )
        );
    }

    #[test]
    fn test_not_with_comparison() {
        assert_eq!(
            parse_branch("not x == 1"),
            unop(UnaryOpTy::Not, binop(BinaryOpTy::Equal, var("x"), int(1)))
        );
    }

    // ── function calls ────────────────────────────────────────────────────────

    #[test]
    fn test_visits_function() {
        let result = parse_branch(
            r#"visits(<span class="mention-element mention" data-id="abc123" data-type="element">label</span>)"#
        );
        match result {
            Expression::Value(Value::Func(call)) => {
                assert_eq!(call.func, FuncTy::Visits);
                assert_eq!(call.args.len(), 1);
                match &call.args[0] {
                    Expression::Value(Value::Mention(m)) => {
                        assert_eq!(m.attrs.get("data-id").and_then(|v| v.as_deref()), Some("abc123"));
                    }
                    _ => panic!("expected Mention argument"),
                }
            }
            _ => panic!("expected Func"),
        }
    }

    #[test]
    fn test_not_visits() {
        let result = parse_branch(
            r#"not visits(<span data-id="abc123" data-type="element">label</span>)"#
        );
        match result {
            Expression::Numeric(NumericOp::UnaryOp { op: UnaryOpTy::Not, expr }) => {
                match *expr {
                    Expression::Value(Value::Func(call)) => assert_eq!(call.func, FuncTy::Visits),
                    _ => panic!("expected Func inside Not"),
                }
            }
            _ => panic!("expected UnaryOp Not"),
        }
    }

    #[test]
    fn test_abs_function() {
        let result = parse_branch("abs(-5)");
        match result {
            Expression::Value(Value::Func(call)) => {
                assert_eq!(call.func, FuncTy::Abs);
                assert_eq!(call.args[0], unop(UnaryOpTy::Minus, int(5)));
            }
            _ => panic!("expected Func"),
        }
    }

    #[test]
    fn test_min_function() {
        let result = parse_branch("min(1, 2)");
        match result {
            Expression::Value(Value::Func(call)) => {
                assert_eq!(call.func, FuncTy::Min);
                assert_eq!(call.args.len(), 2);
            }
            _ => panic!("expected Func"),
        }
    }

    // ── assignments ───────────────────────────────────────────────────────────

    #[test]
    fn test_assign() {
        let stmt = parse_script("<pre><code>x = 5</code></pre>");
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Assign { ty: AssignTy::Assign, var: Variable(name), expr } => {
                    assert_eq!(name, "x");
                    assert_eq!(*expr, int(5));
                }
                _ => panic!("expected Assign"),
            },
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_assign_add() {
        let stmt = parse_script("<pre><code>hp += 10</code></pre>");
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Assign { ty: AssignTy::AssignAdd, var: Variable(name), expr } => {
                    assert_eq!(name, "hp");
                    assert_eq!(*expr, int(10));
                }
                _ => panic!("expected AssignAdd"),
            },
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_assign_boolean() {
        let stmt = parse_script("<pre><code>have_potion = false</code></pre>");
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Assign { ty: AssignTy::Assign, var: Variable(name), expr } => {
                    assert_eq!(name, "have_potion");
                    assert_eq!(*expr, boolean(false));
                }
                _ => panic!("expected Assign"),
            },
            _ => panic!("expected Block"),
        }
    }

    // ── if/else ───────────────────────────────────────────────────────────────

    #[test]
    fn test_if_only() {
        let stmt = parse_script(
            "<pre><code>if x == 1</code></pre><p>yes</p><pre><code>endif</code></pre>",
        );
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Condition { cond, then, alt } => {
                    assert_eq!(*cond, binop(BinaryOpTy::Equal, var("x"), int(1)));
                    assert!(alt.is_none());
                    match then.as_ref() {
                        Statement::Block(inner) => {
                            assert!(matches!(&inner[0], Statement::Paragraph(s) if s == "yes"));
                        }
                        _ => panic!("expected Block in then"),
                    }
                }
                _ => panic!("expected Condition"),
            },
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_if_else() {
        let stmt = parse_script(
            "<pre><code>if x == 1</code></pre><p>yes</p><pre><code>else</code></pre><p>no</p><pre><code>endif</code></pre>",
        );
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Condition { alt: Some(alt), .. } => match alt.as_ref() {
                    Statement::Block(inner) => {
                        assert!(matches!(&inner[0], Statement::Paragraph(s) if s == "no"));
                    }
                    _ => panic!("expected Block in else"),
                },
                _ => panic!("expected Condition with else"),
            },
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_if_elseif_else() {
        let stmt = parse_script(concat!(
            "<pre><code>if x == 1</code></pre><p>one</p>",
            "<pre><code>elseif x == 2</code></pre><p>two</p>",
            "<pre><code>else</code></pre><p>other</p>",
            "<pre><code>endif</code></pre>",
        ));

        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Condition { cond, alt: Some(alt), .. } => {
                    assert_eq!(*cond, binop(BinaryOpTy::Equal, var("x"), int(1)));
                    match alt.as_ref() {
                        Statement::Condition { cond: inner_cond, alt: Some(_), .. } => {
                            assert_eq!(*inner_cond, binop(BinaryOpTy::Equal, var("x"), int(2)));
                        }
                        _ => panic!("expected nested Condition for elseif"),
                    }
                }
                _ => panic!("expected Condition with elseif"),
            },
            _ => panic!("expected Block"),
        }
    }

    // ── paragraphs ────────────────────────────────────────────────────────────

    #[test]
    fn test_paragraph() {
        let stmt = parse_script("<p>Hello world</p>");
        match stmt {
            Statement::Block(stmts) => {
                assert!(matches!(&stmts[0], Statement::Paragraph(s) if s == "Hello world"));
            }
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_paragraph_with_attrs() {
        let stmt = parse_script("<p class=\"foo\">Hello</p>");
        match stmt {
            Statement::Block(stmts) => {
                assert!(matches!(&stmts[0], Statement::Paragraph(s) if s == "Hello"));
            }
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_multiple_paragraphs() {
        let stmt = parse_script("<p>First</p><p>Second</p>");
        match stmt {
            Statement::Block(stmts) => {
                assert_eq!(stmts.len(), 2);
                assert!(matches!(&stmts[0], Statement::Paragraph(s) if s == "First"));
                assert!(matches!(&stmts[1], Statement::Paragraph(s) if s == "Second"));
            }
            _ => panic!("expected Block"),
        }
    }

    // ── blockquote ────────────────────────────────────────────────────────────

    #[test]
    fn test_blockquote() {
        let stmt = parse_script("<blockquote><p>Quoted</p></blockquote>");
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Quote(inner) => {
                    assert!(matches!(&inner[0], Statement::Paragraph(s) if s == "Quoted"));
                }
                _ => panic!("expected Quote"),
            },
            _ => panic!("expected Block"),
        }
    }

    // ── mixed content ─────────────────────────────────────────────────────────

    #[test]
    fn test_if_else_inline_content() {
        // From WANDA_START element content
        let content = concat!(
            "<pre><code>if wanda_health &lt; 40</code></pre>",
            "<p>Help me, Stranger.... I am wounded...</p>",
            "<pre><code>else</code></pre>",
            "<p>Thank you for saving my life!</p>",
            "<pre><code>endif</code></pre>",
        );
        let decoded = html_escape::decode_html_entities(content).into_owned();
        let stmt = parse_script(&decoded);
        match stmt {
            Statement::Block(stmts) => match &stmts[0] {
                Statement::Condition { cond, then, alt: Some(alt) } => {
                    assert_eq!(
                        *cond,
                        binop(BinaryOpTy::LessThan, var("wanda_health"), int(40))
                    );
                    match then.as_ref() {
                        Statement::Block(inner) => {
                            assert!(matches!(&inner[0], Statement::Paragraph(s) if s.contains("wounded")));
                        }
                        _ => panic!("expected Block in then"),
                    }
                    match alt.as_ref() {
                        Statement::Block(inner) => {
                            assert!(matches!(&inner[0], Statement::Paragraph(s) if s.contains("saving")));
                        }
                        _ => panic!("expected Block in else"),
                    }
                }
                _ => panic!("expected Condition"),
            },
            _ => panic!("expected Block"),
        }
    }

    #[test]
    fn test_multi_assignment_script() {
        // From WANDA_GAVE_POTION element
        let content = concat!(
            "<pre><code>wanda_health += 50</code></pre>",
            "<pre><code>have_potion = false</code></pre>",
            "<p><em>[Wanda gulps down the potion.]</em></p>",
        );
        let stmt = parse_script(content);
        match stmt {
            Statement::Block(stmts) => {
                assert!(stmts.len() >= 2);
                assert!(matches!(&stmts[0], Statement::Assign { ty: AssignTy::AssignAdd, .. }));
                assert!(matches!(&stmts[1], Statement::Assign { ty: AssignTy::Assign, .. }));
            }
            _ => panic!("expected Block"),
        }
    }

    // ── trailing input detection ──────────────────────────────────────────────

    #[test]
    fn test_trailing_input_detected() {
        let (remaining, _) = input("wanda_health &gt;= 40").unwrap();
        assert!(!remaining.is_empty(), "should have unconsumed trailing input without decoding");
    }
}