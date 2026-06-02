use crate::{Parse, Path, expr::literal::Literal};

#[derive(Debug, Clone, PartialEq)]
pub enum BaseExpr {
    Literal(Literal),
    Path(Path),
}

impl Parse for BaseExpr {
    fn is_ok(&self) -> bool {
        match self {
            Self::Literal(v) => v.is_ok(),
            Self::Path(v) => v.is_ok(),
        }
    }

    fn parse<'diag, 'source, 'index>(
        mut guard: crate::ParseGuard<'diag, 'source, 'index>,
    ) -> crate::ParseResult<Self> {
        guard
            .with(Literal::parse)
            .map(Self::Literal)
            .or_else(|_| guard.with(Path::parse).map(Self::Path))
    }
}

mod literal {
    use diagnostic::Diagnostic;
    use lexer::{Intern, TokenKind};
    use span::Span;

    use crate::{Parse, ParseError};

    #[derive(Debug, Clone, PartialEq)]
    pub enum Literal {
        Int(IntegerLiteral),
        Str(StringLiteral),
    }

    impl Parse for Literal {
        fn is_ok(&self) -> bool {
            match self {
                Self::Int(v) => v.is_ok(),
                Self::Str(v) => v.is_ok(),
            }
        }

        fn parse<'diag, 'source, 'index>(
            guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let next = guard.peek()?;
            match next.item.kind {
                TokenKind::Integer => Ok(Self::Int(IntegerLiteral::parse(guard)?)),
                TokenKind::StringLit => Ok(Self::Str(StringLiteral::parse(guard)?)),
                _ => Err(ParseError::TokenMismatch(
                    vec![TokenKind::Integer, TokenKind::StringLit],
                    next.span,
                )),
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq)]
    pub enum IntegerLiteral {
        Ok(u128),
        Overflow,
    }

    impl Parse for IntegerLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let next = guard.next_require(TokenKind::Integer)?;
            let Some(int) = next.item.symbol.chars().try_fold(0u128, |c, next| {
                c.checked_mul(10)
                    .and_then(|c| c.checked_add((next as u32 - 0x30) as u128))
            }) else {
                guard.diagnostics.push(Diagnostic::error(
                    next.span,
                    "integer literal overflow: integer literals have a maximum capacity of 2^128",
                    None,
                ));

                return Ok(Self::Overflow);
            };

            Ok(Self::Ok(int))
        }
    }

    #[derive(Debug, Clone, PartialEq)]
    pub enum StringLiteral {
        Ok(Intern<str>),
        InvalidEsc,
    }

    impl Parse for StringLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let token = guard.next_require(TokenKind::StringLit)?;
            let mut buf = String::with_capacity(token.item.symbol.len());

            let mut iter = token.item.symbol.chars().enumerate().skip(1);
            let mut is_fail = false;
            while let Some((i, strch)) = iter.next() {
                let to_append = match strch {
                    '\\' => {
                        let esc_ch = iter.next().unwrap();
                        match esc_ch.1 {
                            't' => '\t',
                            'n' => '\n',
                            '0' => '\0',
                            '"' => '"',
                            '\\' => '\\',
                            '{' => {
                                guard.diagnostics.push(Diagnostic::error(
                                    Span::new(
                                        token.span.start() + i as u32,
                                        token.span.start() + esc_ch.0 as u32,
                                    ),
                                    "unicode escapes are not yet supported",
                                    None,
                                ));
                                is_fail = true;
                                continue;
                            }
                            other => {
                                guard.diagnostics.push(Diagnostic::error(
                                    token.span,
                                    format!("'{other}' is not an escape character"),
                                    None,
                                ));
                                continue;
                            }
                        }
                    }
                    '"' => break,
                    other => other,
                };

                buf.push(to_append);
            }

            if is_fail {
                return Ok(Self::InvalidEsc);
            }

            Ok(Self::Ok(buf.as_str().into()))
        }
    }
}

// --- tests ---
#[cfg(test)]
mod tests {
    use lexer::Intern;
    use span::{Span, Spanned};

    use crate::{
        assert_eq,
        expr::literal::{IntegerLiteral, Literal, StringLiteral},
    };

    #[test]
    fn parse_int() {
        assert_eq(
            "123456789",
            Spanned::new(Span::new(0, 9), Literal::Int(IntegerLiteral::Ok(123456789))),
        );
    }

    #[test]
    fn parse_int_overflow() {
        assert_eq(
            "340282366920938463463374607431768211456",
            Spanned::new(Span::new(0, 39), Literal::Int(IntegerLiteral::Overflow)),
        );
    }

    #[test]
    fn parse_str() {
        assert_eq(
            r#""gallia in tres partes divisa\nest""#,
            Spanned::new(
                Span::new(0, 35),
                Literal::Str(StringLiteral::Ok(Intern::from(
                    "gallia in tres partes divisa\nest",
                ))),
            ),
        )
    }

    #[test]
    fn parse_str_invalid_escape_codes() {
        assert_eq(
            r#""\x\{0001}""#,
            Spanned::new(Span::new(0, 11), Literal::Str(StringLiteral::InvalidEsc)),
        )
    }
}
