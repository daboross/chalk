use super::RecordedItemId;
use crate::RustIrDatabase;
use chalk_ir::{
    interner::Interner,
    visit::Visitor,
    visit::{SuperVisit, Visit},
    AliasTy, DebruijnIndex, TyData, TypeName, WhereClause,
};
use std::collections::BTreeSet;

/// Collects the identifiers needed to resolve all the names for a given
/// set of identifers, excluding identifiers we already have.
///
/// When recording identifiers to print, the `LoggingRustIrDatabase` only
/// records identifiers the solver uses. But the solver assumes well-formedness,
/// and thus skips over many names referenced in the definitions.
///
/// For instance, if we have:
///
/// ```rust,ignore
/// struct S {}
///
/// trait Parent {}
/// trait Child where Self: Parent {}
/// impl Parent for S {}
/// impl Child for S {}
/// ```
///
/// And our goal is `S: Child`, we will only render `S`, `impl Child for S`, and
/// `trait Child`. This will not parse because the `Child` trait's definition
/// references parent. IdCollector solves this by collecting all of the directly
/// related identifiers, allowing those to be rendered as well, ensuring name
/// resolution is successful.
pub fn collect_unrecorded_ids<'i, I: Interner, DB: RustIrDatabase<I>>(
    db: &'i DB,
    identifiers: &'_ BTreeSet<RecordedItemId<I>>,
) -> BTreeSet<RecordedItemId<I>> {
    let mut collector = IdCollector {
        db,
        found_identifiers: BTreeSet::new(),
    };
    for id in identifiers {
        match *id {
            RecordedItemId::Adt(adt_id) => {
                collector
                    .db
                    .adt_datum(adt_id)
                    .visit_with(&mut collector, DebruijnIndex::INNERMOST);
            }
            RecordedItemId::FnDef(fn_def) => {
                collector
                    .db
                    .fn_def_datum(fn_def)
                    .visit_with(&mut collector, DebruijnIndex::INNERMOST);
            }
            RecordedItemId::Trait(trait_id) => {
                let trait_datum = collector.db.trait_datum(trait_id);

                trait_datum.visit_with(&mut collector, DebruijnIndex::INNERMOST);
                for assoc_ty_id in &trait_datum.associated_ty_ids {
                    let assoc_ty_datum = collector.db.associated_ty_data(*assoc_ty_id);
                    assoc_ty_datum
                        .bounds_on_self(collector.db.interner())
                        .visit_with(&mut collector, DebruijnIndex::INNERMOST);
                    assoc_ty_datum.visit_with(&mut collector, DebruijnIndex::INNERMOST)
                }
            }
            RecordedItemId::OpaqueTy(opaque_id) => {
                collector
                    .db
                    .opaque_ty_data(opaque_id)
                    .visit_with(&mut collector, DebruijnIndex::INNERMOST);
            }
            RecordedItemId::Impl(impl_id) => {
                let impl_datum = collector.db.impl_datum(impl_id);
                for id in &impl_datum.associated_ty_value_ids {
                    let assoc_ty_value = collector.db.associated_ty_value(*id);
                    assoc_ty_value.visit_with(&mut collector, DebruijnIndex::INNERMOST);
                }
                impl_datum.visit_with(&mut collector, DebruijnIndex::INNERMOST);
            }
        }
    }
    collector
        .found_identifiers
        .difference(identifiers)
        .copied()
        .collect()
}

struct IdCollector<'i, I: Interner, DB: RustIrDatabase<I>> {
    db: &'i DB,
    found_identifiers: BTreeSet<RecordedItemId<I>>,
}

impl<'i, I: Interner, DB: RustIrDatabase<I>> IdCollector<'i, I, DB> {
    fn record(&mut self, id: impl Into<RecordedItemId<I>>) {
        self.found_identifiers.insert(id.into());
    }
}

