use lexicon::Token;
use lexicon::Token::*;
use lexicon::TemplateKind;
use tokenizer::Tokenizer;
use grammar::*;
use operator::OperatorKind;
use operator::OperatorKind::*;
use owned_slice::OwnedSlice;
use error::{ Result, Error, ParseResult, ParseError };

/// Peek on the next token. Return with an error if tokenizer fails.
macro_rules! peek {
    ($parser:ident) => {
        match $parser.token {
            Some(token) => token,

            None => {
                let token = $parser.tokenizer.get_token()?;

                $parser.token = Some(token);

                token
            }
        }
    }
}

/// Get the next token. Return with an error if tokenizer fails.
macro_rules! next {
    ($parser:ident) => {
        match $parser.token {
            Some(token) => {
                $parser.consume();

                token
            },
            None => $parser.tokenizer.get_token()?
        }
    }
}

/// If the next token matches `$p`, consume that token and execute `$eval`.
macro_rules! allow {
    ($parser:ident, $p:pat => $eval:expr) => {
        match peek!($parser) {
            $p => {
                $parser.consume();
                $eval;
            },
            _ => {}
        }
    }
}

/// Return an error if the next token doesn't match $p.
macro_rules! expect {
    ($parser:ident, $p:pat) => {
        match next!($parser) {
            $p => {},
            _  => unexpected_token!($parser)
        }
    }
}

/// Expect the next token to be an Identifier, extracting the OwnedSlice
/// out of it. Returns an error otherwise.
macro_rules! expect_identifier {
    ($parser:ident) => {
        match next!($parser) {
            Identifier(ident) => ident,
            _                 => unexpected_token!($parser)
        }
    }
}

/// Expecta semicolon to terminate a statement. Will assume a semicolon
/// following the ASI rules.
macro_rules! expect_semicolon {
    ($parser:ident) => {
        // TODO: Tokenizer needs to flag when a new line character has been
        //       consumed to satisfy all ASI rules
        match peek!($parser) {
            Semicolon     => $parser.consume(),

            ParenClose    |
            BraceClose    |
            EndOfProgram  => {},

            _             => {
                if !$parser.tokenizer.asi() {
                    unexpected_token!($parser)
                }
            }
        }
    }
}

/// Return an error for current token.
macro_rules! unexpected_token {
    ($parser:ident) => {
        return Err($parser.tokenizer.invalid_token())
    };
}


pub struct Parser<'a> {
    // Tokenizer will produce tokens from the source
    tokenizer: Tokenizer<'a>,

    // Current token, to be used by peek! and next! macros
    token: Option<Token>,
}

impl<'a> Parser<'a> {
    #[inline]
    pub fn new(source: &'a str) -> Self {
        Parser {
            tokenizer: Tokenizer::new(source),
            token: None,
        }
    }

    #[inline]
    fn consume(&mut self) {
        self.token = None;
    }

    #[inline]
    fn array_expression(&mut self) -> Result<Expression> {
        let mut list = Vec::new();

        loop {
            match next!(self) {
                BracketClose => break,
                Comma        => {
                    list.push(Expression::Void);
                    continue;
                }
                token        => {
                    let expression = self.expression_from_token(token, 0)?;
                    list.push(expression);
                }
            }

            match next!(self) {
                BracketClose => break,
                Comma        => continue,
                _            => unexpected_token!(self)
            }
        }

        Ok(Expression::Array(list))
    }

    #[inline]
    fn object_member_list(&mut self) -> Result<Vec<ObjectMember>> {
        let mut list = Vec::new();

        loop {
            match next!(self) {
                BraceClose => break,
                token      => {
                    list.push(self.object_member(token)?);
                }
            }

            match next!(self) {
                BraceClose => break,
                Comma      => continue,
                _          => unexpected_token!(self)
            }
        }

        Ok(list)
    }

