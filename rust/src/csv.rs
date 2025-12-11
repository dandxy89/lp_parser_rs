use std::error::Error;
use std::path::Path;

use crate::model::{Constraint, VariableType};
use crate::problem::LpProblem;

/// Trait for writing LP problem data to CSV files
pub trait LpCsvWriter {
    /// Write constraints to a CSV file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or written to
    fn write_constraints(&self, base_path: &Path) -> Result<(), Box<dyn Error>>;

    /// Write objectives to a CSV file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or written to
    fn write_objectives(&self, base_path: &Path) -> Result<(), Box<dyn Error>>;

    /// Write variables to a CSV file
    ///
    /// # Errors
    ///
    /// Returns an error if the file cannot be created or written to
    fn write_variables(&self, base_path: &Path) -> Result<(), Box<dyn Error>>;

    /// Writes the problem data to CSV files with normalised structure.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the CSV files cannot be created or written to
    fn to_csv(&self, base_path: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
        self.write_objectives(base_path)?;
        self.write_constraints(base_path)?;
        self.write_variables(base_path)?;

        Ok(())
    }
}

#[inline]
fn f64_to_bytes(value: f64) -> Vec<u8> {
    format!("{value}").into_bytes()
}

impl LpCsvWriter for LpProblem<'_> {
    fn write_constraints(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let headers = ["constraint_name", "constraint_type", "variable_name", "coefficient", "operator", "rhs", "sos_type"];
        let mut const_writer = csv::Writer::from_path(base_path.join("constraints.csv"))?;
        const_writer.write_record(headers)?;
        for (name, constraint) in &self.constraints {
            let name = name.as_bytes();
            match constraint {
                Constraint::Standard { coefficients, operator: op, rhs, .. } => {
                    for c in coefficients {
                        let vals = [name, b"Standard", c.name.as_bytes(), &f64_to_bytes(c.value), op.as_ref(), &f64_to_bytes(*rhs), b""];
                        const_writer.write_record(vals)?;
                    }
                }
                Constraint::SOS { sos_type, weights, .. } => {
                    for c in weights {
                        let vals = [name, b"SOS", c.name.as_bytes(), &f64_to_bytes(c.value), b"", b"", sos_type.as_ref()];
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
        for (name, objective) in &self.objectives {
            for coef in &objective.coefficients {
                let vals = [name.as_bytes(), coef.name.as_bytes(), &f64_to_bytes(coef.value)];
                obj_writer.write_record(vals)?;
            }
        }
        obj_writer.flush()?;

        Ok(())
    }

    fn write_variables(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let mut var_writer = csv::Writer::from_path(base_path.join("variables.csv"))?;
        var_writer.write_record(["variable_name", "type", "lower_bound", "upper_bound"])?;
        for (name, var) in &self.variables {
            let name = name.as_bytes();
            match var.var_type {
                VariableType::LowerBound(lb) => {
                    var_writer.write_record([name, var.var_type.as_ref(), &f64_to_bytes(lb), b""])?;
                }
                VariableType::UpperBound(ub) => {
                    var_writer.write_record([name, var.var_type.as_ref(), b"", &f64_to_bytes(ub)])?;
                }
                VariableType::DoubleBound(lb, ub) => {
                    var_writer.write_record([name, var.var_type.as_ref(), &f64_to_bytes(lb), &f64_to_bytes(ub)])?;
                }
                _ => {
                    var_writer.write_record([name, var.var_type.as_ref(), b"", b""])?;
                }
            }
        }
        var_writer.flush()?;

        Ok(())
    }
}
