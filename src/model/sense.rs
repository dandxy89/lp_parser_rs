#[derive(Debug, Default, PartialEq, Eq, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "diff", derive(diff::Diff))]
#[cfg_attr(feature = "diff", diff(attr(
    #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
)))]
pub enum Sense {
    #[default]
    Minimize,
    Maximize,
}
