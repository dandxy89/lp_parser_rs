//! Per-document symbol index, rebuilt from the syntax tree after each reparse
//! in one linear walk. Drives navigation, rename, hover, completion, code lens
//! and workspace symbols.

use std::collections::HashMap;
use std::ops::Range;

use tree_sitter::{Node, Tree};

use crate::syntax::{self, kind};

/// How a variable occurrence is used.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// Linear term in an objective.
    ObjectiveTerm,
    /// Linear term in a constraint (any constraint section).
    ConstraintTerm,
    /// Inside a `[ ... ]` quadratic block (objective or constraint).
    QuadraticTerm,
    /// `Bounds` entry.
    Bound,
    /// `Generals` entry.
    Generals,
    /// `Integers` entry.
    Integers,
    /// `Binaries` entry.
    Binaries,
    /// `Semi-Continuous` entry.
    SemiContinuous,
    /// SOS set member.
    SosEntry,
    /// Indicator variable (`b = 1 -> ...`).
    Indicator,
    /// General-constraint resultant (`r = MAX(...)`).
    Resultant,
    /// General-constraint function argument.
    GeneralArgument,
}

impl Role {
    /// Type-section entry (`generals`/`integers`/`binaries`/`semi`).
    #[must_use]
    pub const fn is_type_declaration(self) -> bool {
        matches!(self, Self::Generals | Self::Integers | Self::Binaries | Self::SemiContinuous)
    }

    /// Bound or type-section entry: declares something about the variable
    /// without using it in the model.
    #[must_use]
    pub const fn is_declaration(self) -> bool {
        matches!(self, Self::Bound) || self.is_type_declaration()
    }

    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::ObjectiveTerm => "objective term",
            Self::ConstraintTerm => "constraint term",
            Self::QuadraticTerm => "quadratic term",
            Self::Bound => "bound",
            Self::Generals => "generals entry",
            Self::Integers => "integers entry",
            Self::Binaries => "binaries entry",
            Self::SemiContinuous => "semi-continuous entry",
            Self::SosEntry => "SOS entry",
            Self::Indicator => "indicator variable",
            Self::Resultant => "general-constraint resultant",
            Self::GeneralArgument => "general-constraint argument",
        }
    }
}

/// One occurrence of a variable name.
#[derive(Debug, Clone, PartialEq)]
pub struct Occurrence {
    /// Byte range of the name.
    pub range: Range<usize>,
    /// How the variable is used here.
    pub role: Role,
    /// Owning entity (objective, constraint or SOS set), if any.
    pub entity: Option<usize>,
    /// Signed coefficient for linear, quadratic and SOS-weight occurrences.
    pub coefficient: Option<f64>,
}

/// A variable and every place it occurs, in document order.
#[derive(Debug, Clone, PartialEq)]
pub struct Variable {
    /// The variable name.
    pub name: String,
    /// All occurrences in document order (never empty).
    pub occurrences: Vec<Occurrence>,
}

impl Variable {
    /// Defining site: the first `Bounds` entry, else the first occurrence.
    #[must_use]
    pub fn definition(&self) -> &Occurrence {
        debug_assert!(!self.occurrences.is_empty(), "indexed variables have at least one occurrence");
        self.occurrences.iter().find(|o| o.role == Role::Bound).unwrap_or(&self.occurrences[0])
    }

    /// First type-section entry, if any.
    #[must_use]
    pub fn declaration(&self) -> Option<&Occurrence> {
        self.occurrences.iter().find(|o| o.role.is_type_declaration())
    }

    /// Whether the variable is used by the model (not only declared).
    #[must_use]
    pub fn is_used(&self) -> bool {
        self.occurrences.iter().any(|o| !o.role.is_declaration())
    }

    /// Distinct entities (constraints/objectives/SOS) that use this variable.
    #[must_use]
    pub fn entities(&self) -> Vec<usize> {
        let mut out: Vec<usize> = self.occurrences.iter().filter_map(|o| o.entity).collect();
        out.dedup();
        out
    }
}

