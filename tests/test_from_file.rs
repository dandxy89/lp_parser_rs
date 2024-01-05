use std::path::PathBuf;

use lp_parser_rs::{
    model::{lp_problem::LPProblem, sense::Sense},
    parse::{parse_file, parse_lp_file},
};

#[macro_export]
macro_rules! generate_test {
    ($test_name:ident, $file:expr, $sense:ident, $obj_len:expr, $con_len:expr, $var_len:expr, $o_sum:expr, $c_sum:expr) => {
        #[test]
        fn $test_name() {
            let result = read_file_from_resources($file).unwrap();
            // dbg!(&result);
            assert_eq!(result.problem_sense, Sense::$sense);

            assert_eq!(result.objectives.len(), $obj_len, "Failed Objective Count");
            let summation = &result.objectives.iter().map(|o| o.coefficients.iter().map(|c| c.coefficient).sum::<f64>()).sum();
            float_eq::assert_float_eq!($o_sum, *summation, abs <= 1e-3);

            assert_eq!(result.constraints.len(), $con_len, "Failed Constraint Count");
            let summation =
                &result.constraints.iter().map(|(_, constr)| constr.coefficients().iter().map(|c| c.coefficient).sum::<f64>()).sum::<f64>();
            float_eq::assert_float_eq!($c_sum, *summation, abs <= 1e-3);

            assert_eq!(result.variables.len(), $var_len, "Failed Variable Count");
        }
    };
    ($test_name:ident, $file:expr, $name:expr, $sense:ident, $obj_len:expr, $con_len:expr, $var_len:expr, $o_sum:expr, $c_sum:expr) => {
        #[test]
        fn $test_name() {
            let result = read_file_from_resources($file).unwrap();
            // dbg!(&result);
            assert_eq!($name, result.problem_name);
            assert_eq!(result.problem_sense, Sense::$sense);

            assert_eq!(result.objectives.len(), $obj_len, "Failed Objective Count");
            let summation = &result.objectives.iter().map(|o| o.coefficients.iter().map(|c| c.coefficient).sum::<f64>()).sum();
            float_eq::assert_float_eq!($o_sum, *summation, abs <= 1e-3);

            assert_eq!(result.constraints.len(), $con_len, "Failed Constraint Count");
            let summation =
                &result.constraints.iter().map(|(_, constr)| constr.coefficients().iter().map(|c| c.coefficient).sum::<f64>()).sum::<f64>();
            float_eq::assert_float_eq!($c_sum, *summation, abs <= 1e-3);

            assert_eq!(result.variables.len(), $var_len, "Failed Variable Count");
        }
    };
}

generate_test!(afiro, "afiro.lp", "afiro.mps", Minimize, 3, 27, 32, -4.100, 25.369);
generate_test!(afiro_ext, "afiro_ext.lp", "afiro_ext.mps", Minimize, 4, 27, 47, -5.239, 25.369);
generate_test!(boeing1, "boeing1.lp", "boeing1.lp", Minimize, 1, 348, 473, 1187.985, 194612.346);
generate_test!(boeing2, "boeing2.lp", "boeing2.mps", Minimize, 1, 140, 162, 78.488, 20863.836);
generate_test!(fit1d, "fit1d.lp", "fit1d.mps", Minimize, 1, 24, 1026, 82457.0, -146871.180);
generate_test!(kb2, "kb2.lp", "kb2.mps", Minimize, 1, 43, 41, 11.675, 10143.724);
generate_test!(pulp, "pulp.lp", Minimize, 1, 49, 62, 1., 73.44);
generate_test!(pulp2, "pulp2.lp", Maximize, 1, 7, 139, -6.0, 147.);
generate_test!(sc50a, "sc50a.lp", "sc50a.lp", Minimize, 1, 49, 48, -1., 30.3);
generate_test!(no_end_section, "no_end_section.lp", "", Minimize, 4, 2, 3, 1., -186.);
generate_test!(model2, "model2.lp", Minimize, 1, 4, 8, 0., 6.);
generate_test!(limbo, "limbo.lp", Minimize, 2, 2, 4, 2., 0.);
generate_test!(obj3_2cons, "3obj_2cons.lp", Minimize, 4, 2, 3, 1., -186.);
generate_test!(obj_2cons_only_binary_vars, "2obj_2cons_only_binary_vars.lp", Minimize, 2, 2, 3, -7.5, -186.);
generate_test!(obj_2cons_all_variable_types, "2obj_2cons_all_variable_types.lp", Minimize, 2, 2, 3, -7.5, -186.);
generate_test!(obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds.lp", Maximize, 1, 1, 3, -1., 16.5);
generate_test!(semi_continuous, "semi_continuous.lp", Minimize, 2, 2, 7, 2., 0.);
generate_test!(sos, "sos.lp", Maximize, 1, 6, 8, 0., 17.5);
generate_test!(test, "test.lp", Maximize, 1, 4, 12, 2., 2.9899);
generate_test!(test2, "test2.lp", Maximize, 1, 7, 139, -6., 147.);
generate_test!(empty_bounds, "empty_bounds.lp", Minimize, 1, 1, 2, 11., 2.);
generate_test!(blank_lines, "blank_lines.lp", Minimize, 1, 1, 3, 11., 2.);
generate_test!(optional_labels, "optional_labels.lp", Minimize, 1, 1, 4, 11., 1.);
generate_test!(infile_comments, "infile_comments.lp", Minimize, 1, 1, 7, 43., 7.);
generate_test!(infile_comments2, "infile_comments2.lp", Minimize, 1, 0, 7, 43., 0.);
generate_test!(missing_signs, "missing_signs.lp", Minimize, 1, 1, 6, 43., -3.);

#[test]
#[ignore = "fit2d.mps takes > 60 seconds"]
fn fit2d() {
    let result = read_file_from_resources("fit2d.lp").unwrap();
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 25);
    assert_eq!(result.variables.len(), 10500);
}

#[test]
fn invalid() {
    let result = read_file_from_resources("invalid.lp");
    assert!(result.is_err());
}

fn read_file_from_resources(file_name: &str) -> anyhow::Result<LPProblem> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    parse_lp_file(&contents)
}
