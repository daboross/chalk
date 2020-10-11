use super::var::*;
use super::*;
use crate::debug_span;
use crate::infer::instantiate::IntoBindersAndValue;
use chalk_ir::cast::Cast;
use chalk_ir::fold::{Fold, Folder};
use chalk_ir::interner::{HasInterner, Interner};
use chalk_ir::zip::{Zip, Zipper};
use chalk_ir::UnificationDatabase;
use std::fmt::Debug;
use tracing::instrument;

impl<I: Interner> InferenceTable<I> {
    #[instrument(level = "debug", skip(self, interner, db, environment))]
    pub fn relate<T>(
        &mut self,
        interner: &I,
        db: &dyn UnificationDatabase<I>,
        environment: &Environment<I>,
        variance: Variance,
        a: &T,
        b: &T,
    ) -> Fallible<RelationResult<I>>
    where
        T: ?Sized + Zip<I>,
    {
        let snapshot = self.snapshot();
        match Unifier::new(interner, db, self, environment).relate(variance, a, b) {
            Ok(r) => {
                self.commit(snapshot);
                Ok(r)
            }
            Err(e) => {
                self.rollback_to(snapshot);
                Err(e)
            }
        }
    }
}

struct Unifier<'t, I: Interner> {
    table: &'t mut InferenceTable<I>,
    environment: &'t Environment<I>,
    goals: Vec<InEnvironment<Goal<I>>>,
    interner: &'t I,
    db: &'t dyn UnificationDatabase<I>,
}

#[derive(Debug)]
pub struct RelationResult<I: Interner> {
    pub goals: Vec<InEnvironment<Goal<I>>>,
}

impl<'t, I: Interner> Unifier<'t, I> {
    fn new(
        interner: &'t I,
        db: &'t dyn UnificationDatabase<I>,
        table: &'t mut InferenceTable<I>,
        environment: &'t Environment<I>,
    ) -> Self {
        Unifier {
            environment,
            table,
            goals: vec![],
            interner,
            db,
        }
    }

    /// The main entry point for the `Unifier` type and really the
    /// only type meant to be called externally. Performs a
    /// relation of `a` and `b` and returns the Unification Result.
    #[instrument(level = "debug", skip(self))]
    fn relate<T>(mut self, variance: Variance, a: &T, b: &T) -> Fallible<RelationResult<I>>
    where
        T: ?Sized + Zip<I>,
    {
        Zip::zip_with(&mut self, variance, a, b)?;
        Ok(RelationResult { goals: self.goals })
    }

