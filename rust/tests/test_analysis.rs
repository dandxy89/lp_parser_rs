use std::error::Error;
use std::path::PathBuf;

use lp_parser_rs::analysis::AnalysisConfig;
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

fn read_file_from_resources(file_name: &str) -> Result<String, Box<dyn Error + 'static>> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    Ok(contents)
}

macro_rules! generate_analysis_test {
    ($test_name:ident, $file:expr) => {
        #[test]
        #[cfg(feature = "serde")]
        fn $test_name() {
            let input = read_file_from_resources($file).expect("failed to read file from resources");
            let problem = LpProblem::parse(&input).expect("failed to parse LPProblem");
            let analysis = problem.analyze_with_config(&AnalysisConfig::default());

            insta::assert_yaml_snapshot!(analysis, {
                // Sort arrays for deterministic output due to HashMap iteration
                ".variables.free_variables" => insta::sorted_redaction(),
                ".variables.unused_variables" => insta::sorted_redaction(),
                ".variables.fixed_variables" => insta::sorted_redaction(),
                ".variables.invalid_bounds" => insta::sorted_redaction(),
                ".constraints.empty_constraints" => insta::sorted_redaction(),
                ".constraints.singleton_constraints" => insta::sorted_redaction(),
                ".coefficients.large_coefficients" => insta::sorted_redaction(),
                ".coefficients.small_coefficients" => insta::sorted_redaction(),
                ".issues" => insta::sorted_redaction(),
                // Round floats for consistency
                ".summary.density" => insta::rounded_redaction(5),
            });
        }
    };
}

generate_analysis_test!(analysis_diet, "diet.lp");
generate_analysis_test!(analysis_afiro, "afiro.lp");
generate_analysis_test!(analysis_sos, "sos.lp");
generate_analysis_test!(analysis_pulp, "pulp.lp");
generate_analysis_test!(analysis_sudoku, "sudoku.lp");

generate_analysis_test!(analysis_all_variable_types, "2obj_2cons_all_variable_types.lp");
generate_analysis_test!(analysis_binary_vars, "2obj_2cons_only_binary_vars.lp");
generate_analysis_test!(analysis_bounded_vars, "1obj_1cons_all_variables_with_bounds.lp");

generate_analysis_test!(analysis_beer_distribution, "BeerDistributionProblem.lp");
generate_analysis_test!(analysis_american_steel, "AmericanSteelProblem.lp");
generate_analysis_test!(analysis_computer_plant, "ComputerPlantProblem.lp");
generate_analysis_test!(analysis_whiskas, "WhiskasModel2.lp");

generate_analysis_test!(analysis_empty_bounds, "empty_bounds.lp");
generate_analysis_test!(analysis_scientific_notation, "scientific_notation.lp");
generate_analysis_test!(analysis_complex_names, "complex_names.lp");

generate_analysis_test!(analysis_boeing1, "boeing1.lp");
generate_analysis_test!(analysis_sc50a, "sc50a.lp");
generate_analysis_test!(analysis_fit1d, "fit1d.lp");
