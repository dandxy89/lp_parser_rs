//! CSV export: write problem components to per-section `.csv` files.

use std::error::Error;
use std::path::Path;

use crate::model::{Constraint, VariableType};
use crate::problem::LpProblem;

impl LpProblem {
    /// Writes the problem data to three CSV files under `base_path`:
    /// `objectives.csv`, `constraints.csv`, and `variables.csv`.
    ///
    /// # Example
    ///
    /// ```rust,no_run
    /// use std::path::Path;
    ///
    /// use lp_parser_rs::LpProblem;
    ///
    /// let problem = LpProblem::parse("Minimize\n obj: x\nSubject To\n c1: x >= 1\nEnd")?;
    /// problem.to_csv(Path::new("/tmp/lp_export"))?;
    /// // Creates /tmp/lp_export/{objectives,constraints,variables}.csv
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    ///
    /// # Errors
    ///
    /// Returns an error if any of the CSV files cannot be created or written to
    pub fn to_csv(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        self.write_objectives(base_path)?;
        self.write_constraints(base_path)?;
        self.write_variables(base_path)?;

        Ok(())
    }

    fn write_constraints(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let headers = ["constraint_name", "constraint_type", "variable_name", "coefficient", "operator", "rhs", "sos_type"];
        let mut const_writer = csv::Writer::from_path(base_path.join("constraints.csv"))?;
        const_writer.write_record(headers)?;
        for (name_id, constraint) in &self.constraints {
            let name = self.interner.resolve(*name_id);
            let name_bytes = name.as_bytes();
            match constraint {
                Constraint::Standard { coefficients, operator: op, rhs, .. } => {
                    let rhs_str = rhs.to_string();
                    for c in coefficients {
                        let var_name = self.interner.resolve(c.name);
                        let coeff = c.value.to_string();
                        let vals: [&[u8]; 7] =
                            [name_bytes, b"Standard", var_name.as_bytes(), coeff.as_bytes(), op.as_ref(), rhs_str.as_bytes(), b""];
                        const_writer.write_record(vals)?;
                    }
                }
                Constraint::SOS { sos_type, weights, .. } => {
                    for c in weights {
                        let var_name = self.interner.resolve(c.name);
                        let weight = c.value.to_string();
                        let vals: [&[u8]; 7] = [name_bytes, b"SOS", var_name.as_bytes(), weight.as_bytes(), b"", b"", sos_type.as_ref()];
                        const_writer.write_record(vals)?;
                    }
                }
            }
        }
        const_writer.flush()?;

        Ok(())
    }

    fn write_objectives(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let mut obj_writer = csv::Writer::from_path(base_path.join("objectives.csv"))?;
        obj_writer.write_record(["objective_name", "variable_name", "coefficient"])?;
        for (name_id, objective) in &self.objectives {
            let name = self.interner.resolve(*name_id);
            for coef in &objective.coefficients {
                let var_name = self.interner.resolve(coef.name);
                let coeff = coef.value.to_string();
                obj_writer.write_record([name.as_bytes(), var_name.as_bytes(), coeff.as_bytes()])?;
            }
        }
        obj_writer.flush()?;

        Ok(())
    }

