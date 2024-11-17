#![cfg(feature = "diff")]
#![cfg(feature = "serde")]

use std::path::PathBuf;

use lp_parser_rs::{
    model::{coefficient::Coefficient, constraint::Constraint, lp_error::LPParserError, lp_problem::LPProblem, sense::Cmp},
    parse::{parse_file, parse_lp_file},
};

#[test]
fn test_coefficient() {
    use diff::Diff;

    let a = Coefficient { var_name: "a".to_string(), coefficient: 1.0 };
    let b = Coefficient { var_name: "b".to_string(), coefficient: 1.0 };
    insta::assert_yaml_snapshot!(a.diff(&a));
    insta::assert_yaml_snapshot!(a.diff(&b));
}

#[test]
fn test_constraint() {
    use diff::Diff;

    let a = Constraint::Standard {
        name: "a".to_string(),
        coefficients: vec![Coefficient { var_name: "ca".to_string(), coefficient: 1.0 }],
        sense: Cmp::GreaterThan,
        rhs: 1.0,
    };
    let b = Constraint::Standard {
        name: "b".to_string(),
        coefficients: vec![Coefficient { var_name: "ca".to_string(), coefficient: 1.0 }],
        sense: Cmp::GreaterThan,
        rhs: 1.0,
    };
    insta::assert_yaml_snapshot!(a.diff(&a));
    insta::assert_yaml_snapshot!(a.diff(&b));
}

#[test]
fn test_file_comparison() {
    use diff::Diff;

    let a = read_file_from_resources("test.lp").unwrap();
    let a_copy = read_file_from_resources("test_copy.lp").unwrap();
    let b = read_file_from_resources("test2.lp").unwrap();

    insta::assert_yaml_snapshot!(a.diff(&a), {
        ".variables.altered" => insta::sorted_redaction(),
        ".variables.removed" => insta::sorted_redaction(),
        ".objectives.Altered" => insta::sorted_redaction(),
        ".objectives.Altered" => insta::sorted_redaction(),
        ".constraints.altered" => insta::sorted_redaction(),
        ".constraints.removed" => insta::sorted_redaction(),
    });

    insta::assert_yaml_snapshot!(a.diff(&a_copy), {
        ".variables.altered" => insta::sorted_redaction(),
        ".variables.removed" => insta::sorted_redaction(),
        ".objectives.Altered" => insta::sorted_redaction(),
        ".objectives.Altered" => insta::sorted_redaction(),
        ".constraints.altered" => insta::sorted_redaction(),
        ".constraints.removed" => insta::sorted_redaction(),
    });

    insta::assert_yaml_snapshot!(a.diff(&b), {
        ".variables.altered" => insta::sorted_redaction(),
        ".variables.removed" => insta::sorted_redaction(),
        ".objectives.Altered" => insta::sorted_redaction(),
        ".objectives.Altered" => insta::sorted_redaction(),
        ".constraints.altered" => insta::sorted_redaction(),
        ".constraints.removed" => insta::sorted_redaction(),
    });
}

fn read_file_from_resources(file_name: &str) -> Result<LPProblem, LPParserError> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    parse_lp_file(&contents)
}
