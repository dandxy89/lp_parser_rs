use std::path::PathBuf;

use congenial_enigma::{
    model::{LPDefinition, Sense},
    parse::{parse_file, parse_lp_file},
};

#[macro_export]
macro_rules! generate_test {
    ($test_name:ident, $file:expr, $sense:ident, $obj_len:expr, $con_len:expr, $var_len:expr) => {
        #[test]
        fn $test_name() {
            let result = read_file_from_resources($file).unwrap();
            assert_eq!(result.problem_sense, Sense::$sense);
            assert_eq!(result.objectives.len(), $obj_len);
            assert_eq!(result.constraints.len(), $con_len);
            assert_eq!(result.variables.len(), $var_len);
        }
    };
    ($test_name:ident, $file:expr, $name:expr, $sense:ident, $obj_len:expr, $con_len:expr, $var_len:expr) => {
        #[test]
        fn $test_name() {
            let result = read_file_from_resources($file).unwrap();
            assert_eq!($name, result.problem_name);
            assert_eq!(result.problem_sense, Sense::$sense);
            assert_eq!(result.objectives.len(), $obj_len);
            assert_eq!(result.constraints.len(), $con_len);
            assert_eq!(result.variables.len(), $var_len);
        }
    };
}

generate_test!(afiro, "afiro.lp", "afiro.mps", Minimize, 3, 27, 32);
generate_test!(afiro_ext, "afiro_ext.lp", "afiro_ext.mps", Minimize, 4, 27, 47);
generate_test!(boeing1, "boeing1.lp", "boeing1.lp", Minimize, 1, 348, 473);
generate_test!(boeing2, "boeing2.lp", "boeing2.mps", Minimize, 1, 140, 162);
generate_test!(fit1d, "fit1d.lp", "fit1d.mps", Minimize, 1, 24, 1026);
generate_test!(kb2, "kb2.lp", "kb2.mps", Minimize, 1, 43, 41);
generate_test!(pulp, "pulp.lp", Minimize, 1, 49, 62);
generate_test!(pulp2, "pulp2.lp", Maximize, 1, 7, 139);
generate_test!(sc50a, "sc50a.lp", "sc50a.lp", Minimize, 1, 49, 48);
generate_test!(no_end_section, "no_end_section.lp", "", Minimize, 4, 2, 3);
generate_test!(model2, "model2.lp", Minimize, 1, 4, 8);
generate_test!(limbo, "limbo.lp", Minimize, 2, 2, 4);
generate_test!(obj3_2cons, "3obj_2cons.lp", Minimize, 4, 2, 3);
generate_test!(obj_2cons_only_binary_vars, "2obj_2cons_only_binary_vars.lp", Minimize, 2, 2, 3);
generate_test!(obj_2cons_all_variable_types, "2obj_2cons_all_variable_types.lp", Minimize, 2, 2, 3);
generate_test!(obj_1cons_all_variables_with_bounds, "1obj_1cons_all_variables_with_bounds.lp", Maximize, 1, 1, 3);
generate_test!(semi_continuous, "semi_continuous.lp", Minimize, 2, 2, 7);
generate_test!(sos, "sos.lp", Maximize, 1, 6, 8);

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

fn read_file_from_resources(file_name: &str) -> anyhow::Result<LPDefinition> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    parse_lp_file(&contents)
}
