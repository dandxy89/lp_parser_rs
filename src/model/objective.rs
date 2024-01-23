use crate::model::coefficient::Coefficient;

#[derive(Debug, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff))]
#[cfg_attr(feature = "diff", diff(attr(
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
)))]
pub struct Objective {
    pub name: String,
    pub coefficients: Vec<Coefficient>,
}