/// Section an entity lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Section {
    /// Objectives.
    Objectives,
    /// `Subject To`.
    SubjectTo,
    /// `Lazy Constraints`.
    Lazy,
    /// `User Cuts`.
    UserCuts,
    /// `General Constraints`.
    General,
    /// `SOS`.
    Sos,
}

impl Section {
    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Objectives => "objectives",
            Self::SubjectTo => "subject to",
            Self::Lazy => "lazy constraints",
            Self::UserCuts => "user cuts",
            Self::General => "general constraints",
            Self::Sos => "SOS",
        }
    }
}

/// Kind of named entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntityKind {
    /// An objective (`named_objective`, or the unnamed objective expression).
    Objective,
    /// A linear, quadratic, ranged or indicator constraint (`constraint`).
    Constraint,
    /// A general constraint (`general_constraint`).
    GeneralConstraint,
    /// An SOS set (`sos_constraint_header` plus its entries).
    Sos,
}

impl EntityKind {
    /// Objectives and constraints have separate name namespaces upstream;
    /// constraints, general constraints and SOS sets share one.
    #[must_use]
    pub const fn namespace(self) -> Namespace {
        match self {
            Self::Objective => Namespace::Objective,
            Self::Constraint | Self::GeneralConstraint | Self::Sos => Namespace::Constraint,
        }
    }

    /// Human-readable label.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Objective => "objective",
            Self::Constraint => "constraint",
            Self::GeneralConstraint => "general constraint",
            Self::Sos => "SOS set",
        }
    }
}

/// Name namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Namespace {
    /// Objective names.
    Objective,
    /// Constraint, general-constraint and SOS names.
    Constraint,
}

/// An objective, constraint or SOS set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// What it is.
    pub kind: EntityKind,
    /// Section it appears in.
    pub section: Section,
    /// Explicit name, if labelled.
    pub name: Option<String>,
    /// Byte range of the label.
    pub name_range: Option<Range<usize>>,
    /// Byte range of the whole entity (for SOS: header through last entry).
    pub range: Range<usize>,
    /// Tree-sitter kind of the node at `range` (for SOS, the header).
    pub node_kind: &'static str,
}

/// A multi-objective attribute (`Priority=2`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribute {
    /// Owning objective entity.
    pub objective: usize,
    /// Attribute name as written.
    pub name: String,
    /// Byte range of the attribute name.
    pub name_range: Range<usize>,
    /// Byte range of the whole `name=value`.
    pub range: Range<usize>,
}

/// A top-level section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionSpan {
    /// Tree-sitter node kind (one of [`syntax::SECTION_KINDS`]).
    pub kind: &'static str,
    /// Byte range of the section node.
    pub range: Range<usize>,
    /// Byte range of the header keyword (none for objectives).
    pub header: Option<Range<usize>>,
}

/// A pair of entities sharing a name within one namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Duplicate {
    /// The first entity with the name.
    pub first: usize,
    /// A later entity reusing it.
    pub duplicate: usize,
}

/// What a name site refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Symbol {
    /// A variable occurrence: `(variable index, occurrence index)`.
    Variable(usize, usize),
    /// An entity label.
    Entity(usize),
    /// An objective attribute name.
    Attribute(usize),
}

/// Index of every symbol in one document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SymbolIndex {
    /// Variables in order of first occurrence.
    pub variables: Vec<Variable>,
    /// Objectives, constraints and SOS sets in document order.
    pub entities: Vec<Entity>,
    /// Objective attributes in document order.
    pub attributes: Vec<Attribute>,
    /// Top-level sections in document order.
    pub sections: Vec<SectionSpan>,
    /// Name collisions within a namespace.
    pub duplicates: Vec<Duplicate>,
    variable_ids: HashMap<String, usize>,
    /// Every name site, sorted by start offset.
    sites: Vec<(Range<usize>, Symbol)>,
}

