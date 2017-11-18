/// If the next token matches `$p`, consume that token and execute `$eval`.
#[macro_export]
macro_rules! allow {
    ($parser:ident, $p:pat => $eval:expr) => {
        match $parser.lexer.token {
            $p => {
                $parser.lexer.consume();
                $eval;
            },
            _ => {}
        }
    }
}

/// Return an error if the next token doesn't match $p.
#[macro_export]
macro_rules! expect {
    ($parser:ident, $p:pat) => {
        match $parser.lexer.token {
            $p => $parser.lexer.consume(),
            _  => return $parser.error()
        }
    }
}

/// Expect the next token to be an Identifier, extracting the OwnedSlice
/// out of it. Returns an error otherwise.
#[macro_export]
macro_rules! expect_identifier {
    ($parser:ident) => {
        match $parser.lexer.token {
            Identifier => {
                let ident = $parser.lexer.token_as_str();
                $parser.lexer.consume();
                ident
            },
            _                        => return $parser.error()
        }
    }
}

/// Expecta semicolon to terminate a statement. Will assume a semicolon
/// following the ASI rules.
#[macro_export]
macro_rules! expect_semicolon {
    ($parser:ident) => {
        match $parser.asi() {
            Asi::ExplicitSemicolon => $parser.lexer.consume(),
            Asi::ImplicitSemicolon => {},
            Asi::NoSemicolon       => return $parser.error(),
        }
    }
}

#[macro_export]
macro_rules! parameter_key {
    ($parser:ident) => {
        match $parser.lexer.token {
            ParenClose        => {
                $parser.lexer.consume();
                break;
            },
            Identifier => {
                let ident = $parser.lexer.token_as_str();
                $parser.lexer.consume();
                ParameterKey::Identifier(ident)
            },
            _ => return $parser.error()
        }
    }
}

#[cfg(test)]
#[macro_export]
macro_rules! assert_expr {
    ($module:expr, $expr:expr) => ({
        let mut body = $module.body().iter();

        match **body.next().unwrap() {
            Statement::Expression(ref expression) => assert_eq!(expression.item, Expression::from($expr)),
            _ => panic!("Statement isn't an expression!")
        }

        assert_eq!(body.next(), None);
    })
}
