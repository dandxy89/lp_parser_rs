use std::error::Error;
use std::path::PathBuf;

use lp_parser_rs::mps::writer::write_mps_string;
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

fn read_mps_file(file_name: &str) -> Result<String, Box<dyn Error + 'static>> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/mps/{file_name}"));
    let contents = parse_file(&file_path)?;
    Ok(contents)
}

/// Generate a test that parses a single MPS file and validates basic structural properties.
macro_rules! mps_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let input = read_mps_file($file).unwrap_or_else(|e| panic!("Failed to read {}: {e}", $file));
            let problem = LpProblem::parse_mps(&input).unwrap_or_else(|e| panic!("Failed to parse {}: {e}", $file));

            // Every MPS file should produce at least one objective
            assert!(problem.objective_count() >= 1, "{}: expected at least 1 objective, got {}", $file, problem.objective_count());
        }
    };
}

mps_test!(mps_1obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds.mps");
mps_test!(mps_american_steel_problem, "AmericanSteelProblem.mps");
mps_test!(mps_beer_distribution_problem, "BeerDistributionProblem.mps");
mps_test!(mps_blank_lines, "blank_lines.mps");
mps_test!(mps_boeing1, "boeing1.mps");
mps_test!(mps_boeing2, "boeing2.mps");
mps_test!(mps_complex_names, "complex_names.mps");
mps_test!(mps_computer_plant_problem, "ComputerPlantProblem.mps");
mps_test!(mps_cplex, "cplex.mps");
mps_test!(mps_diet, "diet.mps");
mps_test!(mps_empty_bounds, "empty_bounds.mps");
mps_test!(mps_fit1d, "fit1d.mps");
mps_test!(mps_fit2d, "fit2d.mps");
mps_test!(mps_infile_comments, "infile_comments.mps");
mps_test!(mps_infile_comments2, "infile_comments2.mps");
mps_test!(mps_kb2, "kb2.mps");
mps_test!(mps_limbo, "limbo.mps");
mps_test!(mps_lol, "lol.mps");
mps_test!(mps_milo1, "milo1.mps");
mps_test!(mps_missing_signs, "missing_signs.mps");
mps_test!(mps_model, "model.mps");
mps_test!(mps_model2, "model2.mps");
mps_test!(mps_mosek_bounds, "mosek_bounds.mps");
mps_test!(mps_mosek, "mosek.mps");
mps_test!(mps_optional_labels, "optional_labels.mps");
mps_test!(mps_output_cplex_2, "output_cplex_2.mps");
mps_test!(mps_output, "output.mps");
mps_test!(mps_output2_1, "output2_1.mps");
mps_test!(mps_output2_2, "output2_2.mps");
mps_test!(mps_output2_3, "output2_3.mps");
mps_test!(mps_output2_4, "output2_4.mps");
mps_test!(mps_pulp, "pulp.mps");
mps_test!(mps_pulp2, "pulp2.mps");
mps_test!(mps_sc50a, "sc50a.mps");
mps_test!(mps_scientific_notation_2, "scientific_notation_2.mps");
mps_test!(mps_scientific_notation, "scientific_notation.mps");
mps_test!(mps_semi_continuous, "semi_continuous.mps");
mps_test!(mps_sudoku, "sudoku.mps");
mps_test!(mps_test2, "test2.mps");
mps_test!(mps_wbm, "wbm.mps");
mps_test!(mps_whiskas_model2, "WhiskasModel2.mps");

fn read_lp_file(file_name: &str) -> Result<String, Box<dyn Error + 'static>> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    Ok(contents)
}