    #[inline]
    fn object_member(&mut self, token: Token) -> Result<ObjectMember> {
        let key = match token {
            Identifier(key) => {
                match peek!(self) {
                    Colon | ParenOpen => ObjectKey::Literal(key),

                    _ => return Ok(ObjectMember::Shorthand {
                        key: key,
                    })
                }
            },

            BracketOpen => {
                let key = ObjectKey::Computed(self.expression(0)?);

                expect!(self, BracketClose);

                key
            },

            Literal(Value::String(key)) => ObjectKey::Literal(key),

            Literal(Value::Number(key)) => ObjectKey::Literal(key),

            Literal(Value::Binary(num)) => ObjectKey::Binary(num),

            _ => {
                // Allow word tokens such as "null" and "typeof" as identifiers
                match token.as_word() {
                    Some(key) => ObjectKey::Literal(key.into()),
                    None      => unexpected_token!(self)
                }
            }
        };

        Ok(match next!(self) {
            Colon => ObjectMember::Value {
                key: key,
                value: self.expression(0)?,
            },
            ParenOpen => ObjectMember::Method {
                key: key,
                params: self.parameter_list()?,
                body: self.block_body()?,
            },
            _ => unexpected_token!(self)
        })
    }

    #[inline]
    fn object_expression(&mut self) -> Result<Expression> {
        Ok(Expression::Object(self.object_member_list()?))
    }

    #[inline]
    fn block_statement(&mut self) -> Result<Statement> {
        Ok(Statement::Block {
            body: self.block_body_tail()?,
        })
    }

    #[inline]
    fn block_body_tail(&mut self) -> Result<Vec<Statement>> {
        let mut body = Vec::new();

        loop {
            body.push(match next!(self) {
                BraceClose => break,
                token      => self.statement(token)?
            });
        }

        Ok(body)
    }

    #[inline]
    fn block_body(&mut self) -> Result<Vec<Statement>> {
        expect!(self, BraceOpen);
        self.block_body_tail()
    }

    fn arrow_function_expression(&mut self, p: Option<Expression>) -> Result<Expression> {
        let params: Vec<Parameter> = match p {
            None => Vec::new(),

            Some(Expression::Identifier(name)) => {
                vec![Parameter {
                    name    : name,
                    default : None,
                }]
            },

            Some(Expression::Binary {
                parenthesized : true,
                operator      : Assign,
                left,
                right,
            }) => {
                let name = match *left {
                    Expression::Identifier(value) => value,
                    _                 => unexpected_token!(self)
                };

                vec![Parameter {
                    name    : name,
                    default : Some(right),
                }]
            },

            Some(Expression::Sequence(mut list)) => {
                let mut params = Vec::with_capacity(list.len());

                for expression in list.drain(..) {
                    params.push(match expression {
                        Expression::Binary {
                            operator: Assign,
                            left,
                            right,
                            ..
                        } => {
                            let name = match *left {
                                Expression::Identifier(value) => value,
                                _ => unexpected_token!(self)
                            };

                            Parameter {
                                name    : name,
                                default : Some(right),
                            }
                        },

                        Expression::Identifier(name) => {
                            Parameter {
                                name    : name,
                                default : None
                            }
                        },

                        _ => unexpected_token!(self)
                    })
                }

                params
            },
            _ => unexpected_token!(self)
        };

        let body = match next!(self) {
            BraceOpen => {
                Statement::Block {
                    body: self.block_body_tail()?
                }
            }
            token => self.expression_from_token(token, 0)?.into()
        };

        Ok(Expression::ArrowFunction {
            params: params,
            body: Box::new(body)
        })
    }

    #[inline]
    fn prefix_expression(&mut self, operator: OperatorKind) -> Result<Expression> {
        if !operator.prefix() {
            unexpected_token!(self);
        }

        Ok(Expression::Prefix {
            operator: operator,
            operand: Box::new(self.expression(15)?),
        })
    }

    #[inline]
    fn infix_expression(&mut self, left: Expression, bp: u8, op: OperatorKind) -> Result<Expression> {
        Ok(match op {
            Increment | Decrement => Expression::Postfix {
                operator: op,
                operand: Box::new(left),
            },

            Accessor => {
                let ident = match next!(self) {
                    Identifier(ident) => ident,

                    // Allow word tokens such as "null" and "typeof" as identifiers
                    token => match token.as_word() {
                        Some(ident) => ident.into(),
                        None        => unexpected_token!(self)
                    },
                };

                Expression::member(left, ident)
            },

            Conditional => Expression::Conditional {
                test: Box::new(left),
                consequent: Box::new(self.expression(bp)?),
                alternate: {
                    expect!(self, Colon);
                    Box::new(self.expression(bp)?)
                }
            },

            FatArrow => return self.arrow_function_expression(Some(left)),

            _ => {
                if !op.infix() {
                    unexpected_token!(self);
                }

                if op.assignment() {
                    // TODO: verify that left is assignable
                }

                Expression::binary(left, op, self.expression(bp)?)
            }
        })
    }

