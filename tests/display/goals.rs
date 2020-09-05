#[test]
fn test_well_formed_goal() {
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
fn test_normalize_goal() {
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
fn test_is_local_goal() {
    reparse_goal_test! {
        goal {
            IsLocal(u32)
        }
    }
}

#[test]
fn test_is_upstream_goal() {
    reparse_goal_test! {
        goal {
            IsUpstream(u32)
        }
    }
}
#[test]
fn test_is_fully_visible_goal() {
    reparse_goal_test! {
        goal {
            IsFullyVisible(u32)
        }
    }
}
#[test]
fn test_local_impl_allowed() {
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
fn test_compatible_allowed() {
    reparse_goal_test! {
        goal {
            Compatible
        }
    }
}
#[test]
fn test_reveal_allowed() {
    reparse_goal_test! {
        goal {
            Reveal
        }
    }
}

#[test]
fn test_forall_goal() {
    reparse_goal_test! {
        goal {
            forall<'a, T> { WellFormed(&'a T) }
        }
    }
}

#[test]
fn test_not_goal() {
    reparse_goal_test! {
        goal {
            not { WellFormed(u32) }
        }
    }
}

#[test]
fn test_implies_goal() {
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