macro_rules! parity_test {
    ($name:ident, $base:expr) => {
        #[test]
        fn $name() {
            let lp_input = read_lp_file(&format!("{}.lp", $base)).unwrap_or_else(|e| panic!("Failed to read {}.lp: {e}", $base));
            let mps_input = read_mps_file(&format!("{}.mps", $base)).unwrap_or_else(|e| panic!("Failed to read {}.mps: {e}", $base));

            let lp_problem = LpProblem::parse(&lp_input).unwrap_or_else(|e| panic!("Failed to parse {}.lp: {e}", $base));
            let mps_problem = LpProblem::parse_mps(&mps_input).unwrap_or_else(|e| panic!("Failed to parse {}.mps: {e}", $base));

            assert_eq!(
                lp_problem.sense, mps_problem.sense,
                "{}: sense mismatch (LP={:?}, MPS={:?})",
                $base, lp_problem.sense, mps_problem.sense
            );

            assert_eq!(
                lp_problem.constraint_count(),
                mps_problem.constraint_count(),
                "{}: constraint count mismatch (LP={}, MPS={})",
                $base,
                lp_problem.constraint_count(),
                mps_problem.constraint_count()
            );

            assert_eq!(
                lp_problem.objective_count(),
                mps_problem.objective_count(),
                "{}: objective count mismatch (LP={}, MPS={})",
                $base,
                lp_problem.objective_count(),
                mps_problem.objective_count()
            );

            assert_eq!(
                lp_problem.variable_count(),
                mps_problem.variable_count(),
                "{}: variable count mismatch (LP={}, MPS={})",
                $base,
                lp_problem.variable_count(),
                mps_problem.variable_count()
            );
        }
    };
}

