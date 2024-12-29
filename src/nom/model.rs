#[derive(Debug, PartialEq, Eq)]
pub enum ComparisonOp {
    LessThan,
    LessOrEqual,
    Equal,
    GreaterThan,
    GreaterOrEqual,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Sense {
    Minimize,
    Maximize,
}

impl Sense {
    pub fn is_minimization(&self) -> bool {
        self == &Sense::Minimize
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum SOSType {
    S1,
    S2,
}

#[derive(Debug, PartialEq)]
pub enum Bound<'a> {
    Free(&'a str),
    LowerBound(&'a str, f64),
    UpperBound(&'a str, f64),
    DoubleBound(&'a str, f64, f64),
}

#[derive(Debug, PartialEq)]
pub struct Coefficient<'a> {
    pub var_name: &'a str,
    pub coefficient: f64,
}

#[derive(Debug, PartialEq)]
pub enum Constraint<'a> {
    Standard { name: Option<String>, coefficients: Vec<Coefficient<'a>>, operator: ComparisonOp, rhs: f64 },
    SOS { name: Option<String>, sos_type: SOSType, weights: Vec<Coefficient<'a>> },
}

#[derive(Debug, PartialEq)]
pub struct Objective<'a> {
    pub name: Option<String>,
    pub coefficients: Vec<Coefficient<'a>>,
}

#[derive(Debug, PartialEq)]
pub enum VariableType {
    Free,
    Binary,
    Integer,
    General { lower_bound: Option<f64>, upper_bound: Option<f64> },
    SemiContinuous { lower_bound: Option<f64>, upper_bound: Option<f64> },
}

#[derive(Debug, PartialEq)]
pub struct Variable<'a> {
    pub name: &'a str,
    pub var_type: VariableType,
}
