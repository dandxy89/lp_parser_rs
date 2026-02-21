use std::error::Error;
use std::fmt::Write as _;
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

/// Write an f64 into a reusable string buffer and return it as bytes.
/// Clears the buffer before writing to avoid stale data.
#[inline]
fn write_f64_to_buf(buf: &mut String, value: f64) -> &[u8] {
    debug_assert!(value.is_finite(), "write_f64_to_buf called with non-finite value: {value}");
    buf.clear();
    write!(buf, "{value}").expect("writing f64 to String cannot fail");
    buf.as_bytes()
}

impl LpCsvWriter for LpProblem<'_> {
    fn write_constraints(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let headers = ["constraint_name", "constraint_type", "variable_name", "coefficient", "operator", "rhs", "sos_type"];
        let mut const_writer = csv::Writer::from_path(base_path.join("constraints.csv"))?;
        const_writer.write_record(headers)?;
        let mut buf_a = String::with_capacity(24);
        let mut buf_b = String::with_capacity(24);
        for (name, constraint) in &self.constraints {
            let name = name.as_bytes();
            match constraint {
                Constraint::Standard { coefficients, operator: op, rhs, .. } => {
                    for c in coefficients {
                        let coeff_bytes = write_f64_to_buf(&mut buf_a, c.value).to_owned();
                        let rhs_bytes = write_f64_to_buf(&mut buf_b, *rhs);
                        let vals: [&[u8]; 7] = [name, b"Standard", c.name.as_bytes(), &coeff_bytes, op.as_ref(), rhs_bytes, b""];
                        const_writer.write_record(vals)?;
                    }
                }
                Constraint::SOS { sos_type, weights, .. } => {
                    for c in weights {
                        let vals = [name, b"SOS", c.name.as_bytes(), write_f64_to_buf(&mut buf_a, c.value), b"", b"", sos_type.as_ref()];
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
        let mut buf = String::with_capacity(24);
        for (name, objective) in &self.objectives {
            for coef in &objective.coefficients {
                let vals = [name.as_bytes(), coef.name.as_bytes(), write_f64_to_buf(&mut buf, coef.value)];
                obj_writer.write_record(vals)?;
            }
        }
        obj_writer.flush()?;

        Ok(())
    }

    fn write_variables(&self, base_path: &Path) -> Result<(), Box<dyn Error>> {
        let mut var_writer = csv::Writer::from_path(base_path.join("variables.csv"))?;
        var_writer.write_record(["variable_name", "type", "lower_bound", "upper_bound"])?;
        let mut buf_a = String::with_capacity(24);
        let mut buf_b = String::with_capacity(24);
        for (name, var) in &self.variables {
            let name = name.as_bytes();
            match var.var_type {
                VariableType::LowerBound(lb) => {
                    let vals = [name, var.var_type.as_ref(), write_f64_to_buf(&mut buf_a, lb), b""];
                    var_writer.write_record(vals)?;
                }
                VariableType::UpperBound(ub) => {
                    let vals = [name, var.var_type.as_ref(), b"" as &[u8], write_f64_to_buf(&mut buf_a, ub)];
                    var_writer.write_record(vals)?;
                }
                VariableType::DoubleBound(lb, ub) => {
                    let lb_bytes = write_f64_to_buf(&mut buf_a, lb).to_owned();
                    let ub_bytes = write_f64_to_buf(&mut buf_b, ub);
                    let vals: [&[u8]; 4] = [name, var.var_type.as_ref(), &lb_bytes, ub_bytes];
                    var_writer.write_record(vals)?;
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
