#[test]
fn records_struct() {
    logging_db_output_sufficient! {
        program {
            struct S {}

            trait Trait {}

            impl Trait for S {}
        }

        goal {
            S: Trait
        } yields {
            "Unique"
        }
    }
}
