//! Conservative source optimizations; never evaluate user functions or globals.
use crate::obfuscator::ObfuscationReport;
use darklua_core::{
    nodes::*,
    process::{NodePostProcessor, NodePostVisitor, NodeProcessor},
};

const MAX_EXACT_INTEGER: f64 = 9_007_199_254_740_992.0;
const MAX_FOLDED_STRING: usize = 4096;

#[derive(Clone, Copy)]
enum Literal<'a> {
    Nil,
    Bool(bool),
    Number(f64),
    String(&'a [u8]),
}
fn literal(expression: &Expression) -> Option<Literal<'_>> {
    Some(match expression {
        Expression::Nil(_) => Literal::Nil,
        Expression::True(_) => Literal::Bool(true),
        Expression::False(_) => Literal::Bool(false),
        Expression::String(value) => Literal::String(value.get_value()),
        Expression::Number(value) => {
            let value = value.compute_value();
            if !value.is_finite() {
                return None;
            }
            Literal::Number(value)
        }
        Expression::Parenthese(value) => return literal(value.inner_expression()),
        Expression::Unary(value) if value.operator() == UnaryOperator::Minus => {
            let Literal::Number(number) = literal(value.get_expression())? else {
                return None;
            };
            Literal::Number(-number)
        }
        _ => return None,
    })
}
fn truth(expression: &Expression) -> Option<bool> {
    literal(expression).map(|value| !matches!(value, Literal::Nil | Literal::Bool(false)))
}
fn single(expression: Expression) -> Expression {
    match expression {
        Expression::Call(_) | Expression::VariableArguments(_) => {
            ParentheseExpression::new(expression).into()
        }
        value => value,
    }
}
fn exact_integer(value: f64) -> bool {
    value.is_finite()
        && value.fract() == 0.0
        && value.abs() <= MAX_EXACT_INTEGER
        && !(value == 0.0 && value.is_sign_negative())
}
fn equality(left: Literal<'_>, right: Literal<'_>) -> bool {
    match (left, right) {
        (Literal::Nil, Literal::Nil) => true,
        (Literal::Bool(a), Literal::Bool(b)) => a == b,
        (Literal::Number(a), Literal::Number(b)) => a == b,
        (Literal::String(a), Literal::String(b)) => a == b,
        _ => false,
    }
}
fn binary(value: &BinaryExpression) -> Option<Expression> {
    use BinaryOperator::*;
    let op = value.operator();
    if matches!(op, And | Or) {
        let take_right = truth(value.left())? == (op == And);
        return Some(single(
            if take_right {
                value.right()
            } else {
                value.left()
            }
            .clone(),
        ));
    }
    let (left, right) = (literal(value.left())?, literal(value.right())?);
    if matches!(op, Equal | NotEqual) {
        let equal = equality(left, right);
        return Some((if op == Equal { equal } else { !equal }).into());
    }
    match (left, right) {
        (Literal::String(a), Literal::String(b)) if op == Concat => {
            if a.len().checked_add(b.len())? > MAX_FOLDED_STRING {
                return None;
            }
            Some(StringExpression::from_value([a, b].concat()).into())
        }
        (Literal::Number(a), Literal::Number(b)) => {
            let comparison = match op {
                LowerThan => Some(a < b),
                LowerOrEqualThan => Some(a <= b),
                GreaterThan => Some(a > b),
                GreaterOrEqualThan => Some(a >= b),
                _ => None,
            };
            if let Some(result) = comparison {
                return Some(result.into());
            }
            if !exact_integer(a) || !exact_integer(b) {
                return None;
            }
            let result = match op {
                Plus => a + b,
                Minus => a - b,
                Asterisk => a * b,
                Slash if b != 0.0 && a % b == 0.0 => a / b,
                _ => return None,
            };
            if exact_integer(result) {
                Some(result.into())
            } else {
                None
            }
        }
        _ => None,
    }
}
fn fold(expression: &Expression) -> Option<Expression> {
    match expression {
        Expression::Binary(value) => binary(value),
        Expression::Unary(value) if value.operator() == UnaryOperator::Not => {
            Some((!truth(value.get_expression())?).into())
        }
        Expression::Unary(value) if value.operator() == UnaryOperator::Length => {
            let Literal::String(bytes) = literal(value.get_expression())? else {
                return None;
            };
            (bytes.len() <= u32::MAX as usize).then(|| (bytes.len() as f64).into())
        }
        Expression::If(value) => {
            let branches = std::iter::once((value.get_condition(), value.get_result())).chain(
                value
                    .iter_branches()
                    .map(|branch| (branch.get_condition(), branch.get_result())),
            );
            for (condition, result) in branches {
                if truth(condition)? {
                    return Some(single(result.clone()));
                }
            }
            Some(single(value.get_else_result().clone()))
        }
        _ => None,
    }
}

