use ena::unify;
use errors::*;
use ir::*;
use std::borrow::Cow;
use std::sync::Arc;

use super::universe::UniverseIndex;
use super::var::*;

#[derive(Clone)]
pub struct InferenceTable {
    unify: unify::UnificationTable<InferenceVariable>,
    values: Vec<Arc<Ty>>,
}

pub struct InferenceSnapshot {
    unify_snapshot: unify::Snapshot<InferenceVariable>,
    values_len: usize,
}

impl InferenceTable {
    pub fn new() -> Self {
        InferenceTable {
            unify: unify::UnificationTable::new(),
            values: vec![],
        }
    }

    pub fn new_with_vars(vars: usize, universe: UniverseIndex) -> Self {
        let mut table = InferenceTable::new();
        for _ in 0..vars {
            table.new_variable(universe);
        }
        table
    }

    pub fn new_variable(&mut self, ui: UniverseIndex) -> InferenceVariable {
        self.unify.new_key(InferenceValue::Unbound(ui))
    }

    pub fn snapshot(&mut self) -> InferenceSnapshot {
        let unify_snapshot = self.unify.snapshot();
        InferenceSnapshot {
            unify_snapshot: unify_snapshot,
            values_len: self.values.len(),
        }
    }

    pub fn rollback_to(&mut self, snapshot: InferenceSnapshot) {
        self.unify.rollback_to(snapshot.unify_snapshot);
        self.values.truncate(snapshot.values_len);
    }

    fn commit(&mut self, snapshot: InferenceSnapshot) {
        self.unify.commit(snapshot.unify_snapshot);
    }

    fn commit_if_ok<F, R>(&mut self, op: F) -> Result<R>
        where F: FnOnce(&mut Self) -> Result<R>
    {
        let snapshot = self.snapshot();
        match op(self) {
            Ok(v) => {
                self.commit(snapshot);
                Ok(v)
            }

            Err(err) => {
                self.rollback_to(snapshot);
                Err(err)
            }
        }
    }

    fn normalize_shallow(&mut self, leaf: &Ty) -> Option<Arc<Ty>> {
        leaf.inference_var()
            .and_then(|var| {
                match self.unify.probe_value(var) {
                    InferenceValue::Unbound(_) => None,
                    InferenceValue::Bound(val) => Some(self.values[val.as_usize()].clone()),
                }
            })
    }

    fn probe_var(&mut self, var: InferenceVariable) -> Option<Arc<Ty>> {
        match self.unify.probe_value(var) {
            InferenceValue::Unbound(_) => None,
            InferenceValue::Bound(val) => Some(self.values[val.as_usize()].clone()),
        }
    }
}

impl Ty {
    pub fn inference_var(&self) -> Option<InferenceVariable> {
        if let Ty::Var(depth) = *self {
            Some(InferenceVariable::from_depth(depth))
        } else {
            None
        }
    }
}

impl ApplicationTy {
    pub fn universe_index(&self) -> UniverseIndex {
        self.id.universe_index()
    }
}

impl ItemId {
    pub fn universe_index(&self) -> UniverseIndex {
        UniverseIndex { counter: 0 }
    }
}


struct Unifier<'t> {
    table: &'t mut InferenceTable,
    snapshot: InferenceSnapshot,
    normalizations: Vec<NormalizeTo>,
}

impl<'t> Unifier<'t> {
    pub fn new(table: &'t mut InferenceTable) -> Self {
        let snapshot = table.snapshot();
        Unifier {
            table: table,
            snapshot: snapshot,
            normalizations: vec![],
        }
    }

    pub fn unify_ty_ty<'a>(&mut self, a: &'a Ty, b: &'a Ty) -> Result<()> {
        //             ^^                 ^^         ^^ FIXME rustc bug
        if let Some(n_a) = self.table.normalize_shallow(a) {
            return self.unify_ty_ty(&n_a, b);
        } else if let Some(n_b) = self.table.normalize_shallow(b) {
            return self.unify_ty_ty(a, &n_b);
        }

        debug!("unify_in_snapshot, normalized a={:?}", a);
        debug!("unify_in_snapshot, normalized b={:?}", b);

