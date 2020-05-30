//! Utilities and macros for use in display tests.
//!
//! This can't live as a submodule of `test_util.rs`, as then it would conflict
//! with `display/mod.rs` for the name `mod display` when `test_util.rs` is
//! compiled as a standalone test (rather than from `lib.rs`).
use chalk_integration::{program::Program, query::LoweringDatabase, tls};
use chalk_solve::display::write_items;
use regex::Regex;
use std::{fmt::Debug, sync::Arc};

pub fn strip_leading_trailing_braces(input: &str) -> &str {
    assert!(input.starts_with("{"));
    assert!(input.ends_with("}"));

    &input[1..input.len() - 1]
}

macro_rules! reparse_test {
    (program $program:tt) => {
        crate::display::util::reparse_test(crate::display::util::strip_leading_trailing_braces(
            stringify!($program),
        ))
    };
    (program $program:tt produces $diff:tt) => {
        crate::display::util::reparse_into_different_test(
            crate::display::util::strip_leading_trailing_braces(stringify!($program)),
            crate::display::util::strip_leading_trailing_braces(stringify!($diff)),
        )
    };
    (program $program:tt formatting matches $res:literal) => {
        crate::display::util::test_formatting(
            crate::display::util::strip_leading_trailing_braces(stringify!($program)),
            $res,
        )
    };
}

/// Sends all items in a `chalk_integration::Program` through `display` code and
/// returns the string representing the program.
pub fn write_program(program: &Program) -> String {
    let mut out = String::new();
    let ids = std::iter::empty()
        .chain(program.adt_data.keys().copied().map(Into::into))
        .chain(program.trait_data.keys().copied().map(Into::into))
        .chain(program.impl_data.keys().copied().map(Into::into))
        .chain(program.opaque_ty_data.keys().copied().map(Into::into));
    write_items(&mut out, program, ids).unwrap();
    out
}

fn program_diff(original: &impl Debug, produced: &impl Debug) -> String {
    use std::fmt::Write;

    let mut out = String::new();
    let original = format!("{:#?}", original);
    let produced = format!("{:#?}", produced);
    for line in diff::lines(&original, &produced) {
        match line {
            diff::Result::Left(l) => write!(out, "-{}\n", l),
            diff::Result::Both(l, _) => write!(out, " {}\n", l),
            diff::Result::Right(r) => write!(out, "+{}\n", r),
        }
        .expect("writing to string never fails");
    }
    out
}

/// Data from performing a reparse test which can be used to make additional
/// assertions.
///
/// Not necessary for use unless additional assertions are necessary.
#[allow(unused)]
pub struct ReparseTestResult<'a> {
    /// The program text for the original test code
    pub original_text: &'a str,
    /// The program text for the code the test says should be output
    pub target_text: &'a str,
    /// The actual reparsed output text
    pub output_text: String,
    /// Lowered version of `original_text`
    pub original_program: Arc<Program>,
    /// Lowered version of `target_text`
    pub target_program: Arc<Program>,
    /// Lowered version of `output_text`
    pub output_program: Arc<Program>,
}

/// Parses the input, lowers it, prints it, then re-parses and re-lowers,
/// failing if the two lowered programs don't match.
///
/// Note: the comparison here does include IDs, so input order matters. In
/// particular, ProgramWriter always writes traits, then structs, then
/// impls. So all traits must come first, then structs, then all impls, or
/// the reparse will fail.
pub fn reparse_test(program_text: &str) -> ReparseTestResult<'_> {
    reparse_into_different_test(program_text, program_text)
}

/// [`reparse_test`], but allows a non-convergent test program to be tested
/// a different target.
pub fn reparse_into_different_test<'a>(
    program_text: &'a str,
    target_text: &'a str,
) -> ReparseTestResult<'a> {
    let original_db = chalk_integration::db::ChalkDatabase::with(program_text, <_>::default());
    let original_program = original_db.program_ir().unwrap_or_else(|e| {
        panic!(
            "unable to lower test program:\n{}\nSource:\n{}\n",
            e, program_text
        )
    });
    let target_db = chalk_integration::db::ChalkDatabase::with(target_text, <_>::default());
    let target_program = target_db.program_ir().unwrap_or_else(|e| {
        panic!(
            "unable to lower test program:\n{}\nSource:\n{}\n",
            e, program_text
        )
    });
    let output_text =
        tls::set_current_program(&original_program, || write_program(&original_program));
    let output_db = chalk_integration::db::ChalkDatabase::with(&output_text, <_>::default());
    let output_program = output_db.program_ir().unwrap_or_else(|e| {
        panic!(
            "error lowering writer output:\n{}\nNew source:\n{}\n",
            e, output_text
        )
    });
    if output_program != target_program {
        panic!(
            "WriteProgram produced different program.\n\
             Diff:\n{}\n\
             Source:\n{}\n{}\
             New Source:\n{}\n",
            program_diff(&target_program, &output_program),
            program_text,
            if target_text != program_text {
                format!(
                    "Test Should Output (different from original):\n{}\n",
                    target_text
                )
            } else {
                String::new()
            },
            output_text
        );
    }
    eprintln!("\nTest Succeeded:\n\n{}\n---", output_text);
    ReparseTestResult {
        original_text: program_text,
        output_text,
        target_text,
        original_program,
        output_program,
        target_program,
    }
}

pub fn test_formatting(src: &str, acceptable: &str) {
    let result = reparse_test(src);
    let acceptable = Regex::new(acceptable).unwrap();
    if !acceptable.is_match(&result.output_text) {
        panic!(
            "output_text's formatting didn't match the criteria.\
            \noutput_text:\n\"{0}\"\
            \ncriteria:\n\"{1}\"\
            \ndebug output: {0:?}\
            \ndebug criteria: {2:?}\n",
            result.output_text,
            acceptable,
            acceptable.as_str()
        );
    }
}