    fn function_expression(&mut self) -> Result<Expression> {
        let name = match next!(self) {
            Identifier(name) => {
                expect!(self, ParenOpen);

                Some(name)
            },

            ParenOpen => None,

            _         => unexpected_token!(self),
        };

        Ok(Expression::Function {
            name: name,
            params: self.parameter_list()?,
            body: self.block_body()?,
        })
    }

    fn template_expression(&mut self, tag: Option<Box<Expression>>, mut kind: TemplateKind)
    -> Result<Expression> {
        let mut expressions = Vec::new();
        let mut quasis = Vec::new();

        loop {
            match kind {
                TemplateKind::Open(slice) => {
                    quasis.push(slice);

                    let expression = self.sequence_or_expression()?;

                    expressions.push(expression);

                    expect!(self, BraceClose);

                    kind = self.tokenizer.read_template_kind()?;
                }

                TemplateKind::Closed(slice) => {
                    quasis.push(slice);
                    break;
                }
            }
        }


        Ok(Expression::Template {
            tag: tag,
            expressions: expressions,
            quasis: quasis,
        })
    }

    #[inline]
    fn paren_expression(&mut self) -> Result<Expression> {
        match next!(self) {
            ParenClose => {
                expect!(self, Operator(FatArrow));

                self.arrow_function_expression(None)
            },
            token => {
                let expression = self.expression_from_token(token, 0)?;
                let expression = self.sequence_or(expression)?;

                expect!(self, ParenClose);

                Ok(expression.parenthesize())
            }
        }
    }

    #[inline]
    fn sequence_or_expression(&mut self) -> Result<Expression> {
        let token = next!(self);
        self.sequence_or_expression_from_token(token)
    }

    #[inline]
    fn sequence_or_expression_from_token(&mut self, token: Token) -> Result<Expression> {
        let first = self.expression_from_token(token, 0)?;
        self.sequence_or(first)
    }

    #[inline]
    fn sequence_or(&mut self, first: Expression) -> Result<Expression> {
        Ok(match peek!(self) {
            Comma => {
                self.consume();

                let mut list = vec![first, self.expression(0)?];

                loop {
                    match peek!(self) {
                        Comma => {
                            self.consume();

                            list.push(self.expression(0)?);
                        },
                        _ => break,
                    }
                }

                Expression::Sequence(list)
            },
            _ => first
        })
    }

    fn expression_list(&mut self) -> Result<Vec<Expression>> {
        let mut list = Vec::new();

        loop {
            match next!(self) {
                ParenClose => break,
                token      => {
                    let expression = self.expression_from_token(token, 0)?;
                    list.push(expression);
                }
            }

            match next!(self) {
                ParenClose => break,
                Comma      => continue,
                _          => unexpected_token!(self)
            }
        }

        Ok(list)
    }

    #[inline]
    fn expression(&mut self, lbp: u8) -> Result<Expression> {
        let token = next!(self);
        self.expression_from_token(token, lbp)
    }

    #[inline]
    fn expression_from_token(&mut self, token: Token, lbp: u8) -> Result<Expression> {
        let left = match token {
            This               => Expression::This,
            Literal(value)     => Expression::Literal(value),
            Identifier(value)  => Expression::from(value),
            Operator(Division) => self.regular_expression()?,
            Operator(optype)   => self.prefix_expression(optype)?,
            ParenOpen          => self.paren_expression()?,
            BracketOpen        => self.array_expression()?,
            BraceOpen          => self.object_expression()?,
            Function           => self.function_expression()?,
            Template(kind)     => self.template_expression(None, kind)?,
            _                  => unexpected_token!(self)
        };

        self.complex_expression(left, lbp)
    }

