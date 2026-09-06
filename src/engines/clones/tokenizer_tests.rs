use super::*;

fn words(source: &str) -> Vec<(String, usize)> {
    let mut interner = TokenInterner::default();
    tokenize(source, &mut interner)
        .into_iter()
        .map(|token| (interner.symbol(token.symbol).to_owned(), token.line))
        .collect()
}

#[test]
fn declarations_do_not_hide_the_first_executable_line() {
    for declaration in [
        "pub(crate) use crate::{one, two};",
        "pub use crate::one;",
        "import\t{ One } from 'module';",
        "import{One}from 'module';",
        "import\"module\";",
        "import'module';",
        "export * from 'module';",
        "export { One } from 'module';",
        "export type { One } from 'module';",
        "export {One} from 'module';",
        "pub(crate) type Alias = [u8; 2];",
        "export type Alias<T> = T[];",
        "type Alias =\n { one: number } |\n { two: string }",
        "type Alias = number \\\n | string",
        "type { One, Two };",
        "type Opaque;",
        "type Alias = {\n one: [number, string];\n callback: (value: number) => number\n}",
    ] {
        let source = format!("{declaration}\nrun(input);");
        let expected_line = declaration.lines().count() + 1;
        assert_eq!(
            words(&source),
            vec![
                ("run".into(), expected_line),
                ("(".into(), expected_line),
                ("input".into(), expected_line),
                (")".into(), expected_line),
                (";".into(), expected_line)
            ],
            "{declaration}"
        );
    }
}

#[test]
fn quoted_delimiters_and_escapes_do_not_extend_declarations() {
    for declaration in [
        r#"type Alias = { value: '"([{'; other: "'`)]}"; template: `'"[}` };"#,
        r#"type Alias = { value: 'it\'s ] }'; other: "escaped\" (" };"#,
        "type Alias = (([number]));",
    ] {
        assert_eq!(
            words(&format!("{declaration}\nexecute")),
            vec![("execute".into(), 2)],
            "{declaration}"
        );
    }
}

#[test]
fn executable_identifiers_literals_and_malformed_aliases_are_retained() {
    let tokens = words(
        "// comment\n# comment\n/* comment */\n \nlet snake_case2 = 10.5;\nlet text = \"escaped\\\"quote\";\nlet tail = 'unfinished\\",
    );
    assert!(
        tokens
            .iter()
            .any(|(word, line)| word == "snake_case2" && *line == 5)
    );
    assert_eq!(tokens.iter().filter(|(word, _)| word == "_LIT_").count(), 1);
    assert_eq!(tokens.iter().filter(|(word, _)| word == "_STR_").count(), 2);
    for source in [
        "type = number",
        "type invalid-name = number",
        "type Unfinished",
        "public_value = 1",
        "export function run() {}",
        "from_value = 1",
    ] {
        assert!(
            !words(source).is_empty(),
            "executable or malformed input was hidden: {source}"
        );
    }
}

#[test]
fn interned_symbols_distinguish_collisions_and_reuse_exact_matches() {
    let mut interner = TokenInterner::default();
    let first = interner.intern("first".into());
    assert_eq!(first, interner.intern("first".into()));
    interner.by_hash.insert(hash_token("second"), vec![first]);
    let second = interner.intern("second".into());
    assert_ne!(first, second);
    assert_eq!(interner.symbol(first), "first");
    assert_eq!(interner.symbol(second), "second");
    assert_eq!(interner.hash(second), hash_token("second"));
}

#[test]
fn javascript_dynamic_imports_remain_executable_clone_tokens() {
    for source in [
        "import('module');",
        "import ('module');",
        "import\n/* load */\n('module');",
        "import.meta.url",
    ] {
        assert!(
            words(source).iter().any(|(token, _)| token == "import"),
            "{source}"
        );
    }
}
