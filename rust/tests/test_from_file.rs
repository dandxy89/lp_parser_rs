//! This Rust code snippet defines a function to read LP (Linear Programming) files from a resources
//! directory and parse them using the `lp_parser_rs` library.
//!
//! It includes several test functions and a macro to generate tests for various LP files, ensuring they can be parsed correctly.
//!

use std::error::Error;
use std::path::PathBuf;

use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

fn read_file_from_resources(file_name: &str) -> Result<String, Box<dyn Error + 'static>> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;

    Ok(contents)
}

#[test]
fn invalid() {
    let input = read_file_from_resources("invalid.lp").expect("failed to read file from resources");
    assert!(LpProblem::parse(&input).is_err());
}

#[test]
fn nom_fit2d() {
    let input = read_file_from_resources("fit2d.lp").expect("failed to read file from resources");
    let parsed = LpProblem::parse(&input).expect("failed to parse LPProblem");
    assert_eq!(parsed.objective_count(), 1);
    assert_eq!(parsed.constraint_count(), 25);
    assert_eq!(parsed.variable_count(), 10500);

    #[cfg(feature = "serde")]
    insta::assert_yaml_snapshot!(parsed, {
        ".variables" => insta::sorted_redaction(),
        ".objectives" => insta::sorted_redaction(),
        ".constraints" => insta::sorted_redaction(),
    });
}

#[test]
fn nom_debug() {
    let input = read_file_from_resources("infile_comments.lp").expect("failed to read file from resources");
    assert!(LpProblem::parse(&input).is_ok());
}

#[macro_export]
macro_rules! generate_test {
    ($test_name:ident, $file:expr) => {
        #[test]
        #[allow(unused_variables)]
        fn $test_name() {
            let input = read_file_from_resources($file).unwrap();
            let parsed = LpProblem::parse(&input).expect("failed to parse LPProblem");

            #[cfg(feature = "serde")]
            insta::assert_yaml_snapshot!(parsed, {
                ".variables" => insta::sorted_redaction(),
                ".objectives" => insta::sorted_redaction(),
                ".constraints" => insta::sorted_redaction(),
            });
        }
    };
}

generate_test!(pulp, "pulp.lp");
generate_test!(pulp2, "pulp2.lp");
generate_test!(limbo, "limbo.lp");
// generate_test!(semi_continuous, "semi_continuous.lp");
generate_test!(sos, "sos.lp");
generate_test!(test, "test.lp");
generate_test!(test2, "test2.lp");
generate_test!(empty_bounds, "empty_bounds.lp");
generate_test!(blank_lines, "blank_lines.lp");
// generate_test!(optional_labels, "optional_labels.lp");
generate_test!(infile_comments, "infile_comments.lp");
generate_test!(infile_comments2, "infile_comments2.lp");
generate_test!(missing_signs, "missing_signs.lp");
generate_test!(scientific_notation_2, "scientific_notation_2.lp");
generate_test!(output, "output.lp");
generate_test!(output2_1, "output2_1.lp");
generate_test!(output2_2, "output2_2.lp");
generate_test!(output2_3, "output2_3.lp");
generate_test!(output2_4, "output2_4.lp");
generate_test!(complex_names, "complex_names.lp");

// Test files from various open source projects on Github

// Mosek: <https://docs.mosek.com/latest/capi/lp-format.html>
generate_test!(scientific_notation, "scientific_notation.lp");
generate_test!(mosek, "mosek.lp");
generate_test!(mosek_bounds, "mosek_bounds.lp");
generate_test!(lol, "lol.lp");
generate_test!(milo1, "milo1.lp");

// From <https://github.com/asbestian/jplex>
generate_test!(obj3_2cons, "3obj_2cons.lp");

// generate_test!(no_end_section, "no_end_section.lp");
generate_test!(obj_2cons_only_binary_vars, "2obj_2cons_only_binary_vars.lp");
generate_test!(obj_2cons_all_variable_types, "2obj_2cons_all_variable_types.lp");
generate_test!(obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds.lp");
generate_test!(afiro, "afiro.lp");
generate_test!(afiro_ext, "afiro_ext.lp");
generate_test!(boeing1, "boeing1.lp");
generate_test!(boeing2, "boeing2.lp");
generate_test!(fit1d, "fit1d.lp");
generate_test!(kb2, "kb2.lp");
generate_test!(sc50a, "sc50a.lp");

// From <https://github.com/odow/LPWriter.jl>
generate_test!(model2, "model2.lp");
#[test]
fn corrupt() {
    let input = read_file_from_resources("corrupt.lp").expect("failed to read file from resources");
    assert!(LpProblem::parse(&input).is_err());
}

// From <https://github.com/brymck/lp-parse/tree>
generate_test!(model, "model.lp");

// From <https://github.com/josephcslater/JupyterExamples>
generate_test!(sudoku, "sudoku.lp");

// From <https://github.com/afaf-taik/vehicularFL>
generate_test!(wbm, "wbm.lp");

// From <https://github.com/IBMDecisionOptimization/cplexrunonwml>
generate_test!(diet, "diet.lp");

// From <https://github.com/claudiosa/CCS>
// generate_test!(output_cplex_2, "output_cplex_2.lp");

// From <https://github.com/coin-or/pulp>
generate_test!(american_steel_problem, "AmericanSteelProblem.lp");
generate_test!(beer_distribution_problem, "BeerDistributionProblem.lp");
generate_test!(computer_plant_problem, "ComputerPlantProblem.lp");
generate_test!(whiskas_model_2, "WhiskasModel2.lp");

// From <https://lpsolve.sourceforge.net/5.0/CPLEX-format.htm>
generate_test!(cplex, "cplex.lp");