    #[inline]
    fn complex_expression(&mut self, mut left: Expression, lbp: u8) -> Result<Expression> {
        loop {
            left = match peek!(self) {
                Operator(op) => {
                    let rbp = op.binding_power();

                    if lbp > rbp {
                        break;
                    }

                    self.consume();

                    self.infix_expression(left, rbp, op)?
                },

                ParenOpen => {
                    if lbp > 18 {
                        break;
                    }

                    self.consume();

                    Expression::Call {
                        callee: Box::new(left),
                        arguments: self.expression_list()?,
                    }
                },

                BracketOpen => {
                    if lbp > 19 {
                        break;
                    }

                    self.consume();

                    let property = self.sequence_or_expression()?;

                    expect!(self, BracketClose);

                    Expression::ComputedMember {
                        object: Box::new(left),
                        property: Box::new(property),
                    }
                },

                Template(kind) => {
                    if lbp > 0 {
                        break;
                    }

                    self.consume();

                    let tag = Some(Box::new(left));

                    self.template_expression(tag, kind)?
                },

                _ => break
            }
        }

        Ok(left)
    }

    /// Helper for the `for` loops that doesn't consume semicolons
    fn variable_declaration(&mut self, kind: VariableDeclarationKind) -> Result<Statement> {
        Ok(Statement::VariableDeclaration {
            kind: kind,
            declarators: self.variable_declarators()?,
        })
    }

    #[inline]
    fn variable_declarators(&mut self) -> Result<Vec<VariableDeclarator>> {
        let mut declarators = Vec::new();

        loop {
            declarators.push(VariableDeclarator {
                name: expect_identifier!(self),
                value: match peek!(self) {
                    Operator(Assign) => {
                        self.consume();

                        Some(self.expression(0)?)
                    },
                    _ => None
                }
            });

            allow!(self, Comma => continue);

            break;
        }

        Ok(declarators)
    }

    #[inline]
    fn variable_declaration_statement(&mut self, kind: VariableDeclarationKind) -> Result<Statement> {
        let statement = self.variable_declaration(kind)?;

        expect_semicolon!(self);

        Ok(statement)
    }

    #[inline]
    fn labeled_or_expression_statement(&mut self, label: OwnedSlice) -> Result<Statement> {
        allow!(self, Colon => {
            return Ok(Statement::Labeled {
                label: label,
                body: Box::new(self.expect_statement()?)
            })
        });

        let first = self.complex_expression(label.into(), 0)?;

        let expression = self.sequence_or(first);

        expect_semicolon!(self);

        expression.map(|expr| Statement::from(expr))
    }

    #[inline]
    fn expression_statement(&mut self, token: Token) -> Result<Statement> {
        let statement = self.sequence_or_expression_from_token(token)?.into();

        expect_semicolon!(self);

        Ok(statement)
    }

    #[inline]
    fn return_statement(&mut self) -> Result<Statement> {
        let statement = Statement::Return {
            value: match peek!(self) {
                EndOfProgram => None,
                Semicolon    => None,
                _            => {
                    if self.tokenizer.asi() {
                        None
                    } else {
                        Some(self.sequence_or_expression()?)
                    }
                }
            }
        };

        expect_semicolon!(self);

        Ok(statement)
    }

    #[inline]
    fn throw_statement(&mut self) -> Result<Statement> {
        let statement = Statement::Throw {
            value: self.sequence_or_expression()?
        };

        expect_semicolon!(self);

        Ok(statement)
    }

    fn try_statement(&mut self) -> Result<Statement> {
        let body = self.expect_statement()?;

        expect!(self, Catch);
        expect!(self, ParenOpen);

        let error = expect_identifier!(self);

        expect!(self, ParenClose);

        let handler = self.expect_statement()?;

        Ok(Statement::Try {
            body: Box::new(body),
            error: error,
            handler: Box::new(handler),
        })
    }

    #[inline]
    fn break_statement(&mut self) -> Result<Statement> {
        let statement = Statement::Break {
            label: match peek!(self) {
                EndOfProgram => None,
                Semicolon    => None,
                _            => {
                    if self.tokenizer.asi() {
                        None
                    } else {
                        Some(expect_identifier!(self))
                    }
                }
            }
        };

        expect_semicolon!(self);

        Ok(statement)
    }

    fn if_statement(&mut self) -> Result<Statement> {
        expect!(self, ParenOpen);

        let test = self.expression(0)?;

        expect!(self, ParenClose);

        let consequent = Box::new(self.expect_statement()?);

        let alternate = match peek!(self) {
            Else => {
                self.consume();

                Some(Box::new(self.expect_statement()?))
            },

            _ => None
        };

        Ok(Statement::If {
            test: test,
            consequent: consequent,
            alternate: alternate,
        })
    }

