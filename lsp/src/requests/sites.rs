//! Source ranges for model names: the library reports names, the LSP maps them
//! to ranges with the tree-sitter index (design decision D2).

use std::cell::OnceCell;

use lp_parser_rs::{LpProblem, NameId};
use ls_types as lsp;
use rustc_hash::FxHashMap;

use crate::document::Document;
use crate::index::Namespace;

/// Where an objective or constraint is written.
#[derive(Debug, Clone, PartialEq)]
pub struct EntitySite {
    /// Index of the entity in the document's symbol index.
    pub entity: usize,
    /// The whole entity.
    pub range: lsp::Range,
    /// Its label (the whole entity when unlabelled).
    pub selection: lsp::Range,
    /// Whether the source writes this name; `false` for generated names such
    /// as `C3` or `c1_rng`.
    pub labelled: bool,
}

/// Name-to-range lookups for one document and its parsed model.
pub struct Sites<'a> {
    doc: &'a Document,
    problem: &'a LpProblem,
    /// Model name of the first objective or constraint written at each entity.
    entity_names: OnceCell<FxHashMap<usize, NameId>>,
}

impl<'a> Sites<'a> {
    /// Lookups for `problem`, parsed from `doc`'s current text.
    #[must_use]
    pub const fn new(doc: &'a Document, problem: &'a LpProblem) -> Self {
        Self { doc, problem, entity_names: OnceCell::new() }
    }

    /// Site of the constraint `id`.
    #[must_use]
    pub fn constraint(&self, id: NameId) -> Option<EntitySite> {
        let offset = self.problem.constraints.get(&id).and_then(lp_parser_rs::model::Constraint::byte_offset);
        self.entity(id, offset, Namespace::Constraint)
    }

    /// Site of the objective `id`.
    #[must_use]
    pub fn objective(&self, id: NameId) -> Option<EntitySite> {
        let offset = self.problem.objectives.get(&id).and_then(|o| o.byte_offset);
        self.entity(id, offset, Namespace::Objective)
    }

    fn entity(&self, id: NameId, offset: Option<usize>, namespace: Namespace) -> Option<EntitySite> {
        let index = self.doc.index();
        let name = self.problem.resolve(id);
        let entity = offset
            .and_then(|o| index.entity_at(o))
            .filter(|&e| index.entities[e].kind.namespace() == namespace)
            .or_else(|| index.entities_named(name, namespace).next().map(|(e, _)| e))?;
        Some(self.entity_site(entity, name))
    }

    fn entity_site(&self, entity: usize, name: &str) -> EntitySite {
        let e = &self.doc.index().entities[entity];
        let range = self.doc.range(e.range.clone());
        let selection = e.name_range.clone().map_or(range, |r| self.doc.range(r));
        EntitySite { entity, range, selection, labelled: e.name.as_deref() == Some(name) }
    }

    /// Definition range of the variable `name`.
    #[must_use]
    pub fn variable(&self, name: &str) -> Option<lsp::Range> {
        self.doc.index().variable(name).map(|v| self.doc.range(v.definition().range.clone()))
    }

    /// The model name of symbol-index entity `entity`: its label, else the
    /// generated name of the objective or constraint written there.
    #[must_use]
    pub fn entity_name(&self, entity: usize) -> Option<String> {
        let index = self.doc.index();
        if let Some(name) = index.entities.get(entity).and_then(|e| e.name.clone()) {
            return Some(name);
        }
        let names = self.entity_names.get_or_init(|| {
            let mut names = FxHashMap::default();
            let objectives = self.problem.objectives.values().map(|o| (o.name, o.byte_offset, Namespace::Objective));
            let constraints = self.problem.constraints.values().map(|c| (c.name(), c.byte_offset(), Namespace::Constraint));
            for (name, offset, namespace) in objectives.chain(constraints) {
                if let Some(e) = offset.and_then(|o| index.entity_at(o)).filter(|&e| index.entities[e].kind.namespace() == namespace) {
                    names.entry(e).or_insert(name);
                }
            }
            names
        });
        names.get(&entity).map(|&id| self.problem.resolve(id).to_owned())
    }
}
