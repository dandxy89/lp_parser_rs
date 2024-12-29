use crate::nom::model::{Constraint, Objective, Sense, Variable};

#[derive(Debug, PartialEq)]
pub struct LPProblem<'a> {
    pub name: Option<&'a str>,
    pub sense: Sense,
    pub objectives: Vec<Objective<'a>>,
    pub constraints: Vec<Constraint<'a>>,
    pub variables: Vec<Variable<'a>>,
}

impl<'a> LPProblem<'a> {
    #[inline]
    pub fn name(&self) -> Option<&str> {
        self.name
    }

    #[inline]
    pub fn is_minimization(&self) -> bool {
        self.sense.is_minimization()
    }

    #[inline]
    pub fn constraint_count(&self) -> usize {
        self.constraints.len()
    }

    #[inline]
    pub fn objective_count(&self) -> usize {
        self.objectives.len()
    }
}
