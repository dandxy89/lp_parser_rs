use crate::Rule;

pub trait Prefix {
    fn prefix(&self) -> &'static str;
}

impl Prefix for Rule {
    fn prefix(&self) -> &'static str {
        match self {
            Self::OBJECTIVE_NAME => "obj_",
            Self::CONSTRAINT_NAME => "con_",
            _ => "",
        }
    }
}