    #[inline]
    fn while_statement(&mut self) -> Result<Statement> {
        expect!(self, ParenOpen);

        let test = self.expression(0)?;

        expect!(self, ParenClose);

        let body = Box::new(self.expect_statement()?);

        Ok(Statement::While {
            test: test,
            body: body,
        })
    }

    #[inline]
    fn for_statement(&mut self) -> Result<Statement> {
        expect!(self, ParenOpen);

        let init = match next!(self) {
            Semicolon         => None,

            Declaration(kind) => {
                let mut declarators = self.variable_declarators()?;

                if declarators.len() == 1 {
                    let value = declarators[0].value.take();

                    match value {
                        Some(Expression::Binary {
                            operator: In,
                            left,
                            right,
                            ..
                        }) => {
                            declarators[0].value = Some(*left);

                            let left = Statement::VariableDeclaration {
                                kind: kind,
                                declarators: declarators,
                            };

                            return self.for_in_statement_from_parts(left, *right);
                        },

                        _ => declarators[0].value = value
                    }
                }

                Some(Box::new(Statement::VariableDeclaration {
                    kind: kind,
                    declarators: declarators,
                }))
            }

            token => {
                let expression = self.sequence_or_expression_from_token(token)?;

                if let Expression::Binary {
                    operator: In,
                    left,
                    right,
                    ..
                } = expression {
                    return self.for_in_statement_from_parts(*left, *right);
                }

                Some(Box::new(expression.into()))
            },
        };

        if init.is_some() {
            match next!(self) {
                Operator(In)      => return self.for_in_statement(init.unwrap()),
                Identifier(ident) => {
                    if ident.as_str() != "of" {
                        unexpected_token!(self);
                    }
                    return self.for_of_statement(init.unwrap());
                },
                Semicolon         => {},
                _                 => unexpected_token!(self),
            }
        }

        let test = match next!(self) {
            Semicolon => None,
            token     => Some(self.sequence_or_expression_from_token(token)?),
        };

        if !test.is_none() {
            expect!(self, Semicolon);
        }

        let update = match next!(self) {
            ParenClose => None,
            token      => Some(self.sequence_or_expression_from_token(token)?),
        };
        if !update.is_none() {
            expect!(self, ParenClose);
        }

        let body = Box::new(self.expect_statement()?);

        Ok(Statement::For {
            init: init,
            test: test,
            update: update,
            body: body,
        })
    }

    fn for_in_statement_from_parts<S>(&mut self, left: S, right: Expression) -> Result<Statement>
    where S: Into<Statement> {
        let left = Box::new(left.into());

        expect!(self, ParenClose);

        let body = Box::new(self.expect_statement()?);

        Ok(Statement::ForIn {
            left: left,
            right: right,
            body: body,
        })
    }

    fn for_in_statement(&mut self, left: Box<Statement>) -> Result<Statement> {
        let right = self.sequence_or_expression()?;

        expect!(self, ParenClose);

        let body = Box::new(self.expect_statement()?);

        Ok(Statement::ForIn {
            left: left,
            right: right,
            body: body,
        })
    }

    fn for_of_statement(&mut self, left: Box<Statement>) -> Result<Statement> {
        let right = self.sequence_or_expression()?;

        expect!(self, ParenClose);

        let body = Box::new(self.expect_statement()?);

        Ok(Statement::ForOf {
            left: left,
            right: right,
            body: body,
        })
    }

    fn parameter_list(&mut self) -> Result<Vec<Parameter>> {
        let mut list = Vec::new();
        let mut default_params = false;

        loop {
            let name = match next!(self) {
                ParenClose       => break,
                Identifier(name) => name,
                _ => unexpected_token!(self)
            };

            list.push(match peek!(self) {
                Operator(Assign) => {
                    self.consume();
                    let expression = self.expression(0)?;
                    default_params = true;
                    Parameter {
                        name: name,
                        default: Some(Box::new(expression))
                    }
                }
                _ => {
                    if default_params {
                        unexpected_token!(self);
                    }
                    Parameter {
                        name: name,
                        default: None
                    }
                }
            });

            match next!(self) {
                ParenClose => break,
                Comma      => {},
                _          => unexpected_token!(self)
            }
        }

        Ok(list)
    }

