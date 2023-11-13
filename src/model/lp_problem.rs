use std::collections::{hash_map::Entry, HashMap};

use crate::model::{constraint::Constraint, objective::Objective, sense::Sense, variable::VariableType};

#[derive(Debug, Default, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LPProblem {
    pub problem_name: String,
    pub problem_sense: Sense,
    pub variables: HashMap<String, VariableType>,
    pub objectives: Vec<Objective>,
    pub constraints: HashMap<String, Constraint>,
}

impl LPProblem {
    #[must_use]
    pub fn with_problem_name(self, problem_name: &str) -> Self {
        Self { problem_name: problem_name.to_string(), ..self }
    }

    #[must_use]
    pub fn with_sense(self, problem_sense: Sense) -> Self {
        Self { problem_sense, ..self }
    }

    pub fn add_variable(&mut self, name: &str) {
        if !name.is_empty() {
            self.variables.entry(name.to_string()).or_default();
        }
    }

    pub fn set_var_bounds(&mut self, name: &str, kind: VariableType) {
        if !name.is_empty() {
            match self.variables.entry(name.to_string()) {
                Entry::Occupied(k) if matches!(kind, VariableType::SemiContinuous) => {
                    k.into_mut().set_semi_continuous();
                }
                Entry::Occupied(k) => *k.into_mut() = kind,
                Entry::Vacant(k) => {
                    k.insert(kind);
                }
            }
        }
    }

    pub fn add_objective(&mut self, objectives: Vec<Objective>) {
        for ob in &objectives {
            ob.coefficients.iter().for_each(|c| {
                self.add_variable(&c.var_name);
            });
        }
        self.objectives = objectives;
    }

    pub fn add_constraints(&mut self, constraints: Vec<Constraint>) {
        for con in constraints {
            let name = if con.name().is_empty() { format!("UnnamedConstraint:{}", self.constraints.len()) } else { con.name() };
            con.coefficients().iter().for_each(|c| {
                self.add_variable(&c.var_name);
            });
            self.constraints.entry(name).or_insert(con);
        }
    }
}
