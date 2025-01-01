//! This module provides a collection of specialized parsers for different elements
//! of LP files. Each sub module handles a specific aspect of the LP format,
//! working together to provide comprehensive parsing capabilities.

pub mod coefficient;
pub mod constraint;
pub mod number;
pub mod objective;
pub mod parser_traits;
pub mod problem_name;
pub mod sense;
pub mod sos_constraint;
pub mod variable;
