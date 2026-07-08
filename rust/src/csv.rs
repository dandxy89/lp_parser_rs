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