    fn relate_ty_ty(&mut self, variance: Variance, a: &Ty<I>, b: &Ty<I>) -> Fallible<()> {
        let interner = self.interner;

        let n_a = self.table.normalize_ty_shallow(interner, a);
        let n_b = self.table.normalize_ty_shallow(interner, b);
        let a = n_a.as_ref().unwrap_or(a);
        let b = n_b.as_ref().unwrap_or(b);

        debug_span!("relate_ty_ty", ?variance, ?a, ?b);

        match (a.data(interner), b.data(interner)) {
            // Relating two inference variables:
            // If `Invariant`, unify them in the underlying ena table.
            // If `Covariant` or `Contravariant`, push `SubtypeGoal`
            (
                &TyData::InferenceVar(var1, kind1),
                &TyData::InferenceVar(var2, kind2),
            ) => {
                match variance {
                    Variance::Invariant => {
                        if kind1 == kind2 {
                            self.unify_var_var(var1, var2)
                        } else if kind1 == TyKind::General {
                            self.unify_general_var_specific_ty(var1, b.clone())
                        } else if kind2 == TyKind::General {
                            self.unify_general_var_specific_ty(var2, a.clone())
                        } else {
                            debug!(
                                "Tried to unify mis-matching inference variables: {:?} and {:?}",
                                kind1, kind2
                            );
                            Err(NoSolution)
                        }
                    },
                    Variance::Covariant => {
                        self.push_subtype_goal(a.clone(), b.clone());
                        Ok(())
                    },
                    Variance::Contravariant => {
                        self.push_subtype_goal(b.clone(), a.clone());
                        Ok(())
                    },
                }
            }

            // FIXME: needs to handle relating a var and ty; needs generalization
            // Relating an inference variable with a non-inference variable.
            (&TyData::InferenceVar(var, kind), ty_data @ &TyData::Apply(_))
            | (&TyData::InferenceVar(var, kind), ty_data @ &TyData::Placeholder(_))
            | (&TyData::InferenceVar(var, kind), ty_data @ &TyData::Dyn(_))
            | (&TyData::InferenceVar(var, kind), ty_data @ &TyData::Function(_))
            // The reflexive matches
            | (ty_data @ &TyData::Apply(_), &TyData::InferenceVar(var, kind))
            | (ty_data @ &TyData::Placeholder(_), &TyData::InferenceVar(var, kind))
            | (ty_data @ &TyData::Dyn(_), &TyData::InferenceVar(var, kind))
            | (ty_data @ &TyData::Function(_), &TyData::InferenceVar(var, kind))
            => {
                let ty = ty_data.clone().intern(interner);

                match (kind, ty.is_integer(interner), ty.is_float(interner)) {
                    // General inference variables can unify with any type
                    (TyKind::General, _, _)
                    // Integer inference variables can only unify with integer types
                    | (TyKind::Integer, true, _)
                    // Float inference variables can only unify with float types
                    | (TyKind::Float, _, true) => self.relate_var_ty(variance, var, &ty),
                    _ => Err(NoSolution),
                }
            }

            // Unifying `forall<X> { T }` with some other forall type `forall<X> { U }`
            (&TyData::Function(ref fn1), &TyData::Function(ref fn2)) => {
                if fn1.abi == fn2.abi && fn1.safety == fn2.safety && fn1.variadic == fn2.variadic {
                    Zip::zip_with(self, variance, &fn1.clone().into_binders(interner), &fn2.clone().into_binders(interner))
                } else {
                    Err(NoSolution)
                }
            }

            // This would correspond to unifying a `fn` type with a non-fn
            // type in Rust; error.
            (&TyData::Function(_), &TyData::Apply(_))
            | (&TyData::Function(_), &TyData::Dyn(_))
            | (&TyData::Function(_), &TyData::Placeholder(_))
            | (&TyData::Apply(_), &TyData::Function(_))
            | (&TyData::Placeholder(_), &TyData::Function(_))
            | (&TyData::Dyn(_), &TyData::Function(_)) => Err(NoSolution),

            (&TyData::Placeholder(ref p1), &TyData::Placeholder(ref p2)) => {
                Zip::zip_with(self, variance, p1, p2)
            }

            (&TyData::Apply(ref apply1), &TyData::Apply(ref apply2)) => {
                debug!("ty_ty apply/apply hit - {:?} unifying with {:?}", apply1, apply2);
                Zip::zip_with(self, variance, apply1, apply2)
            }

            // Cannot unify (e.g.) some struct type `Foo` and a placeholder like `T`
            (&TyData::Apply(_), &TyData::Placeholder(_))
            | (&TyData::Placeholder(_), &TyData::Apply(_)) => Err(NoSolution),

            // Cannot unify `dyn Trait` with things like structs or placeholders
            (&TyData::Placeholder(_), &TyData::Dyn(_))
            | (&TyData::Dyn(_), &TyData::Placeholder(_))
            | (&TyData::Apply(_), &TyData::Dyn(_))
            | (&TyData::Dyn(_), &TyData::Apply(_)) => Err(NoSolution),

            // Unifying two dyn is possible if they have the same bounds.
            (&TyData::Dyn(ref qwc1), &TyData::Dyn(ref qwc2)) => Zip::zip_with(self, variance, qwc1, qwc2),

            // Unifying an alias type with some other type `U`.
            (&TyData::Apply(_), &TyData::Alias(ref alias))
            | (&TyData::Placeholder(_), &TyData::Alias(ref alias))
            | (&TyData::Function(_), &TyData::Alias(ref alias))
            | (&TyData::InferenceVar(_, _), &TyData::Alias(ref alias))
            | (&TyData::Dyn(_), &TyData::Alias(ref alias)) => self.relate_alias_ty(variance.invert(), alias, a),

            (&TyData::Alias(ref alias), &TyData::Alias(_))
            | (&TyData::Alias(ref alias), &TyData::Apply(_))
            | (&TyData::Alias(ref alias), &TyData::Placeholder(_))
            | (&TyData::Alias(ref alias), &TyData::Function(_))
            | (&TyData::Alias(ref alias), &TyData::InferenceVar(_, _))
            | (&TyData::Alias(ref alias), &TyData::Dyn(_)) => self.relate_alias_ty(variance, alias, b),

            (TyData::BoundVar(_), _) | (_, TyData::BoundVar(_)) => panic!(
                "unification encountered bound variable: a={:?} b={:?}",
                a, b
            ),
        }
    }

    /// Unify two inference variables
    #[instrument(level = "debug", skip(self))]
    fn unify_var_var(&mut self, a: InferenceVar, b: InferenceVar) -> Fallible<()> {
        debug_span!("unify_var_var", ?a, ?b);
        let var1 = EnaVariable::from(a);
        let var2 = EnaVariable::from(b);
        Ok(self
            .table
            .unify
            .unify_var_var(var1, var2)
            .expect("unification of two unbound variables cannot fail"))
    }

