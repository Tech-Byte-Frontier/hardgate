use tree_sitter::Node;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Truth {
    Yes,
    No,
    Unknown,
}

impl Truth {
    fn not(self) -> Self {
        match self {
            Self::Yes => Self::No,
            Self::No => Self::Yes,
            Self::Unknown => Self::Unknown,
        }
    }
}

pub(super) fn requires_test(attribute: Node<'_>, source: &[u8]) -> bool {
    let Some(attr) = attribute.named_child(0) else {
        return false;
    };
    let Some(name) = attr
        .named_child(0)
        .and_then(|node| node.utf8_text(source).ok())
    else {
        return false;
    };
    if matches!(name, "test" | "bench") {
        return true;
    }
    if name != "cfg" {
        return false;
    }
    let Some(args) = attr.child_by_field_name("arguments") else {
        return false;
    };
    let mut tokens = Vec::new();
    leaves(args, source, &mut tokens);
    if tokens.first() != Some(&"(") || tokens.last() != Some(&")") {
        return false;
    }
    let tokens = &tokens[1..tokens.len() - 1];
    evaluate(tokens, false) == Truth::No && evaluate(tokens, true) != Truth::No
}

fn leaves<'a>(node: Node<'_>, source: &'a [u8], tokens: &mut Vec<&'a str>) {
    if matches!(node.kind(), "line_comment" | "block_comment") {
        return;
    }
    if node.child_count() == 0 || matches!(node.kind(), "string_literal" | "raw_string_literal") {
        if let Ok(text) = node.utf8_text(source) {
            tokens.push(text);
        }
        return;
    }
    for child in node.children(&mut node.walk()) {
        leaves(child, source, tokens);
    }
}

fn evaluate(tokens: &[&str], testing: bool) -> Truth {
    let mut index = 0;
    let value = predicate(tokens, &mut index, testing, 0);
    if index == tokens.len() {
        value
    } else {
        Truth::Unknown
    }
}

fn predicate(tokens: &[&str], index: &mut usize, testing: bool, depth: usize) -> Truth {
    if depth > 64 {
        return Truth::Unknown;
    }
    let Some(name) = tokens.get(*index).copied() else {
        return Truth::Unknown;
    };
    *index += 1;
    if tokens.get(*index) == Some(&"=") {
        *index = (*index + 2).min(tokens.len());
        return Truth::Unknown;
    }
    if tokens.get(*index) != Some(&"(") {
        return atom(name, testing);
    }
    let Some(values) = arguments(tokens, index, testing, depth) else {
        return Truth::Unknown;
    };
    combine(name, &values)
}

fn atom(name: &str, testing: bool) -> Truth {
    match name {
        "test" if testing => Truth::Yes,
        "test" | "false" => Truth::No,
        "true" => Truth::Yes,
        _ => Truth::Unknown,
    }
}

fn arguments(
    tokens: &[&str],
    index: &mut usize,
    testing: bool,
    depth: usize,
) -> Option<Vec<Truth>> {
    *index += 1;
    let mut values = Vec::new();
    while *index < tokens.len() && tokens[*index] != ")" {
        values.push(predicate(tokens, index, testing, depth + 1));
        match tokens.get(*index) {
            Some(&",") => *index += 1,
            Some(&")") => break,
            _ => return None,
        }
    }
    if tokens.get(*index) != Some(&")") {
        return None;
    }
    *index += 1;
    Some(values)
}

fn combine(name: &str, values: &[Truth]) -> Truth {
    match name {
        "not" if values.len() == 1 => values[0].not(),
        "all" => combine_all(values),
        "any" => combine_all(&values.iter().map(|value| value.not()).collect::<Vec<_>>()).not(),
        _ => Truth::Unknown,
    }
}

fn combine_all(values: &[Truth]) -> Truth {
    if values.contains(&Truth::No) {
        Truth::No
    } else if values.iter().all(|value| *value == Truth::Yes) {
        Truth::Yes
    } else {
        Truth::Unknown
    }
}

pub(super) fn path_attribute(attribute: Node<'_>, source: &[u8]) -> Option<String> {
    let attr = attribute.named_child(0)?;
    if attr.named_child(0)?.utf8_text(source).ok()? != "path" {
        return None;
    }
    let value = attr.child_by_field_name("value")?.utf8_text(source).ok()?;
    if value.starts_with('r') {
        let first = value.find('"')?;
        let last = value.rfind('"')?;
        return (last > first).then(|| value[first + 1..last].to_string());
    }
    serde_json::from_str(value).ok()
}
