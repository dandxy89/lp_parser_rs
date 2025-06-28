use divan::Bencher;
use lp_parser_rs::parsers::coefficient::parse_coefficient;
use lp_parser_rs::parsers::parser_traits::parse_variable;
use lp_parser_rs::parsers::sense::parse_sense;

fn main() {
    divan::main();
}

// ============================================================================
// NUMBER PARSING BENCHMARKS
// ============================================================================

#[divan::bench(args = [
    "123 x1",
    "-456 x2",
    "123.456 var",
    "-789.123 y",
    "1.23e10 z",
    "-4.56e-7 w",
    "0 x",
    "0.0 y"
])]
fn parse_number_through_coefficient_micro(bencher: Bencher, input: &str) {
    bencher.bench(|| parse_coefficient(input).expect("Failed to parse coefficient"))
}

// ============================================================================
// VARIABLE NAME PARSING BENCHMARKS
// ============================================================================

#[divan::bench(args = [
    "x1",
    "variable_name_123",
    "X_VAR_WITH_UNDERSCORES",
    "a",
    "x123y456z789"
])]
fn parse_variable_name_micro(bencher: Bencher, input: &str) {
    bencher.bench(|| parse_variable(input).expect("Failed to parse variable"))
}

// ============================================================================
// COEFFICIENT PARSING BENCHMARKS
// ============================================================================

#[divan::bench(args = [
    "2.5 x1",
    "-3 variable_name",
    "x1", // coefficient of 1
    "-x2", // coefficient of -1
    "123.456 long_variable_name_here"
])]
fn parse_coefficient_micro(bencher: Bencher, input: &str) {
    bencher.bench(|| parse_coefficient(input).expect("Failed to parse coefficient"))
}

// ============================================================================
// OPTIMISATION SENSE PARSING BENCHMARKS
// ============================================================================

#[divan::bench(args = [
    "minimize",
    "minimise",
    "maximize",
    "maximise",
    "min",
    "max"
])]
fn parse_sense_micro(bencher: Bencher, input: &str) {
    bencher.bench(|| parse_sense(input).expect("Failed to parse sense"))
}

// ============================================================================
// WHITESPACE HANDLING BENCHMARKS
// ============================================================================

mod whitespace_benchmarks {
    use nom::IResult;
    use nom::character::complete::multispace0;

    use super::*;

    #[divan::bench(args = [
        "",
        " ",
        "   ",
        "\t",
        "\n",
        " \t\n ",
        "      \t\t\n\n    "
    ])]
    fn parse_whitespace_micro(bencher: Bencher, input: &str) {
        bencher.bench(|| {
            let _: IResult<&str, &str> = multispace0(input);
        })
    }
}

// ============================================================================
// STRING COMPARISON BENCHMARKS
// ============================================================================

mod string_comparison_benchmarks {
    use nom::IResult;
    use nom::bytes::complete::tag_no_case;

    use super::*;

    const KEYWORDS: &[&str] = &["minimize", "maximise", "subject", "to", "bounds", "binary", "integer", "general", "semi", "end"];

    #[divan::bench(args = KEYWORDS)]
    fn case_insensitive_tag_micro(bencher: Bencher, keyword: &str) {
        let input = keyword.to_uppercase(); // Test with different case

        bencher.bench(|| {
            let _: IResult<&str, &str> = tag_no_case(keyword)(&input);
        })
    }

    #[divan::bench(args = KEYWORDS)]
    fn standard_string_comparison_micro(bencher: Bencher, keyword: &str) {
        let input = keyword.to_uppercase();

        bencher.bench(|| input.to_lowercase() == keyword.to_lowercase())
    }
}

// ============================================================================
// REALISTIC PARSING SCENARIOS
// ============================================================================

mod realistic_scenarios {
    use super::*;

    // Test realistic constraint parsing scenarios
    #[divan::bench(args = [
        "x1 + 2*x2 + 3.5*x3 <= 10",
        "2*variable_a - 4.5*variable_b + variable_c >= 20.5",
        "x1 = 5",
        "-x1 - x2 - x3 <= -10",
        "0.001*x1 + 1000*x2 <= 1e6"
    ])]
    fn realistic_constraint_expressions(bencher: Bencher, expr: &str) {
        bencher.bench(|| {
            let parts: Vec<&str> = expr.split_whitespace().collect();
            parts.len()
        })
    }

    // Test realistic objective function scenarios
    #[divan::bench(args = [
        "obj: x1 + 2*x2 + 3*x3",
        "profit: 10*product_a + 15*product_b + 8*product_c",
        "cost: -x1 - 2*x2",
        "objective_function: 0.5*var1 + 0.3*var2 + 0.2*var3"
    ])]
    fn realistic_objective_expressions(bencher: Bencher, expr: &str) {
        bencher.bench(|| {
            let parts: Vec<&str> = expr.split('+').collect();
            parts.len()
        })
    }
}

// ============================================================================
// ERROR HANDLING BENCHMARKS
// ============================================================================

mod error_handling_benchmarks {
    use super::*;

    #[divan::bench(args = [
        "invalid_number_abc",
        "123.456.789", // Invalid number format
        "", // Empty input
        "   ", // Only whitespace
        "123xyz" // Number followed by invalid chars
    ])]
    fn error_handling_micro(bencher: Bencher, input: &str) {
        bencher.bench(|| {
            let result = parse_coefficient(input);
            result.is_err()
        })
    }
}

// ============================================================================
// MEMORY ALLOCATION MICRO-BENCHMARKS
// ============================================================================

mod allocation_micro_benchmarks {
    use divan::black_box;

    use super::*;

    #[divan::bench(args = [10, 100, 1000])]
    fn string_vec_allocation_micro(bencher: Bencher, size: usize) {
        bencher.bench(|| {
            let mut vec = Vec::with_capacity(size);
            for i in 0..size {
                vec.push(format!("var_{i}"));
            }
            black_box(vec.len())
        })
    }

    #[divan::bench(args = [10, 100, 1000])]
    fn hashmap_allocation_micro(bencher: Bencher, size: usize) {
        bencher.bench(|| {
            use std::collections::HashMap;
            let mut map = HashMap::with_capacity(size);
            for i in 0..size {
                map.insert(format!("key_{i}"), i);
            }
            black_box(map.len())
        })
    }
}
