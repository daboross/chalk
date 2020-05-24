#![allow(non_snake_case)]

use chalk_solve::SolverChoice;
#[cfg(feature = "bench")]
mod bench;
mod coherence;
mod wf_lowering;

pub use super::test_util::{test::*, *};

mod arrays;
mod auto_traits;
mod coherence_goals;
mod coinduction;
mod constants;
mod cycle;
mod existential_types;
mod fn_def;
mod functions;
mod implied_bounds;
mod impls;
mod misc;
mod negation;
mod never;
mod numerics;
mod object_safe;
mod opaque_types;
mod projection;
mod refs;
mod scalars;
mod slices;
mod string;
mod tuples;
mod unify;
mod wf_goals;
