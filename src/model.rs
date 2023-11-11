use std::collections::{hash_map::Entry, HashMap};

use pest::iterators::Pairs;

use crate::{
    common::{AsFloat, RuleExt},
    Rule,
};

#[derive(Debug, Default)]
pub enum VariableType {
    #[default]
    Free, // [-Infinity, +Infinity]
    LB(f64),
    UB(f64),
    Bounded(f64, f64),
    Integer, // [0, 1]
    Binary,
    General, // [0, +Infinity]
}

#[derive(Debug)]
pub struct Objective {
    pub name: String,
    pub coefficients: Vec<Coefficient>,
}

#[derive(Debug)]
pub struct Coefficient {
    pub var_name: String,
    pub coefficient: f64,
}

impl TryFrom<Pairs<'_, Rule>> for Coefficient {
    type Error = anyhow::Error;

    #[allow(clippy::unreachable, clippy::wildcard_enum_match_arm)]
    fn try_from(values: Pairs<'_, Rule>) -> anyhow::Result<Self> {
        let (mut value, mut var_name) = (1.0, String::new());
        for item in values {
            match item.as_rule() {
                r if r.is_numeric() => {
                    value *= item.as_float()?;
                }
                Rule::VARIABLE => {
                    var_name = item.as_str().to_string();
                }
                _ => unreachable!("Unexpected rule encountered"),
            }
        }
        Ok(Self { var_name, coefficient: value })
    }
}

#[derive(Debug)]
pub struct Constraint {
    pub name: String,
    pub coefficients: Vec<Coefficient>,
    pub sense: String,
    pub rhs: f64,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub enum Sense {
    #[default]
    Minimize,
    Maximize,
}

#[derive(Debug, Default)]
pub struct LPDefinition {
    pub problem_sense: Sense,
    pub variables: HashMap<String, VariableType>,
    pub objectives: Vec<Objective>,
    pub constraints: Vec<Constraint>,
}

impl LPDefinition {
    #[must_use]
    pub fn with_sense(&mut self, problem_sense: Sense) -> Self {
        Self { problem_sense, ..Default::default() }
    }

    pub fn add_variable(&mut self, name: &str) {
        self.variables.entry(name.to_string()).or_default();
    }

    pub fn set_var_bounds(&mut self, name: &str, kind: VariableType) {
        match self.variables.entry(name.to_string()) {
            Entry::Occupied(k) => *k.into_mut() = kind,
            Entry::Vacant(k) => {
                k.insert(kind);
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
        for ob in &constraints {
            ob.coefficients.iter().for_each(|c| {
                self.add_variable(&c.var_name);
            });
        }
        self.constraints = constraints;
    }
}