impl<'i, I: Interner, DB: RustIrDatabase<I>> Visitor<'i, I> for IdCollector<'i, I, DB>
where
    I: 'i,
{
    type Result = ();
    fn as_dyn(&mut self) -> &mut dyn Visitor<'i, I, Result = Self::Result> {
        self
    }
    fn interner(&self) -> &'i I {
        self.db.interner()
    }

    fn visit_ty(
        &mut self,
        ty: &chalk_ir::Ty<I>,
        outer_binder: chalk_ir::DebruijnIndex,
    ) -> Self::Result {
        let ty_data = ty.data(self.db.interner());
        match ty_data {
            TyData::Apply(apply_ty) => match apply_ty.name {
                TypeName::Adt(adt) => self.record(adt),
                TypeName::FnDef(fn_def) => self.record(fn_def),
                TypeName::OpaqueType(opaque) => self.record(opaque),
                _ => {}
            },
            TyData::Alias(alias) => match alias {
                AliasTy::Projection(projection_ty) => {
                    let assoc_ty_datum = self.db.associated_ty_data(projection_ty.associated_ty_id);
                    self.record(assoc_ty_datum.trait_id)
                }
                AliasTy::Opaque(_opaque_ty) => todo!("opaque types!"),
            },
            TyData::BoundVar(..) => (),
            TyData::Dyn(..) => (),
            TyData::Function(..) => (),
            TyData::InferenceVar(..) => (),
            TyData::Placeholder(..) => (),
        }
        ty.super_visit_with(self, outer_binder)
    }

    fn visit_where_clause(
        &mut self,
        where_clause: &WhereClause<I>,
        outer_binder: DebruijnIndex,
    ) -> Self::Result {
        match where_clause {
            WhereClause::Implemented(trait_ref) => self.record(trait_ref.trait_id),
            WhereClause::AliasEq(alias_eq) => match &alias_eq.alias {
                AliasTy::Projection(projection_ty) => {
                    let assoc_ty_datum = self.db.associated_ty_data(projection_ty.associated_ty_id);
                    self.record(assoc_ty_datum.trait_id)
                }
                AliasTy::Opaque(_opaque_ty) => todo!("opaque types!"),
            },
            WhereClause::LifetimeOutlives(_lifetime_outlives) => (),
        }
        where_clause.super_visit_with(self.as_dyn(), outer_binder)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    use chalk_integration::{
        interner::{ChalkIr, Identifier},
        query::LoweringDatabase,
    };

    fn id_str_to_recorded_id(
        program: &chalk_integration::program::Program,
        id_str: &str,
    ) -> RecordedItemId<ChalkIr> {
        let id_identifier = Identifier::from(id_str);
        None.or_else(|| {
            program
                .adt_ids
                .get(&id_identifier)
                .copied()
                .map(RecordedItemId::from)
        })
        .or_else(|| {
            program
                .fn_def_ids
                .get(&id_identifier)
                .copied()
                .map(RecordedItemId::from)
        })
        // .or_else(|| program.closure_ids.get(&id_identifier).copied().map(RecordedItemId::from))
        .or_else(|| {
            program
                .trait_ids
                .get(&id_identifier)
                .copied()
                .map(RecordedItemId::from)
        })
        .or_else(|| {
            program
                .opaque_ty_ids
                .get(&id_identifier)
                .copied()
                .map(RecordedItemId::from)
        })
        .or_else(|| {
            program
                .adt_ids
                .get(&id_identifier)
                .copied()
                .map(RecordedItemId::from)
        })
        .unwrap_or_else(|| panic!("unknown identifier {}", id_str))
    }

    fn collector_test(program_text: &str, ids: &[&str], expected: &[&str]) {
        assert!(program_text.starts_with("{") && program_text.ends_with("}"));
        let program_text = &program_text[1..program_text.len() - 1];
        let db = chalk_integration::db::ChalkDatabase::with(program_text, <_>::default());
        let program = db
            .program_ir()
            .unwrap_or_else(|e| panic!("couldn't lower program {}: {}", program_text, e));
        // inefficient but otherwise correct mapping from names into IDs
        let ids = ids
            .iter()
            .map(|id| id_str_to_recorded_id(&*program, id))
            .collect::<BTreeSet<_>>();
        let expected_ids = expected
            .iter()
            .map(|id| id_str_to_recorded_id(&*program, id))
            .collect::<BTreeSet<_>>();

        // let out = collect_unrecorded_ids::<chalk_integration::interner::ChalkIr, _>(&*program, &ids);
        // assert_eq!(out, expected_ids);
    }

    fn takes_rustirdb<I: crate::Interner, T: crate::RustIrDatabase<I>>() {}

    fn uses_above() {
        takes_rustirdb::<chalk_integration::interner::ChalkIr, chalk_integration::program::Program>(
        );
    }

    macro_rules! collector_test {
        (program $program:tt given [$($given_id:literal),*] produces_exactly [$($expected_id:literal),*]) => {
            collector_test(stringify!($program), &[$($given_id),*], &[$($expected_id),*])
        };
    }

    #[test]
    fn collects_trait_bound_ids() {
        collector_test! {
            program {
                trait B {}
                trait C {}
                trait A: B + C {}
            }
            given ["A"]
            produces_exactly ["B", "C"]
        }
    }

    #[test]
    fn collects_opaque_type_bound_ids() {
        collector_test! {
            program {
                trait B {}
                struct C {}
                opaque type A: B = C;
            }
            given ["A"]
            produces_exactly ["B", "C"]
        }
    }

    #[test]
    fn collects_trait_where_clause_ids() {
        collector_test! {
            program {
                struct C
                trait B where Self: C {}
                trait A<T> where T: B {}
            }
            given ["A"]
            produces_exactly ["B"]
        }
    }

    #[test]
    fn collects_assoc_type_bound_ids() {}

    #[test]
    fn collects_assoc_type_value_ids() {}

    #[test]
    fn collects_traits_in_dyn() {}
}
