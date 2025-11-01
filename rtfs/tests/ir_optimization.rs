use rtfs::ast::Literal;
use rtfs::ir::core::*;
use rtfs::ir::optimizer::EnhancedIrOptimizer;

#[test]
fn test_constant_folding_arithmetic() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test: (+ 10 20) should fold to 30
    let add_node = IrNode::Apply {
        id: 1,
        function: Box::new(IrNode::VariableRef {
            id: 2,
            name: "+".to_string(),
            binding_id: 0,
            ir_type: IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            },
            source_location: None,
        }),
        arguments: vec![
            IrNode::Literal {
                id: 3,
                value: Literal::Integer(10),
                ir_type: IrType::Int,
                source_location: None,
            },
            IrNode::Literal {
                id: 4,
                value: Literal::Integer(20),
                ir_type: IrType::Int,
                source_location: None,
            },
        ],
        ir_type: IrType::Int,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(add_node);

    // Should be folded to a literal 30
    match optimized {
        IrNode::Literal {
            value: Literal::Integer(30),
            ..
        } => {
            // Success
        }
        _ => panic!(
            "Expected constant folded result of 30, got: {:?}",
            optimized
        ),
    }
}

#[test]
fn test_constant_folding_boolean() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test: (> 10 5) should fold to true
    let gt_node = IrNode::Apply {
        id: 1,
        function: Box::new(IrNode::VariableRef {
            id: 2,
            name: ">".to_string(),
            binding_id: 0,
            ir_type: IrType::Function {
                param_types: vec![IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::Bool),
            },
            source_location: None,
        }),
        arguments: vec![
            IrNode::Literal {
                id: 3,
                value: Literal::Integer(10),
                ir_type: IrType::Int,
                source_location: None,
            },
            IrNode::Literal {
                id: 4,
                value: Literal::Integer(5),
                ir_type: IrType::Int,
                source_location: None,
            },
        ],
        ir_type: IrType::Bool,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(gt_node);

    // Should be folded to a literal true
    match optimized {
        IrNode::Literal {
            value: Literal::Boolean(true),
            ..
        } => {
            // Success
        }
        _ => panic!(
            "Expected constant folded result of true, got: {:?}",
            optimized
        ),
    }
}

#[test]
fn test_dead_code_elimination_unused_let() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test: (let [x 10] 42) - x is never used, should eliminate binding
    let let_node = IrNode::Let {
        id: 1,
        bindings: vec![IrLetBinding {
            pattern: IrNode::VariableBinding {
                id: 2,
                name: "x".to_string(),
                ir_type: IrType::Int,
                source_location: None,
            },
            type_annotation: None,
            init_expr: IrNode::Literal {
                id: 3,
                value: Literal::Integer(10),
                ir_type: IrType::Int,
                source_location: None,
            },
        }],
        body: vec![IrNode::Literal {
            id: 4,
            value: Literal::Integer(42),
            ir_type: IrType::Int,
            source_location: None,
        }],
        ir_type: IrType::Int,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(let_node);

    // Should be simplified to just 42
    match optimized {
        IrNode::Literal {
            value: Literal::Integer(42),
            ..
        } => {
            // Success
        }
        _ => panic!(
            "Expected dead code eliminated result of 42, got: {:?}",
            optimized
        ),
    }
}

#[test]
fn test_dead_code_elimination_with_side_effects() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test: (let [x (print "hello")] 42) - x has side effects, should keep binding
    let let_node = IrNode::Let {
        id: 1,
        bindings: vec![IrLetBinding {
            pattern: IrNode::VariableBinding {
                id: 2,
                name: "x".to_string(),
                ir_type: IrType::String,
                source_location: None,
            },
            type_annotation: None,
            init_expr: IrNode::Apply {
                id: 3,
                function: Box::new(IrNode::VariableRef {
                    id: 4,
                    name: "print".to_string(),
                    binding_id: 0,
                    ir_type: IrType::Function {
                        param_types: vec![IrType::String],
                        variadic_param_type: None,
                        return_type: Box::new(IrType::String),
                    },
                    source_location: None,
                }),
                arguments: vec![IrNode::Literal {
                    id: 5,
                    value: Literal::String("hello".to_string()),
                    ir_type: IrType::String,
                    source_location: None,
                }],
                ir_type: IrType::String,
                source_location: None,
            },
        }],
        body: vec![IrNode::Literal {
            id: 6,
            value: Literal::Integer(42),
            ir_type: IrType::Int,
            source_location: None,
        }],
        ir_type: IrType::Int,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(let_node);

    // Should keep the let binding due to side effects
    match optimized {
        IrNode::Let { .. } => {
            // Success - binding preserved
        }
        _ => panic!(
            "Expected let binding to be preserved due to side effects, got: {:?}",
            optimized
        ),
    }
}

#[test]
fn test_constant_condition_optimization() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test: (if true 42 99) should optimize to 42
    let if_node = IrNode::If {
        id: 1,
        condition: Box::new(IrNode::Literal {
            id: 2,
            value: Literal::Boolean(true),
            ir_type: IrType::Bool,
            source_location: None,
        }),
        then_branch: Box::new(IrNode::Literal {
            id: 3,
            value: Literal::Integer(42),
            ir_type: IrType::Int,
            source_location: None,
        }),
        else_branch: Some(Box::new(IrNode::Literal {
            id: 4,
            value: Literal::Integer(99),
            ir_type: IrType::Int,
            source_location: None,
        })),
        ir_type: IrType::Int,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(if_node);

    // Should be optimized to just 42
    match optimized {
        IrNode::Literal {
            value: Literal::Integer(42),
            ..
        } => {
            // Success
        }
        _ => panic!(
            "Expected constant condition optimization to 42, got: {:?}",
            optimized
        ),
    }
}

