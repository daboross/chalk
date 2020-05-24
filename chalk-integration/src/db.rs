use crate::{
    error::ChalkError,
    interner::ChalkIr,
    lowering::LowerGoal,
    program::Program,
    query::{Lowering, LoweringDatabase},
    tls,
};
use chalk_engine::forest::SubstitutionResult;
use chalk_ir::{
    AdtId, AssocTypeId, Canonical, ConstrainedSubst, Environment, FnDefId, GenericArg, Goal,
    ImplId, InEnvironment, OpaqueTyId, ProgramClause, ProgramClauses, TraitId, UCanonical,
};
use chalk_solve::rust_ir::{
    AdtDatum, AssociatedTyDatum, AssociatedTyValue, AssociatedTyValueId, FnDefDatum, ImplDatum,
    OpaqueTyDatum, TraitDatum, WellKnownTrait,
};
use chalk_solve::{RustIrDatabase, Solution, SolverChoice};
use salsa::Database;
use std::sync::Arc;

#[salsa::database(Lowering)]
#[derive(Debug, Default)]
pub struct ChalkDatabase {
    runtime: salsa::Runtime<ChalkDatabase>,
}

impl Database for ChalkDatabase {
    fn salsa_runtime(&self) -> &salsa::Runtime<ChalkDatabase> {
        &self.runtime
    }
}

impl ChalkDatabase {
    pub fn with(program_text: &str, solver_choice: SolverChoice) -> Self {
        let mut db = ChalkDatabase::default();
        db.set_program_text(Arc::new(program_text.to_string()));
        db.set_solver_choice(solver_choice);
        db
    }

    pub fn with_program<R>(&self, op: impl FnOnce(&Program) -> R) -> R {
        let program = &self.checked_program().unwrap();
        tls::set_current_program(&program, || op(&program))
    }

    pub fn parse_and_lower_goal(&self, text: &str) -> Result<Goal<ChalkIr>, ChalkError> {
        let program = self.checked_program()?;
        Ok(chalk_parse::parse_goal(text)?.lower(&*program)?)
    }

    pub fn solve(
        &self,
        goal: &UCanonical<InEnvironment<Goal<ChalkIr>>>,
    ) -> Option<Solution<ChalkIr>> {
        let solver = self.solver();
        let solution = solver.lock().unwrap().solve(self, goal);
        solution
    }

    pub fn solve_multiple(
        &self,
        goal: &UCanonical<InEnvironment<Goal<ChalkIr>>>,
        f: impl FnMut(SubstitutionResult<Canonical<ConstrainedSubst<ChalkIr>>>, bool) -> bool,
    ) -> bool {
        let solver = self.solver();
        let solution = solver.lock().unwrap().solve_multiple(self, goal, f);
        solution
    }
}

impl RustIrDatabase<ChalkIr> for ChalkDatabase {
    fn custom_clauses(&self) -> Vec<ProgramClause<ChalkIr>> {
        self.program_ir().unwrap().custom_clauses()
    }

    fn associated_ty_data(&self, ty: AssocTypeId<ChalkIr>) -> Arc<AssociatedTyDatum<ChalkIr>> {
        self.program_ir().unwrap().associated_ty_data(ty)
    }

    fn trait_datum(&self, id: TraitId<ChalkIr>) -> Arc<TraitDatum<ChalkIr>> {
        self.program_ir().unwrap().trait_datum(id)
    }

    fn impl_datum(&self, id: ImplId<ChalkIr>) -> Arc<ImplDatum<ChalkIr>> {
        self.program_ir().unwrap().impl_datum(id)
    }

    fn associated_ty_value(
        &self,
        id: AssociatedTyValueId<ChalkIr>,
    ) -> Arc<AssociatedTyValue<ChalkIr>> {
        self.program_ir().unwrap().associated_ty_values[&id].clone()
    }

    fn opaque_ty_data(&self, id: OpaqueTyId<ChalkIr>) -> Arc<OpaqueTyDatum<ChalkIr>> {
        self.program_ir().unwrap().opaque_ty_data(id)
    }

    fn adt_datum(&self, id: AdtId<ChalkIr>) -> Arc<AdtDatum<ChalkIr>> {
        self.program_ir().unwrap().adt_datum(id)
    }

    fn fn_def_datum(&self, id: FnDefId<ChalkIr>) -> Arc<FnDefDatum<ChalkIr>> {
        self.program_ir().unwrap().fn_def_datum(id)
    }

    fn impls_for_trait(
        &self,
        trait_id: TraitId<ChalkIr>,
        generic_args: &[GenericArg<ChalkIr>],
    ) -> Vec<ImplId<ChalkIr>> {
        self.program_ir()
            .unwrap()
            .impls_for_trait(trait_id, generic_args)
    }

    fn local_impls_to_coherence_check(&self, trait_id: TraitId<ChalkIr>) -> Vec<ImplId<ChalkIr>> {
        self.program_ir()
            .unwrap()
            .local_impls_to_coherence_check(trait_id)
    }

    fn impl_provided_for(&self, auto_trait_id: TraitId<ChalkIr>, adt_id: AdtId<ChalkIr>) -> bool {
        self.program_ir()
            .unwrap()
            .impl_provided_for(auto_trait_id, adt_id)
    }

    fn well_known_trait_id(&self, well_known_trait: WellKnownTrait) -> Option<TraitId<ChalkIr>> {
        self.program_ir()
            .unwrap()
            .well_known_trait_id(well_known_trait)
    }

    fn program_clauses_for_env(
        &self,
        environment: &Environment<ChalkIr>,
    ) -> ProgramClauses<ChalkIr> {
        chalk_solve::program_clauses_for_env(self, environment)
    }

    fn interner(&self) -> &ChalkIr {
        &ChalkIr
    }

    fn is_object_safe(&self, trait_id: TraitId<ChalkIr>) -> bool {
        self.program_ir().unwrap().is_object_safe(trait_id)
    }

    fn trait_name(&self, trait_id: TraitId<ChalkIr>) -> String {
        self.program_ir().unwrap().trait_name(trait_id)
    }

    fn adt_name(&self, struct_id: AdtId<ChalkIr>) -> String {
        self.program_ir().unwrap().adt_name(struct_id)
    }

    fn assoc_type_name(&self, assoc_ty_id: AssocTypeId<ChalkIr>) -> String {
        self.program_ir().unwrap().assoc_type_name(assoc_ty_id)
    }

    fn opaque_type_name(&self, opaque_ty_id: OpaqueTyId<ChalkIr>) -> String {
        self.program_ir().unwrap().opaque_type_name(opaque_ty_id)
    }
}