    fn write_variables(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let mut var_writer = csv::Writer::from_path(base_path.join("variables.csv"))?;
        var_writer.write_record(["variable_name", "type", "lower_bound", "upper_bound"])?;
        for (name_id, var) in &self.variables {
            let name = self.interner.resolve(*name_id);
            let name_bytes = name.as_bytes();
            let (lb, ub) = match var.var_type {
                VariableType::LowerBound(lb) => (lb.to_string(), String::new()),
                VariableType::UpperBound(ub) => (String::new(), ub.to_string()),
                VariableType::DoubleBound(lb, ub) => (lb.to_string(), ub.to_string()),
                _ => (String::new(), String::new()),
            };
            let vals: [&[u8]; 4] = [name_bytes, var.var_type.as_ref(), lb.as_bytes(), ub.as_bytes()];
            var_writer.write_record(vals)?;
        }
        var_writer.flush()?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;

    use crate::model::{Coefficient, Objective};
    use crate::problem::LpProblem;

    /// Create a unique, empty temporary directory for one test.
    ///
    /// `tempfile` is not a dev-dependency, so build a per-test directory under
    /// the system temporary directory instead. Each caller passes a distinct
    /// tag so parallel tests never collide.
    fn unique_temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("lp_parser_rs_csv_test_{}_{tag}", std::process::id()));
        // A leftover directory from a previous run must not pollute assertions.
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("failed to clear stale test directory");
        }
        fs::create_dir_all(&dir).expect("failed to create test directory");
        dir
    }

    fn read_lines(path: &PathBuf) -> Vec<String> {
        fs::read_to_string(path).unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display())).lines().map(str::to_string).collect()
    }

    #[test]
    fn test_to_csv_full_output() {
        let input = "\
Minimize
 obj: x + 2 y
Subject To
 c1: x + y <= 10
Bounds
 x >= 1
 y <= 5
 2 <= z <= 8
 w free
SOS
 sos_a: S1:: x:1 y:2.5
End
";
        let problem = LpProblem::parse(input).expect("test LP must parse");
        let dir = unique_temp_dir("full_output");
        problem.to_csv(&dir).expect("to_csv must succeed");

        // Objectives: header plus one row per coefficient, in insertion order.
        let obj_lines = read_lines(&dir.join("objectives.csv"));
        assert_eq!(obj_lines[0], "objective_name,variable_name,coefficient");
        assert_eq!(obj_lines[1], "obj,x,1");
        assert_eq!(obj_lines[2], "obj,y,2");
        assert_eq!(obj_lines.len(), 3);

        // Constraints: standard rows carry operator and rhs with an empty
        // sos_type column; SOS rows carry the sos_type with empty operator/rhs.
        let con_lines = read_lines(&dir.join("constraints.csv"));
        assert_eq!(con_lines[0], "constraint_name,constraint_type,variable_name,coefficient,operator,rhs,sos_type");
        assert!(con_lines.contains(&"c1,Standard,x,1,<=,10,".to_string()), "got: {con_lines:?}");
        assert!(con_lines.contains(&"c1,Standard,y,1,<=,10,".to_string()), "got: {con_lines:?}");
        assert!(con_lines.contains(&"sos_a,SOS,x,1,,,S1".to_string()), "got: {con_lines:?}");
        assert!(con_lines.contains(&"sos_a,SOS,y,2.5,,,S1".to_string()), "got: {con_lines:?}");
        assert_eq!(con_lines.len(), 5);

        // Variables: the bound-column mapping per variable type.
        let var_lines = read_lines(&dir.join("variables.csv"));
        assert_eq!(var_lines[0], "variable_name,type,lower_bound,upper_bound");
        assert!(var_lines.contains(&"x,LowerBound,1,".to_string()), "got: {var_lines:?}");
        assert!(var_lines.contains(&"y,UpperBound,,5".to_string()), "got: {var_lines:?}");
        assert!(var_lines.contains(&"z,DoubleBound,2,8".to_string()), "got: {var_lines:?}");
        assert!(var_lines.contains(&"w,Free,,".to_string()), "got: {var_lines:?}");
        assert_eq!(var_lines.len(), 5);

        fs::remove_dir_all(&dir).expect("cleanup failed");
    }

    #[test]
    fn test_to_csv_quotes_names_with_comma_and_double_quote() {
        let mut problem = LpProblem::new();
        let obj_id = problem.intern("obj");
        // A name containing both a comma and a double quote must be quoted.
        let awkward_name = "x,\"1\"";
        let var_id = problem.intern(awkward_name);
        problem.add_objective(Objective { name: obj_id, coefficients: vec![Coefficient { name: var_id, value: 1.0 }], byte_offset: None });

        let dir = unique_temp_dir("quoting");
        problem.to_csv(&dir).expect("to_csv must succeed");

        let path = dir.join("objectives.csv");
        let raw = fs::read_to_string(&path).expect("failed to read objectives.csv");
        // RFC 4180: the field is wrapped in quotes and the inner quote doubled.
        assert!(raw.contains("\"x,\"\"1\"\"\""), "raw CSV must quote the awkward name, got:\n{raw}");

        // And it must parse back to the original name.
        let mut reader = csv::Reader::from_path(&path).expect("csv must be readable");
        let record = reader.records().next().expect("one data row expected").expect("row must parse");
        assert_eq!(&record[0], "obj");
        assert_eq!(&record[1], awkward_name);
        assert_eq!(&record[2], "1");

        fs::remove_dir_all(&dir).expect("cleanup failed");
    }

    #[test]
    fn test_to_csv_empty_problem_writes_header_only() {
        let problem = LpProblem::new();
        let dir = unique_temp_dir("empty_problem");
        problem.to_csv(&dir).expect("to_csv must succeed");

        assert_eq!(read_lines(&dir.join("objectives.csv")), vec!["objective_name,variable_name,coefficient".to_string()]);
        assert_eq!(
            read_lines(&dir.join("constraints.csv")),
            vec!["constraint_name,constraint_type,variable_name,coefficient,operator,rhs,sos_type".to_string()]
        );
        assert_eq!(read_lines(&dir.join("variables.csv")), vec!["variable_name,type,lower_bound,upper_bound".to_string()]);

        fs::remove_dir_all(&dir).expect("cleanup failed");
    }

    #[test]
    fn test_to_csv_uncreatable_path_returns_err() {
        let dir = unique_temp_dir("error_path");
        // A regular file cannot have children, so any path beneath it is
        // uncreatable.
        let blocker = dir.join("blocker");
        fs::write(&blocker, b"not a directory").expect("failed to create blocker file");

        let problem = LpProblem::new();
        let result = problem.to_csv(&blocker.join("x").join("y"));
        assert!(result.is_err(), "writing beneath a regular file must fail");

        fs::remove_dir_all(&dir).expect("cleanup failed");
    }
}
