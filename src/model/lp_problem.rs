use std::collections::{hash_map::Entry, HashMap};

use pest::iterators::Pair;
use unique_id::sequence::SequenceGenerator;

use crate::{
    model::{constraint::Constraint, objective::Objective, sense::Sense, variable::Variable},
    Rule,
};

pub trait LPPart
where
    Self: Sized,
{
    type Output;

    fn try_into(pair: Pair<'_, Rule>, gen: &mut SequenceGenerator) -> anyhow::Result<Self::Output>;
}

#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff), diff(attr(#[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)])))]
pub struct LPProblem {
    pub problem_name: String,
    pub problem_sense: Sense,
    pub variables: HashMap<String, Variable>,
    pub objectives: Vec<Objective>,
    pub constraints: HashMap<String, Constraint>,
}

impl LPProblem {
    #[inline]
    #[must_use]
    pub fn with_problem_name(self, problem_name: &str) -> Self {
        Self { problem_name: problem_name.to_owned(), ..self }
    }

    #[inline]
    #[must_use]
    pub fn with_sense(self, problem_sense: Sense) -> Self {
        Self { problem_sense, ..self }
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
                Entry::Occupied(k) if matches!(kind, Variable::SemiContinuous) => {
                    k.into_mut().set_semi_continuous();
                }
                Entry::Occupied(k) => *k.into_mut() = kind,
                Entry::Vacant(k) => {
                    k.insert(kind);
                }
            }
        }
    }

    #[inline]
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
