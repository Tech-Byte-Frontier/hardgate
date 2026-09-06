use hardgate::config::CloneConfig;
use hardgate::engines::{CloneDetector, CloneViolation};
use std::path::{Path, PathBuf};

fn detect(files: &[(&str, String)]) -> Vec<CloneViolation> {
    CloneDetector::new(&CloneConfig {
        enabled: true,
        min_lines: 5,
        min_tokens: 50,
        excludes: None,
    })
    .detect_clones(
        &files
            .iter()
            .map(|(path, text)| (PathBuf::from(path), text.clone()))
            .collect::<Vec<_>>(),
        Path::new("."),
    )
    .unwrap()
}

fn table(rows: usize, tuple: bool) -> String {
    let data = (0..rows).map(|i| if tuple {
        format!("  ['field{i}', 'Label {i}', 'number'],\n")
    } else { format!("  {{\n    key: 'field{i}',\n    label: 'Label {i}',\n    format: 'number'\n  }},\n") }).collect::<String>();
    format!("export const columns = [\n{data}];\n")
}

#[test]
fn homogeneous_literal_arrays_do_not_match_themselves_or_other_tables() {
    for tuple in [false, true] {
        for rows in [15, 35, 600] {
            let source = table(rows, tuple);
            assert!(
                detect(&[("columns.ts", source.clone())]).is_empty(),
                "self match: {rows} rows"
            );
            assert!(
                detect(&[("columns.ts", source.clone()), ("other.ts", source)]).is_empty(),
                "cross match: {rows} rows"
            );
        }
    }
}

const LOGIC: &str = "(value) => {\n  const first = value + 1;\n  const second = first * 2;\n  const third = second - 3;\n  const fourth = third / 4;\n  const fifth = fourth % 5;\n  const sixth = fifth + 6;\n  return sixth + value;\n}";

#[test]
fn data_boundaries_keep_embedded_handlers_and_following_logic() {
    for source in [
        format!("export const columns = [\n{{ key: 'value', format: {LOGIC} }}\n];\n"),
        format!("{}\nexport const calculate = {LOGIC};\n", table(20, false)),
        format!("export const items = [\n{LOGIC},\n{LOGIC}\n];\n"),
    ] {
        let clones = detect(&[("a.ts", source.clone()), ("b.ts", source)]);
        assert!(!clones.is_empty(), "executable data entry was hidden");
    }
}

fn wrapper(expression: &str) -> String {
    format!(
        "export function Chart() {{\n return (\n  <ResponsiveContainer width=\"100%\" height={{300}}>\n   <LineChart data={{data}}>\n    <CartesianGrid strokeDasharray=\"3 3\" />\n    <XAxis dataKey=\"name\" />\n    <YAxis />\n    <Tooltip />\n    <Legend />\n    <Line type=\"monotone\" dataKey=\"amount\" stroke=\"#123\" />\n    {expression}\n   </LineChart>\n  </ResponsiveContainer>\n );\n}}\n"
    )
}

#[test]
fn declarative_jsx_wrappers_do_not_obscure_embedded_executable_clones() {
    for extension in ["tsx", "jsx"] {
        let a = format!("a.{extension}");
        let b = format!("b.{extension}");
        assert!(detect(&[(&a, wrapper("")), (&b, wrapper(""))]).is_empty());
        let handler = wrapper(&format!("<button onClick={{{LOGIC}}}>Run</button>"));
        assert!(!detect(&[(&a, handler.clone()), (&b, handler)]).is_empty());
        let expression = wrapper(
            "{rows.map((row) => {\n const first = row.value + 1;\n const second = first * 2;\n const third = second - 3;\n const fourth = third / 4;\n const fifth = fourth % 5;\n const sixth = fifth + 6;\n return <span>{fourth}</span>;\n})}",
        );
        assert!(!detect(&[(&a, expression.clone()), (&b, expression)]).is_empty());
    }
}

#[test]
fn rust_literal_tuple_tables_are_distinct_from_executable_arrays() {
    let rows = (0..30)
        .map(|i| format!(" (\"key{i}\", \"label{i}\", {i}),\n"))
        .collect::<String>();
    let source = format!("const ROWS: &[(&str, &str, u32)] = &[\n{rows}];\n");
    assert!(detect(&[("rows.rs", source)]).is_empty());
    let logic = "fn calculate(value: i32) -> i32 {\n let first = value + 1;\n let second = first * 2;\n let third = second - 3;\n let fourth = third / 4;\n let fifth = fourth % 5;\n fifth + value\n}\n";
    assert!(!detect(&[("a.rs", logic.into()), ("b.rs", logic.into())]).is_empty());
}
