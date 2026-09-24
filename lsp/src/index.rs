//! Per-document symbol index, rebuilt from the syntax tree after each reparse
//! in one linear walk. Drives navigation, rename, hover, completion, code lens
//! and workspace symbols.

use std::ops::Range;
use std::sync::OnceLock;

use rustc_hash::FxHashMap;
use tree_sitter::{Node, Tree, TreeCursor};

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
    variable_ids: FxHashMap<String, usize>,
    /// Every name site, sorted by start offset.
    sites: Vec<(Range<usize>, Symbol)>,
}

impl SymbolIndex {
    /// Build the index from a parsed tree. `ERROR` subtrees are skipped.
    #[must_use]
    pub fn build(tree: &Tree, text: &str) -> Self {
        let mut index = Self::default();
        // Rough capacities from the text size avoid repeated regrowth on large files.
        index.sites.reserve(text.len() / 16);
        index.entities.reserve(text.len() / 64);
        let mut builder = Builder { text, ids: Ids::get(), index };
        let mut cursor = tree.root_node().walk();
        builder.children(&mut cursor, |b, c| {
            if c.node().is_named() {
                b.section(c);
            }
        });
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
        let mut seen: FxHashMap<(Namespace, &str), usize> = FxHashMap::default();
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

/// Grammar symbol and field ids, resolved once. Comparing `u16` ids avoids the
/// UTF-8 check and string compare behind every `Node::kind()`.
struct Ids {
    objectives_section: u16,
    constraints_section: u16,
    lazy_constraints_section: u16,
    user_cuts_section: u16,
    general_constraints_section: u16,
    bounds_section: u16,
    generals_section: u16,
    integers_section: u16,
    binaries_section: u16,
    semi_continuous_section: u16,
    sos_section: u16,
    named_objective: u16,
    objective_attribute: u16,
    linear_expression: u16,
    constraint: u16,
    general_constraint: u16,
    bound_declaration: u16,
    indicator: u16,
    term: u16,
    quadratic_block: u16,
    quadratic_term: u16,
    sos_constraint_header: u16,
    sos_entry: u16,
    identifier: u16,
    number: u16,
    plus: u16,
    minus: u16,
    name_field: u16,
    resultant_field: u16,
}

impl Ids {
    fn get() -> &'static Self {
        static IDS: OnceLock<Ids> = OnceLock::new();
        IDS.get_or_init(|| {
            let language = syntax::language();
            let named = |kind: &str| {
                let id = language.id_for_node_kind(kind, true);
                debug_assert_ne!(id, 0, "unknown node kind {kind}");
                id
            };
            let field = |name: &str| language.field_id_for_name(name).map_or(0, std::num::NonZeroU16::get);
            Self {
                objectives_section: named(kind::OBJECTIVES_SECTION),
                constraints_section: named(kind::CONSTRAINTS_SECTION),
                lazy_constraints_section: named(kind::LAZY_CONSTRAINTS_SECTION),
                user_cuts_section: named(kind::USER_CUTS_SECTION),
                general_constraints_section: named(kind::GENERAL_CONSTRAINTS_SECTION),
                bounds_section: named(kind::BOUNDS_SECTION),
                generals_section: named(kind::GENERALS_SECTION),
                integers_section: named(kind::INTEGERS_SECTION),
                binaries_section: named(kind::BINARIES_SECTION),
                semi_continuous_section: named(kind::SEMI_CONTINUOUS_SECTION),
                sos_section: named(kind::SOS_SECTION),
                named_objective: named(kind::NAMED_OBJECTIVE),
                objective_attribute: named(kind::OBJECTIVE_ATTRIBUTE),
                linear_expression: named(kind::LINEAR_EXPRESSION),
                constraint: named(kind::CONSTRAINT),
                general_constraint: named(kind::GENERAL_CONSTRAINT),
                bound_declaration: named(kind::BOUND_DECLARATION),
                indicator: named(kind::INDICATOR),
                term: named(kind::TERM),
                quadratic_block: named(kind::QUADRATIC_BLOCK),
                quadratic_term: named(kind::QUADRATIC_TERM),
                sos_constraint_header: named(kind::SOS_CONSTRAINT_HEADER),
                sos_entry: named(kind::SOS_ENTRY),
                identifier: named(kind::IDENTIFIER),
                number: named(kind::NUMBER),
                plus: language.id_for_node_kind("+", false),
                minus: language.id_for_node_kind("-", false),
                name_field: field("name"),
                resultant_field: field("resultant"),
            }
        })
    }
}

/// Single-cursor tree walk. Every visit takes the cursor on a node and leaves
/// it on that same node.
struct Builder<'a> {
    text: &'a str,
    ids: &'static Ids,
    index: SymbolIndex,
}

