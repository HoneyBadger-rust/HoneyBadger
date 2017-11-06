use parser::Parser;
use lexer::Token::*;
use lexer::{Asi, Token};
use ast::{Ptr, Loc, List, ListBuilder, Declarator, DeclarationKind};
use ast::{Statement, StatementPtr, Expression, ExpressionPtr};
use ast::OperatorKind::*;

impl<'ast> Parser<'ast> {
    #[inline]
    pub fn statement(&mut self, token: Token<'ast>) -> StatementPtr<'ast> {
        match token {
            Semicolon         => self.alloc_in_loc(Statement::Empty),
            Identifier(label) => self.labeled_or_expression_statement(label),
            BraceOpen         => self.block_statement(),
            Declaration(kind) => self.variable_declaration_statement(kind),
            Return            => self.return_statement(),
            Break             => self.break_statement(),
            Function          => self.function_statement(),
            Class             => self.class_statement(),
            If                => self.if_statement(),
            While             => self.while_statement(),
            Do                => self.do_statement(),
            For               => self.for_statement(),
            Throw             => self.throw_statement(),
            Try               => self.try_statement(),
            _                 => self.expression_statement(token),
        }
    }

    #[inline]
    fn expect_statement(&mut self) -> StatementPtr<'ast> {
        let token = self.next();
        self.statement(token)
    }

    #[inline]
    pub fn block_statement(&mut self) -> StatementPtr<'ast> {
        let body = self.block_body_tail();

