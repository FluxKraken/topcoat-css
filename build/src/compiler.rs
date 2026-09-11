use lightningcss::{
    css_modules::{Config, CssModuleExports, CssModuleReference, Pattern},
    rules::CssRule,
    stylesheet::{ParserOptions, PrinterOptions, StyleSheet},
    values::url::Url,
    visitor::{Visit, VisitTypes, Visitor},
};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) struct Compiled {
    pub css: String,
    pub fields: BTreeMap<String, String>,
}

pub(crate) fn compile(
    css: &str,
    namespace: &str,
    file: &str,
    index: usize,
    minify: bool,
) -> Result<Compiled, String> {
    if css.trim().is_empty() {
        return Err("css! requires a nonempty stylesheet".into());
    }
    // Do not include absolute paths or source line numbers: moving a checkout or
    // adding unrelated lines must not rename every class.
    let mut hash = Sha256::new();
    for part in ["topcoat-css-v1", namespace, file, &index.to_string(), css] {
        hash.update((part.len() as u64).to_le_bytes());
        hash.update(part.as_bytes());
    }
    let hash: String = hash
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let pattern =
        Pattern::parse(&format!("[local]_tc{}", &hash[..24])).map_err(|e| e.to_string())?;
    let mut sheet = StyleSheet::parse(
        css,
        ParserOptions {
            filename: file.to_owned(),
            css_modules: Some(Config {
                pattern,
                ..Config::default()
            }),
            error_recovery: false,
            ..ParserOptions::default()
        },
    )
    .map_err(|e| format!("invalid CSS: {e}"))?;
    sheet
        .visit(&mut Validate)
        .map_err(|e| format!("unsupported CSS: {e}"))?;
    let result = sheet
        .to_css(PrinterOptions {
            minify,
            ..PrinterOptions::default()
        })
        .map_err(|e| format!("cannot compile CSS module: {e}"))?;
    let exports = result.exports.unwrap_or_default();
    let mut fields = BTreeMap::new();
    let mut originals = BTreeMap::new();
    for name in exports.keys().collect::<BTreeSet<_>>() {
        let field = field_name(name)?;
        if let Some(previous) = originals.insert(field.clone(), name) {
            return Err(format!(
                "CSS exports `{previous}` and `{name}` both produce Rust field `{field}`; rename one"
            ));
        }
        let mut values = Vec::new();
        resolve_export(name, &exports, &mut BTreeSet::new(), &mut values)?;
        fields.insert(field, values.join(" "));
    }
    Ok(Compiled {
        css: result.code,
        fields,
    })
}

fn field_name(name: &str) -> Result<String, String> {
    let field = name.replace('-', "_");
    if matches!(field.as_str(), "_" | "self" | "Self" | "super" | "crate") {
        return Err(format!(
            "CSS export `{name}` cannot become a Rust field; rename this export"
        ));
    }
    for candidate in [field.clone(), format!("r#{field}")] {
        if syn::parse_str::<syn::Ident>(&candidate).is_ok() {
            return Ok(candidate);
        }
    }
    Err(format!(
        "CSS export `{name}` cannot become a Rust field; use letters, digits, underscores, and hyphens, starting with a letter or underscore"
    ))
}

fn resolve_export(
    name: &str,
    exports: &CssModuleExports,
    visiting: &mut BTreeSet<String>,
    output: &mut Vec<String>,
) -> Result<(), String> {
    if !visiting.insert(name.to_owned()) {
        return Err(format!("cyclic CSS composition involving `{name}`"));
    }
    let export = &exports[name];
    push_unique(output, &export.name);
    for reference in &export.composes {
        match reference {
            CssModuleReference::Local { name: compiled } => {
                let (name, _) = exports
                    .iter()
                    .find(|(_, export)| &export.name == compiled)
                    .ok_or_else(|| {
                        format!("composition refers to undefined local class `{compiled}`")
                    })?;
                resolve_export(name, exports, visiting, output)?;
            }
            CssModuleReference::Global { name } => push_unique(output, name),
            CssModuleReference::Dependency { specifier, .. } => {
                return Err(format!(
                    "cross-file composes from `{specifier}` is unsupported; compose within the same css! module"
                ));
            }
        }
    }
    visiting.remove(name);
    Ok(())
}
fn push_unique(output: &mut Vec<String>, value: &str) {
    if !output.iter().any(|item| item == value) {
        output.push(value.to_owned());
    }
}

struct Validate;
impl<'i> Visitor<'i> for Validate {
    type Error = String;
    fn visit_types(&self) -> VisitTypes {
        VisitTypes::RULES | VisitTypes::URLS
    }
    fn visit_rule(&mut self, rule: &mut CssRule<'i>) -> Result<(), String> {
        match rule {
            CssRule::Import(_) => return Err("@import cannot be combined safely; load external stylesheets separately in the layout".into()),
            CssRule::Namespace(_) => return Err("@namespace is unsupported in a combined stylesheet".into()),
            CssRule::Unknown(rule) => return Err(format!("unknown @{} rule cannot be scoped safely", rule.name)),
            _ => {}
        }
        rule.visit_children(self)
    }
    fn visit_url(&mut self, url: &mut Url<'i>) -> Result<(), String> {
        let value = url.url.as_ref();
        // Generated CSS moves into Topcoat's bundle; source-relative resources
        // would resolve relative to that new URL, so reject them explicitly.
        let has_scheme = value.split_once(':').is_some_and(|(scheme, _)| {
            scheme.starts_with(|c: char| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        });
        if !(value.starts_with('/') || value.starts_with('#') || has_scheme) {
            return Err(format!(
                "relative url({value}) would break in the asset bundle; use a root-relative URL, absolute URL, or data URL"
            ));
        }
        Ok(())
    }
}
