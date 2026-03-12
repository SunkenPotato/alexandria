mod literal {
    use diagnostic::Diagnostic;
    use lexer::{Intern, TokenKind};

    use crate::Parse;

    #[derive(Debug, Clone, PartialEq)]
    pub enum Literal {
        Int(IntegerLiteral),
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
                    .and_then(|c| c.checked_add(next as u8 as u128))
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
    }

    impl Parse for StringLiteral {
        fn is_ok(&self) -> bool {
            matches!(self, Self::Ok(..))
        }

        fn parse<'diag, 'source, 'index>(
            mut guard: crate::ParseGuard<'diag, 'source, 'index>,
        ) -> crate::ParseResult<Self> {
            let next = guard.next_require(TokenKind::StringLit)?;
            let mut buf = String::with_capacity(next.item.symbol.len());

            let mut iter = next.item.symbol.chars();
            #[expect(
                clippy::unwrap_used,
                reason = "lexer checks that strings end in a quote"
            )]
            while let Some(ch) = iter.next() {
                match ch {
                    '"' => break,
                    '\\' => match iter.next().unwrap() {
                        'n' => '\n',
                        't' => '\t',
                        'a' => match iter.next().unwrap() {
                            '{' => {
                                let mut ascii_value: Option<u32> = None;
                                while let Some(v) = iter.clone().next()
                                    && v != '}'
                                {
                                    match v {
                                        x @ ('0'..='9' | 'a'..='f' | 'A'..='F') => {
                                            let ascii_v = ascii_value.get_or_insert_default();
                                            *ascii_v = ascii_v
                                                .checked_mul(16)h
                                                .checked_add(x.to_digit(16).unwrap());
                                        }
                                    }
                                }
                            }
                        },
                    },
                }
            }
        }
    }
}
