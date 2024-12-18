use std::collections::{hash_map::Entry, HashMap};

use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    model::{constraint::Constraint, lp_error::LPParserError, objective::Objective, sense::Sense, variable::Variable},
    Rule,
};

pub trait LPPart
where
    Self: Sized,
{
    type Output;

    /// # Errors
    /// Returns an error if the rule cannot be converted to an `LPProblem`
    fn try_into(pair: Pair<'_, Rule>, id_gen: &mut SequenceGenerator) -> Result<Self::Output, LPParserError>;
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub struct LPProblem {
    pub constraints: HashMap<String, Constraint>,
    pub objectives: Vec<Objective>,
    pub problem_name: Option<String>,
    pub problem_sense: Sense,
    pub variables: HashMap<String, Variable>,
}

impl LPProblem {
    #[inline]
    pub fn add_constraint(&mut self, constraint: Constraint) {
        let name = if constraint.name().is_empty() { format!("UnnamedConstraint:{}", self.constraints.len()) } else { constraint.name() };
        constraint.coefficients().iter().for_each(|coefficient| {
            self.add_variable(&coefficient.var_name);
        });
        self.constraints.entry(name).or_insert(constraint);
    }

    #[inline]
    pub fn add_constraints(&mut self, constraints: Vec<Constraint>) {
        log::debug!("Adding {} constraints", constraints.len());
        for con in constraints {
            let name = if con.name().is_empty() { format!("UnnamedConstraint:{}", self.constraints.len()) } else { con.name() };
            con.coefficients().iter().for_each(|coefficient| {
                self.add_variable(&coefficient.var_name);
            });
            self.constraints.entry(name).or_insert(con);
        }
    }

    #[inline]
    pub fn add_objectives(&mut self, objectives: Vec<Objective>) {
        log::debug!("Adding {} objectives", objectives.len());
        for ob in &objectives {
            ob.coefficients.iter().for_each(|coefficient| {
                self.add_variable(&coefficient.var_name);
            });
        }
        self.objectives = objectives;
    }

    #[inline]
    pub fn add_variable(&mut self, name: &str) {
        if !name.is_empty() {
            self.variables.entry(name.to_owned()).or_default();
        }
    }

    #[inline]
    pub fn set_variable_bounds(&mut self, name: &str, kind: Variable) {
        if !name.is_empty() {
            match self.variables.entry(name.to_owned()) {
                Entry::Occupied(entry) if matches!(kind, Variable::SemiContinuous) => {
                    entry.into_mut().set_semi_continuous();
                }
                Entry::Occupied(entry) => *entry.into_mut() = kind,
                Entry::Vacant(entry) => {
                    entry.insert(kind);
                }
            }
        }
    }

    #[inline]
    #[must_use]
    pub fn with_problem_name(self, problem_name: &str) -> Self {
        log::debug!("Setting Problem Name: {problem_name}");
        Self { problem_name: Some(problem_name.to_owned()), ..self }
    }

    #[inline]
    #[must_use]
    pub fn with_sense(self, problem_sense: Sense) -> Self {
        log::debug!("Setting Problem Sense: {problem_sense:?}");
        Self { problem_sense, ..self }
    }
}