parity_test!(parity_1obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds");
parity_test!(parity_american_steel_problem, "AmericanSteelProblem");
parity_test!(parity_beer_distribution_problem, "BeerDistributionProblem");
parity_test!(parity_blank_lines, "blank_lines");
parity_test!(parity_boeing1, "boeing1");
parity_test!(parity_boeing2, "boeing2");
parity_test!(parity_complex_names, "complex_names");
parity_test!(parity_computer_plant_problem, "ComputerPlantProblem");
parity_test!(parity_cplex, "cplex");
parity_test!(parity_diet, "diet");
parity_test!(parity_empty_bounds, "empty_bounds");
parity_test!(parity_fit1d, "fit1d");
parity_test!(parity_fit2d, "fit2d");
parity_test!(parity_infile_comments, "infile_comments");
parity_test!(parity_infile_comments2, "infile_comments2");
parity_test!(parity_kb2, "kb2");
parity_test!(parity_limbo, "limbo");
parity_test!(parity_lol, "lol");
parity_test!(parity_milo1, "milo1");
parity_test!(parity_missing_signs, "missing_signs");
parity_test!(parity_model, "model");
parity_test!(parity_model2, "model2");
parity_test!(parity_mosek_bounds, "mosek_bounds");
parity_test!(parity_mosek, "mosek");
parity_test!(parity_optional_labels, "optional_labels");
parity_test!(parity_output_cplex_2, "output_cplex_2");
parity_test!(parity_output, "output");
parity_test!(parity_output2_1, "output2_1");
parity_test!(parity_output2_2, "output2_2");
parity_test!(parity_output2_3, "output2_3");
parity_test!(parity_output2_4, "output2_4");
parity_test!(parity_pulp, "pulp");
parity_test!(parity_pulp2, "pulp2");
parity_test!(parity_sc50a, "sc50a");
parity_test!(parity_scientific_notation_2, "scientific_notation_2");
parity_test!(parity_scientific_notation, "scientific_notation");
parity_test!(parity_semi_continuous, "semi_continuous");
parity_test!(parity_sudoku, "sudoku");
parity_test!(parity_test2, "test2");
parity_test!(parity_wbm, "wbm");
parity_test!(parity_whiskas_model2, "WhiskasModel2");

/// Generate a round-trip test: parse MPS → write MPS → re-parse → assert structural parity.
macro_rules! mps_round_trip_test {
    ($name:ident, $file:expr) => {
        #[test]
        fn $name() {
            let input = read_mps_file($file).unwrap_or_else(|e| panic!("Failed to read {}: {e}", $file));
            let original = LpProblem::parse_mps(&input).unwrap_or_else(|e| panic!("Failed to parse {}: {e}", $file));

            let written = write_mps_string(&original).unwrap_or_else(|e| panic!("Failed to write {}: {e}", $file));
            let round_tripped =
                LpProblem::parse_mps(&written).unwrap_or_else(|e| panic!("Failed to re-parse {}: {e}\n\nWritten MPS:\n{written}", $file));

            assert_eq!(original.sense, round_tripped.sense, "{}: sense mismatch", $file);
            assert_eq!(
                original.objective_count(),
                round_tripped.objective_count(),
                "{}: objective count mismatch (original={}, round-tripped={})",
                $file,
                original.objective_count(),
                round_tripped.objective_count()
            );
            assert_eq!(
                original.constraint_count(),
                round_tripped.constraint_count(),
                "{}: constraint count mismatch (original={}, round-tripped={})",
                $file,
                original.constraint_count(),
                round_tripped.constraint_count()
            );
            assert_eq!(
                original.variable_count(),
                round_tripped.variable_count(),
                "{}: variable count mismatch (original={}, round-tripped={})",
                $file,
                original.variable_count(),
                round_tripped.variable_count()
            );
        }
    };
}

mps_round_trip_test!(rt_1obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds.mps");
mps_round_trip_test!(rt_american_steel_problem, "AmericanSteelProblem.mps");
mps_round_trip_test!(rt_beer_distribution_problem, "BeerDistributionProblem.mps");
mps_round_trip_test!(rt_blank_lines, "blank_lines.mps");
mps_round_trip_test!(rt_boeing1, "boeing1.mps");
mps_round_trip_test!(rt_boeing2, "boeing2.mps");
mps_round_trip_test!(rt_complex_names, "complex_names.mps");
mps_round_trip_test!(rt_computer_plant_problem, "ComputerPlantProblem.mps");
mps_round_trip_test!(rt_cplex, "cplex.mps");
mps_round_trip_test!(rt_diet, "diet.mps");
mps_round_trip_test!(rt_empty_bounds, "empty_bounds.mps");
mps_round_trip_test!(rt_fit1d, "fit1d.mps");
mps_round_trip_test!(rt_fit2d, "fit2d.mps");
mps_round_trip_test!(rt_infile_comments, "infile_comments.mps");
mps_round_trip_test!(rt_infile_comments2, "infile_comments2.mps");
mps_round_trip_test!(rt_kb2, "kb2.mps");
mps_round_trip_test!(rt_limbo, "limbo.mps");
mps_round_trip_test!(rt_lol, "lol.mps");
mps_round_trip_test!(rt_milo1, "milo1.mps");
mps_round_trip_test!(rt_missing_signs, "missing_signs.mps");
mps_round_trip_test!(rt_model, "model.mps");
mps_round_trip_test!(rt_model2, "model2.mps");
mps_round_trip_test!(rt_mosek_bounds, "mosek_bounds.mps");
mps_round_trip_test!(rt_mosek, "mosek.mps");
mps_round_trip_test!(rt_optional_labels, "optional_labels.mps");
mps_round_trip_test!(rt_output_cplex_2, "output_cplex_2.mps");
mps_round_trip_test!(rt_output, "output.mps");
mps_round_trip_test!(rt_output2_1, "output2_1.mps");
mps_round_trip_test!(rt_output2_2, "output2_2.mps");
mps_round_trip_test!(rt_output2_3, "output2_3.mps");
mps_round_trip_test!(rt_output2_4, "output2_4.mps");
mps_round_trip_test!(rt_pulp, "pulp.mps");
mps_round_trip_test!(rt_pulp2, "pulp2.mps");
mps_round_trip_test!(rt_sc50a, "sc50a.mps");
mps_round_trip_test!(rt_scientific_notation_2, "scientific_notation_2.mps");
mps_round_trip_test!(rt_scientific_notation, "scientific_notation.mps");
mps_round_trip_test!(rt_semi_continuous, "semi_continuous.mps");
mps_round_trip_test!(rt_sudoku, "sudoku.mps");
mps_round_trip_test!(rt_test2, "test2.mps");
mps_round_trip_test!(rt_wbm, "wbm.mps");
mps_round_trip_test!(rt_whiskas_model2, "WhiskasModel2.mps");