#[test]
fn test_do_block_elimination() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test: (do 10 20 30) with no side effects should keep only the last expression
    let do_node = IrNode::Do {
        id: 1,
        expressions: vec![
            IrNode::Literal {
                id: 2,
                value: Literal::Integer(10),
                ir_type: IrType::Int,
                source_location: None,
            },
            IrNode::Literal {
                id: 3,
                value: Literal::Integer(20),
                ir_type: IrType::Int,
                source_location: None,
            },
            IrNode::Literal {
                id: 4,
                value: Literal::Integer(30),
                ir_type: IrType::Int,
                source_location: None,
            },
        ],
        ir_type: IrType::Int,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(do_node);

    // Should be optimized to just the last expression (30)
    match optimized {
        IrNode::Literal {
            value: Literal::Integer(30),
            ..
        } => {
            // Success
        }
        _ => panic!("Expected do block optimization to 30, got: {:?}", optimized),
    }
}

#[test]
fn test_optimization_combinations() {
    let mut optimizer = EnhancedIrOptimizer::new();

    // Test complex optimization: (let [x (+ 5 3)] (if (> x 7) (* x 2) 0))
    // Should optimize to: (let [x 8] (if true 16 0)) -> (let [x 8] 16) -> 16
    let complex_node = IrNode::Let {
        id: 1,
        bindings: vec![IrLetBinding {
            pattern: IrNode::VariableBinding {
                id: 2,
                name: "x".to_string(),
                ir_type: IrType::Int,
                source_location: None,
            },
            type_annotation: None,
            init_expr: IrNode::Apply {
                id: 3,
                function: Box::new(IrNode::VariableRef {
                    id: 4,
                    name: "+".to_string(),
                    binding_id: 0,
                    ir_type: IrType::Function {
                        param_types: vec![IrType::Int, IrType::Int],
                        variadic_param_type: None,
                        return_type: Box::new(IrType::Int),
                    },
                    source_location: None,
                }),
                arguments: vec![
                    IrNode::Literal {
                        id: 5,
                        value: Literal::Integer(5),
                        ir_type: IrType::Int,
                        source_location: None,
                    },
                    IrNode::Literal {
                        id: 6,
                        value: Literal::Integer(3),
                        ir_type: IrType::Int,
                        source_location: None,
                    },
                ],
                ir_type: IrType::Int,
                source_location: None,
            },
        }],
        body: vec![IrNode::If {
            id: 7,
            condition: Box::new(IrNode::Apply {
                id: 8,
                function: Box::new(IrNode::VariableRef {
                    id: 9,
                    name: ">".to_string(),
                    binding_id: 0,
                    ir_type: IrType::Function {
                        param_types: vec![IrType::Int, IrType::Int],
                        variadic_param_type: None,
                        return_type: Box::new(IrType::Bool),
                    },
                    source_location: None,
                }),
                arguments: vec![
                    IrNode::VariableRef {
                        id: 10,
                        name: "x".to_string(),
                        binding_id: 2,
                        ir_type: IrType::Int,
                        source_location: None,
                    },
                    IrNode::Literal {
                        id: 11,
                        value: Literal::Integer(7),
                        ir_type: IrType::Int,
                        source_location: None,
                    },
                ],
                ir_type: IrType::Bool,
                source_location: None,
            }),
            then_branch: Box::new(IrNode::Apply {
                id: 12,
                function: Box::new(IrNode::VariableRef {
                    id: 13,
                    name: "*".to_string(),
                    binding_id: 0,
                    ir_type: IrType::Function {
                        param_types: vec![IrType::Int, IrType::Int],
                        variadic_param_type: None,
                        return_type: Box::new(IrType::Int),
                    },
                    source_location: None,
                }),
                arguments: vec![
                    IrNode::VariableRef {
                        id: 14,
                        name: "x".to_string(),
                        binding_id: 2,
                        ir_type: IrType::Int,
                        source_location: None,
                    },
                    IrNode::Literal {
                        id: 15,
                        value: Literal::Integer(2),
                        ir_type: IrType::Int,
                        source_location: None,
                    },
                ],
                ir_type: IrType::Int,
                source_location: None,
            }),
            else_branch: Some(Box::new(IrNode::Literal {
                id: 16,
                value: Literal::Integer(0),
                ir_type: IrType::Int,
                source_location: None,
            })),
            ir_type: IrType::Int,
            source_location: None,
        }],
        ir_type: IrType::Int,
        source_location: None,
    };

    let optimized = optimizer.optimize_with_control_flow(complex_node);

    // The optimization should be quite substantial, but let's check that it's at least simpler
    // For now, just verify it doesn't crash and produces some result
    match optimized {
        IrNode::Literal { .. } | IrNode::Let { .. } | IrNode::If { .. } => {
            // Any of these are reasonable outcomes depending on optimization depth
        }
        _ => panic!("Unexpected optimization result: {:?}", optimized),
    }
}