impl SymbolIndex {
    /// Build the index from a parsed tree. `ERROR` subtrees are skipped.
    #[must_use]
    pub fn build(tree: &Tree, text: &str) -> Self {
        let mut builder = Builder { text, index: Self::default() };
        let root = tree.root_node();
        let mut cursor = root.walk();
        for child in root.named_children(&mut cursor) {
            builder.section(child);
        }
        builder.index.find_duplicates();
        debug_assert!(builder.index.sites.windows(2).all(|w| w[0].0.start <= w[1].0.start), "sites must be in document order");
        builder.index
    }

    /// Look up a variable by exact name.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<&Variable> {
        self.variable_ids.get(name).map(|&i| &self.variables[i])
    }

    /// Index of a variable by exact name.
    #[must_use]
    pub fn variable_id(&self, name: &str) -> Option<usize> {
        self.variable_ids.get(name).copied()
    }

    /// Entities with this exact name in `namespace`.
    pub fn entities_named<'a>(&'a self, name: &'a str, namespace: Namespace) -> impl Iterator<Item = (usize, &'a Entity)> + 'a {
        self.entities.iter().enumerate().filter(move |(_, e)| e.kind.namespace() == namespace && e.name.as_deref() == Some(name))
    }

    /// The name site at `offset`, also matching a site that ends exactly at
    /// `offset` (cursor right after a name).
    #[must_use]
    pub fn symbol_at(&self, offset: usize) -> Option<(Range<usize>, Symbol)> {
        let after = self.sites.partition_point(|(range, _)| range.start <= offset);
        self.sites[..after].iter().rev().take(2).find(|(range, _)| range.start <= offset && offset <= range.end).cloned()
    }

    /// All name sites in document order.
    #[must_use]
    pub fn sites(&self) -> &[(Range<usize>, Symbol)] {
        &self.sites
    }

    /// Entity whose range contains `offset`.
    #[must_use]
    pub fn entity_at(&self, offset: usize) -> Option<usize> {
        let after = self.entities.partition_point(|e| e.range.start <= offset);
        (after > 0 && offset <= self.entities[after - 1].range.end).then(|| after - 1)
    }

    /// Variables occurring in an entity, with their occurrences there.
    #[must_use]
    pub fn entity_variables(&self, entity: usize) -> Vec<(&Variable, Vec<&Occurrence>)> {
        self.variables
            .iter()
            .filter_map(|v| {
                let occurrences: Vec<&Occurrence> = v.occurrences.iter().filter(|o| o.entity == Some(entity)).collect();
                (!occurrences.is_empty()).then_some((v, occurrences))
            })
            .collect()
    }

    fn find_duplicates(&mut self) {
        let mut seen: HashMap<(Namespace, &str), usize> = HashMap::new();
        for (i, entity) in self.entities.iter().enumerate() {
            let Some(name) = entity.name.as_deref() else { continue };
            match seen.get(&(entity.kind.namespace(), name)) {
                Some(&first) => self.duplicates.push(Duplicate { first, duplicate: i }),
                None => {
                    seen.insert((entity.kind.namespace(), name), i);
                }
            }
        }
    }
}

struct Builder<'a> {
    text: &'a str,
    index: SymbolIndex,
}