    /// Unify a general inference variable with a specific inference variable
    /// (type kind is not `General`). For example, unify a `TyKind::General`
    /// inference variable with a `TyKind::Integer` variable, resulting in the
    /// general inference variable narrowing to an integer variable.

    #[instrument(level = "debug", skip(self))]
    fn unify_general_var_specific_ty(
        &mut self,
        general_var: InferenceVar,
        specific_ty: Ty<I>,
    ) -> Fallible<()> {
        debug_span!("unify_general_var_specific_ty", ?general_var, ?specific_ty);
        self.table
            .unify
            .unify_var_value(
                general_var,
                InferenceValue::from_ty(self.interner, specific_ty),
            )
            .unwrap();

        Ok(())
    }

    #[instrument(level = "debug", skip(self))]
    fn relate_binders<'a, T, R>(
        &mut self,
        variance: Variance,
        a: impl IntoBindersAndValue<'a, I, Value = T> + Copy + Debug,
        b: impl IntoBindersAndValue<'a, I, Value = T> + Copy + Debug,
    ) -> Fallible<()>
    where
        T: Fold<I, Result = R>,
        R: Zip<I> + Fold<I, Result = R>,
        't: 'a,
    {
        debug_span!("relate_binders", ?variance, ?a, ?b);
        // for<'a...> T == for<'b...> U
        //
        // if:
        //
        // for<'a...> exists<'b...> T == U &&
        // for<'b...> exists<'a...> T == U

        let interner = self.interner;

        {
            let a_universal = self.table.instantiate_binders_universally(interner, a);
            let b_existential = self.table.instantiate_binders_existentially(interner, b);
            Zip::zip_with(self, variance, &a_universal, &b_existential)?;
        }

        {
            let b_universal = self.table.instantiate_binders_universally(interner, b);
            let a_existential = self.table.instantiate_binders_existentially(interner, a);
            Zip::zip_with(self, variance, &a_existential, &b_universal)
        }
    }

    /// Relate an alias like `<T as Trait>::Item` or `impl Trait` with some other
    /// type `ty`. If the variance is `Invariant`, creates a goal like
    ///
    /// ```notrust
    /// AliasEq(<T as Trait>::Item = U) // associated type projection
    /// AliasEq(impl Trait = U) // impl trait
    /// ```
    /// Otherwise, this creates a new variable `?X`, creates a goal like
    /// ```notrust
    /// AliasEq(Alias = ?X)
    /// ```
    /// and relates `?X` and `ty`.
    fn relate_alias_ty(
        &mut self,
        variance: Variance,
        alias: &AliasTy<I>,
        ty: &Ty<I>,
    ) -> Fallible<()> {
        debug_span!("relate_alias_ty", ?variance, ?alias, ?ty);
        let interner = self.interner;
        match variance {
            Variance::Invariant => {
                self.goals.push(InEnvironment::new(
                    self.environment,
                    AliasEq {
                        alias: alias.clone(),
                        ty: ty.clone(),
                    }
                    .cast(interner),
                ));
                Ok(())
            }
            Variance::Covariant | Variance::Contravariant => {
                let var = self
                    .table
                    .new_variable(UniverseIndex::root())
                    .to_ty(interner);
                self.goals.push(InEnvironment::new(
                    self.environment,
                    AliasEq {
                        alias: alias.clone(),
                        ty: var.clone(),
                    }
                    .cast(interner),
                ));
                self.relate_ty_ty(variance, &var, ty)
            }
        }
    }

    fn generalize_generic_var(
        &mut self,
        variance: Variance,
        sub_var: &GenericArg<I>,
        universe_index: UniverseIndex,
    ) -> Fallible<GenericArg<I>> {
        // TODO: this is probably relating variance wrong, since we use outer
        // variance without considering anything from the structs.
        let interner = self.interner;
        let ena_var = self.table.new_variable(universe_index);
        let var = (match sub_var.data(interner) {
            GenericArgData::Ty(old_ty) => {
                let new_var = ena_var.to_ty(interner);
                self.relate_ty_ty(variance, old_ty, &new_var).map_err(|e| {
                    debug!("relate_ty_ty failed (no solution)");
                    e
                })?;

                GenericArgData::Ty(new_var)
            }
            GenericArgData::Lifetime(old_lifetime) => {
                let new_var = ena_var.to_lifetime(interner);
                self.relate_lifetime_lifetime(variance, old_lifetime, &new_var)
                    .map_err(|e| {
                        debug!("relate_ty_ty failed (no solution)");
                        e
                    })?;
                GenericArgData::Lifetime(new_var)
            }
            GenericArgData::Const(const_value) => {
                let new_var = ena_var.to_const(interner, const_value.data(interner).ty.clone());
                self.relate_const_const(variance, const_value, &new_var)
                    .map_err(|e| {
                        debug!("relate_ty_ty failed (no solution)");
                        e
                    })?;

                GenericArgData::Const(new_var)
            }
        })
        .intern(interner);

        Ok(var)
    }

    /// Generalizes all but the first
    fn generalize_substitution_skip_self(
        &mut self,
        variance: Variance,
        substitution: &Substitution<I>,
        universe_index: UniverseIndex,
    ) -> Fallible<Substitution<I>> {
        debug_span!(
            "generalize_substitution_skip_self",
            ?substitution,
            ?universe_index
        );
        let interner = self.interner;
        let vars = substitution.iter(interner).take(1).cloned().chain(
            substitution
                .iter(interner)
                .skip(1)
                .map(|sub_var| self.generalize_generic_var(variance, sub_var, universe_index))
                .collect::<Result<Vec<_>, _>>()?,
        );
        Ok(Substitution::from_iter(interner, vars))
    }

    fn generalize_substitution(
        &mut self,
        variance: Variance,
        substitution: &Substitution<I>,
        universe_index: UniverseIndex,
    ) -> Fallible<Substitution<I>> {
        debug_span!("generalize_substitution", ?substitution, ?universe_index);
        let interner = self.interner;
        let vars = substitution
            .iter(interner)
            .map(|sub_var| self.generalize_generic_var(variance, sub_var, universe_index))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Substitution::from_iter(interner, vars))
    }

    /// Unify an inference variable `var` with some non-inference
    /// variable `ty`, just bind `var` to `ty`. But we must enforce two conditions:
    ///
    /// - `var` does not appear inside of `ty` (the standard `OccursCheck`)
    /// - `ty` does not reference anything in a lifetime that could not be named in `var`
    ///   (the extended `OccursCheck` created to handle universes)
    fn relate_var_ty(&mut self, variance: Variance, var: InferenceVar, ty: &Ty<I>) -> Fallible<()> {
        debug_span!("relate_var_ty", ?var, ?ty);

        let interner = self.interner;
        let var = EnaVariable::from(var);

        // Determine the universe index associated with this
        // variable. This is basically a count of the number of
        // `forall` binders that had been introduced at the point
        // this variable was created -- though it may change over time
        // as the variable is unified.
        // let universe_index = self.table.universe_of_unbound_var(var);
        let universe_index = self.table.max_universe();

        debug!("relate_var_ty: universe index of var: {:?}", universe_index);

        debug!("trying fold_with on {:?}", ty);
        let ty1 = ty
            .fold_with(
                &mut OccursCheck::new(self, var, universe_index),
                DebruijnIndex::INNERMOST,
            )
            .map_err(|e| {
                debug!("failed to fold {:?}", ty);
                e
            })?;

        // "Generalize" types. This ensures that we aren't accidentally forcing
        // too much onto `var`. Instead of directly setting `var` equal to `ty`,
        // we just take the outermost structure we _know_ `var` holds, and then
        // apply that to `ty`. This involves creating new inference vars for
        // everything inside `var`, then recursing down to unify those new
        // inference variables with

        // TODO: the justification for why we need to generalize here is a bit
        // weak. Could we include a concrete example of what this fixes? Or,
        // alternatively, link to a test case which requires this & say "it's
        // complicated why exactly we need this".

        let universe_index = self.table.max_universe;

        // Example operation: consider `ty` as `&'x SomeType`. To generalize
        // this, we create two new vars `'0` and `1`. Then we relate `var` with
        // `&'0 1` and `&'0 1` with `&'x SomeType`. The second relation will
        // recurse, and we'll end up relating `'0` with `'x` and `1` with `SomeType`.
        let generalized_val = match ty1.data(interner) {
            TyData::Apply(aty_data) => {
                let ApplicationTy { substitution, name } = aty_data;
                let substitution =
                    self.generalize_substitution(variance, substitution, universe_index)?;
                let name = name.clone();
                TyData::Apply(ApplicationTy { substitution, name }).intern(interner)
            }
            TyData::Dyn(dyn_ty) => {
                let DynTy {
                    bounds,
                    lifetime: _,
                } = dyn_ty;
                let lifetime_var = self.table.new_variable(universe_index);
                let lifetime = lifetime_var.to_lifetime(interner);

                let mut error = None;

                let bounds = bounds.map_ref(|value| {
                    // let universe_index = universe_index.next();
                    let iter = value.iter(interner).map(|sub_var| {
                        sub_var.map_ref(|clause| {
                            // let universe_index = universe_index.next();
                            // let universe_index = self.table.new_universe();
                            match clause {
                                WhereClause::Implemented(trait_ref) => {
                                    let TraitRef {
                                        ref substitution,
                                        trait_id,
                                    } = *trait_ref;
                                    let old_sub = substitution;
                                    let substitution = self.generalize_substitution_skip_self(
                                        variance,
                                        substitution,
                                        universe_index,
                                    );
                                    let substitution = match substitution {
                                        Ok(v) => v,
                                        Err(e) => {
                                            error = Some(e);
                                            return clause.clone();
                                        }
                                    };
                                    WhereClause::Implemented(TraitRef {
                                        substitution,
                                        trait_id,
                                    })
                                }
                                WhereClause::AliasEq(alias_eq) => {
                                    let AliasEq { alias, ty: _ } = alias_eq;
                                    let alias = match alias {
                                        AliasTy::Opaque(opaque_ty) => {
                                            let OpaqueTy {
                                                ref substitution,
                                                opaque_ty_id,
                                            } = *opaque_ty;
                                            let substitution = self.generalize_substitution(
                                                variance,
                                                substitution,
                                                universe_index,
                                            );
                                            let substitution = match substitution {
                                                Ok(v) => v,
                                                Err(e) => {
                                                    error = Some(e);
                                                    return clause.clone();
                                                }
                                            };
                                            AliasTy::Opaque(OpaqueTy {
                                                substitution,
                                                opaque_ty_id,
                                            })
                                        }
                                        AliasTy::Projection(projection_ty) => {
                                            let ProjectionTy {
                                                ref substitution,
                                                associated_ty_id,
                                            } = *projection_ty;
                                            // TODO: We should be skipping "self", which
                                            // would be the first element of
                                            // "trait_params" if we had a
                                            // `RustIrDatabase` to call
                                            // `split_projection` on...
                                            // let (assoc_ty_datum, trait_params, assoc_type_params) = s.db().split_projection(&self);
                                            let substitution = self.generalize_substitution(
                                                variance,
                                                substitution,
                                                universe_index,
                                            );
                                            let substitution = match substitution {
                                                Ok(v) => v,
                                                Err(e) => {
                                                    error = Some(e);
                                                    return clause.clone();
                                                }
                                            };
                                            AliasTy::Projection(ProjectionTy {
                                                substitution,
                                                associated_ty_id,
                                            })
                                        }
                                    };
                                    let ty =
                                        self.table.new_variable(universe_index).to_ty(interner);
                                    WhereClause::AliasEq(AliasEq { alias, ty })
                                }
                                WhereClause::TypeOutlives(_) => {
                                    let lifetime_var = self.table.new_variable(universe_index);
                                    let lifetime = lifetime_var.to_lifetime(interner);
                                    let ty_var = self.table.new_variable(universe_index);
                                    let ty = ty_var.to_ty(interner);
                                    WhereClause::TypeOutlives(TypeOutlives {
                                        ty: ty,
                                        lifetime: lifetime,
                                    })
                                }
                                WhereClause::LifetimeOutlives(_) => {
                                    unreachable!("dyn Trait never contains LifetimeOutlive bounds")
                                }
                            }
                        })
                    });
                    QuantifiedWhereClauses::from_iter(interner, iter)
                });

                if let Some(error) = error {
                    return Err(error);
                }

                TyData::Dyn(DynTy { bounds, lifetime }).intern(interner)
            }
            TyData::Function(fn_ptr) => {
                let FnPointer {
                    num_binders,
                    abi,
                    safety,
                    variadic,
                    ref substitution,
                } = *fn_ptr;

                let substitution = FnSubst(self.generalize_substitution(
                    variance,
                    &substitution.0,
                    universe_index,
                )?);
                TyData::Function(FnPointer {
                    num_binders,
                    abi,
                    safety,
                    variadic,
                    substitution,
                })
                .intern(interner)
            }
            TyData::Placeholder(_) | TyData::BoundVar(_) => {
                debug!("just generalizing to the ty itself: {:?}", ty1);
                // BoundVar and PlaceHolder have no internal values to be
                // generic over, so we just relate directly to it
                ty1.clone()
            }
            TyData::Alias(_) | TyData::InferenceVar(_, _) => {
                unreachable!("relate_var_ty is not be called with ty = Alias or InferenceVar");
            }
        };

        debug!("var {:?} generalized to {:?}", var, generalized_val);

        self.table
            .unify
            .unify_var_value(
                var,
                InferenceValue::from_ty(interner, generalized_val.clone()),
            )
            .unwrap();
        debug!("var {:?} set to {:?}", var, generalized_val);

        Ok(())
    }

    fn relate_lifetime_lifetime(
        &mut self,
        variance: Variance,
        a: &Lifetime<I>,
        b: &Lifetime<I>,
    ) -> Fallible<()> {
        let interner = self.interner;

        let n_a = self.table.normalize_lifetime_shallow(interner, a);
        let n_b = self.table.normalize_lifetime_shallow(interner, b);
        let a = n_a.as_ref().unwrap_or(a);
        let b = n_b.as_ref().unwrap_or(b);

        debug_span!("relate_lifetime_lifetime", ?variance, ?a, ?b);

        match (a.data(interner), b.data(interner)) {
            (&LifetimeData::InferenceVar(var_a), &LifetimeData::InferenceVar(var_b)) => {
                let var_a = EnaVariable::from(var_a);
                let var_b = EnaVariable::from(var_b);
                debug!(?var_a, ?var_b);
                self.table.unify.unify_var_var(var_a, var_b).unwrap();
                Ok(())
            }

            (&LifetimeData::InferenceVar(a_var), &LifetimeData::Placeholder(b_idx)) => {
                self.unify_lifetime_var(variance, a, b, a_var, b, b_idx.ui)
            }

            (&LifetimeData::Placeholder(a_idx), &LifetimeData::InferenceVar(b_var)) => {
                self.unify_lifetime_var(variance, a, b, b_var, a, a_idx.ui)
            }

            (&LifetimeData::Placeholder(_), &LifetimeData::Placeholder(_)) => {
                if a != b {
                    Ok(self.push_lifetime_eq_goals(variance, a.clone(), b.clone()))
                } else {
                    Ok(())
                }
            }

            (LifetimeData::BoundVar(_), _) | (_, LifetimeData::BoundVar(_)) => panic!(
                "unification encountered bound variable: a={:?} b={:?}",
                a, b
            ),

            (LifetimeData::Phantom(..), _) | (_, LifetimeData::Phantom(..)) => unreachable!(),
        }
    }

    #[instrument(level = "debug", skip(self, a, b))]
    fn unify_lifetime_var(
        &mut self,
        variance: Variance,
        a: &Lifetime<I>,
        b: &Lifetime<I>,
        var: InferenceVar,
        value: &Lifetime<I>,
        value_ui: UniverseIndex,
    ) -> Fallible<()> {
        debug_span!(
            "unify_lifetime_var",
            ?variance,
            ?a,
            ?b,
            ?var,
            ?value,
            ?value_ui
        );
        let var = EnaVariable::from(var);
        let var_ui = self.table.universe_of_unbound_var(var);
        if var_ui.can_see(value_ui) {
            debug!("{:?} in {:?} can see {:?}; unifying", var, var_ui, value_ui);
            self.table
                .unify
                .unify_var_value(
                    var,
                    InferenceValue::from_lifetime(&self.interner, value.clone()),
                )
                .unwrap();
            Ok(())
        } else {
            debug!(
                "{:?} in {:?} cannot see {:?}; pushing constraint",
                var, var_ui, value_ui
            );
            Ok(self.push_lifetime_eq_goals(variance, a.clone(), b.clone()))
        }
    }

    fn relate_const_const<'a>(
        &mut self,
        variance: Variance,
        a: &'a Const<I>,
        b: &'a Const<I>,
    ) -> Fallible<()> {
        let interner = self.interner;

        let n_a = self.table.normalize_const_shallow(interner, a);
        let n_b = self.table.normalize_const_shallow(interner, b);
        let a = n_a.as_ref().unwrap_or(a);
        let b = n_b.as_ref().unwrap_or(b);

        debug_span!("relate_const_const", ?variance, ?a, ?b);

        let ConstData {
            ty: a_ty,
            value: a_val,
        } = a.data(interner);
        let ConstData {
            ty: b_ty,
            value: b_val,
        } = b.data(interner);

        self.relate_ty_ty(variance, a_ty, b_ty)?;

        match (a_val, b_val) {
            // Unifying two inference variables: unify them in the underlying
            // ena table.
            (&ConstValue::InferenceVar(var1), &ConstValue::InferenceVar(var2)) => {
                debug!(?var1, ?var2, "relate_ty_ty");
                let var1 = EnaVariable::from(var1);
                let var2 = EnaVariable::from(var2);
                Ok(self
                    .table
                    .unify
                    .unify_var_var(var1, var2)
                    .expect("unification of two unbound variables cannot fail"))
            }

            // Unifying an inference variables with a non-inference variable.
            (&ConstValue::InferenceVar(var), &ConstValue::Concrete(_))
            | (&ConstValue::InferenceVar(var), &ConstValue::Placeholder(_)) => {
                debug!(?var, ty=?b, "unify_var_ty");
                self.unify_var_const(var, b)
            }

            (&ConstValue::Concrete(_), &ConstValue::InferenceVar(var))
            | (&ConstValue::Placeholder(_), &ConstValue::InferenceVar(var)) => {
                debug!(?var, ty=?a, "unify_var_ty");
                self.unify_var_const(var, a)
            }

            (&ConstValue::Placeholder(p1), &ConstValue::Placeholder(p2)) => {
                Zip::zip_with(self, variance, &p1, &p2)
            }

            (&ConstValue::Concrete(ref ev1), &ConstValue::Concrete(ref ev2)) => {
                if ev1.const_eq(a_ty, ev2, interner) {
                    Ok(())
                } else {
                    Err(NoSolution)
                }
            }

            (&ConstValue::Concrete(_), &ConstValue::Placeholder(_))
            | (&ConstValue::Placeholder(_), &ConstValue::Concrete(_)) => Err(NoSolution),

            (ConstValue::BoundVar(_), _) | (_, ConstValue::BoundVar(_)) => panic!(
                "unification encountered bound variable: a={:?} b={:?}",
                a, b
            ),
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn unify_var_const(&mut self, var: InferenceVar, c: &Const<I>) -> Fallible<()> {
        debug_span!("unify_var_const", ?var, ?c);
        let interner = self.interner;
        let var = EnaVariable::from(var);

        self.table
            .unify
            .unify_var_value(var, InferenceValue::from_const(interner, c.clone()))
            .unwrap();
        debug!("unify_var_const: var {:?} set to {:?}", var, c);

        Ok(())
    }

    fn push_lifetime_eq_goals(&mut self, variance: Variance, a: Lifetime<I>, b: Lifetime<I>) {
        if matches!(variance, Variance::Invariant | Variance::Covariant) {
            self.goals.push(InEnvironment::new(
                self.environment,
                WhereClause::LifetimeOutlives(LifetimeOutlives {
                    a: a.clone(),
                    b: b.clone(),
                })
                .cast(self.interner),
            ));
        }
        if matches!(variance, Variance::Invariant | Variance::Contravariant) {
            self.goals.push(InEnvironment::new(
                self.environment,
                WhereClause::LifetimeOutlives(LifetimeOutlives { a: b, b: a }).cast(self.interner),
            ));
        }
    }

    fn push_subtype_goal(&mut self, a: Ty<I>, b: Ty<I>) {
        let subtype_goal = GoalData::SubtypeGoal(SubtypeGoal { a, b }).intern(self.interner());
        self.goals
            .push(InEnvironment::new(self.environment, subtype_goal));
    }
}

impl<'i, I: Interner> Zipper<'i, I> for Unifier<'i, I> {
    fn zip_tys(&mut self, variance: Variance, a: &Ty<I>, b: &Ty<I>) -> Fallible<()> {
        debug!("zip_tys {:?}, {:?}, {:?}", variance, a, b);
        self.relate_ty_ty(variance, a, b)
    }

    fn zip_lifetimes(
        &mut self,
        variance: Variance,
        a: &Lifetime<I>,
        b: &Lifetime<I>,
    ) -> Fallible<()> {
        self.relate_lifetime_lifetime(variance, a, b)
    }

    fn zip_consts(&mut self, variance: Variance, a: &Const<I>, b: &Const<I>) -> Fallible<()> {
        self.relate_const_const(variance, a, b)
    }

    fn zip_binders<T>(&mut self, variance: Variance, a: &Binders<T>, b: &Binders<T>) -> Fallible<()>
    where
        T: HasInterner<Interner = I> + Zip<I> + Fold<I, Result = T>,
    {
        // The binders that appear in types (apart from quantified types, which are
        // handled in `unify_ty`) appear as part of `dyn Trait` and `impl Trait` types.
        //
        // They come in two varieties:
        //
        // * The existential binder from `dyn Trait` / `impl Trait`
        //   (representing the hidden "self" type)
        // * The `for<..>` binders from higher-ranked traits.
        //
        // In both cases we can use the same `relate_binders` routine.

        self.relate_binders(variance, a, b)
    }

    fn interner(&self) -> &'i I {
        self.interner
    }

    fn unification_database(&self) -> &dyn UnificationDatabase<I> {
        self.db
    }
}