    #[inline]
    fn function_statement(&mut self) -> Result<Statement> {
        let name = expect_identifier!(self);

        expect!(self, ParenOpen);

        Ok(Statement::Function {
            name: name,
            params: self.parameter_list()?,
            body: self.block_body()?,
        })
    }

    fn class_member(&mut self, key: ClassKey, is_static: bool) -> Result<ClassMember> {
        Ok(match next!(self) {
            ParenOpen => {
                if !is_static && key.is_constructor() {
                    ClassMember::Constructor {
                        params: self.parameter_list()?,
                        body: self.block_body()?,
                    }
                } else {
                    ClassMember::Method {
                        is_static: is_static,
                        key: key,
                        params: self.parameter_list()?,
                        body: self.block_body()?,
                    }
                }
            },
            Operator(Assign) => {
                ClassMember::Property {
                    is_static: is_static,
                    key: key,
                    value: self.expression(0)?,
                }
            },
            _ => unexpected_token!(self),
        })
    }

    #[inline]
    fn class_statement(&mut self) -> Result<Statement> {
        let name = expect_identifier!(self);
        let super_class = match next!(self) {
            Extends   => {
                let name = expect_identifier!(self);

                expect!(self, BraceOpen);

                Some(name)
            },
            BraceOpen => None,
            _         => unexpected_token!(self)
        };

        let mut members = Vec::new();

        loop {
            let mut token = next!(self);

            let is_static = match token {
                Static => {
                    token = next!(self);

                    true
                },

                _ => false
            };

            let key = match token {
                Semicolon => continue,

                BraceClose => break,

                Literal(Value::Number(num)) => ClassKey::Number(num),

                Literal(Value::Binary(num)) => ClassKey::Binary(num),

                Identifier(key) => ClassKey::Literal(key),

                BracketOpen => {
                    let expr = self.sequence_or_expression()?;

                    expect!(self, BracketClose);

                    ClassKey::Computed(expr)
                }

                _ => {
                    // Allow word tokens such as "null" and "typeof" as identifiers
                    match token.as_word() {
                        Some(key) => ClassKey::Literal(key.into()),
                        _         => unexpected_token!(self)
                    }
                }
            };

            members.push(self.class_member(key, is_static)?);
        }

        Ok(Statement::Class {
            name: name,
            extends: super_class,
            body: members,
        })
    }

    #[inline]
    fn regular_expression(&mut self) -> Result<Expression> {
        self.tokenizer.read_regular_expression()
    }

    #[inline]
    fn expect_statement(&mut self) -> Result<Statement> {
        let token = next!(self);

        self.statement(token)
    }

    #[inline]
    fn statement(&mut self, token: Token) -> Result<Statement> {
        match token {
            Semicolon          => Ok(Statement::Empty),
            BraceOpen          => self.block_statement(),
            Declaration(kind)  => self.variable_declaration_statement(kind),
            Return             => self.return_statement(),
            Break              => self.break_statement(),
            Function           => self.function_statement(),
            Class              => self.class_statement(),
            If                 => self.if_statement(),
            While              => self.while_statement(),
            For                => self.for_statement(),
            Identifier(label)  => self.labeled_or_expression_statement(label),
            Throw              => self.throw_statement(),
            Try                => self.try_statement(),
            _                  => self.expression_statement(token),
        }
    }

    #[inline]
    pub fn parse(&mut self) -> Result<Vec<Statement>> {
        let mut body = Vec::new();

        loop {
            body.push(match next!(self) {
                EndOfProgram => break,
                token        => self.statement(token)?
            })
        }

        Ok(body)
    }
}

pub fn parse(source: String) -> ParseResult<Program> {
    match Parser::new(&source).parse() {
        Ok(body) => Ok(Program {
            source: source,
            body: body
        }),
        Err(err) => match err {
            Error::UnexpectedEndOfProgram => {
                Err(ParseError::UnexpectedEndOfProgram)
            },

            Error::UnexpectedToken {
                start,
                end
            } => {
                Err(ParseError::UnexpectedToken {
                    source: source,
                    start: start,
                    end: end
                })
            }
        }
    }
}