impl Builder<'_> {
    fn section(&mut self, node: Node<'_>) {
        if !syntax::is_section(node) {
            return;
        }
        let header = node.child(0).filter(|c| c.kind().ends_with("_keyword")).map(|c| c.byte_range());
        self.index.sections.push(SectionSpan { kind: static_kind(node.kind()), range: node.byte_range(), header });

        let mut cursor = node.walk();
        let children: Vec<Node<'_>> = node.named_children(&mut cursor).collect();
        match node.kind() {
            kind::OBJECTIVES_SECTION => {
                for child in children {
                    match child.kind() {
                        kind::LINEAR_EXPRESSION => {
                            let entity = self.entity(EntityKind::Objective, Section::Objectives, None, child);
                            self.expression(child, entity, Role::ObjectiveTerm);
                        }
                        kind::NAMED_OBJECTIVE => self.named_objective(child),
                        _ => {}
                    }
                }
            }
            kind::CONSTRAINTS_SECTION | kind::LAZY_CONSTRAINTS_SECTION | kind::USER_CUTS_SECTION => {
                let section = match node.kind() {
                    kind::CONSTRAINTS_SECTION => Section::SubjectTo,
                    kind::LAZY_CONSTRAINTS_SECTION => Section::Lazy,
                    _ => Section::UserCuts,
                };
                for child in children.into_iter().filter(|c| c.kind() == kind::CONSTRAINT) {
                    self.constraint(child, section);
                }
            }
            kind::GENERAL_CONSTRAINTS_SECTION => {
                for child in children.into_iter().filter(|c| c.kind() == kind::GENERAL_CONSTRAINT) {
                    self.general_constraint(child);
                }
            }
            kind::BOUNDS_SECTION => {
                for declaration in children.into_iter().filter(|c| c.kind() == kind::BOUND_DECLARATION) {
                    let mut cursor = declaration.walk();
                    for id in declaration.named_children(&mut cursor).filter(|c| c.kind() == kind::IDENTIFIER) {
                        self.occurrence(id, Role::Bound, None, None);
                    }
                }
            }
            kind::GENERALS_SECTION | kind::INTEGERS_SECTION | kind::BINARIES_SECTION | kind::SEMI_CONTINUOUS_SECTION => {
                let role = match node.kind() {
                    kind::GENERALS_SECTION => Role::Generals,
                    kind::INTEGERS_SECTION => Role::Integers,
                    kind::BINARIES_SECTION => Role::Binaries,
                    _ => Role::SemiContinuous,
                };
                for id in children.into_iter().filter(|c| c.kind() == kind::IDENTIFIER) {
                    self.occurrence(id, role, None, None);
                }
            }
            kind::SOS_SECTION => self.sos_section(&children),
            _ => {}
        }
    }

    fn named_objective(&mut self, node: Node<'_>) {
        let name = node.child_by_field_name("name");
        let entity = self.entity(EntityKind::Objective, Section::Objectives, name, node);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                kind::OBJECTIVE_ATTRIBUTE => {
                    if let Some(attr) = child.child_by_field_name("name") {
                        let id = self.index.attributes.len();
                        self.index.attributes.push(Attribute {
                            objective: entity,
                            name: syntax::text(attr, self.text).to_owned(),
                            name_range: attr.byte_range(),
                            range: child.byte_range(),
                        });
                        self.index.sites.push((attr.byte_range(), Symbol::Attribute(id)));
                    }
                }
                kind::LINEAR_EXPRESSION => self.expression(child, entity, Role::ObjectiveTerm),
                _ => {}
            }
        }
    }

    fn constraint(&mut self, node: Node<'_>, section: Section) {
        let name = node.child_by_field_name("name");
        let entity = self.entity(EntityKind::Constraint, section, name, node);
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor) {
            match child.kind() {
                kind::INDICATOR => {
                    if let Some(id) = child.named_child(0).filter(|c| c.kind() == kind::IDENTIFIER) {
                        self.occurrence(id, Role::Indicator, Some(entity), None);
                    }
                }
                kind::LINEAR_EXPRESSION => self.expression(child, entity, Role::ConstraintTerm),
                _ => {}
            }
        }
    }

    fn general_constraint(&mut self, node: Node<'_>) {
        let name = node.child_by_field_name("name");
        let entity = self.entity(EntityKind::GeneralConstraint, Section::General, name, node);
        let resultant = node.child_by_field_name("resultant");
        let mut cursor = node.walk();
        for child in node.named_children(&mut cursor).filter(|c| c.kind() == kind::IDENTIFIER) {
            let role = if Some(child) == resultant { Role::Resultant } else { Role::GeneralArgument };
            self.occurrence(child, role, Some(entity), None);
        }
    }

    fn sos_section(&mut self, children: &[Node<'_>]) {
        let mut current: Option<usize> = None;
        for &child in children {
            match child.kind() {
                kind::SOS_CONSTRAINT_HEADER => {
                    let name = child.child_by_field_name("name");
                    current = Some(self.entity(EntityKind::Sos, Section::Sos, name, child));
                }
                kind::SOS_ENTRY => {
                    if let Some(entity) = current {
                        self.index.entities[entity].range.end = child.end_byte();
                    }
                    let weight = child.named_child(1).map(|n| signed_value(n, self.text));
                    if let Some(id) = child.named_child(0).filter(|c| c.kind() == kind::IDENTIFIER) {
                        self.occurrence(id, Role::SosEntry, current, weight.flatten());
                    }
                }
                _ => {}
            }
        }
    }

    /// Walk a `linear_expression`, tracking the sign before each item.
    fn expression(&mut self, node: Node<'_>, entity: usize, role: Role) {
        let mut sign = 1.0;
        let mut cursor = node.walk();
        for child in node.children(&mut cursor) {
            match child.kind() {
                "-" => sign = -1.0,
                "+" => sign = 1.0,
                kind::TERM => {
                    if let Some(id) = last_child(child).filter(|c| c.kind() == kind::IDENTIFIER) {
                        let coefficient = child
                            .child(0)
                            .filter(|c| c.id() != id.id())
                            .map_or(Some(1.0), |c| syntax::parse_number(syntax::text(c, self.text)));
                        self.occurrence(id, role, Some(entity), coefficient.map(|c| sign * c));
                    }
                    sign = 1.0;
                }
                kind::QUADRATIC_BLOCK => {
                    self.quadratic(child, entity, sign);
                    sign = 1.0;
                }
                _ => {}
            }
        }
    }

    fn quadratic(&mut self, block: Node<'_>, entity: usize, block_sign: f64) {
        let mut sign = block_sign;
        let mut cursor = block.walk();
        for child in block.children(&mut cursor) {
            match child.kind() {
                "-" => sign = -block_sign,
                "+" => sign = block_sign,
                kind::QUADRATIC_TERM => {
                    let mut inner = child.walk();
                    let parts: Vec<Node<'_>> = child.named_children(&mut inner).collect();
                    let coefficient = parts
                        .first()
                        .filter(|n| n.kind() == kind::NUMBER)
                        .and_then(|n| syntax::parse_number(syntax::text(*n, self.text)))
                        .unwrap_or(1.0);
                    for id in parts.iter().filter(|n| n.kind() == kind::IDENTIFIER) {
                        self.occurrence(*id, Role::QuadraticTerm, Some(entity), Some(sign * coefficient));
                    }
                    sign = block_sign;
                }
                _ => {}
            }
        }
    }

    fn entity(&mut self, kind: EntityKind, section: Section, name: Option<Node<'_>>, node: Node<'_>) -> usize {
        let id = self.index.entities.len();
        self.index.entities.push(Entity {
            kind,
            section,
            name: name.map(|n| syntax::text(n, self.text).to_owned()),
            name_range: name.map(|n| n.byte_range()),
            range: node.byte_range(),
            node_kind: static_kind(node.kind()),
        });
        if let Some(n) = name {
            self.index.sites.push((n.byte_range(), Symbol::Entity(id)));
        }
        id
    }

    fn occurrence(&mut self, node: Node<'_>, role: Role, entity: Option<usize>, coefficient: Option<f64>) {
        debug_assert_eq!(node.kind(), kind::IDENTIFIER);
        let name = syntax::text(node, self.text);
        // Look up before inserting: most occurrences repeat a known name, and
        // `entry` would allocate the key every time.
        let var = if let Some(&var) = self.index.variable_ids.get(name) {
            var
        } else {
            let var = self.index.variables.len();
            self.index.variable_ids.insert(name.to_owned(), var);
            self.index.variables.push(Variable { name: name.to_owned(), occurrences: Vec::new() });
            var
        };
        let occurrences = &mut self.index.variables[var].occurrences;
        self.index.sites.push((node.byte_range(), Symbol::Variable(var, occurrences.len())));
        occurrences.push(Occurrence { range: node.byte_range(), role, entity, coefficient });
    }
}

