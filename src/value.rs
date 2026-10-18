//! Serialize a runtime value as one CSS custom-property value.

use cssparser::{ParseError, Parser, ParserInput, ToCss, Token};

pub fn value(value: &str) -> String {
    let mut input = ParserInput::new(value);
    let mut parser = Parser::new(&mut input);
    let mut result = String::new();
    serialize(&mut parser, &mut result, true, 0)
        .expect("css! interpolation must be a CSS value, without declarations or !important");
    result
}

fn serialize<'i>(
    input: &mut Parser<'i, '_>,
    output: &mut String,
    top_level: bool,
    depth: usize,
) -> Result<(), ParseError<'i, ()>> {
    // Bound recursive input independently of the Rust expression's type.
    if depth > 128 {
        return Err(input.new_custom_error(()));
    }
    while let Ok(token) = input.next_including_whitespace_and_comments().cloned() {
        if token.is_parse_error()
            || (top_level && matches!(token, Token::Semicolon | Token::Delim('!')))
        {
            return Err(input.new_unexpected_token_error(token));
        }
        let close = match token {
            Token::Function(_) | Token::ParenthesisBlock => Some(')'),
            Token::SquareBracketBlock => Some(']'),
            Token::CurlyBracketBlock => Some('}'),
            _ => None,
        };
        token.to_css(output).expect("writing CSS to a String");
        if let Some(close) = close {
            input.parse_nested_block(|input| serialize(input, output, false, depth + 1))?;
            output.push(close);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_css_values_and_contains_unterminated_tokens() {
        for css in [
            "hsl(200 100% 50%)",
            "1rem 2rem",
            "calc(100% - 1px)",
            "url(\"/image;a.png\")",
            "\"quoted; value\"",
            "var(--color, red)",
        ] {
            assert_eq!(value(css), css);
        }
        // CSS accepts EOF as the end of these tokens. Close them before appending
        // another generated declaration so one value cannot consume the next.
        assert_eq!(value("calc(1px"), "calc(1px)");
        assert_eq!(value("/* comment"), "/* comment*/");
        assert_eq!(value("\"text"), "\"text\"");
    }

    #[test]
    fn rejects_values_that_can_escape_their_declaration() {
        for css in [
            "red; color: blue",
            "red !important",
            "red!important",
            "red}",
            ")",
            "url(bad url)",
            "\"bad\nstring\"",
            "red; /*",
        ] {
            assert!(std::panic::catch_unwind(|| value(css)).is_err(), "{css}");
        }
    }
}
