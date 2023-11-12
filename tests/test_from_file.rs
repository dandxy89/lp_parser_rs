use std::path::PathBuf;

use congenial_enigma::{
    model::{LPDefinition, Sense},
    parse::{parse_file, parse_lp_file},
};

#[test]
fn afiro() {
    let result = read_file_from_resources("afiro.lp").unwrap();
    assert_eq!("afiro.mps", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 3);
    assert_eq!(result.constraints.len(), 27);
    assert_eq!(result.variables.len(), 32);
}

#[test]
fn afiro_ext() {
    let result = read_file_from_resources("afiro_ext.lp").unwrap();
    assert_eq!("afiro.mps", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 4);
    assert_eq!(result.constraints.len(), 27);
    assert_eq!(result.variables.len(), 47);
}

#[test]
fn boeing1() {
    let result = read_file_from_resources("boeing1.lp").unwrap();
    assert_eq!("boeing1.lp", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 348);
    assert_eq!(result.variables.len(), 473);
}

#[test]
fn boeing2() {
    let result = read_file_from_resources("boeing2.lp").unwrap();
    assert_eq!("boeing2.mps", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 140);
    assert_eq!(result.variables.len(), 162);
}

#[test]
fn fit1d() {
    let result = read_file_from_resources("fit1d.lp").unwrap();
    assert_eq!("fit1d.mps", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 24);
    assert_eq!(result.variables.len(), 1026);
}

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
fn kb2() {
    let result = read_file_from_resources("kb2.lp").unwrap();
    assert_eq!("kb2.mps", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 43);
    assert_eq!(result.variables.len(), 41);
}

#[test]
fn pulp() {
    let result = read_file_from_resources("pulp.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 49);
    assert_eq!(result.variables.len(), 62);
}

#[test]
fn pulp2() {
    let result = read_file_from_resources("pulp2.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Maximize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 7);
    assert_eq!(result.variables.len(), 139);
}

#[test]
fn sc50a() {
    let result = read_file_from_resources("sc50a.lp").unwrap();
    assert_eq!("sc50a.lp", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 49);
    assert_eq!(result.variables.len(), 48);
}

#[test]
fn invalid() {
    let result = read_file_from_resources("invalid.lp");
    assert!(result.is_err());
}

#[test]
fn no_end_section() {
    let result = read_file_from_resources("no_end_section.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 4);
    assert_eq!(result.constraints.len(), 2);
    assert_eq!(result.variables.len(), 3);
}

#[test]
fn model2() {
    let result = read_file_from_resources("model2.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 4);
    assert_eq!(result.variables.len(), 8);
}

#[test]
fn limbo() {
    let result = read_file_from_resources("limbo.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 2);
    assert_eq!(result.constraints.len(), 2);
    assert_eq!(result.variables.len(), 4);
}

#[test]
fn obj3_2cons() {
    let result = read_file_from_resources("3obj_2cons.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 4);
    assert_eq!(result.constraints.len(), 2);
    assert_eq!(result.variables.len(), 3);
}

#[test]
fn obj_2cons_only_binary_vars() {
    let result = read_file_from_resources("2obj_2cons_only_binary_vars.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 2);
    assert_eq!(result.constraints.len(), 2);
    assert_eq!(result.variables.len(), 3);
}

#[test]
fn obj_2cons_all_variable_types() {
    let result = read_file_from_resources("2obj_2cons_all_variable_types.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 2);
    assert_eq!(result.constraints.len(), 2);
    assert_eq!(result.variables.len(), 3);
}

#[test]
fn obj_1cons_all_variables_with_bounds() {
    let result = read_file_from_resources("1obj_1cons_all_variables_with_bounds.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Maximize);
    assert_eq!(result.objectives.len(), 1);
    assert_eq!(result.constraints.len(), 1);
    assert_eq!(result.variables.len(), 3);
}

#[test]
fn semi_continuous() {
    let result = read_file_from_resources("semi_continuous.lp").unwrap();
    assert_eq!("", result.problem_name);
    assert_eq!(result.problem_sense, Sense::Minimize);
    assert_eq!(result.objectives.len(), 2);
    assert_eq!(result.constraints.len(), 2);
    assert_eq!(dbg!(result.variables).len(), 7);
}

fn read_file_from_resources(file_name: &str) -> anyhow::Result<LPDefinition> {
    let mut file_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    file_path.push(format!("resources/{file_name}"));
    let contents = parse_file(&file_path)?;
    parse_lp_file(&contents)
}
