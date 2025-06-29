use std::path::Path;

use divan::{Bencher, black_box};
use lp_parser_rs::parser::parse_file;
use lp_parser_rs::problem::LpProblem;

fn main() {
    divan::main();
}

const SMALL_FILES: &[&str] = &["test.lp", "diet.lp", "pulp.lp", "3obj_2cons.lp"];
const MEDIUM_FILES: &[&str] = &["afiro.lp", "boeing1.lp", "kb2.lp"];
const LARGE_FILES: &[&str] = &["fit2d.lp", "boeing2.lp", "sc50a.lp"];
const COMPLEX_FILES: &[&str] = &["sos.lp", "semi_continuous.lp", "scientific_notation.lp", "complex_names.lp"];

fn load_test_file(filename: &str) -> String {
    let path = Path::new("resources").join(filename);
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("Failed to read test file: {filename}"))
}

fn get_test_file_path(filename: &str) -> std::path::PathBuf {
    Path::new("resources").join(filename)
}

// ============================================================================
// END-TO-END PARSING BENCHMARKS
// ============================================================================

#[divan::bench(args = SMALL_FILES)]
fn parse_small_files_from_string(bencher: Bencher, filename: &str) {
    let content = load_test_file(filename);

    bencher.with_inputs(|| content.clone()).bench_values(|content| {
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

#[divan::bench(args = MEDIUM_FILES)]
fn parse_medium_files_from_string(bencher: Bencher, filename: &str) {
    let content = load_test_file(filename);

    bencher.with_inputs(|| content.clone()).bench_values(|content| {
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

#[divan::bench(args = LARGE_FILES)]
fn parse_large_files_from_string(bencher: Bencher, filename: &str) {
    let content = load_test_file(filename);

    bencher.with_inputs(|| content.clone()).bench_values(|content| {
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

#[divan::bench(args = COMPLEX_FILES)]
fn parse_complex_files_from_string(bencher: Bencher, filename: &str) {
    let content = load_test_file(filename);

    bencher.with_inputs(|| content.clone()).bench_values(|content| {
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

// ============================================================================
// FILE I/O BENCHMARKS
// ============================================================================

#[divan::bench(args = SMALL_FILES)]
fn parse_small_files_with_io(bencher: Bencher, filename: &str) {
    let path = get_test_file_path(filename);

    bencher.bench(|| {
        let content = parse_file(&path).expect("Failed to read file");
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

#[divan::bench(args = MEDIUM_FILES)]
fn parse_medium_files_with_io(bencher: Bencher, filename: &str) {
    let path = get_test_file_path(filename);

    bencher.bench(|| {
        let content = parse_file(&path).expect("Failed to read file");
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

#[divan::bench(args = LARGE_FILES)]
fn parse_large_files_with_io(bencher: Bencher, filename: &str) {
    let path = get_test_file_path(filename);

    bencher.bench(|| {
        let content = parse_file(&path).expect("Failed to read file");
        let problem = LpProblem::parse(&content).expect("Failed to parse LP problem");
        black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
    })
}

// ============================================================================
// COMPONENT-LEVEL BENCHMARKS
// ============================================================================

mod component_benchmarks {
    use divan::black_box;
    use lp_parser_rs::parsers::constraint::parse_constraints;
    use lp_parser_rs::parsers::objective::parse_objectives;
    use lp_parser_rs::parsers::variable::{parse_binary_section, parse_bounds_section};

    use super::*;

    #[divan::bench]
    fn parse_objectives_component(bencher: Bencher) {
        let content = load_test_file("boeing1.lp");

        bencher.bench(|| {
            let result = parse_objectives(&content).expect("Failed to parse objectives");
            black_box(result.0.len())
        })
    }

    #[divan::bench]
    fn parse_constraints_component(bencher: Bencher) {
        let content = load_test_file("boeing1.lp");

        bencher.bench(|| {
            let result = parse_constraints(&content).expect("Failed to parse constraints");
            black_box(result.0.len())
        })
    }

    #[divan::bench]
    fn parse_bounds_component(bencher: Bencher) {
        let content = load_test_file("1obj_1cons_all_variables_with_bounds.lp");

        bencher.bench(|| {
            let result = parse_bounds_section(&content).expect("Failed to parse bounds");
            black_box(result.0.len())
        })
    }

    #[divan::bench]
    fn parse_binary_variables_component(bencher: Bencher) {
        let content = load_test_file("2obj_2cons_only_binary_vars.lp");

        bencher.bench(|| {
            let result = parse_binary_section(&content).expect("Failed to parse binary variables");
            black_box(result.0.len())
        })
    }
}

// ============================================================================
// MEMORY ALLOCATION BENCHMARKS
// ============================================================================

mod memory_benchmarks {
    use std::collections::HashMap;

    use divan::black_box;

    use super::*;

    #[divan::bench(args = ["test.lp", "boeing1.lp", "fit2d.lp"])]
    fn memory_usage_during_parsing(bencher: Bencher, filename: &str) {
        let content = load_test_file(filename);

        bencher.with_inputs(|| content.clone()).bench_values(|content| {
            let problem = LpProblem::parse(&content).expect("Failed to parse");
            black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
        })
    }

    #[divan::bench]
    fn string_allocation_patterns(bencher: Bencher) {
        let large_content = load_test_file("fit2d.lp");

        bencher.bench(|| {
            let lines: Vec<&str> = large_content.lines().collect();
            let processed: Vec<String> = lines.iter().filter(|line| !line.trim().is_empty()).map(|line| line.to_lowercase()).collect();
            black_box(processed.len())
        })
    }

    #[divan::bench]
    fn variable_name_storage_benchmark(bencher: Bencher) {
        bencher.bench(|| {
            let mut variables = HashMap::new();
            for i in 0..1000 {
                let name = format!("var_{i}");
                variables.insert(name.clone(), format!("value_{i}"));
            }

            black_box(variables.len())
        })
    }
}

// ============================================================================
// REGRESSION AND STRESS TESTS
// ============================================================================

mod regression_tests {
    use super::*;

    #[divan::bench]
    fn parsing_scalability_test(bencher: Bencher) {
        let base_content = load_test_file("test.lp");
        let large_content = base_content.repeat(10); // 10x larger

        bencher.with_inputs(|| large_content.clone()).bench_values(|content| {
            let problem = LpProblem::parse(&content).expect("Failed to parse scaled content");
            black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
        })
    }

    #[divan::bench(args = [10, 100, 1000, 10000])]
    fn variable_count_scaling(bencher: Bencher, var_count: usize) {
        let mut content = String::from("minimize\nobj: ");

        for i in 0..var_count {
            if i > 0 {
                content.push_str(" + ");
            }
            content.push_str(&format!("x{i}"));
        }

        content.push_str("\nsubject to\n");
        content.push_str(&format!("c1: x0 <= {var_count}\n"));
        content.push_str("bounds\n");

        for i in 0..var_count {
            content.push_str(&format!("x{i} >= 0\n"));
        }

        content.push_str("end\n");

        bencher.with_inputs(|| content.clone()).bench_values(|content| {
            let problem = LpProblem::parse(&content).expect("Failed to parse synthetic content");
            black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
        })
    }

    #[divan::bench(args = [10, 100, 1000])]
    fn constraint_count_scaling(bencher: Bencher, constraint_count: usize) {
        let mut content = String::from("minimize\nobj: x1 + x2\nsubject to\n");

        for i in 0..constraint_count {
            content.push_str(&format!("c{}: x1 + x2 <= {}\n", i, i + 1));
        }

        content.push_str("bounds\nx1 >= 0\nx2 >= 0\nend\n");

        bencher.with_inputs(|| content.clone()).bench_values(|content| {
            let problem = LpProblem::parse(&content).expect("Failed to parse synthetic content");
            black_box((problem.constraint_count(), problem.variable_count(), problem.objective_count()))
        })
    }
}