struct OccursCheck<'u, 't, I: Interner> {
    unifier: &'u mut Unifier<'t, I>,
    var: EnaVariable<I>,
    universe_index: UniverseIndex,
}

impl<'u, 't, I: Interner> OccursCheck<'u, 't, I> {
    fn new(
        unifier: &'u mut Unifier<'t, I>,
        var: EnaVariable<I>,
        universe_index: UniverseIndex,
    ) -> Self {
        OccursCheck {
            unifier,
            var,
            universe_index,
        }
    }
}

impl<'i, I: Interner> Folder<'i, I> for OccursCheck<'_, 'i, I>
where
    I: 'i,
{
    fn as_dyn(&mut self) -> &mut dyn Folder<'i, I> {
        self
    }

    fn fold_free_placeholder_ty(
        &mut self,
        universe: PlaceholderIndex,
        _outer_binder: DebruijnIndex,
    ) -> Fallible<Ty<I>> {
        let interner = self.interner();
        if self.universe_index < universe.ui {
            debug!(
                "OccursCheck aborting because self.universe_index ({:?}) < universe.ui ({:?})",
                self.universe_index, universe.ui
            );
            Err(NoSolution)
        } else {
            Ok(universe.to_ty(interner)) // no need to shift, not relative to depth
        }
    }

    #[instrument(level = "debug", skip(self))]
    fn fold_free_placeholder_lifetime(
        &mut self,
        ui: PlaceholderIndex,
        _outer_binder: DebruijnIndex,
    ) -> Fallible<Lifetime<I>> {
        debug_span!("fold_free_placeholder_lifetime", ?ui, ?_outer_binder);
        let interner = self.interner();
        if self.universe_index < ui.ui {
            // Scenario is like:
            //
            // exists<T> forall<'b> ?T = Foo<'b>
            //
            // unlike with a type variable, this **might** be
            // ok.  Ultimately it depends on whether the
            // `forall` also introduced relations to lifetimes
            // nameable in T. To handle that, we introduce a
            // fresh region variable `'x` in same universe as `T`
            // and add a side-constraint that `'x = 'b`:
            //
            // exists<'x> forall<'b> ?T = Foo<'x>, where 'x = 'b

            let tick_x = self.unifier.table.new_variable(self.universe_index);
            self.unifier.push_lifetime_eq_goals(
                Variance::Invariant,
                tick_x.to_lifetime(interner),
                ui.to_lifetime(interner),
            );
            Ok(tick_x.to_lifetime(interner))
        } else {
            // If the `ui` is higher than `self.universe_index`, then we can name
            // this lifetime, no problem.
            Ok(ui.to_lifetime(interner)) // no need to shift, not relative to depth
        }
    }

    fn fold_inference_ty(
        &mut self,
        var: InferenceVar,
        kind: TyKind,
        _outer_binder: DebruijnIndex,
    ) -> Fallible<Ty<I>> {
        let interner = self.interner();
        let var = EnaVariable::from(var);
        match self.unifier.table.unify.probe_value(var) {
            // If this variable already has a value, fold over that value instead.
            InferenceValue::Bound(normalized_ty) => {
                let normalized_ty = normalized_ty.assert_ty_ref(interner);
                let normalized_ty = normalized_ty.fold_with(self, DebruijnIndex::INNERMOST)?;
                assert!(!normalized_ty.needs_shift(interner));
                Ok(normalized_ty)
            }

            // Otherwise, check the universe of the variable, and also
            // check for cycles with `self.var` (which this will soon
            // become the value of).
            InferenceValue::Unbound(ui) => {
                if self.unifier.table.unify.unioned(var, self.var) {
                    return Err(NoSolution);
                }

                if self.universe_index < ui {
                    // Scenario is like:
                    //
                    // ?A = foo(?B)
                    //
                    // where ?A is in universe 0 and ?B is in universe 1.
                    // This is OK, if ?B is promoted to universe 0.
                    self.unifier
                        .table
                        .unify
                        .unify_var_value(var, InferenceValue::Unbound(self.universe_index))
                        .unwrap();
                }

                Ok(var.to_ty_with_kind(interner, kind))
            }
        }
    }

    fn fold_inference_lifetime(
        &mut self,
        var: InferenceVar,
        outer_binder: DebruijnIndex,
    ) -> Fallible<Lifetime<I>> {
        // a free existentially bound region; find the
        // inference variable it corresponds to
        let interner = self.interner();
        let var = EnaVariable::from(var);
        match self.unifier.table.unify.probe_value(var) {
            InferenceValue::Unbound(ui) => {
                if self.universe_index < ui {
                    // Scenario is like:
                    //
                    // exists<T> forall<'b> exists<'a> ?T = Foo<'a>
                    //
                    // where ?A is in universe 0 and `'b` is in universe 1.
                    // This is OK, if `'b` is promoted to universe 0.
                    self.unifier
                        .table
                        .unify
                        .unify_var_value(var, InferenceValue::Unbound(self.universe_index))
                        .unwrap();
                }
                Ok(var.to_lifetime(interner))
            }

            InferenceValue::Bound(l) => {
                let l = l.assert_lifetime_ref(interner);
                let l = l.fold_with(self, outer_binder)?;
                assert!(!l.needs_shift(interner));
                Ok(l)
            }
        }
    }

    fn forbid_free_vars(&self) -> bool {
        true
    }

    fn interner(&self) -> &'i I {
        self.unifier.interner
    }

    fn target_interner(&self) -> &'i I {
        self.interner()
    }
}