impl<'t> Builder<'_> {
    /// Call `f` with the cursor on each child of the current node.
    fn children(&mut self, cursor: &mut TreeCursor<'t>, mut f: impl FnMut(&mut Self, &mut TreeCursor<'t>)) {
        if cursor.goto_first_child() {
            loop {
                f(self, cursor);
                if !cursor.goto_next_sibling() {
                    break;
                }
            }
            cursor.goto_parent();
        }
    }

    fn section(&mut self, cursor: &mut TreeCursor<'t>) {
        let node = cursor.node();
        if !syntax::is_section(node) {
            return;
        }
        let header = node.child(0).filter(|c| c.kind().ends_with("_keyword")).map(|c| c.byte_range());
        self.index.sections.push(SectionSpan { kind: static_kind(node.kind()), range: node.byte_range(), header });

        let ids = self.ids;
        let id = node.kind_id();
        if id == ids.objectives_section {
            self.children(cursor, |b, c| {
                let child = c.node().kind_id();
                if child == ids.linear_expression {
                    let entity = b.entity(EntityKind::Objective, Section::Objectives, None, c.node());
                    b.expression(c, entity, Role::ObjectiveTerm);
                } else if child == ids.named_objective {
                    b.named_objective(c);
                }
            });
        } else if id == ids.constraints_section || id == ids.lazy_constraints_section || id == ids.user_cuts_section {
            let section = if id == ids.constraints_section {
                Section::SubjectTo
            } else if id == ids.lazy_constraints_section {
                Section::Lazy
            } else {
                Section::UserCuts
            };
            self.children(cursor, |b, c| {
                if c.node().kind_id() == ids.constraint {
                    b.constraint(c, section);
                }
            });
        } else if id == ids.general_constraints_section {
            self.children(cursor, |b, c| {
                if c.node().kind_id() == ids.general_constraint {
                    b.general_constraint(c);
                }
            });
        } else if id == ids.bounds_section {
            self.children(cursor, |b, c| {
                if c.node().kind_id() == ids.bound_declaration {
                    b.identifiers(c, Role::Bound, None);
                }
            });
        } else if id == ids.sos_section {
            self.sos_section(cursor);
        } else {
            let role = if id == ids.generals_section {
                Role::Generals
            } else if id == ids.integers_section {
                Role::Integers
            } else if id == ids.binaries_section {
                Role::Binaries
            } else {
                debug_assert_eq!(id, ids.semi_continuous_section);
                Role::SemiContinuous
            };
            self.identifiers(cursor, role, None);
        }
    }

    /// Record every identifier child of the current node.
    fn identifiers(&mut self, cursor: &mut TreeCursor<'t>, role: Role, entity: Option<usize>) {
        let identifier = self.ids.identifier;
        self.children(cursor, |b, c| {
            if c.node().kind_id() == identifier {
                b.occurrence(c.node(), role, entity, None);
            }
        });
    }

    fn named_objective(&mut self, cursor: &mut TreeCursor<'t>) {
        let node = cursor.node();
        let ids = self.ids;
        let entity = self.entity(EntityKind::Objective, Section::Objectives, node.child_by_field_id(ids.name_field), node);
        self.children(cursor, |b, c| {
            let child = c.node();
            if child.kind_id() == ids.objective_attribute {
                if let Some(attr) = child.child_by_field_id(ids.name_field) {
                    let id = b.index.attributes.len();
                    b.index.attributes.push(Attribute {
                        objective: entity,
                        name: syntax::text(attr, b.text).to_owned(),
                        name_range: attr.byte_range(),
                        range: child.byte_range(),
                    });
                    b.index.sites.push((attr.byte_range(), Symbol::Attribute(id)));
                }
            } else if child.kind_id() == ids.linear_expression {
                b.expression(c, entity, Role::ObjectiveTerm);
            }
        });
    }

    fn constraint(&mut self, cursor: &mut TreeCursor<'t>, section: Section) {
        let node = cursor.node();
        let ids = self.ids;
        let entity = self.entity(EntityKind::Constraint, section, node.child_by_field_id(ids.name_field), node);
        self.children(cursor, |b, c| {
            let child = c.node().kind_id();
            if child == ids.indicator {
                if let Some(id) = c.node().named_child(0).filter(|n| n.kind_id() == ids.identifier) {
                    b.occurrence(id, Role::Indicator, Some(entity), None);
                }
            } else if child == ids.linear_expression {
                b.expression(c, entity, Role::ConstraintTerm);
            }
        });
    }

    fn general_constraint(&mut self, cursor: &mut TreeCursor<'t>) {
        let node = cursor.node();
        let ids = self.ids;
        let entity = self.entity(EntityKind::GeneralConstraint, Section::General, node.child_by_field_id(ids.name_field), node);
        self.children(cursor, |b, c| {
            let child = c.node();
            if child.kind_id() == ids.identifier {
                let role = if c.field_id().map(std::num::NonZeroU16::get) == Some(ids.resultant_field) {
                    Role::Resultant
                } else {
                    Role::GeneralArgument
                };
                b.occurrence(child, role, Some(entity), None);
            }
        });
    }

    fn sos_section(&mut self, cursor: &mut TreeCursor<'t>) {
        let ids = self.ids;
        let mut current: Option<usize> = None;
        self.children(cursor, |b, c| {
            let child = c.node();
            if child.kind_id() == ids.sos_constraint_header {
                current = Some(b.entity(EntityKind::Sos, Section::Sos, child.child_by_field_id(ids.name_field), child));
            } else if child.kind_id() == ids.sos_entry {
                if let Some(entity) = current {
                    b.index.entities[entity].range.end = child.end_byte();
                }
                // `name : [sign] value`
                let (mut name, mut sign, mut weight) = (None, 1.0, None);
                b.children(c, |b, c| {
                    let part = c.node();
                    let id = part.kind_id();
                    if id == ids.identifier && name.is_none() {
                        name = Some(part);
                    } else if id == ids.minus {
                        sign = -1.0;
                    } else if part.is_named() && name.is_some() {
                        weight = syntax::parse_number(syntax::text(part, b.text)).map(|w| sign * w);
                    }
                });
                if let Some(name) = name {
                    b.occurrence(name, Role::SosEntry, current, weight);
                }
            }
        });
    }

    /// Walk a `linear_expression`, tracking the sign before each item.
    fn expression(&mut self, cursor: &mut TreeCursor<'t>, entity: usize, role: Role) {
        let ids = self.ids;
        let mut sign = 1.0;
        self.children(cursor, |b, c| {
            let child = c.node();
            let id = child.kind_id();
            if id == ids.minus {
                sign = -1.0;
            } else if id == ids.plus {
                sign = 1.0;
            } else if id == ids.term {
                // `[constant] identifier` or a bare constant.
                let count = child.child_count();
                if let Some(name) = child.child(count.saturating_sub(1)).filter(|n| n.kind_id() == ids.identifier) {
                    let coefficient =
                        if count > 1 { child.child(0).and_then(|n| syntax::parse_number(syntax::text(n, b.text))) } else { Some(1.0) };
                    b.occurrence(name, role, Some(entity), coefficient.map(|v| sign * v));
                }
                sign = 1.0;
            } else if id == ids.quadratic_block {
                b.quadratic(c, entity, sign);
                sign = 1.0;
            }
        });
    }

    fn quadratic(&mut self, cursor: &mut TreeCursor<'t>, entity: usize, block_sign: f64) {
        let ids = self.ids;
        let mut sign = block_sign;
        self.children(cursor, |b, c| {
            let id = c.node().kind_id();
            if id == ids.minus {
                sign = -block_sign;
            } else if id == ids.plus {
                sign = block_sign;
            } else if id == ids.quadratic_term {
                let mut coefficient = 1.0;
                let mut first = true;
                b.children(c, |b, c| {
                    let part = c.node();
                    if first && part.kind_id() == ids.number {
                        coefficient = syntax::parse_number(syntax::text(part, b.text)).unwrap_or(1.0);
                    } else if part.kind_id() == ids.identifier {
                        b.occurrence(part, Role::QuadraticTerm, Some(entity), Some(sign * coefficient));
                    }
                    first = false;
                });
                sign = block_sign;
            }
        });
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
