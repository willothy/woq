use std::{collections::VecDeque, fmt::Display};

use thiserror::Error;

use crate::{
    err,
    error::{common::*, ParseError::*},
    lexer::{Constraint, Lexer, OpKind, Token, TokenKind, MAX_PRECEDENCE},
    rule::{Action, State, Strategy},
};

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Expr {
    Sym(String),
    Var(String, Constraint),
    Num(i64),
    Str(String),
    Fun(Box<Expr>, Box<Expr>),
    BinaryOp(OpKind, Box<Expr>, Box<Expr>),
    UnaryOp(OpKind, Box<Expr>),
    List(Vec<Expr>, Repeat),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Repeat {
    None,
    ZeroOrMore(Option<OpKind /* Separator */>),
}

#[derive(Debug, Error)]
#[error("Parse error: {0}")]
pub(crate) struct ParseError(String);

impl Expr {
    pub(crate) fn is_num(&self) -> bool {
        matches!(self, Expr::Num(_))
    }

    pub(crate) fn is_const_expr(&self) -> bool {
        use Expr::*;
        match self {
            Num(_) => true,
            BinaryOp(op, lhs, rhs) => lhs.is_const_expr() && rhs.is_const_expr() && op.is_const(),
            UnaryOp(op, expr) => expr.is_const_expr() && op.is_const() && op.is_unary(),
            List(elements, _) => elements.iter().all(|e| e.is_const_expr()),
            _ => false,
        }
    }

    pub(crate) fn eval(&self, strategy: &mut impl Strategy) -> Expr {
        fn eval_subexprs(expr: &Expr, strategy: &mut impl Strategy) -> (Expr, bool) {
            use Expr::*;
            match expr {
                Sym(_) | Var(_, _) | Num(_) | Str(_) => (expr.clone(), false),
                Fun(head, body) => {
                    let (new_head, halt) = eval_impl(head, strategy);
                    if halt {
                        return (Fun(box new_head, body.clone()), true);
                    }
                    let (new_body, halt) = eval_impl(body, strategy);
                    (Fun(box new_head, box new_body), halt)
                }
                BinaryOp(op, lhs, rhs) => {
                    let (new_lhs, halt) = eval_impl(lhs, strategy);
                    if halt {
                        return (BinaryOp(op.clone(), box new_lhs, rhs.clone()), true);
                    }
                    let (new_rhs, halt) = eval_impl(rhs, strategy);
                    (BinaryOp(op.clone(), box new_lhs, box new_rhs), halt)
                }
                List(elements, repeat) => {
                    let mut new_elements = vec![];
                    let mut halt_elements = false;
                    for element in elements {
                        if halt_elements {
                            new_elements.push(element.clone());
                        } else {
                            let (arg, arg_halt) = eval_impl(element, strategy);
                            new_elements.push(arg);
                            halt_elements = arg_halt;
                        }
                    }
                    (List(new_elements, repeat.clone()), false)
                }
                UnaryOp(op, expr) => {
                    let (new_expr, halt) = eval_impl(expr, strategy);
                    (UnaryOp(op.clone(), box new_expr), halt)
                }
            }
        }

        fn apply_eval(expr: &Expr) -> Expr {
            use Expr::*;
            match expr {
                /* List(elements, repeat) => List(
                    elements.iter().map(|el| apply_eval(el)).collect(),
                    repeat.clone(),
                ), */
                BinaryOp(op, lhs, rhs) => {
                    let lhs = apply_eval(lhs.as_ref());
                    let rhs = apply_eval(rhs.as_ref());
                    match (lhs, rhs) {
                        (Num(lhs), Num(rhs)) => match op {
                            OpKind::Add => Num(lhs + rhs),
                            OpKind::Sub => Num(lhs - rhs),
                            OpKind::Mul => Num(lhs * rhs),
                            OpKind::Div => Num(lhs / rhs),
                            OpKind::Pow => Num(lhs.pow(rhs as u32)),
                            OpKind::Dot => unreachable!(),
                        },
                        (List(lhs, _), List(rhs, _)) => match op {
                            OpKind::Add => List(
                                lhs.iter()
                                    .zip(rhs)
                                    .map(|(l, r)| {
                                        BinaryOp(OpKind::Add, box l.clone(), box r.clone())
                                    })
                                    .collect(),
                                Repeat::None,
                            ),
                            OpKind::Sub => List(
                                lhs.iter()
                                    .zip(rhs)
                                    .map(|(l, r)| {
                                        BinaryOp(OpKind::Sub, box l.clone(), box r.clone())
                                    })
                                    .collect(),
                                Repeat::None,
                            ),
                            OpKind::Mul => List(
                                lhs.iter()
                                    .zip(rhs)
                                    .map(|(l, r)| {
                                        BinaryOp(OpKind::Mul, box l.clone(), box r.clone())
                                    })
                                    .collect(),
                                Repeat::None,
                            ),
                            OpKind::Div => List(
                                lhs.iter()
                                    .zip(rhs)
                                    .map(|(l, r)| {
                                        BinaryOp(OpKind::Div, box l.clone(), box r.clone())
                                    })
                                    .collect(),
                                Repeat::None,
                            ),
                            OpKind::Pow => List(
                                lhs.iter()
                                    .zip(rhs)
                                    .map(|(l, r)| {
                                        BinaryOp(OpKind::Pow, box l.clone(), box r.clone())
                                    })
                                    .collect(),
                                Repeat::None,
                            ),
                            OpKind::Dot => {
                                let mut new_elements: VecDeque<Expr> = lhs
                                    .iter()
                                    .zip(rhs)
                                    .map(|(l, r)| {
                                        BinaryOp(OpKind::Mul, box l.clone(), box r.clone())
                                    })
                                    .collect();

                                let mut result_expr = new_elements.pop_front().unwrap();
                                while let Some(next) = new_elements.pop_front() {
                                    result_expr = BinaryOp(OpKind::Add, box result_expr, box next);
                                }

                                result_expr
                            }
                        },
                        (lhs, rhs) => BinaryOp(op.clone(), box lhs, box rhs),
                    }
                }
                Num(_) => expr.clone(),
                other => other.clone(),
            }
        }

        fn eval_impl(expr: &Expr, strategy: &mut impl Strategy) -> (Expr, bool) {
            if expr.is_const_expr() {
                let resolution = strategy.matched();
                let new_expr = match resolution.action {
                    Action::Apply => apply_eval(expr),
                    Action::Skip => expr.clone(),
                    Action::Check => {
                        if let Some(matches) = strategy.matches() {
                            matches.push((expr.clone(), apply_eval(expr)));
                        }
                        expr.clone()
                    }
                };
                match resolution.state {
                    State::Bail => (new_expr, false),
                    State::Halt => (new_expr, true),
                    State::Cont => eval_subexprs(&new_expr, strategy),
                }
            } else {
                eval_subexprs(expr, strategy)
            }
        }

        eval_impl(self, strategy).0
    }

    fn var_or_sym(name: &str, constraint: Constraint) -> Expr {
        name.chars()
            .nth(0)
            .filter(|c| c.is_uppercase() || *c == '_')
            .map(|_| Expr::Var(name.to_owned(), constraint))
            .unwrap_or(Expr::Sym(name.to_owned()))
    }

    fn parse_list(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Expr> {
        if lexer
            .next_if(|tok| tok.kind == TokenKind::CloseParen)
            .is_some()
        {
            return Ok(Expr::List(vec![], Repeat::None));
        }

        let mut result = vec![Self::parse(lexer)?]; // Vec with first element
        let mut repeat = Repeat::None;
        while lexer.next_if(|t| t.kind == TokenKind::Comma).is_some() {
            if lexer.next_if(|t| t.kind == TokenKind::DoubleDot).is_some() {
                repeat = Repeat::ZeroOrMore(None);
                break;
            } else if let TokenKind::Op(op) = lexer.peek().clone().kind {
                if let TokenKind::DoubleDot = &lexer.peek_next().kind {
                    lexer.catchup();
                    repeat = Repeat::ZeroOrMore(Some(op));
                    break;
                } else {
                    result.push(Self::parse(lexer)?);
                }
            } else {
                result.push(Self::parse(lexer)?);
            }
        }

        if lexer
            .next_if(|tok| tok.kind == TokenKind::CloseParen)
            .is_none()
        {
            let t = lexer.next_token();
            return err!(Parse UnexpectedToken(t.text), t.loc).with_message("Expected ')'");
        }

        Ok(Expr::List(result, repeat))
    }

    fn parse_fn_or_var_or_sym(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Self> {
        let mut head = {
            match lexer.next_token() {
                Token {
                    kind: TokenKind::OpenParen,
                    ..
                } => Self::parse_list(lexer)?,
                Token {
                    kind: TokenKind::Ident,
                    text,
                    constraint,
                    ..
                } => Self::var_or_sym(&text, constraint),
                Token {
                    kind: TokenKind::Number,
                    text,
                    loc,
                    ..
                } => Expr::Num(text.parse().inherit(loc)?),
                Token {
                    kind: TokenKind::String,
                    text,
                    ..
                } => Expr::Str(text),
                Token {
                    kind: TokenKind::Op(op),
                    ..
                } => Expr::UnaryOp(op, box Expr::parse(lexer)?),
                t => {
                    return err!(Parse UnexpectedToken(t.text), t.loc)
                        .with_message("Expected symbol")
                }
            }
        };

        while lexer
            .next_if(|tok| tok.kind == TokenKind::OpenParen)
            .is_some()
        {
            head = Expr::Fun(box head, box Self::parse_list(lexer)?);
        }
        Ok(head)
    }

    fn parse_binop(
        lexer: &mut Lexer<impl Iterator<Item = char>>,
        precedence: usize,
    ) -> Result<Self> {
        if precedence > MAX_PRECEDENCE {
            return Self::parse_fn_or_var_or_sym(lexer);
        }

        let mut result = Self::parse_binop(lexer, precedence + 1)?;
        while let Some(Token {
            kind: TokenKind::Op(op),
            ..
        }) = lexer.next_if(|tok| match &tok.kind {
            TokenKind::Op(op) => op.precedence() == precedence,
            _ => false,
        }) {
            let rhs = Self::parse_binop(lexer, precedence)?;
            result = Expr::BinaryOp(op, box result, box rhs);
        }

        Ok(result)
    }

    pub(crate) fn parse(lexer: &mut Lexer<impl Iterator<Item = char>>) -> Result<Self> {
        Self::parse_binop(lexer, 0)
    }
}

impl TryFrom<&str> for Expr {
    type Error = Error;
    fn try_from(s: &str) -> Result<Self> {
        Expr::parse(&mut crate::lexer::Lexer::new(s.chars().peekable()))
    }
}

impl Display for Expr {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Expr::List(exprs, repeat) => {
                write!(f, "(")?;
                for (i, expr) in exprs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", expr)?;
                }
                if let Repeat::ZeroOrMore(sep) = repeat {
                    write!(
                        f,
                        ", {}..",
                        sep.clone().map(|x| x.to_string()).unwrap_or("".to_owned())
                    )?;
                }
                write!(f, ")")
            }
            Expr::Sym(name) | Expr::Var(name, ..) => write!(f, "{}", name),
            Expr::Fun(head, body) => {
                match &**head {
                    Expr::Sym(name) | Expr::Var(name, ..) => write!(f, "{}", name)?,
                    other => write!(f, "({})", other)?,
                }
                write!(f, "(")?;
                match &**body {
                    Expr::List(exprs, repeat) => {
                        for (i, expr) in exprs.iter().enumerate() {
                            if i > 0 {
                                write!(f, ", ")?;
                            }
                            write!(f, "{}", expr)?;
                        }
                        if let Repeat::ZeroOrMore(sep) = repeat {
                            write!(
                                f,
                                ", {}..",
                                sep.clone().map(|x| x.to_string()).unwrap_or("".to_owned())
                            )?;
                        }
                    }
                    other => write!(f, "{}", other)?,
                }
                write!(f, ")")
            }
            Expr::UnaryOp(op, expr) => {
                write!(f, "{}", op)?;
                match expr.as_ref() {
                    Expr::BinaryOp(sub_op, _, _) => {
                        if sub_op.precedence() <= op.precedence() {
                            write!(f, "({})", expr)
                        } else {
                            write!(f, "{}", expr)
                        }
                    }
                    Expr::UnaryOp(op, _) => {
                        if op.precedence() <= op.precedence() {
                            write!(f, "({})", expr)
                        } else {
                            write!(f, "{}", expr)
                        }
                    }
                    _ => write!(f, "{}", expr),
                }
            }
            Expr::BinaryOp(op, lhs, rhs) => {
                match lhs.as_ref() {
                    Expr::BinaryOp(sub_op, _, _) => {
                        if sub_op.precedence() <= op.precedence() {
                            write!(f, "({})", lhs)?
                        } else {
                            write!(f, "{}", lhs)?
                        }
                    }
                    _ => write!(f, "{}", lhs)?,
                }
                if op.precedence() == 0 {
                    write!(f, " {} ", op)?;
                } else {
                    write!(f, "{}", op)?;
                }
                match rhs.as_ref() {
                    Expr::BinaryOp(sub_op, _, _) => {
                        if sub_op.precedence() <= op.precedence() {
                            write!(f, "({})", rhs)
                        } else {
                            write!(f, "{}", rhs)
                        }
                    }
                    _ => write!(f, "{}", rhs),
                }
            }
            Expr::Num(n) => write!(f, "{}", n),
            Expr::Str(s) => write!(f, "\"{}\"", s),
        }
    }
}