        self.alloc(Statement::Block { body }.at(0, 0))
    }

    #[inline]
    pub fn expression_statement(&mut self, token: Token<'ast>) -> StatementPtr<'ast> {
        let expression = self.sequence_or_expression_from(token);

        let start = expression.start;
        let end = expression.end;

        expect_semicolon!(self);

        self.alloc(Statement::Expression { expression }.at(start, end))
    }

    #[inline]
    pub fn labeled_or_expression_statement(&mut self, label: &'ast str) -> StatementPtr<'ast> {
        if let Colon = self.peek() {
            self.consume();

            let body = self.expect_statement();

            return self.alloc(Statement::Labeled {
                label,
                body,
            }.at(0, 0))
        }

        let first = self.alloc_in_loc(Expression::Identifier(label));
        let first = self.complex_expression(first, 0);
        let expression = self.sequence_or(first);

        expect_semicolon!(self);

        self.alloc(Statement::Expression { expression }.at(0, 0))
    }

    #[inline]
    pub fn function_statement(&mut self) -> StatementPtr<'ast> {
        let name = expect_identifier!(self);
        let name = self.alloc_in_loc(name);
        let function = self.function(name);

        self.alloc(Statement::Function { function }.at(0, 0))
    }

    #[inline]
    fn class_statement(&mut self) -> StatementPtr<'ast> {
        let name = expect_identifier!(self);
        let name = self.alloc_in_loc(name);
        let class = self.class(name);

        self.alloc(Statement::Class { class }.at(0, 0))
    }

    #[inline]
    pub fn return_statement(&mut self) -> StatementPtr<'ast> {
        let value = match self.asi() {
            Asi::NoSemicolon => {
                let expression = self.sequence_or_expression();

                expect_semicolon!(self);

                Some(expression)
            }

            Asi::ImplicitSemicolon => None,
            Asi::ExplicitSemicolon => {
                self.consume();

                None
            }
        };

        self.alloc(Statement::Return { value }.at(0, 0))
    }

    #[inline]
    pub fn variable_declaration_statement(&mut self, kind: DeclarationKind) -> StatementPtr<'ast> {
        let declarators = self.variable_declarators();

        let declaration = self.alloc(Statement::Declaration {
            kind: kind,
            declarators
        }.at(0, 0));

        expect_semicolon!(self);

        declaration
    }

    #[inline]
    pub fn variable_declarator(&mut self) -> Ptr<'ast, Loc<Declarator<'ast>>> {
        let name = match self.next() {
            BraceOpen        => self.object_expression(),
            BracketOpen      => self.array_expression(),
            Identifier(name) => self.alloc_in_loc(Expression::Identifier(name)),
            _                => unexpected_token!(self),
        };

        let value = match self.peek() {
            Operator(Assign) => {
                self.consume();
                Some(self.expression(0))
            },
            _ => None
        };

        self.alloc(Loc::new(0, 0, Declarator {
            name,
            value,
        }))
    }

    #[inline]
    pub fn variable_declarators(&mut self) -> List<'ast, Loc<Declarator<'ast>>> {
        let mut builder = ListBuilder::new(self.arena, self.variable_declarator());

        match self.peek() {
            Comma => self.consume(),
            _     => return builder.into_list(),
        }

        loop {
            builder.push(self.variable_declarator());

            match self.peek() {
                Comma => self.consume(),
                _     => return builder.into_list(),
            }
        }
    }

    #[inline]
    pub fn break_statement(&mut self) -> StatementPtr<'ast> {
        let label = match self.asi() {
            Asi::ExplicitSemicolon => {
                self.consume();
                None
            },
            Asi::ImplicitSemicolon => None,
            Asi::NoSemicolon => {
                let label = expect_identifier!(self);

                expect_semicolon!(self);

                Some(self.alloc_in_loc(Expression::Identifier(label)))
            }
        };

        self.alloc(Statement::Break { label }.at(0, 0))
    }

    #[inline]
    pub fn throw_statement(&mut self) -> StatementPtr<'ast> {
        let value = self.sequence_or_expression();

        expect_semicolon!(self);

        self.alloc(Statement::Throw { value }.at(0, 0))
    }

    #[inline]
    pub fn try_statement(&mut self) -> StatementPtr<'ast> {
        let body = self.block_body();
        expect!(self, Catch);
        expect!(self, ParenOpen);

        let error = expect_identifier!(self);
        let error = self.alloc_in_loc(Expression::Identifier(error));
        expect!(self, ParenClose);

        let handler = self.block_body();
        expect_semicolon!(self);

        self.alloc(Statement::Try {
            body,
            error,
            handler,
        }.at(0, 0))
    }

    #[inline]
    pub fn if_statement(&mut self) -> StatementPtr<'ast> {
        expect!(self, ParenOpen);

        let test = self.expression(0);
        expect!(self, ParenClose);

        let consequent = self.expect_statement();

        let alternate = match self.peek() {
            Else => {
                self.consume();
                Some(self.expect_statement())
            },
            _ => None
        };

        self.alloc(Statement::If {
            test,
            consequent,
            alternate,
        }.at(0, 0))
    }

    #[inline]
    pub fn while_statement(&mut self) -> StatementPtr<'ast> {
        expect!(self, ParenOpen);

        let test = self.expression(0);
        expect!(self, ParenClose);

        let body = self.expect_statement();

        self.alloc(Statement::While {
            test,
            body,
        }.at(0, 0))
    }

    #[inline]
    pub fn do_statement(&mut self) -> StatementPtr<'ast> {
        let body = self.expect_statement();
        expect!(self, While);

        let test = self.expression(0);

        self.alloc(Statement::Do {
            body,
            test,
        }.at(0, 0))
    }

    #[inline]
    fn for_statement(&mut self) -> StatementPtr<'ast> {
        expect!(self, ParenOpen);

        let init = match self.next() {
            Semicolon => None,

            Declaration(kind) => {
                let declarators = self.variable_declarators();

                if let Some(&Loc {
                    item: Declarator {
                        value: Some(ref value),
                        ..
                    },
                    ..
                }) = declarators.only_element() {
                    if let Expression::Binary { operator: In, right, .. } = value.item {
                        let left = self.alloc(Statement::Declaration {
                            kind,
                            declarators,
                        }.at(0, 0));

                        return self.for_in_statement_from_parts(left, right);
                    }
                }

                Some(self.alloc(Statement::Declaration {
                    kind,
                    declarators,
                }.at(0, 0)))
            }

            token => {
                let expression = self.expression_from(token, 0);

                if let Expression::Binary {
                    operator: In,
                    left,
                    right,
                    ..
                } = expression.item {
                    let left = self.alloc(Statement::Expression {
                        expression: left
                    }.at(0, 0));

                    return self.for_in_statement_from_parts(left, right);
                }

                Some(self.alloc(Statement::Expression {
                    expression
                }.at(0, 0)))
            },
        };

        if let Some(init) = init {
            match self.next() {
                Operator(In)     => return self.for_in_statement(init),
                Identifier("of") => return self.for_of_statement(init),
                Semicolon        => {},
                _                => unexpected_token!(self),
            }
        }

        let test = match self.next() {
            Semicolon => None,
            token     => {
                let test = self.expression_from(token, 0);
                expect!(self, Semicolon);

                Some(test)
            }
        };

        let update = match self.next() {
            ParenClose => None,
            token      => {
                let update = self.expression_from(token, 0);
                expect!(self, ParenClose);

                Some(update)
            }
        };

        let body = self.expect_statement();

        self.alloc(Statement::For {
            init,
            test,
            update,
            body,
        }.at(0, 0))
    }

    fn for_in_statement_from_parts(&mut self, left: StatementPtr<'ast>, right: ExpressionPtr<'ast>) -> StatementPtr<'ast> {
        expect!(self, ParenClose);

        let body = self.expect_statement();

        self.alloc(Statement::ForIn {
            left,
            right,
            body,
        }.at(0, 0))
    }

    fn for_in_statement(&mut self, left: StatementPtr<'ast>) -> StatementPtr<'ast> {
        let right = self.sequence_or_expression();

        expect!(self, ParenClose);

        let body = self.expect_statement();

        self.alloc(Statement::ForIn {
            left,
            right,
            body,
        }.at(0, 0))
    }

    fn for_of_statement(&mut self, left: StatementPtr<'ast>) -> StatementPtr<'ast> {
        let right = self.sequence_or_expression();

        expect!(self, ParenClose);

        let body = self.expect_statement();

        self.alloc(Statement::ForOf {
            left,
            right,
            body,
        }.at(0, 0))
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use parser::parse;
    use parser::mock::Mock;
    use ast::{List, Value, ObjectMember, Function, Class, OperatorKind};

    #[test]
    fn block_statement() {
        let src = "{ true }";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Block {
                body: mock.list([
                    Statement::Expression {
                        expression: mock.ptr(Expression::Value(Value::True))
                    }
                ])
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn labeled_block_statement() {
        let src = "foobar: { true }";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Labeled {
                label: "foobar",
                body: mock.ptr(Statement::Block {
                    body: mock.list([
                        Statement::Expression {
                            expression: mock.ptr(Expression::Value(Value::True))
                        }
                    ])
                })
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn if_statement() {
        let src = "if (true) foo;";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::If {
                test: mock.ptr(Expression::Value(Value::True)),
                consequent: mock.ptr(Statement::Expression {
                    expression: mock.ident("foo")
                }),
                alternate: None
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn if_else_statement() {
        let src = "if (true) foo; else { bar; }";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::If {
                test: mock.ptr(Expression::Value(Value::True)),
                consequent: mock.ptr(Statement::Expression {
                    expression: mock.ident("foo")
                }),
                alternate: Some(mock.ptr(Statement::Block {
                    body: mock.list([
                        Statement::Expression {
                            expression: mock.ident("bar")
                        }
                    ])
                }))
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn while_statement() {
        let src = "while (true) foo;";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::While {
                test: mock.ptr(Expression::Value(Value::True)),
                body: mock.ptr(Statement::Expression {
                    expression: mock.ident("foo")
                })
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn while_statement_block() {
        let src = "while (true) { foo; }";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::While {
                test: mock.ptr(Expression::Value(Value::True)),
                body: mock.ptr(Statement::Block {
                    body: mock.list([
                        Statement::Expression {
                            expression: mock.ident("foo")
                        }
                    ])
                })
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn do_statement() {
        let src = "do foo; while (true)";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Do {
                body: mock.ptr(Statement::Expression {
                    expression: mock.ident("foo")
                }),
                test: mock.ptr(Expression::Value(Value::True))
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn break_statement() {
        let src = "break;";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Break {
                label: None,
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn break_statement_label() {
        let src = "break foo;";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Break {
                label: Some(mock.ident("foo")),
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn throw_statement() {
        let src = "throw '3'";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Throw {
                value: mock.ptr(Expression::Value(Value::String("'3'"))),
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn try_statement_empty() {
        let src = "try {} catch (err) {}";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Try {
                body: List::empty(),
                error: mock.ident("err"),
                handler: List::empty()
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn try_statement() {
        let src = "try { foo; } catch (err) { bar; }";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Try {
                body: mock.list([
                    Statement::Expression {
                        expression: mock.ident("foo")
                    }
                ]),
                error: mock.ident("err"),
                handler: mock.list([
                    Statement::Expression {
                        expression: mock.ident("bar")
                    }
                ]),
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn variable_declaration_statement() {
        let src = "var x, y, z = 42;";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Declaration {
                kind: DeclarationKind::Var,
                declarators: mock.list([
                    Declarator {
                        name: mock.ident("x"),
                        value: None,
                    },
                    Declarator {
                        name: mock.ident("y"),
                        value: None,
                    },
                    Declarator {
                        name: mock.ident("z"),
                        value: Some(mock.number("42"))
                    }
                ])
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn variable_declaration_statement_destructuring_array() {
        let src = "let [x, y] = [1, 2];";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Declaration {
                kind: DeclarationKind::Let,
                declarators: mock.list([
                    Declarator {
                        name: mock.ptr(Expression::Array {
                            body: mock.list([
                                Expression::Identifier("x"),
                                Expression::Identifier("y"),
                            ])
                        }),
                        value: Some(mock.ptr(Expression::Array {
                            body: mock.list([
                                Expression::Value(Value::Number("1")),
                                Expression::Value(Value::Number("2")),
                            ])
                        })),
                    },
                ])
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn variable_declaration_statement_destructuring_object() {
        let src = "const { x, y } = { a, b };";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Declaration {
                kind: DeclarationKind::Const,
                declarators: mock.list([
                    Declarator {
                        name: mock.ptr(Expression::Object {
                            body: mock.list([
                                ObjectMember::Shorthand("x"),
                                ObjectMember::Shorthand("y"),
                            ])
                        }),
                        value: Some(mock.ptr(Expression::Object {
                            body: mock.list([
                                ObjectMember::Shorthand("a"),
                                ObjectMember::Shorthand("b"),
                            ])
                        })),
                    },
                ])
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn for_statement() {
        let src = "for (let i = 0; i < 10; i++) {}";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::For {
                init: Some(mock.ptr(Statement::Declaration {
                    kind: DeclarationKind::Let,
                    declarators: mock.list([
                        Declarator {
                            name: mock.ident("i"),
                            value: Some(mock.number("0")),
                        }
                    ]),
                })),
                test: Some(mock.ptr(Expression::Binary {
                    operator: OperatorKind::Lesser,
                    left: mock.ident("i"),
                    right: mock.number("10"),
                })),
                update: Some(mock.ptr(Expression::Postfix {
                    operator: OperatorKind::Increment,
                    operand: mock.ident("i")
                })),
                body: mock.ptr(Statement::Block {
                    body: List::empty()
                })
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn empty_for_statement() {
        let src = "for (;;) {}";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::For {
                init: None,
                test: None,
                update: None,
                body: mock.ptr(Statement::Block {
                    body: List::empty()
                })
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn function_statement() {
        let src = "function foo() {}";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Function {
                function: Function {
                    name: mock.ptr("foo").into(),
                    params: List::empty(),
                    body: List::empty(),
                }
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    fn function_statement_must_have_name() {
        assert!(parse("function() {}").is_err());
    }

    #[test]
    fn class_statement() {
        let src = "class Foo {}";
        let module = parse(src).unwrap();
        let mock = Mock::new();

        let expected = mock.list([
            Statement::Class {
                class: Class {
                    name: mock.ptr("Foo").into(),
                    extends: None,
                    body: List::empty(),
                }
            }
        ]);

        assert_eq!(module.body(), expected);
    }

    #[test]
    #[should_panic]
    fn class_statement_must_have_name() {
        parse("class {}").unwrap();
    }
}