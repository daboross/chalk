use super::*;
#[test]
fn test_simple_traits_and_bounds() {
    reparse_test!(
        program {
            trait Foo {}
        }
    );
    reparse_test!(
        program {
            trait Foo<T> {}
        }
    );
    reparse_test!(
        program {
            trait Foo<T> where T: Trait {}
            trait Trait {}
        }
    );
}

#[test]
fn test_basic_trait_impl() {
    reparse_test!(
        program {
            struct Foo { }
            trait Bar {}
            impl Bar for Foo { }
        }
    );
}

#[test]
fn test_trait_flags() {
    let flags = vec![
        "auto",
        "marker",
        "upstream",
        "fundamental",
        "non_enumerable",
        "coinductive",
    ];
    reparse_test(&format!(
        "{}trait Hello {{}}",
        flags
            .iter()
            .map(|f| format!("#[{}]", f))
            .collect::<Vec<_>>()
            .join("\n")
    ));
    for flag in flags {
        reparse_test(&format!(
            "
            #[{0}]
            trait Hello_{0} {{}}
            ",
            flag
        ));
    }
}

#[test]
fn test_wellknown_traits() {
    let well_knowns = vec![
        "sized", "copy", "clone", "drop", "fn_once", "fn_mut", "fn", "unsize",
    ];
    for flag in well_knowns {
        reparse_test(&format!(
            "
            #[lang({0})]
            trait Hello_{0} {{}}
            ",
            flag
        ));
    }
    reparse_test!(
        program {
            #[auto]
            #[lang(sized)]
            trait Foo {

            }
        }
    );
}
