use std::fs;
use tempfile::TempDir;
use topcoat_css_build::{BuildConfig, BuildOutput};
use topcoat_css_core::Manifest;

struct Project(TempDir);
impl Project {
    fn new(source: &str) -> Self {
        let project = Self(tempfile::tempdir().unwrap());
        fs::create_dir(project.0.path().join("src")).unwrap();
        project.write("src/main.rs", source);
        project
    }
    fn write(&self, file: &str, source: &str) {
        fs::write(self.0.path().join(file), source).unwrap();
    }
    fn config(&self) -> BuildConfig {
        BuildConfig::new()
            .manifest_dir(self.0.path())
            .out_dir(self.0.path().join("out"))
            .namespace("test-app")
    }
    fn build(&self) -> (String, Manifest) {
        let output = self.config().compile().unwrap();
        read(output)
    }
}
fn read(output: BuildOutput) -> (String, Manifest) {
    (
        fs::read_to_string(output.stylesheet).unwrap(),
        serde_json::from_slice(&fs::read(output.manifest).unwrap()).unwrap(),
    )
}
fn raw(css: &str) -> String {
    format!("fn component() {{ let style = css!(r###\"{css}\"###); }}")
}

#[test]
fn scopes_selectors_without_rewriting_strings_attributes_urls_or_numbers() {
    let (css, manifest) = Project::new(&raw(r#"
        .card:hover .image, .card[data-label=".literal"] {
            padding: 0.5rem 1rem;
            opacity: 0.8;
            content: ".literal";
            background: url("https://example.com/picture.png");
            color: var(--foreground);
        }
    "#))
    .build();
    let fields = &manifest.modules[0].fields;
    assert_eq!(fields.len(), 2);
    assert!(css.contains(&format!(".{}:hover .{}", fields["card"], fields["image"])));
    assert!(css.contains(".literal"));
    assert!(css.contains("https://example.com/picture.png"));
    assert!(css.contains(".5rem 1rem") || css.contains("0.5rem 1rem"));
    assert!(css.contains("var(--foreground)"));
    assert!(!css.contains("literal_tc"));
}

#[test]
fn preserves_whitespace_sensitive_selectors_in_token_syntax() {
    let (css, manifest) = Project::new(
        r#"
        fn example() {
            let a = css! { .card :hover { color: red; } };
            let b = css! { .card:hover { color: red; } };
            let c = css! { .card .title { margin: 0.5rem; } };
            let d = css! { .card.title { margin: 0.5rem; } };
        }
    "#,
    )
    .build();
    assert!(css.contains(&format!(".{} :hover", manifest.modules[0].fields["card"])));
    assert!(css.contains(&format!(".{}:hover", manifest.modules[1].fields["card"])));
    assert!(css.contains(&format!(
        ".{} .{}",
        manifest.modules[2].fields["card"], manifest.modules[2].fields["title"]
    )));
    assert!(css.contains(&format!(
        ".{}.{}",
        manifest.modules[3].fields["card"], manifest.modules[3].fields["title"]
    )));
}

#[test]
fn supports_global_nested_rules_keyframes_and_custom_properties() {
    let (css, manifest) = Project::new(&raw(r#"
        :global(.theme) .card {
            --foreground: green;
            animation: fade-in 1s ease;
            &:hover { color: var(--foreground); }
        }
        @media (width < 800px) {
            @supports (display: grid) { .card { display: grid; } }
        }
        @keyframes fade-in { from { opacity: 0; } to { opacity: 1; } }
    "#))
    .build();
    let fields = &manifest.modules[0].fields;
    assert!(!fields.contains_key("theme"));
    assert!(css.contains(&format!(".theme .{}", fields["card"])));
    assert!(css.contains(&format!("@keyframes {}", fields["fade_in"])));
    assert!(
        css.lines()
            .any(|line| line.contains("animation:") && line.contains(&fields["fade_in"]))
    );
    assert!(css.contains("--foreground: green"));
    assert!(css.contains("@media"));
    assert!(css.contains("@supports"));
    assert!(css.contains("&:hover"));
}

#[test]
fn modules_are_separate_and_relocatable() {
    let source = "fn a() { css! { .card { color: red; } }; css! { .card { color: red; } }; }";
    let first = Project::new(source).build();
    let second = Project::new(source).build();
    assert_eq!(first.0, second.0);
    assert_ne!(
        first.1.modules[0].fields["card"],
        first.1.modules[1].fields["card"]
    );
    let shifted = Project::new(&format!("// unrelated line\n{source}")).build();
    assert_eq!(first.0, shifted.0);
}

#[test]
fn rebuild_replaces_deleted_and_changed_modules_and_adds_new_files() {
    let project = Project::new("fn a() { css! { .old { color: red; } }; }");
    let old = project.build().1.modules[0].fields["old"].clone();
    project.write("src/main.rs", "fn main() {}");
    project.write("src/new.rs", "fn new() { css! { .new { color: blue; } }; }");
    let (css, manifest) = project.build();
    assert!(!css.contains(&old));
    assert_eq!(manifest.modules.len(), 1);
    assert!(manifest.modules[0].fields.contains_key("new"));
    fs::remove_file(project.0.path().join("src/new.rs")).unwrap();
    assert!(project.build().1.modules.is_empty());
}

#[test]
fn scanner_ignores_comments_and_strings_and_handles_unicode_and_aliases() {
    let project = Project::new(
        r##"
        // css! { broken }
        const TEXT: &str = "css! { also broken }";
        fn example() { let _ = "café 🦀"; let style = custom::styles! { .café { content: "🦀"; } }; }
    "##,
    );
    let (css, manifest) = read(project.config().macro_name("styles").compile().unwrap());
    assert_eq!(manifest.modules.len(), 1);
    assert!(manifest.modules[0].fields.contains_key("café"));
    assert!(css.contains("🦀"));
}

#[test]
fn scanner_accepts_shebang_bom_and_crlf() {
    let source =
        "\u{feff}#!/usr/bin/env rust-script\r\nfn a() { css! { .card { color: red; } }; }\r\n";
    assert_eq!(Project::new(source).build().1.modules.len(), 1);
}

#[test]
fn export_fields_handle_hyphens_and_rust_keywords() {
    let (_, manifest) = Project::new(&raw(".user-card, .type, .async { color: red; }")).build();
    let fields = &manifest.modules[0].fields;
    assert!(fields.contains_key("user_card"));
    assert!(fields.contains_key("r#type"));
    assert!(fields.contains_key("r#async"));
}

#[test]
fn composition_resolves_transitive_local_and_global_names() {
    let (_, manifest) = Project::new(&raw(r#"
        .base { color: red; }
        .middle { composes: base; }
        .card { composes: middle; composes: external from global; }
    "#))
    .build();
    let fields = &manifest.modules[0].fields;
    let card: Vec<_> = fields["card"].split_whitespace().collect();
    assert!(card.contains(&fields["base"].as_str()));
    assert!(card.contains(&fields["middle"].split_whitespace().next().unwrap()));
    assert!(card.contains(&"external"));
}

#[test]
fn reports_css_and_export_errors_without_panicking() {
    for (css, expected) in [
        (".card { color red; }", "invalid CSS"),
        (
            ".user-card, .user_card { color: red; }",
            "both produce Rust field",
        ),
        (".self { color: red; }", "cannot become a Rust field"),
        ("@import 'other.css'; .a {}", "@import"),
        (
            "@namespace svg 'http://www.w3.org/2000/svg'; .a {}",
            "@namespace",
        ),
        ("@madeup { .a {} }", "unknown @madeup"),
        (
            ".card { background: url(images/photo.png); }",
            "relative url",
        ),
        (
            ".card { composes: base from './base.css'; }",
            "cross-file composes",
        ),
        (
            ".a { composes: b; } .b { composes: a; }",
            "cyclic CSS composition",
        ),
    ] {
        let error = Project::new(&raw(css))
            .config()
            .compile()
            .unwrap_err()
            .to_string();
        assert!(
            error.contains(expected),
            "{css}: expected {expected}, got {error}"
        );
        assert!(error.contains("main.rs"));
    }
}

#[test]
fn minification_preserves_export_names() {
    let project = Project::new(&raw(".card { color: red; padding: 1rem 2rem; }"));
    let pretty = project.build();
    let minified = read(project.config().minify(true).compile().unwrap());
    assert_eq!(pretty.1.modules[0].fields, minified.1.modules[0].fields);
    assert!(minified.0.len() < pretty.0.len());
}

#[test]
fn does_not_rewrite_unchanged_outputs() {
    let project = Project::new(&raw(".card { color: red; }"));
    let first = project.config().compile().unwrap();
    let timestamp = fs::metadata(&first.stylesheet).unwrap().modified().unwrap();
    let second = project.config().compile().unwrap();
    assert_eq!(
        timestamp,
        fs::metadata(second.stylesheet).unwrap().modified().unwrap()
    );
}
