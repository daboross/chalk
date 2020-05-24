use std::sync::Arc;

use chalk_solve::{SolverChoice, logging_db::LoggingRustIrDatabase, Solution};
use chalk_integration::{lowering::LowerGoal, db::ChalkDatabase, interner::ChalkIr, query::LoweringDatabase, program::Program};
use chalk_solve::ext::*;
use chalk_solve::RustIrDatabase;

use crate::test_util::assert_same;
use crate::test_util::test::{TestGoal, assert_result};

macro_rules! logging_db_output_sufficient {
    ($($arg:tt)*) => {
        use chalk_solve::SolverChoice;
        use crate::test_util::test::*;
        let (program, goals) = parse_test_data!($($arg)*);
        crate::test_util::logging_db::logging_db_output_sufficient(program, goals)
    };
}

pub fn logging_db_output_sufficient(program_text: &str, goals: Vec<(&str, SolverChoice, TestGoal)>) {
    println!("program {}", program_text);
    assert!(program_text.starts_with("{"));
    assert!(program_text.ends_with("}"));

    let output_text = {
        let db = ChalkDatabase::with(
            &program_text[1..program_text.len() - 1],
            SolverChoice::default(),
        );

        let program = db.checked_program().unwrap();
        let wrapped = LoggingRustIrDatabase::<_,Program,_>::new(program.clone());
        for (goal_text, solver_choice, expected) in goals.clone() {
            let mut solver = solver_choice.into_solver();

            chalk_integration::tls::set_current_program(&program, || {
                println!("----------------------------------------------------------------------");
                println!("---- first run on original test code ---------------------------------");
                println!("goal {}", goal_text);
                assert!(goal_text.starts_with("{"));
                assert!(goal_text.ends_with("}"));
                let goal = chalk_parse::parse_goal(&goal_text[1..goal_text.len() - 1])
                    .unwrap()
                    .lower(&*program)
                    .unwrap();

                println!("using solver: {:?}", solver_choice);
                let peeled_goal = goal.into_peeled_goal(db.interner());
                match expected {
                    TestGoal::Aggregated(expected) => {
                        let result = solver.solve(&wrapped, &peeled_goal);
                        assert_result(result, expected);
                    }
                    _ => panic!("only aggregated test goals supported for logger goals"),
                }
            });
        }

        wrapped.to_string()
    };
    
    println!("----------------------------------------------------------------------");
    println!("logging db output program:\n{}\n", output_text);
    
    let db = ChalkDatabase::with(
        &output_text,
        SolverChoice::default(),
    );

    let program = db.checked_program().unwrap();

    for (goal_text, solver_choice, expected) in goals {
        let solver = solver_choice.into_solver::<ChalkIr>();

        chalk_integration::tls::set_current_program(&program, || {
            println!("----------------------------------------------------------------------");
            println!("---- second run on original logger output code -----------------------");
            println!("goal {}", goal_text);
            assert!(goal_text.starts_with("{"));
            assert!(goal_text.ends_with("}"));
            let goal = chalk_parse::parse_goal(&goal_text[1..goal_text.len() - 1])
                .unwrap()
                .lower(&*program)
                .unwrap();

            println!("using solver: {:?}", solver_choice);
            let peeled_goal = goal.into_peeled_goal(db.interner());
            match expected {
                TestGoal::Aggregated(expected) => {
                    let result = solver.solve(&db, &peeled_goal);
                    assert_result(result, expected);
                }
                _ => panic!("only aggregated test goals supported for logger goals"),
            }
        });
    }
}
