use super::{render_trait::RenderAsRust, state::InternalWriterState};
use chalk_ir::{interner::Interner, DomainGoal, FromEnv, Normalize, WellFormed};
use chalk_ir::{Goal, GoalData, ProgramClause, QuantifierKind};
use itertools::Itertools;
use std::fmt::Write;
use std::fmt::{Formatter, Result};

impl<I: Interner> RenderAsRust<I> for Goal<I> {
    fn fmt(&self, s: &InternalWriterState<'_, I>, f: &mut Formatter<'_>) -> Result {
        let interner = s.db().interner();
        let goal_data = self.data(interner);
        match goal_data {
            GoalData::Not(goal) => write!(f, "not {{ {} }}", goal.display(s)),
            GoalData::All(goals) => write!(
                f,
                "{}",
                goals
                    .iter(interner)
                    .map(|goal| { goal.display(s) })
                    .format(", ")
            ),
            GoalData::Quantified(kind, binders) => {
                // quantified kind
                // exists<T> { WellFormed(Vec<T>) }
                // ^^^^^^
                let kind = match kind {
                    QuantifierKind::ForAll => "forall",
                    QuantifierKind::Exists => "exists",
                };
                write!(f, "{}", kind)?;

                // generic binders
                // exists<T,B> { Subtype(Vec<T>,Vec<B>) }
                //       ^^^^^
                let s = &s.add_debrujin_index(None);
                let display_binders = s.binder_var_display(&binders.binders);
                let value = binders.skip_binders();
                write_joined_non_empty_list!(f, "<{}>", display_binders, ", ")?;

                // body
                // exists<T,B> { Subtype(Vec<T>,Vec<B>) }
                //             ^^^^^^^^^^^^^^^^^^^^^^^^^^
                write!(f, " {{ {} }}", value.display(s))
            }
            GoalData::Implies(hypothetical, goal) => {
                write!(
                    f,
                    "if ({}) ",
                    hypothetical
                        .iter(interner)
                        .map(|clause| clause.display(s))
                        .format("; ")
                )?;
                write!(f, "{{ {} }}", goal.display(s))
            }
            GoalData::EqGoal(eq_goal) => write!(
                f,
                "{} = {}",
                eq_goal.a.data(interner).display(s),
                eq_goal.b.data(interner).display(s)
            ),
            GoalData::DomainGoal(domain_goal) => write!(f, "{}", domain_goal.display(s)),
            // make unreachable?
            GoalData::CannotProve => write!(f, "{{cannot prove}}"),
        }
    }
}

impl<I: Interner> RenderAsRust<I> for ProgramClause<I> {
    fn fmt(&self, s: &InternalWriterState<'_, I>, f: &mut Formatter<'_>) -> Result {
        let interner = s.db().interner();
        let clause_data = self.data(interner);

        let s = &s.add_debrujin_index(None);
        let binders = &clause_data.0.binders;
        let value = clause_data.0.skip_binders();

        // the clause, which may get wrapped in a forall
        let mut st = String::new();
        write!(st, "{}", value.consequence.display(s))?;
        if !value.conditions.is_empty(interner) {
            write!(
                st,
                " :- {}",
                value
                    .conditions
                    .iter(interner)
                    .map(|goal| goal.display(s))
                    .format(",")
            )?;
        }
        // surrounding forall
        if !binders.is_empty(interner) {
            write!(f, "forall")?;
            let binders_display = s.binder_var_display(binders);
            write_joined_non_empty_list!(f, "<{}>", binders_display, ", ")?;
            write!(f, " {{ {} }}", st)
        } else {
            write!(f, "{}", st)
        }
    }
}

impl<I: Interner> RenderAsRust<I> for DomainGoal<I> {
    fn fmt(&self, s: &InternalWriterState<'_, I>, f: &mut Formatter<'_>) -> Result {
        match self {
            DomainGoal::Holds(wc) => write!(f, "{}", wc.display(s)),
            DomainGoal::WellFormed(wformed) => match wformed {
                WellFormed::Ty(ty) => write!(f, "WellFormed({})", ty.display(s)),
                WellFormed::Trait(traitref) => write!(f, "WellFormed({})", traitref.display(s)),
            },
            DomainGoal::FromEnv(fromenv) => match fromenv {
                FromEnv::Ty(ty) => write!(f, "FromEnv({})", ty.display(s)),
                FromEnv::Trait(traitref) => write!(f, "FromEnv({})", traitref.display(s)),
            },
            DomainGoal::IsUpstream(ty) => write!(f, "IsUpstream({})", ty.display(s)),
            DomainGoal::IsLocal(ty) => write!(f, "IsLocal({})", ty.display(s)),
            DomainGoal::IsFullyVisible(ty) => write!(f, "IsFullyVisible({})", ty.display(s)),
            DomainGoal::DownstreamType(ty) => write!(f, "DownstreamType({})", ty.display(s)),
            DomainGoal::Compatible => write!(f, "Compatible"),
            DomainGoal::Reveal => write!(f, "Reveal"),
            DomainGoal::Normalize(normalize) => write!(f, "Normalize({})", normalize.display(s)),
            DomainGoal::ObjectSafe(id) => write!(f, "ObjectSafe({})", id.display(s)),
            DomainGoal::LocalImplAllowed(traitref) => {
                write!(f, "LocalImplAllowed({})", traitref.display(s))
            }
        }
    }
}

impl<I: Interner> RenderAsRust<I> for Normalize<I> {
    fn fmt(&self, s: &InternalWriterState<'_, I>, f: &mut Formatter<'_>) -> Result {
        write!(f, "{} -> {}", self.alias.display(s), self.ty.display(s))
    }
}
