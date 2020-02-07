use crate::interner::ChalkIr;
use crate::{tls, Identifier, TypeKind};
use chalk_ir::could_match::CouldMatch;
use chalk_ir::debug::Angle;
use chalk_ir::interner::{ Interner };
use chalk_ir::{
    debug::SeparatorTraitRef, AdtId, AliasTy, ApplicationTy, AssocTypeId, Binders, ClosureId,
    FnDefId, GenericArg, Goal, Goals, ImplId, Lifetime, OpaqueTy, OpaqueTyId, ProgramClause,
    ProgramClauseImplication, ProgramClauses, ProjectionTy, Substitution, TraitId, Ty,
};
use chalk_solve::rust_ir::{
    AdtDatum, AssociatedTyDatum, AssociatedTyValue, AssociatedTyValueId, ClosureKind, FnDefDatum,
    FnDefInputsAndOutputDatum, ImplDatum, ImplType, OpaqueTyDatum, TraitDatum, WellKnownTrait,
};
use chalk_solve::split::Split;
use chalk_solve::RustIrDatabase;
use std::collections::{BTreeMap, HashSet};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    /// From ADT name to item-id. Used during lowering only.
    pub adt_ids: BTreeMap<Identifier, AdtId<ChalkIr>>,

    /// For each ADT:
    pub adt_kinds: BTreeMap<AdtId<ChalkIr>, TypeKind>,

    pub fn_def_ids: BTreeMap<Identifier, FnDefId<ChalkIr>>,

    pub fn_def_kinds: BTreeMap<FnDefId<ChalkIr>, TypeKind>,

    pub closure_ids: BTreeMap<Identifier, ClosureId<ChalkIr>>,

    pub closure_upvars: BTreeMap<ClosureId<ChalkIr>, Binders<Ty<ChalkIr>>>,

    pub closure_kinds: BTreeMap<ClosureId<ChalkIr>, TypeKind>,

    /// From trait name to item-id. Used during lowering only.
    pub trait_ids: BTreeMap<Identifier, TraitId<ChalkIr>>,

    /// For each trait:
    pub trait_kinds: BTreeMap<TraitId<ChalkIr>, TypeKind>,

    /// For each ADT:
    pub adt_data: BTreeMap<AdtId<ChalkIr>, Arc<AdtDatum<ChalkIr>>>,

    pub fn_def_data: BTreeMap<FnDefId<ChalkIr>, Arc<FnDefDatum<ChalkIr>>>,

    pub closure_inputs_and_output:
        BTreeMap<ClosureId<ChalkIr>, Binders<FnDefInputsAndOutputDatum<ChalkIr>>>,

    // Weird name, but otherwise would overlap with `closure_kinds` above.
    pub closure_closure_kind: BTreeMap<ClosureId<ChalkIr>, ClosureKind>,

    /// For each impl:
    pub impl_data: BTreeMap<ImplId<ChalkIr>, Arc<ImplDatum<ChalkIr>>>,

    /// For each associated ty value `type Foo = XXX` found in an impl:
    pub associated_ty_values:
        BTreeMap<AssociatedTyValueId<ChalkIr>, Arc<AssociatedTyValue<ChalkIr>>>,

    // From opaque type name to item-id. Used during lowering only.
    pub opaque_ty_ids: BTreeMap<Identifier, OpaqueTyId<ChalkIr>>,

    /// For each opaque type:
    pub opaque_ty_kinds: BTreeMap<OpaqueTyId<ChalkIr>, TypeKind>,

    /// For each opaque type:
    pub opaque_ty_data: BTreeMap<OpaqueTyId<ChalkIr>, Arc<OpaqueTyDatum<ChalkIr>>>,

    /// Stores the hidden types for opaque types
    pub hidden_opaque_types: BTreeMap<OpaqueTyId<ChalkIr>, Arc<Ty<ChalkIr>>>,

    /// For each trait:
    pub trait_data: BTreeMap<TraitId<ChalkIr>, Arc<TraitDatum<ChalkIr>>>,

    /// For each trait lang item
    pub well_known_traits: BTreeMap<WellKnownTrait, TraitId<ChalkIr>>,

    /// For each associated ty declaration `type Foo` found in a trait:
    pub associated_ty_data: BTreeMap<AssocTypeId<ChalkIr>, Arc<AssociatedTyDatum<ChalkIr>>>,

    /// For each user-specified clause
    pub custom_clauses: Vec<ProgramClause<ChalkIr>>,

    /// Store the traits marked with `#[object_safe]`
    pub object_safe_traits: HashSet<TraitId<ChalkIr>>,
}

impl Program {
    /// Returns the ids for all impls declared in this crate.
    pub(crate) fn local_impl_ids(&self) -> Vec<ImplId<ChalkIr>> {
        self.impl_data
            .iter()
            .filter(|(_, impl_datum)| impl_datum.impl_type == ImplType::Local)
            .map(|(&impl_id, _)| impl_id)
            .collect()
    }
}