        match (a, b) {
            (&Ty::Var(depth1), &Ty::Var(depth2)) => {
                let var1 = InferenceVariable::from_depth(depth1);
                let var2 = InferenceVariable::from_depth(depth2);
                debug!("unify_in_snapshot: unify_var_var({:?}, {:?})", var1, var2);
                Ok(self.table
                    .unify
                    .unify_var_var(var1, var2)
                    .expect("unification of two unbound variables cannot fail"))
            }

            (&Ty::Var(depth), &Ty::Apply(ref apply)) |
            (&Ty::Apply(ref apply), &Ty::Var(depth)) => {
                self.unify_var_apply(InferenceVariable::from_depth(depth), apply)
            }

            (&Ty::Apply(ref apply1), &Ty::Apply(ref apply2)) => {
                self.unify_apply_apply(apply1, apply2)
            }

            (ty, &Ty::Projection(ref proj)) |
            (&Ty::Projection(ref proj), ty) => {
                Ok(self.normalizations.push(NormalizeTo {
                    projection: proj.clone(),
                    ty: ty.clone(),
                }))
            }
        }
    }

    fn unify_apply_apply(&mut self, apply1: &ApplicationTy, apply2: &ApplicationTy) -> Result<()> {
        if apply1.id != apply2.id {
            bail!("incompatible constants {:?} vs {:?}", apply1.id, apply2.id);
        }

        assert_eq!(apply1.args.len(), apply2.args.len());
        for (arg1, arg2) in apply1.args.iter().zip(&apply2.args) {
            self.unify_ty_ty(arg1, arg2)?;
        }
        Ok(())
    }

    fn unify_var_apply(&mut self, var: InferenceVariable, apply: &ApplicationTy) -> Result<()> {
        debug!("unify_var_apply(var={:?}, apply={:?})", var, apply);

        // Determine the universe index associated with this
        // variable. This is basically a count of the number of
        // `forall` binders that had been introduced at the point
        // this variable was created -- though it may change over time
        // as the variable is unified.
        let universe_index = match self.table.unify.probe_value(var) {
            InferenceValue::Unbound(ui) => ui,
            InferenceValue::Bound(_) => panic!("`unify_var_apply` invoked on bound var"),
        };

        self.universe_check(universe_index, apply.universe_index())?;
        self.occurs_check_apply(var, universe_index, apply)?;
        Ok(())
    }

    fn universe_check(&mut self,
                      universe_index: UniverseIndex,
                      application_universe_index: UniverseIndex)
                      -> Result<()> {
        debug!("universe_check({:?}, {:?})",
               universe_index,
               application_universe_index);
        if universe_index < application_universe_index {
            bail!("incompatible universes(universe_index={:?}, application_universe_index={:?})",
                  universe_index,
                  application_universe_index)
        } else {
            Ok(())
        }
    }

    fn occurs_check_apply(&mut self,
                          var: InferenceVariable,
                          universe_index: UniverseIndex,
                          apply: &ApplicationTy)
                          -> Result<()> {
        for arg in &apply.args {
            self.occurs_check_arg(var, universe_index, arg)?;
        }
        Ok(())
    }

    fn occurs_check_arg(&mut self,
                        var: InferenceVariable,
                        universe_index: UniverseIndex,
                        arg: &Ty)
                        -> Result<()> {
        if let Some(n_arg) = self.table.normalize_shallow(arg) {
            return self.occurs_check_arg(var, universe_index, &n_arg);
        }

        match *arg {
            Ty::Apply(ref arg_apply) => {
                self.universe_check(universe_index, arg_apply.universe_index())?;
                self.occurs_check_apply(var, universe_index, arg_apply)?;
            }

            Ty::Var(depth) => {
                let v = InferenceVariable::from_depth(depth);
                let ui = match self.table.unify.probe_value(v) {
                    InferenceValue::Unbound(ui) => ui,
                    InferenceValue::Bound(_) => unreachable!("expected `arg` to be normalized"),
                };

                if self.table.unify.unioned(v, var) {
                    bail!("cycle during unification");
                }

                if universe_index < ui {
                    // Scenario is like:
                    //
                    // ?A = foo(?B)
                    //
                    // where ?A is in universe 0 and ?B is in universe 1.
                    // This is OK, if ?B is promoted to universe 0.
                    self.table
                        .unify
                        .unify_var_value(v, InferenceValue::Unbound(universe_index))
                        .unwrap();
                }
            }

            Ty::Projection(ref proj) => panic!("unimplemented: projection {:?}", proj),
        }
        Ok(())
    }
}
