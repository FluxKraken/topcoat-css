//! Internal protocol shared by the build helper and procedural macros.

use std::collections::BTreeMap;

use proc_macro2::{TokenStream, TokenTree};
use serde::{Deserialize, Serialize};

pub const MANIFEST_VERSION: u32 = 1;
pub const MANIFEST_ENV: &str = "TOPCOAT_CSS_MANIFEST";
pub const STYLESHEET_ENV: &str = "TOPCOAT_CSS_STYLESHEET";

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub modules: Vec<Module>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Module {
    /// Canonical source path, used only to match an invocation at expansion time.
    pub file: String,
    /// One-based line and Unicode character column of the first input token.
    pub line: usize,
    pub column: usize,
    pub fingerprint: String,
    /// Valid Rust field names (including raw identifiers) to scoped CSS names.
    pub fields: BTreeMap<String, String>,
}

/// Compare token contents without relying on TokenStream's display whitespace.
/// CSS whitespace is preserved separately by the build-time source scanner.
pub fn fingerprint(tokens: TokenStream) -> String {
    let mut result = String::new();
    for token in tokens {
        let (kind, value) = match token {
            TokenTree::Group(group) => (
                format!("{:?}", group.delimiter()),
                fingerprint(group.stream()),
            ),
            TokenTree::Ident(ident) => ("ident".into(), ident.to_string()),
            TokenTree::Punct(punct) => ("punct".into(), punct.as_char().to_string()),
            TokenTree::Literal(literal) => ("literal".into(), literal.to_string()),
        };
        result.push_str(&format!("{}:{kind}{}:{value}", kind.len(), value.len()));
    }
    result
}