impl tls::DebugContext for Program {
    fn debug_adt_id(
        &self,
        adt_id: AdtId<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        if let Some(k) = self.adt_kinds.get(&adt_id) {
            write!(fmt, "{}", k.name)
        } else {
            fmt.debug_struct("InvalidAdtId")
                .field("index", &adt_id.0)
                .finish()
        }
    }

    fn debug_trait_id(
        &self,
        trait_id: TraitId<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        if let Some(k) = self.trait_kinds.get(&trait_id) {
            write!(fmt, "{}", k.name)
        } else {
            fmt.debug_struct("InvalidTraitId")
                .field("index", &trait_id.0)
                .finish()
        }
    }

    fn debug_assoc_type_id(
        &self,
        assoc_type_id: AssocTypeId<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        if let Some(d) = self.associated_ty_data.get(&assoc_type_id) {
            write!(fmt, "({:?}::{})", d.trait_id, d.name)
        } else {
            fmt.debug_struct("InvalidItemId")
                .field("index", &assoc_type_id.0)
                .finish()
        }
    }

    fn debug_opaque_ty_id(
        &self,
        opaque_ty_id: OpaqueTyId<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        if let Some(k) = self.opaque_ty_kinds.get(&opaque_ty_id) {
            write!(fmt, "{}", k.name)
        } else {
            fmt.debug_struct("InvalidItemId")
                .field("index", &opaque_ty_id.0)
                .finish()
        }
    }

    fn debug_alias(
        &self,
        alias_ty: &AliasTy<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        match alias_ty {
            AliasTy::Projection(projection_ty) => self.debug_projection_ty(projection_ty, fmt),
            AliasTy::Opaque(opaque_ty) => self.debug_opaque_ty(opaque_ty, fmt),
        }
    }

    fn debug_projection_ty(
        &self,
        projection_ty: &ProjectionTy<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let (associated_ty_data, trait_params, other_params) = self.split_projection(projection_ty);
        write!(
            fmt,
            "<{:?} as {:?}{:?}>::{}{:?}",
            &trait_params[0],
            associated_ty_data.trait_id,
            Angle(&trait_params[1..]),
            associated_ty_data.name,
            Angle(&other_params)
        )
    }

    fn debug_opaque_ty(
        &self,
        opaque_ty: &OpaqueTy<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        write!(fmt, "{:?}", opaque_ty.opaque_ty_id)
    }

    fn debug_ty(&self, ty: &Ty<ChalkIr>, fmt: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", ty.data(interner))
    }

    fn debug_lifetime(
        &self,
        lifetime: &Lifetime<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", lifetime.data(interner))
    }

    fn debug_generic_arg(
        &self,
        generic_arg: &GenericArg<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", generic_arg.data(interner).inner_debug())
    }

    fn debug_variable_kinds(
        &self,
        variable_kinds: &chalk_ir::VariableKinds<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", variable_kinds.as_slice(interner))
    }

    fn debug_variable_kinds_with_angles(
        &self,
        variable_kinds: &chalk_ir::VariableKinds<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", variable_kinds.inner_debug(interner))
    }

    fn debug_canonical_var_kinds(
        &self,
        variable_kinds: &chalk_ir::CanonicalVarKinds<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", variable_kinds.as_slice(interner))
    }

    fn debug_goal(
        &self,
        goal: &Goal<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", goal.data(interner))
    }

    fn debug_goals(
        &self,
        goals: &Goals<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", goals.debug(interner))
    }

    fn debug_program_clause_implication(
        &self,
        pci: &ProgramClauseImplication<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", pci.debug(interner))
    }

    fn debug_program_clause(
        &self,
        clause: &ProgramClause<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", clause.data(interner))
    }

    fn debug_program_clauses(
        &self,
        clauses: &ProgramClauses<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", clauses.as_slice(interner))
    }

    fn debug_application_ty(
        &self,
        application_ty: &ApplicationTy<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", application_ty.debug(interner))
    }

    fn debug_substitution(
        &self,
        substitution: &Substitution<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", substitution.debug(interner))
    }

    fn debug_separator_trait_ref(
        &self,
        separator_trait_ref: &SeparatorTraitRef<'_, ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", separator_trait_ref.debug(interner))
    }

    fn debug_quantified_where_clauses(
        &self,
        clauses: &chalk_ir::QuantifiedWhereClauses<ChalkIr>,
        fmt: &mut fmt::Formatter<'_>,
    ) -> Result<(), fmt::Error> {
        let interner = self.interner();
        write!(fmt, "{:?}", clauses.as_slice(interner))
    }
}

impl RustIrDatabase<ChalkIr> for Program {
    fn custom_clauses(&self) -> Vec<ProgramClause<ChalkIr>> {
        self.custom_clauses.clone()
    }

    fn associated_ty_data(&self, ty: AssocTypeId<ChalkIr>) -> Arc<AssociatedTyDatum<ChalkIr>> {
        self.associated_ty_data[&ty].clone()
    }

    fn trait_datum(&self, id: TraitId<ChalkIr>) -> Arc<TraitDatum<ChalkIr>> {
        self.trait_data[&id].clone()
    }

    fn impl_datum(&self, id: ImplId<ChalkIr>) -> Arc<ImplDatum<ChalkIr>> {
        self.impl_data[&id].clone()
    }