struct Optimizer<'a> {
    report: &'a mut ObfuscationReport,
}
impl NodeProcessor for Optimizer<'_> {}
impl NodePostProcessor for Optimizer<'_> {
    fn process_after_expression(&mut self, expression: &mut Expression) {
        if let Some(value) = fold(expression) {
            *expression = value;
            self.report.optimized_expressions += 1;
        }
    }
    fn process_after_statement(&mut self, statement: &mut Statement) {
        match statement {
            Statement::While(value) if truth(value.get_condition()) == Some(false) => {
                *statement = DoStatement::new(Block::default()).into();
                self.report.removed_loops += 1;
            }
            Statement::If(value) => {
                let mut branches = Vec::new();
                let mut otherwise = value.get_else_block().cloned();
                let mut removed = 0;
                for (index, branch) in value.iter_branches().enumerate() {
                    match truth(branch.get_condition()) {
                        Some(false) => removed += 1,
                        Some(true) => {
                            removed +=
                                value.branch_count() - index + usize::from(otherwise.is_some());
                            otherwise = Some(branch.get_block().clone());
                            break;
                        }
                        None => branches.push(branch.clone()),
                    }
                }
                if removed == 0 {
                    return;
                }
                self.report.removed_branches += removed;
                *statement = if branches.is_empty() {
                    DoStatement::new(otherwise.unwrap_or_default()).into()
                } else {
                    IfStatement::new(branches, otherwise).into()
                };
            }
            _ => {}
        }
    }
    fn process_after_block(&mut self, block: &mut Block) {
        let statements = block
            .take_statements()
            .into_iter()
            .filter(|statement| {
                if matches!(statement, Statement::Do(value) if value.get_block().is_empty()) {
                    self.report.removed_empty_blocks += 1;
                    false
                } else {
                    true
                }
            })
            .collect();
        block.set_statements(statements);
    }
}
struct RuntimeOnly;
impl NodePostVisitor<Optimizer<'_>> for RuntimeOnly {
    fn visit_type_function_statement(_: &mut TypeFunctionStatement, _: &mut Optimizer<'_>) {}
    fn visit_type(_: &mut Type, _: &mut Optimizer<'_>) {}
}
pub(crate) fn optimize(block: &mut Block, report: &mut ObfuscationReport) {
    RuntimeOnly::visit_block(block, &mut Optimizer { report });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unsafe_numeric_and_effectful_expressions_stay_intact() {
        for source in [
            "return 0/0",
            "return 1/0",
            "return -0*1",
            "return 0.1+0.2",
            "return 9007199254740992+2",
            "return t+0",
            "return t*1",
            "return read().value",
            "return #setmetatable({}, {})",
        ] {
            let mut block = crate::syntax::parse(source).unwrap();
            let before = crate::syntax::generate(&block, true);
            let mut report = ObfuscationReport::default();
            optimize(&mut block, &mut report);
            assert_eq!(crate::syntax::generate(&block, true), before, "{source}");
            assert_eq!(report.optimized_expressions, 0, "{source}");
        }
    }
}
