use proptest::prelude::*;
use proptest::test_runner::Config as ProptestConfig;
use rtfs::ast::Keyword;
use rtfs::ir::core::{IrMapTypeEntry, IrType};
use rtfs::ir::type_checker::{is_subtype, type_join};

fn arb_key_type() -> impl Strategy<Value = IrType> {
    prop_oneof![
        Just(IrType::String),
        Just(IrType::Keyword),
        Just(IrType::Any),
        Just(IrType::Union(vec![IrType::String, IrType::Keyword])),
    ]
}

fn arb_ir_type() -> impl Strategy<Value = IrType> {
    let leaf = prop_oneof![
        Just(IrType::Int),
        Just(IrType::Float),
        Just(IrType::String),
        Just(IrType::Bool),
        Just(IrType::Nil),
        Just(IrType::Keyword),
        Just(IrType::Symbol),
        Just(IrType::Any),
        Just(IrType::Never),
    ];

    leaf.prop_recursive(
        4,  // depth
        64, // max size
        8,  // items per collection
        |inner| {
            prop_oneof![
                inner.clone().prop_map(|t| IrType::Vector(Box::new(t))),
                inner.clone().prop_map(|t| IrType::List(Box::new(t))),
                prop::collection::vec(inner.clone(), 0..=3).prop_map(IrType::Tuple),
                // Unions/intersections (small, non-empty)
                prop::collection::vec(inner.clone(), 1..=3).prop_map(IrType::Union),
                prop::collection::vec(inner.clone(), 1..=3).prop_map(IrType::Intersection),
                // Simple first-order functions (fixed arity, no variadic) to keep tests tractable
                (prop::collection::vec(inner.clone(), 0..=3), inner.clone()).prop_map(
                    |(params, ret)| IrType::Function {
                        param_types: params,
                        variadic_param_type: None,
                        return_type: Box::new(ret),
                    }
                ),
                // Structural record maps (keyword fields)
                (prop::collection::vec((0u8..=8u8, inner.clone(), any::<bool>()), 0..=4)).prop_map(
                    |entries| {
                        let mut map_entries = Vec::new();
                        for (k, t, optional) in entries {
                            map_entries.push(IrMapTypeEntry {
                                key: Keyword::new(&format!("k{}", k)),
                                value_type: t,
                                optional,
                            });
                        }
                        IrType::Map {
                            entries: map_entries,
                            wildcard: None,
                        }
                    }
                ),
                // Parametric dictionary maps
                (arb_key_type(), inner.clone()).prop_map(|(k, v)| IrType::ParametricMap {
                    key_type: Box::new(k),
                    value_type: Box::new(v),
                }),
            ]
        },
    )
}

proptest! {
    #![proptest_config(ProptestConfig {
        // Do not write `.proptest-regressions` files into the repo.
        failure_persistence: None,
        .. ProptestConfig::default()
    })]
    #[test]
    fn prop_subtype_reflexive(t in arb_ir_type()) {
        prop_assert!(is_subtype(&t, &t));
    }

    #[test]
    fn prop_any_is_top(t in arb_ir_type()) {
        prop_assert!(is_subtype(&t, &IrType::Any));
    }

    #[test]
    fn prop_never_is_bottom(t in arb_ir_type()) {
        prop_assert!(is_subtype(&IrType::Never, &t));
    }

    #[test]
    fn prop_subtype_transitive_constructed(a in arb_ir_type(), x in arb_ir_type(), y in arb_ir_type()) {
        // Avoid "too many rejects" by constructing supertypes using join (LUB).
        let b = type_join(&a, &x);
        let c = type_join(&b, &y);

        prop_assert!(is_subtype(&a, &b));
        prop_assert!(is_subtype(&b, &c));
        prop_assert!(is_subtype(&a, &c));
    }

    #[test]
    fn prop_union_right_intro(a in arb_ir_type(), b in arb_ir_type()) {
        let u = IrType::Union(vec![a.clone(), b]);
        prop_assert!(is_subtype(&a, &u));
    }

    #[test]
    fn prop_intersection_right_intro(a in arb_ir_type()) {
        let i = IrType::Intersection(vec![a.clone(), IrType::Any]);
        prop_assert!(is_subtype(&a, &i));
    }

    #[test]
    fn prop_vector_covariant_constructed(a in arb_ir_type(), x in arb_ir_type()) {
        // Construct an element supertype to avoid rejection-heavy assumptions.
        let b = type_join(&a, &x);

        let va = IrType::Vector(Box::new(a));
        let vb = IrType::Vector(Box::new(b));
        prop_assert!(is_subtype(&va, &vb));
    }

    #[test]
    fn prop_join_is_upper_bound(a in arb_ir_type(), b in arb_ir_type()) {
        let j = type_join(&a, &b);
        prop_assert!(is_subtype(&a, &j));
        prop_assert!(is_subtype(&b, &j));
    }
}