/// Last child of `node`.
fn last_child(node: Node<'_>) -> Option<Node<'_>> {
    let mut cursor = node.walk();
    node.children(&mut cursor).last()
}

/// Value of a `_numeric_value` whose last token is `node` (a number or
/// infinity), including a preceding sign token.
fn signed_value(node: Node<'_>, text: &str) -> Option<f64> {
    let value = syntax::parse_number(syntax::text(node, text))?;
    let negative = node.prev_sibling().is_some_and(|s| s.kind() == "-");
    Some(if negative { -value } else { value })
}

fn static_kind(kind: &str) -> &'static str {
    const EXTRA: &[&str] =
        &[kind::CONSTRAINT, kind::GENERAL_CONSTRAINT, kind::NAMED_OBJECTIVE, kind::LINEAR_EXPRESSION, kind::SOS_CONSTRAINT_HEADER];
    syntax::SECTION_KINDS.iter().chain(EXTRA).find(|k| **k == kind).copied().unwrap_or("unknown")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &str = r"\ every construct
Maximize multi-objectives
 o1: Priority=2 Weight=1 3 x + 2 y - z + [ x ^ 2 + 2 x * y ] / 2
 o2: - x
Subject To
 c1: x + y <= 10
 r1: 2 <= x + z <= 8
 -3 x + 4 >= 2
 ind1: b = 1 -> x + y =< 3
 q1: [ x ^ 2 ] <= 4
Lazy Constraints
 l1: x - y >= -1
User Cuts
 u1: y <= 5
General Constraints
 g1: r = MAX ( x , y , 3 )
Bounds
 0 <= x <= 40
 y free
 z >= -inf
Generals
 x
Binaries
 b
Semi-Continuous
 z
SOS
 s1: S1 :: x : 1 y : -2
End
";

    fn index(text: &str) -> SymbolIndex {
        SymbolIndex::build(&syntax::parse(text, None), text)
    }

    fn roles(index: &SymbolIndex, name: &str) -> Vec<Role> {
        index.variable(name).unwrap().occurrences.iter().map(|o| o.role).collect()
    }

    #[test]
    fn indexes_every_construct() {
        use Role::*;
        assert!(!syntax::parse(ALL, None).root_node().has_error());
        let idx = index(ALL);
        assert_eq!(
            roles(&idx, "x"),
            [
                ObjectiveTerm,
                QuadraticTerm,
                QuadraticTerm,
                ObjectiveTerm,
                ConstraintTerm,
                ConstraintTerm,
                ConstraintTerm,
                ConstraintTerm,
                QuadraticTerm,
                ConstraintTerm,
                GeneralArgument,
                Bound,
                Generals,
                SosEntry
            ]
        );
        assert_eq!(roles(&idx, "b"), [Indicator, Binaries]);
        assert_eq!(roles(&idx, "r"), [Resultant]);
        assert_eq!(roles(&idx, "z"), [ObjectiveTerm, ConstraintTerm, Bound, SemiContinuous]);

        let names: Vec<(EntityKind, Section, Option<&str>)> = idx.entities.iter().map(|e| (e.kind, e.section, e.name.as_deref())).collect();
        assert_eq!(
            names,
            [
                (EntityKind::Objective, Section::Objectives, Some("o1")),
                (EntityKind::Objective, Section::Objectives, Some("o2")),
                (EntityKind::Constraint, Section::SubjectTo, Some("c1")),
                (EntityKind::Constraint, Section::SubjectTo, Some("r1")),
                (EntityKind::Constraint, Section::SubjectTo, None),
                (EntityKind::Constraint, Section::SubjectTo, Some("ind1")),
                (EntityKind::Constraint, Section::SubjectTo, Some("q1")),
                (EntityKind::Constraint, Section::Lazy, Some("l1")),
                (EntityKind::Constraint, Section::UserCuts, Some("u1")),
                (EntityKind::GeneralConstraint, Section::General, Some("g1")),
                (EntityKind::Sos, Section::Sos, Some("s1")),
            ]
        );
        assert_eq!(idx.attributes.iter().map(|a| a.name.as_str()).collect::<Vec<_>>(), ["Priority", "Weight"]);
        assert_eq!(idx.sections.len(), 10);
        assert_eq!(idx.duplicates, []);

        // SOS set spans its entries.
        let sos = &idx.entities[10];
        assert!(ALL[sos.range.clone()].ends_with("y : -2"));
    }

    #[test]
    fn records_signed_coefficients() {
        let idx = index(ALL);
        let x = idx.variable("x").unwrap();
        assert_eq!(x.occurrences[0].coefficient, Some(3.0));
        let o2 = x.occurrences.iter().find(|o| o.entity == Some(1)).unwrap();
        assert_eq!(o2.coefficient, Some(-1.0));
        let z = idx.variable("z").unwrap();
        assert_eq!(z.occurrences[0].coefficient, Some(-1.0));
        let unnamed = x.occurrences.iter().find(|o| o.entity == Some(4)).unwrap();
        assert_eq!(unnamed.coefficient, Some(-3.0));
        let y = idx.variable("y").unwrap();
        assert_eq!(y.occurrences.last().unwrap().coefficient, Some(-2.0));
        // Quadratic `2 x * y`.
        let q = &y.occurrences[1];
        assert_eq!((q.role, q.coefficient), (Role::QuadraticTerm, Some(2.0)));
    }

    #[test]
    fn definition_prefers_bounds_and_declaration_is_type_entry() {
        let idx = index(ALL);
        let x = idx.variable("x").unwrap();
        assert_eq!(x.definition().role, Role::Bound);
        assert_eq!(x.declaration().unwrap().role, Role::Generals);
        assert_eq!(idx.variable("r").unwrap().definition().role, Role::Resultant);
    }

    #[test]
    fn symbol_at_finds_names_and_nothing_between() {
        let idx = index(ALL);
        let c1 = ALL.find("c1:").unwrap();
        assert!(matches!(idx.symbol_at(c1 + 1), Some((_, Symbol::Entity(2)))));
        assert!(matches!(idx.symbol_at(c1 + 2), Some((_, Symbol::Entity(2)))));
        let le = ALL.find("<= 10").unwrap();
        assert!(idx.symbol_at(le + 1).is_none());
    }

    #[test]
    fn detects_duplicates_per_namespace() {
        let text = "min\n c1: x\nst\n c1: x >= 1\n c1: x <= 2\nsos\n c1: S1 :: x : 1\nend\n";
        let idx = index(text);
        // Objective `c1` does not clash with constraints; the two constraints and the SOS set do.
        assert_eq!(idx.duplicates, [Duplicate { first: 1, duplicate: 2 }, Duplicate { first: 1, duplicate: 3 }]);
    }

    #[test]
    fn keyword_named_variables_are_variables() {
        // `bin` mid-line is a name; at line start it would open a Binaries section.
        let text = "min\n obj: bin + end\nst\n c: bin >= 1\nbinaries\n x bin\nend\n";
        let idx = index(text);
        assert_eq!(roles(&idx, "bin"), [Role::ObjectiveTerm, Role::ConstraintTerm, Role::Binaries]);
        assert!(idx.variable("end").is_some());
    }
}
