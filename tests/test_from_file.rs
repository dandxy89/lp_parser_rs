use std::path::PathBuf;

use lp_parser_rs::{
    model::{lp_error::LPParserError, lp_problem::LPProblem},
    parse::{parse_file, parse_lp_file},
};

#[macro_export]
macro_rules! generate_test {
    ($test_name:ident, $file:expr, $o_sum:expr, $c_sum:expr) => {
        #[test]
        fn $test_name() {
            let result = read_file_from_resources($file).unwrap();

            #[cfg(feature = "serde")]
            insta::assert_yaml_snapshot!(result, {
                ".variables" => insta::sorted_redaction(),
                ".objectives" => insta::sorted_redaction(),
                ".constraints" => insta::sorted_redaction(),
            });

            let summation = &result.objectives.iter().map(|o| o.coefficients.iter().map(|c| c.coefficient).sum::<f64>()).sum();
            float_eq::assert_float_eq!($o_sum, *summation, abs <= 1e-2);

            let summation = &result.constraints.iter().map(|(_, c)| c.coefficients().iter().map(|c| c.coefficient).sum::<f64>()).sum();
            float_eq::assert_float_eq!($c_sum, *summation, abs <= 1e-2);
        }
    };
}

generate_test!(afiro, "afiro.lp", -4.100, 25.369);
generate_test!(afiro_ext, "afiro_ext.lp", -5.239, 25.369);
generate_test!(boeing1, "boeing1.lp", 1187.985, 194612.346);
generate_test!(boeing2, "boeing2.lp", 78.488, 20863.836);
generate_test!(fit1d, "fit1d.lp", 82457.0, -146871.180);
generate_test!(kb2, "kb2.lp", 11.675, 10143.724);
generate_test!(pulp, "pulp.lp", 1., 73.44);
generate_test!(pulp2, "pulp2.lp", -6.0, 147.);
generate_test!(sc50a, "sc50a.lp", -1., 30.3);
generate_test!(model2, "model2.lp", 0., 6.);
generate_test!(limbo, "limbo.lp", 2., 0.);
generate_test!(obj3_2cons, "3obj_2cons.lp", 1., -186.);
generate_test!(obj_2cons_only_binary_vars, "2obj_2cons_only_binary_vars.lp", -7.5, -186.);
generate_test!(obj_2cons_all_variable_types, "2obj_2cons_all_variable_types.lp", -7.5, -186.);
generate_test!(obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds.lp", -1., 16.5);
generate_test!(semi_continuous, "semi_continuous.lp", 2., 0.);
generate_test!(sos, "sos.lp", 0., 17.5);
generate_test!(test, "test.lp", 2., 2.9899);
generate_test!(test2, "test2.lp", -6., 147.);
generate_test!(empty_bounds, "empty_bounds.lp", 11., 2.);
generate_test!(blank_lines, "blank_lines.lp", 11., 2.);
generate_test!(optional_labels, "optional_labels.lp", 11., 1.);
generate_test!(infile_comments, "infile_comments.lp", 43., 7.);
generate_test!(infile_comments2, "infile_comments2.lp", 43., 0.);
generate_test!(missing_signs, "missing_signs.lp", 43., -3.);
generate_test!(fit2d, "fit2d.lp", 349048.9, -296677.389);
generate_test!(scientific_notation, "scientific_notation.lp", 238.0324, -43.87680);

#[test]
fn invalid() {
    let result = read_file_from_resources("invalid.lp");
    assert!(result.is_err());
}

fn read_file_from_resources(file_name: &str) -> Result<LPProblem, LPParserError> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    parse_lp_file(&contents)
}