    fn associated_ty_value(
        &self,
        id: AssociatedTyValueId<ChalkIr>,
    ) -> Arc<AssociatedTyValue<ChalkIr>> {
        self.associated_ty_values[&id].clone()
    }

    fn opaque_ty_data(&self, id: OpaqueTyId<ChalkIr>) -> Arc<OpaqueTyDatum<ChalkIr>> {
        self.opaque_ty_data[&id].clone()
    }

    fn hidden_opaque_type(&self, id: OpaqueTyId<ChalkIr>) -> Ty<ChalkIr> {
        (*self.hidden_opaque_types[&id]).clone()
    }

    fn adt_datum(&self, id: AdtId<ChalkIr>) -> Arc<AdtDatum<ChalkIr>> {
        self.adt_data[&id].clone()
    }

    fn fn_def_datum(&self, id: FnDefId<ChalkIr>) -> Arc<FnDefDatum<ChalkIr>> {
        self.fn_def_data[&id].clone()
    }

    fn impls_for_trait(
        &self,
        trait_id: TraitId<ChalkIr>,
        parameters: &[GenericArg<ChalkIr>],
    ) -> Vec<ImplId<ChalkIr>> {
        let interner = self.interner();
        self.impl_data
            .iter()
            .filter(|(_, impl_datum)| {
                let trait_ref = &impl_datum.binders.skip_binders().trait_ref;
                trait_id == trait_ref.trait_id && {
                    assert_eq!(trait_ref.substitution.len(interner), parameters.len());
                    <[_] as CouldMatch<[_]>>::could_match(
                        &parameters,
                        interner,
                        &trait_ref.substitution.parameters(interner),
                    )
                }
            })
            .map(|(&impl_id, _)| impl_id)
            .collect()
    }

    fn local_impls_to_coherence_check(&self, trait_id: TraitId<ChalkIr>) -> Vec<ImplId<ChalkIr>> {
        self.impl_data
            .iter()
            .filter(|(_, impl_datum)| {
                impl_datum.trait_id() == trait_id && impl_datum.impl_type == ImplType::Local
            })
            .map(|(&impl_id, _)| impl_id)
            .collect()
    }

    fn impl_provided_for(&self, auto_trait_id: TraitId<ChalkIr>, adt_id: AdtId<ChalkIr>) -> bool {
        let interner = self.interner();
        // Look for an impl like `impl Send for Foo` where `Foo` is
        // the ADT.  See `push_auto_trait_impls` for more.
        self.impl_data.values().any(|impl_datum| {
            impl_datum.trait_id() == auto_trait_id
                && impl_datum.self_type_adt_id(interner) == Some(adt_id)
        })
    }

    fn well_known_trait_id(&self, well_known_trait: WellKnownTrait) -> Option<TraitId<ChalkIr>> {
        self.well_known_traits.get(&well_known_trait).map(|x| *x)
    }

    fn program_clauses_for_env(
        &self,
        environment: &chalk_ir::Environment<ChalkIr>,
    ) -> ProgramClauses<ChalkIr> {
        chalk_solve::program_clauses_for_env(self, environment)
    }

    fn interner(&self) -> &ChalkIr {
        &ChalkIr
    }

    fn is_object_safe(&self, trait_id: TraitId<ChalkIr>) -> bool {
        self.object_safe_traits.contains(&trait_id)
    }

    // For all the closure functions: this is different than how rustc does it.
    // In rustc, the substitution, closure kind, fnsig, and upvars are stored
    // together. Here, we store the closure kind, signature, and upvars
    // separately, since it's easier. And this is opaque to `chalk-solve`.

    fn closure_inputs_and_output(
        &self,
        closure_id: ClosureId<ChalkIr>,
        _substs: &Substitution<ChalkIr>,
    ) -> Binders<FnDefInputsAndOutputDatum<ChalkIr>> {
        self.closure_inputs_and_output[&closure_id].clone()
    }

    fn closure_kind(
        &self,
        closure_id: ClosureId<ChalkIr>,
        _substs: &Substitution<ChalkIr>,
    ) -> ClosureKind {
        self.closure_closure_kind[&closure_id]
    }

    fn closure_upvars(
        &self,
        closure_id: ClosureId<ChalkIr>,
        _substs: &Substitution<ChalkIr>,
    ) -> Binders<Ty<ChalkIr>> {
        self.closure_upvars[&closure_id].clone()
    }

    fn closure_fn_substitution(
        &self,
        _closure_id: ClosureId<ChalkIr>,
        substs: &Substitution<ChalkIr>,
    ) -> Substitution<ChalkIr> {
        substs.clone()
    }

    fn trait_name(&self, trait_id: TraitId<ChalkIr>) -> String {
        self.trait_kinds.get(&trait_id).unwrap().name.to_string()
    }

    fn struct_name(&self, struct_id: StructId<ChalkIr>) -> String {
        self.struct_kinds.get(&struct_id).unwrap().name.to_string()
    }

    fn identifier_name(&self, ident: &<ChalkIr as Interner>::Identifier) -> String {
        ident.to_string()
    }
}
