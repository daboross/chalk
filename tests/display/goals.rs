#[test]
fn test_well_formed_goal() {
    // Test printing WellFormed domain goal
    reparse_goal_test! {
        program {
            trait Foo { }
            impl Foo for u32 { }
        }
        goal {
            WellFormed(u32),
            WellFormed(u32 : Foo)
        }
    }
}

#[test]
fn test_from_env_goal() {
    // Test printing FromEnv domain goal
    reparse_goal_test! {
        program {
            trait Foo { }
            impl Foo for u32 { }
        }
        goal {
            FromEnv(u32),
            FromEnv(u32 : Foo)
        }
    }
}

#[test]
fn test_is_local_goal() {
    // Test printing IsLocal domain goal
    reparse_goal_test! {
        goal {
            IsLocal(u32)
        }
    }
}

#[test]
fn test_is_upstream_goal() {
    // Test printing IsUpstream domain goal
    reparse_goal_test! {
        goal {
            IsUpstream(u32)
        }
    }
}
#[test]
fn test_is_fully_visible_goal() {
    // Test printing IsFullyVisible domain goal
    reparse_goal_test! {
        goal {
            IsFullyVisible(u32)
        }
    }
}
#[test]
fn test_local_impl_allowed_goal() {
    // Test printing LocalImplAllowed domain goal
    reparse_goal_test! {
        program {
            trait Foo { }
        }
        goal {
            LocalImplAllowed(u32: Foo)
        }
    }
}
#[test]
fn test_compatible_goal() {
    // Test printing Compatible domain goal
    reparse_goal_test! {
        goal {
            Compatible
        }
    }
}
#[test]
fn test_reveal_goal() {
    // Test printing Reveal domain goal
    reparse_goal_test! {
        goal {
            Reveal
        }
    }
}

#[test]
fn test_normalize_goal() {
    // Test printing Normalize domain goal
    reparse_goal_test! {
        program {
            trait Foo {
                type Assoc;
            }
            impl Foo for u32 { }
        }
        goal {
            Normalize(<u32 as Foo>::Assoc -> i32)
        }
    }
}

#[test]
fn test_object_safe_goal() {
    reparse_goal_test! {
        program {
            trait Foo {
            }
        }
        goal {
            ObjectSafe(Foo)
        }
    }
}

#[test]
fn test_forall_goal() {
    // Test printing forall goal
    reparse_goal_test! {
        goal {
            forall<'a, T> { WellFormed(&'a T) }
        }
    }
}

#[test]
fn test_not_goal() {
    // Test printing not goal
    reparse_goal_test! {
        goal {
            not { WellFormed(u32) }
        }
    }
}

#[test]
fn test_implies_goal() {
    // Test printing implies goal
    reparse_goal_test! {
        program {
            trait Foo { }
        }
        goal {
            exists<'a,'b,T> {
                if ('a : 'b) {
                    WellFormed(&'a T),
                    WellFormed(&'b T)
                },
                if ('a : 'b; T: Foo) {
                    WellFormed(&'a T),
                    WellFormed(&'b T)
                },
                if (forall<'c> { 'c : 'a }; WellFormed(&'a T) :- WellFormed(&'b T)) {
                    WellFormed(&'a T)
                }
            }
        }
    }
}

#[test]
fn test_exists_goal() {
    // Test printing exists goal
    reparse_goal_test! {
        goal {
            exists<'a,T,E> {
                exists<G> {
                    T = &'a G
                },
                exists<'b> {
                    T = &'b E
                }
            }
        }
    }
}

#[test]
fn test_unify_goal() {
    // Test printing unify goal
    reparse_goal_test! {
        goal {
            exists<A,B> {
                A = B
            }
        }
    }
}

#[test]
fn test_where_clause() {
    // Test printing where-clause goal
    reparse_goal_test! {
        program {
            trait ATrait { }
            trait BTrait{
                type Assoc;
            }
        }
        goal {
            exists<A,B,'a,'b> {
                'a: 'b,
                A: 'a,
                A: ATrait,
                B: BTrait<Assoc = A>
            }
        }
        produces {
            exists<A,B,'a,'b> {
                'a: 'b,
                A: 'a,
                A: ATrait,
                B: BTrait<Assoc = A>,
                B: BTrait
            }
        }
    }
}

#[test]
fn test_and_goal() {
    // Test printing And goal
    reparse_goal_test! {
        goal {
            WellFormed(u32), WellFormed(i32), Compatible
        }
    }
}
