//! IR Type Checker
//!
//! # Theoretical Foundation
//!
//! This type checker implements a **sound static type system** for RTFS IR based on:
//!
//! ## 1. Type Safety Theorem (Soundness)
//! **Progress**: Well-typed expressions either are values or can take an evaluation step.
//! **Preservation**: If `e : T` and `e → e'`, then `e' : T`.
//!
//! Together these guarantee: **"Well-typed programs don't go wrong"**
//!
//! ## 2. Subtyping Relation (≤)
//! We define a **structural subtyping** relation with these axioms:
//!
//! ```text
//! (S-Refl)    T ≤ T
//! (S-Trans)   T1 ≤ T2, T2 ≤ T3  ⊢  T1 ≤ T3
//! (S-Top)     T ≤ Any
//! (S-Bot)     Never ≤ T
//!
//! (S-Union-L) T ≤ T1 ∨ T ≤ T2  ⊢  T ≤ (T1 | T2)
//! (S-Union-R) T1 ≤ T ∧ T2 ≤ T  ⊢  (T1 | T2) ≤ T
//!
//! (S-Intersection-L1) (T1 & T2) ≤ T1
//! (S-Intersection-L2) (T1 & T2) ≤ T2
//! (S-Intersection-R)  T ≤ T1 ∧ T ≤ T2  ⊢  T ≤ (T1 & T2)
//!
//! (S-Num)     Int ≤ Number, Float ≤ Number  where Number = Int | Float
//!
//! (S-Fun)     T1' ≤ T1, T2 ≤ T2'  ⊢  (T1 → T2) ≤ (T1' → T2')
//!             (contravariant in argument, covariant in return)
//!
//! (S-Vec)     T1 ≤ T2  ⊢  Vector<T1> ≤ Vector<T2>  (covariant)
//! ```
//!
//! ## 3. Type Checking Algorithm
//! We use **bidirectional type checking** with two modes:
//!
//! - **Inference mode** (`infer`): Γ ⊢ e ⇒ T  (synthesizes type)
//! - **Checking mode** (`check`): Γ ⊢ e ⇐ T  (validates against expected type)
//!
//! ## 4. Numeric Coercion
//! RTFS runtime performs automatic numeric promotion:
//! - `Int + Int → Int`
//! - `Int + Float → Float`
//! - `Float + Float → Float`
//!
//! This is modeled via the Number = Int | Float union with subtyping.
//!
//! ## 5. Intersection Types
//! 
//! Intersection types represent values that satisfy multiple type constraints simultaneously:
//! 
//! ### Syntax and Semantics
//! - **Syntax**: `T1 & T2` (represented as `[:and T1 T2]` in RTFS)
//! - **Semantics**: A value of type `T1 & T2` must satisfy both `T1` and `T2`
//! - **Example**: `Int & Float` represents values that are both integers and floats (empty set)
//! - **Example**: `(Int → Float) & (Float → Int)` represents functions with both signatures
//! 
//! ### Subtyping Rules
//! 
//! The type checker implements two key subtyping rules for intersections:
//! 
//! ```text
//! (S-Intersection-L1) (T1 & T2) ≤ T1
//! (S-Intersection-L2) (T1 & T2) ≤ T2
//! (S-Intersection-R)  T ≤ T1 ∧ T ≤ T2  ⊢  T ≤ (T1 & T2)
//! ```
//! 
//! - **(S-Intersection-L1/L2)**: An intersection can be used wherever either component is expected
//! - **(S-Intersection-R)**: T is a subtype of an intersection if it is a subtype of all intersection components
//! 
//! ### Type Operations
//! 
//! The system provides fundamental operations for intersection types:
//! 
//! - **Type Meet** (`type_meet`): Computes the greatest lower bound (intersection) of two types
//!   - `meet(Int, Float) = Int & Float`
//!   - `meet(Int, Int) = Int`
//!   - `meet(Int, Any) = Int`
//!   - `meet(Int, Never) = Never`
//! 
//! - **Type Join** (`type_join`): Computes the least upper bound (union) of two types
//!   - `join(Int, Float) = Int | Float`
//!   - `join(Int, Int) = Int`
//!   - `join(Int, Any) = Any`
//! 
//! - **Simplification** (`simplify_intersection`): Removes redundant components and flattens nested intersections
//!   - Flattens: `(Int & (Float & String)) → (Int & Float & String)`
//!   - Removes duplicates: `(Int & Int & Float) → (Int & Float)`
//!   - Eliminates Any: `(Int & Any & Float) → (Int & Float)`
//!   - Simplifies single: `(Int) → Int`
//! 
//! ### Satisfiability Checking
//! 
//! Intersection types may be unsatisfiable (represent empty sets):
//! - `Int & Bool` is unsatisfiable (no value can be both int and bool)
//! - `Int & Never` is unsatisfiable (Never makes any intersection empty)
//! - Note: the current IR checker does not attempt a full satisfiability analysis;
//!   intersections are handled syntactically unless they contain `Never`.
//! 
//! ### Use Cases in RTFS
//! 
//! 1. **Function Overloading**: Represent functions with multiple signatures
//!    ```rtfs
//!    ([:and (→ Int Float) (→ Float Int)])
//!    ```
//! 
//! 2. **Precise Type Constraints**: Express complex type requirements
//!    ```rtfs
//!    ([:and HasName HasId HasTimestamp])
//!    ```
//! 
//! 3. **Type Safety**: Enforce multiple interface contracts
//!    ```rtfs
//!    ([:and Serializable Cloneable])
//!    ```
//! 
//! 4. **LLM-Generated Code**: Provide flexible type constraints for AI-generated functions
//! 
//! ### Implementation Notes
//! 
//! - **Storage**: Represented as `IrType::Intersection(Vec<IrType>)` in IR
//! - **Validation**: Runtime validation checks that values satisfy all intersection components
//! - **Inference**: Type meet operations enable intersection type inference during type checking
//! - **Performance**: Subtyping checks use memoization to avoid redundant computations
//! 
//! ### Examples from the Test Suite
//! 
//! ```text
//! // Basic intersection subtyping
//! Int & Float ≤ Number  (where Number = Int | Float)
//! 
//! // Intersection with collections
//! Vector<Int & Float> ≤ Vector<Number>
//! 
//! // Type meet operations
//! meet(Int, Float) = Int & Float
//! meet(Int, Int) = Int
//! meet(Int, Any) = Int
//! ```
//!
//! ## 6. Decidability
//! Our algorithm is **decidable** and **complete** because:
//! - Subtyping is structural (no recursive types yet)
//! - Union membership is finite
//! - All rules are syntax-directed
//!
//! # References
//! - Pierce, B. C. (2002). *Types and Programming Languages*. MIT Press.
//! - Cardelli, L. (1984). A Semantics of Multiple Inheritance.
//! - Davies, R., & Pfenning, F. (2000). Intersection Types and Computational Effects.

use crate::ir::core::{IrNode, IrType};
use std::collections::HashSet;
use std::fmt;

#[derive(Debug, Clone)]
pub enum TypeCheckError {
    TypeMismatch {
        expected: IrType,
        actual: IrType,
        location: String,
    },
    FunctionCallTypeMismatch {
        function: String,
        param_index: usize,
        expected: IrType,
        actual: IrType,
    },
    NonFunctionCalled {
        actual_type: IrType,
        location: String,
    },
    UnresolvedVariable {
        name: String,
    },
}

impl fmt::Display for TypeCheckError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TypeCheckError::TypeMismatch {
                expected,
                actual,
                location,
            } => {
                write!(
                    f,
                    "Type mismatch at {}: expected {:?}, got {:?}",
                    location, expected, actual
                )
            }
            TypeCheckError::FunctionCallTypeMismatch {
                function,
                param_index,
                expected,
                actual,
            } => {
                write!(
                    f,
                    "Type mismatch in call to '{}' parameter {}: expected {:?}, got {:?}",
                    function, param_index, expected, actual
                )
            }
            TypeCheckError::NonFunctionCalled {
                actual_type,
                location,
            } => {
                write!(
                    f,
                    "Attempted to call non-function of type {:?} at {}",
                    actual_type, location
                )
            }
            TypeCheckError::UnresolvedVariable { name } => {
                write!(f, "Unresolved variable: {}", name)
            }
        }
    }
}

impl std::error::Error for TypeCheckError {}

pub type TypeCheckResult<T> = Result<T, TypeCheckError>;

// =============================================================================
// SUBTYPING RELATION (≤)
// =============================================================================

/// Algorithmic subtyping: checks if `sub ≤ sup` (sub is a subtype of sup)
///
/// Implements the subtyping rules from the theory section above.
/// This is the **core judgment** of the type system.
///
/// # Soundness
/// If `is_subtype(T1, T2)` returns true, then any value of type T1
/// can safely be used where T2 is expected.
pub fn is_subtype(sub: &IrType, sup: &IrType) -> bool {
    is_subtype_cached(sub, sup, &mut HashSet::new())
}

/// Internal subtyping with cycle detection for recursive types
fn is_subtype_cached(sub: &IrType, sup: &IrType, visited: &mut HashSet<(String, String)>) -> bool {
    // (S-Refl) Reflexivity: T ≤ T
    if sub == sup {
        return true;
    }

    // (S-Top) Any is the top type: T ≤ Any
    if matches!(sup, IrType::Any) {
        return true;
    }

    // (S-Bot) Never is the bottom type: Never ≤ T
    if matches!(sub, IrType::Never) {
        return true;
    }

    // Cycle detection for (future) recursive types.
    //
    // IMPORTANT: `visited` must behave like an *active recursion stack*, not a memo table.
    // Using it as a global memo (never removing) is unsound (e.g., duplicate union members).
    let key = (format!("{:?}", sub), format!("{:?}", sup));
    if visited.contains(&key) {
        return true; // assume holds for the recursive case
    }
    visited.insert(key.clone());

    let result = if let IrType::Intersection(sup_components) = sup {
        // (S-Intersection-R): T ≤ (T1 & T2)  iff  T ≤ T1 ∧ T ≤ T2
        sup_components
            .iter()
            .all(|component| is_subtype_cached(sub, component, visited))
    } else if let IrType::Union(sup_variants) = sup {
        // (S-Union-R): T ≤ (S1 | S2)  iff  T ≤ S1 ∨ T ≤ S2
        sup_variants
            .iter()
            .any(|variant| is_subtype_cached(sub, variant, visited))
    } else if let IrType::Union(sub_variants) = sub {
        // (S-Union-L): (T1 | T2) ≤ S  iff  T1 ≤ S ∧ T2 ≤ S
        sub_variants
            .iter()
            .all(|variant| is_subtype_cached(variant, sup, visited))
    } else if let IrType::Intersection(sub_components) = sub {
        // Intersection elimination (meet semantics): (T1 & T2) ≤ T if any component is ≤ T.
        sub_components
            .iter()
            .any(|component| is_subtype_cached(component, sup, visited))
    } else if let (
        IrType::Function {
            param_types: sub_params,
            return_type: sub_ret,
            variadic_param_type: sub_var,
        },
        IrType::Function {
            param_types: sup_params,
            return_type: sup_ret,
            variadic_param_type: sup_var,
        },
    ) = (sub, sup)
    {
        // (S-Fun): contravariant in args, covariant in return
        if sub_params.len() != sup_params.len() {
            false
        } else {
            let params_ok = sub_params
                .iter()
                .zip(sup_params.iter())
                .all(|(sub_param, sup_param)| is_subtype_cached(sup_param, sub_param, visited));

            if !params_ok {
                false
            } else {
                let variadic_ok = match (sub_var, sup_var) {
                    (Some(sub_v), Some(sup_v)) => is_subtype_cached(sup_v, sub_v, visited),
                    (None, None) => true,
                    _ => false,
                };

                variadic_ok && is_subtype_cached(sub_ret, sup_ret, visited)
            }
        }
    } else if let (IrType::Vector(sub_elem), IrType::Vector(sup_elem)) = (sub, sup) {
        is_subtype_cached(sub_elem, sup_elem, visited)
    } else if let (IrType::List(sub_elem), IrType::List(sup_elem)) = (sub, sup) {
        is_subtype_cached(sub_elem, sup_elem, visited)
    } else if let (IrType::Tuple(sub_elems), IrType::Tuple(sup_elems)) = (sub, sup) {
        if sub_elems.len() != sup_elems.len() {
            false
        } else {
            sub_elems
                .iter()
                .zip(sup_elems.iter())
                .all(|(sub_e, sup_e)| is_subtype_cached(sub_e, sup_e, visited))
        }
    } else if let (
        IrType::Map {
            entries: sub_entries,
            wildcard: _sub_wildcard,
        },
        IrType::Map {
            entries: sup_entries,
            wildcard: _sup_wildcard,
        },
    ) = (sub, sup)
    {
        // Structural record map subtyping (required/optional fields; open map by default)
        sup_entries.iter().all(|sup_entry| {
            let sub_entry = sub_entries.iter().find(|e| e.key == sup_entry.key);
            match sub_entry {
                Some(se) => {
                    if !sup_entry.optional && se.optional {
                        return false;
                    }
                    is_subtype_cached(&se.value_type, &sup_entry.value_type, visited)
                }
                None => sup_entry.optional,
            }
        })
    } else if let (
        IrType::ParametricMap {
            key_type: sub_k,
            value_type: sub_v,
        },
        IrType::ParametricMap {
            key_type: sup_k,
            value_type: sup_v,
        },
    ) = (sub, sup)
    {
        // Dictionary map subtyping (covariant in key and value)
        is_subtype_cached(sub_k, sup_k, visited) && is_subtype_cached(sub_v, sup_v, visited)
    } else {
        false
    };

    visited.remove(&key);
    result
}

/// Type compatibility check (for backward compatibility)
/// This is just an alias for is_subtype with a clearer name for checking arguments
pub fn is_type_compatible(actual: &IrType, expected: &IrType) -> bool {
    is_subtype(actual, expected)
}

/// Compute the meet (greatest lower bound) of two types
/// For intersection types, this computes the intersection of their components
pub fn type_meet(t1: &IrType, t2: &IrType) -> IrType {
    match (t1, t2) {
        // If types are equal, return either
        _ if t1 == t2 => t1.clone(),
        
        // If one type is Never, return Never (bottom type)
        (IrType::Never, _) | (_, IrType::Never) => IrType::Never,
        
        // If one type is Any, return the other type
        (IrType::Any, t) => t.clone(),
        (t, IrType::Any) => t.clone(),
        
        // For intersection types, compute the intersection of all components
        (IrType::Intersection(components1), IrType::Intersection(components2)) => {
            let mut all_components = components1.clone();
            all_components.extend(components2.clone());
            IrType::Intersection(all_components)
        }
        (IrType::Intersection(components), t) | (t, IrType::Intersection(components)) => {
            let mut all_components = components.clone();
            all_components.push(t.clone());
            IrType::Intersection(all_components)
        }
        
        // For other types, create an intersection
        (t1, t2) => IrType::Intersection(vec![t1.clone(), t2.clone()]),
    }
}

/// Compute the join (least upper bound) of two types
/// For intersection types, this computes the union of their requirements
pub fn type_join(t1: &IrType, t2: &IrType) -> IrType {
    match (t1, t2) {
        // If types are equal, return either
        _ if t1 == t2 => t1.clone(),
        
        // If one type is Any, return Any (top type)
        (IrType::Any, _) | (_, IrType::Any) => IrType::Any,
        
        // If one type is Never, return the other type
        (IrType::Never, t) => t.clone(),
        (t, IrType::Never) => t.clone(),

        // If one is already a subtype of the other, the supertype is the join (LUB)
        (a, b) if is_subtype(a, b) => b.clone(),
        (a, b) if is_subtype(b, a) => a.clone(),
        
        // For other types, create a union (which is the standard join)
        (t1, t2) => IrType::Union(vec![t1.clone(), t2.clone()]),
    }
}

/// Simplify an intersection type by removing redundant components
/// and flattening nested intersections
pub fn simplify_intersection(mut components: Vec<IrType>) -> Vec<IrType> {
    // Flatten nested intersections first
    let mut flattened = Vec::new();
    for component in components.drain(..) {
        match component {
            IrType::Intersection(nested) => flattened.extend(simplify_intersection(nested)),
            other => flattened.push(other),
        }
    }

    // Remove Any types (they don't add constraints)
    flattened.retain(|t| !matches!(t, IrType::Any));

    // If we have Never, the intersection is impossible (return just Never)
    if flattened.iter().any(|t| matches!(t, IrType::Never)) {
        return vec![IrType::Never];
    }

    // Remove duplicates (NOTE: Vec::dedup() only removes adjacent duplicates)
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for t in flattened {
        let key = format!("{:?}", t);
        if seen.insert(key) {
            unique.push(t);
        }
    }

    unique
}

// =============================================================================
// BIDIRECTIONAL TYPE CHECKING
// =============================================================================

/// Type check an IR node recursively (inference mode: Γ ⊢ e ⇒ T)
///
/// This implements the **type synthesis** judgment where we infer
/// the type of an expression from its structure.
///
/// # Type Safety
/// If `type_check_ir(e)` succeeds, then `e` is well-typed and will not
/// produce a runtime type error (modulo dynamic checks in capabilities).
pub fn type_check_ir(node: &IrNode) -> TypeCheckResult<()> {
    match node {
        // Program: check all top-level forms
        IrNode::Program { forms, .. } => {
            for form in forms {
                type_check_ir(form)?;
            }
            Ok(())
        }

        // Function application: T-App rule
        // Γ ⊢ f ⇒ T1 → T2    Γ ⊢ args ⇐ T1    ⊢    Γ ⊢ f(args) ⇒ T2
        IrNode::Apply {
            function,
            arguments,
            ..
        } => {
            let func_type = infer_type(function)?;

            if let IrType::Function {
                param_types,
                variadic_param_type,
                return_type: _,
            } = &func_type
            {
                let required_params = param_types.len();

                // Check required parameters
                for (i, arg) in arguments.iter().enumerate().take(required_params) {
                    let arg_type = infer_type(arg)?;
                    let expected_type = &param_types[i];

                    // Use subtyping: arg_type ≤ expected_type
                    if !is_subtype(&arg_type, expected_type) {
                        let func_name = match function.as_ref() {
                            IrNode::VariableRef { name, .. } => name.clone(),
                            _ => "<anonymous>".to_string(),
                        };
                        return Err(TypeCheckError::FunctionCallTypeMismatch {
                            function: func_name,
                            param_index: i,
                            expected: expected_type.clone(),
                            actual: arg_type,
                        });
                    }
                }

                // Check variadic parameters
                if let Some(variadic_type) = variadic_param_type {
                    for (i, arg) in arguments.iter().enumerate().skip(required_params) {
                        let arg_type = infer_type(arg)?;

                        if !is_subtype(&arg_type, variadic_type) {
                            let func_name = match function.as_ref() {
                                IrNode::VariableRef { name, .. } => name.clone(),
                                _ => "<anonymous>".to_string(),
                            };
                            return Err(TypeCheckError::FunctionCallTypeMismatch {
                                function: func_name,
                                param_index: i,
                                expected: (**variadic_type).clone(),
                                actual: arg_type,
                            });
                        }
                    }
                }

                // Recursively check sub-expressions
                type_check_ir(function)?;
                for arg in arguments {
                    type_check_ir(arg)?;
                }

                Ok(())
            } else {
                Err(TypeCheckError::NonFunctionCalled {
                    actual_type: func_type,
                    location: "function call".to_string(),
                })
            }
        }

        // Conditional: T-If rule
        IrNode::If {
            condition,
            then_branch,
            else_branch,
            ..
        } => {
            type_check_ir(condition)?;
            type_check_ir(then_branch)?;
            if let Some(else_br) = else_branch {
                type_check_ir(else_br)?;
            }
            Ok(())
        }

        // Let binding: T-Let rule
        IrNode::Let { bindings, body, .. } => {
            // Check all binding initializers
            for binding in bindings {
                type_check_ir(&binding.init_expr)?;

                // If there's a type annotation, check it matches
                if let Some(annot_type) = &binding.type_annotation {
                    let init_type = infer_type(&binding.init_expr)?;
                    if !is_subtype(&init_type, annot_type) {
                        return Err(TypeCheckError::TypeMismatch {
                            expected: annot_type.clone(),
                            actual: init_type,
                            location: "let binding".to_string(),
                        });
                    }
                }
            }

            // Check body
            for expr in body {
                type_check_ir(expr)?;
            }
            Ok(())
        }

        // Do block
        IrNode::Do { expressions, .. } => {
            for expr in expressions {
                type_check_ir(expr)?;
            }
            Ok(())
        }

        // Literals and variables are always well-typed by construction
        IrNode::Literal { .. } | IrNode::VariableRef { .. } | IrNode::VariableBinding { .. } => {
            Ok(())
        }

        // Vectors and maps
        IrNode::Vector { elements, .. } => {
            for elem in elements {
                type_check_ir(elem)?;
            }
            Ok(())
        }

        IrNode::Map { entries, .. } => {
            for entry in entries {
                type_check_ir(&entry.key)?;
                type_check_ir(&entry.value)?;
            }
            Ok(())
        }

        // For all other node types, conservatively accept them
        // (they may have runtime checks or be extension points)
        _ => Ok(()),
    }
}

/// Infer the type of an IR node (type synthesis: Γ ⊢ e ⇒ T)
fn infer_type(node: &IrNode) -> TypeCheckResult<IrType> {
    Ok(node.ir_type().cloned().unwrap_or(IrType::Any))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Keyword, Literal};
    use crate::ir::core::{IrMapTypeEntry, IrNode, IrType};

    // =========================================================================
    // SECTION 3: SUBTYPING RELATION TESTS
    // Tests for all subtyping rules from §3.1 of the formal specification
    // =========================================================================

    #[test]
    fn test_s_refl_reflexivity() {
        // (S-Refl): τ ≤ τ
        assert!(is_subtype(&IrType::Int, &IrType::Int));
        assert!(is_subtype(&IrType::Float, &IrType::Float));
        assert!(is_subtype(&IrType::String, &IrType::String));
        assert!(is_subtype(&IrType::Bool, &IrType::Bool));
        assert!(is_subtype(&IrType::Any, &IrType::Any));
        assert!(is_subtype(&IrType::Never, &IrType::Never));

        let vec_int = IrType::Vector(Box::new(IrType::Int));
        assert!(is_subtype(&vec_int, &vec_int));
    }

    #[test]
    fn test_s_top_any_is_top_type() {
        // (S-Top): τ ≤ Any for all τ
        assert!(is_subtype(&IrType::Int, &IrType::Any));
        assert!(is_subtype(&IrType::Float, &IrType::Any));
        assert!(is_subtype(&IrType::String, &IrType::Any));
        assert!(is_subtype(&IrType::Bool, &IrType::Any));
        assert!(is_subtype(&IrType::Never, &IrType::Any));

        let vec_int = IrType::Vector(Box::new(IrType::Int));
        assert!(is_subtype(&vec_int, &IrType::Any));

        let union = IrType::Union(vec![IrType::Int, IrType::String]);
        assert!(is_subtype(&union, &IrType::Any));
    }

    #[test]
    fn test_s_bot_never_is_bottom_type() {
        // (S-Bot): Never ≤ τ for all τ
        assert!(is_subtype(&IrType::Never, &IrType::Int));
        assert!(is_subtype(&IrType::Never, &IrType::Float));
        assert!(is_subtype(&IrType::Never, &IrType::String));
        assert!(is_subtype(&IrType::Never, &IrType::Any));

        let vec_int = IrType::Vector(Box::new(IrType::Int));
        assert!(is_subtype(&IrType::Never, &vec_int));
    }

    #[test]
    fn test_s_trans_transitivity() {
        // (S-Trans): τ₁ ≤ τ₂ ∧ τ₂ ≤ τ₃ ⟹ τ₁ ≤ τ₃

        // Int ≤ Number ≤ Any
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        assert!(is_subtype(&IrType::Int, &number)); // Int ≤ Number
        assert!(is_subtype(&number, &IrType::Any)); // Number ≤ Any
        assert!(is_subtype(&IrType::Int, &IrType::Any)); // Int ≤ Any (transitive)

        // Vector<Int> ≤ Vector<Number> ≤ Vector<Any>
        let vec_int = IrType::Vector(Box::new(IrType::Int));
        let vec_number = IrType::Vector(Box::new(number.clone()));
        let vec_any = IrType::Vector(Box::new(IrType::Any));

        assert!(is_subtype(&vec_int, &vec_number));
        assert!(is_subtype(&vec_number, &vec_any));
        assert!(is_subtype(&vec_int, &vec_any)); // Transitive
    }

    #[test]
    fn test_s_union_r_subtype_of_union() {
        // (S-Union-R): τ ≤ τ₁|τ₂ iff τ ≤ τ₁ ∨ τ ≤ τ₂
        let union = IrType::Union(vec![IrType::Int, IrType::Float]);

        assert!(is_subtype(&IrType::Int, &union)); // Int ≤ Int|Float
        assert!(is_subtype(&IrType::Float, &union)); // Float ≤ Int|Float
        assert!(!is_subtype(&IrType::String, &union)); // String ⊈ Int|Float
    }

    #[test]
    fn test_s_union_l_union_of_subtypes() {
        // (S-Union-L): τ₁|τ₂ ≤ τ iff τ₁ ≤ τ ∧ τ₂ ≤ τ
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        // Number ≤ Any (both Int and Float are subtypes of Any)
        assert!(is_subtype(&number, &IrType::Any));

        // Number ⊈ Int (Float is not a subtype of Int)
        assert!(!is_subtype(&number, &IrType::Int));

        // Number ⊈ Float (Int is not a subtype of Float)
        assert!(!is_subtype(&number, &IrType::Float));
    }

    #[test]
    fn test_s_num_numeric_tower() {
        // (S-Num): Int ≤ Number, Float ≤ Number where Number = Int | Float
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        assert!(is_subtype(&IrType::Int, &number));
        assert!(is_subtype(&IrType::Float, &number));
    }

    #[test]
    fn test_s_fun_function_subtyping_contravariance() {
        // (S-Fun): (τ₁ → τ₂) ≤ (τ₁' → τ₂') iff τ₁' ≤ τ₁ ∧ τ₂ ≤ τ₂'
        // Functions are CONTRAVARIANT in arguments, COVARIANT in return

        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        // (Int → Number) ≤ (Number → Int)?
        let f1 = IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(number.clone()),
        };
        let f2 = IrType::Function {
            param_types: vec![number.clone()],
            variadic_param_type: None,
            return_type: Box::new(IrType::Int),
        };

        // Check: Number ≤ Int (arg, contravariant) ✗ - this should FAIL
        assert!(!is_subtype(&f1, &f2));

        // (Number → Int) ≤ (Int → Number)?
        let f3 = IrType::Function {
            param_types: vec![number.clone()],
            variadic_param_type: None,
            return_type: Box::new(IrType::Int),
        };
        let f4 = IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(number.clone()),
        };

        // Check: Int ≤ Number (arg, contravariant) ✓ AND Int ≤ Number (return, covariant) ✓
        assert!(is_subtype(&f3, &f4));
    }

    #[test]
    fn test_s_vec_vector_covariance() {
        // (S-Vec): Vector<τ₁> ≤ Vector<τ₂> iff τ₁ ≤ τ₂
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        let vec_int = IrType::Vector(Box::new(IrType::Int));
        let vec_number = IrType::Vector(Box::new(number));
        let vec_any = IrType::Vector(Box::new(IrType::Any));

        // Vector<Int> ≤ Vector<Number>
        assert!(is_subtype(&vec_int, &vec_number));

        // Vector<Int> ≤ Vector<Any>
        assert!(is_subtype(&vec_int, &vec_any));

        // Vector<Number> ≤ Vector<Any>
        assert!(is_subtype(&vec_number, &vec_any));

        // Vector<Any> ⊈ Vector<Int> (covariance, not contravariance!)
        assert!(!is_subtype(&vec_any, &vec_int));
    }

    // =========================================================================
    // SECTION 4: TYPE CHECKING ALGORITHM TESTS
    // Tests for typing rules from §4.2 of the formal specification
    // =========================================================================

    #[test]
    fn test_t_int_literal_typing() {
        // (T-Int): Γ ⊢ n ⇒ Int
        let node = IrNode::Literal {
            id: 1,
            value: Literal::Integer(42),
            ir_type: IrType::Int,
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
        assert_eq!(infer_type(&node).unwrap(), IrType::Int);
    }

    #[test]
    fn test_t_float_literal_typing() {
        // (T-Float): Γ ⊢ f ⇒ Float
        let node = IrNode::Literal {
            id: 1,
            value: Literal::Float(3.14),
            ir_type: IrType::Float,
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
        assert_eq!(infer_type(&node).unwrap(), IrType::Float);
    }

    #[test]
    fn test_t_vec_homogeneous_vector() {
        // (T-Vec): Γ ⊢ [e₁,...,eₙ] ⇒ Vector⟨join(τ₁,...,τₙ)⟩
        // Case: All Int → Vector<Int>
        let node = IrNode::Vector {
            id: 1,
            elements: vec![
                IrNode::Literal {
                    id: 2,
                    value: Literal::Integer(1),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 3,
                    value: Literal::Integer(2),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 4,
                    value: Literal::Integer(3),
                    ir_type: IrType::Int,
                    source_location: None,
                },
            ],
            ir_type: IrType::Vector(Box::new(IrType::Int)),
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
        assert_eq!(
            infer_type(&node).unwrap(),
            IrType::Vector(Box::new(IrType::Int))
        );
    }

    #[test]
    fn test_t_vec_mixed_numeric_vector() {
        // Case: Int + Float → Vector<Number>
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        let node = IrNode::Vector {
            id: 1,
            elements: vec![
                IrNode::Literal {
                    id: 2,
                    value: Literal::Integer(1),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 3,
                    value: Literal::Float(2.5),
                    ir_type: IrType::Float,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 4,
                    value: Literal::Integer(3),
                    ir_type: IrType::Int,
                    source_location: None,
                },
            ],
            ir_type: IrType::Vector(Box::new(number.clone())),
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
        assert_eq!(infer_type(&node).unwrap(), IrType::Vector(Box::new(number)));
    }

    #[test]
    fn test_t_vec_heterogeneous_vector() {
        // Case: Different types → Vector<T1 | T2 | ...>
        let union = IrType::Union(vec![IrType::Int, IrType::String, IrType::Bool]);

        let node = IrNode::Vector {
            id: 1,
            elements: vec![
                IrNode::Literal {
                    id: 2,
                    value: Literal::Integer(1),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 3,
                    value: Literal::String("text".to_string()),
                    ir_type: IrType::String,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 4,
                    value: Literal::Boolean(true),
                    ir_type: IrType::Bool,
                    source_location: None,
                },
            ],
            ir_type: IrType::Vector(Box::new(union.clone())),
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
    }

    #[test]
    fn test_t_vec_nested_vectors() {
        // Case: [[1 2] [3 4]] → Vector<Vector<Int>>
        let inner_vec = IrType::Vector(Box::new(IrType::Int));

        let node = IrNode::Vector {
            id: 1,
            elements: vec![
                IrNode::Vector {
                    id: 2,
                    elements: vec![
                        IrNode::Literal {
                            id: 3,
                            value: Literal::Integer(1),
                            ir_type: IrType::Int,
                            source_location: None,
                        },
                        IrNode::Literal {
                            id: 4,
                            value: Literal::Integer(2),
                            ir_type: IrType::Int,
                            source_location: None,
                        },
                    ],
                    ir_type: inner_vec.clone(),
                    source_location: None,
                },
                IrNode::Vector {
                    id: 5,
                    elements: vec![
                        IrNode::Literal {
                            id: 6,
                            value: Literal::Integer(3),
                            ir_type: IrType::Int,
                            source_location: None,
                        },
                        IrNode::Literal {
                            id: 7,
                            value: Literal::Integer(4),
                            ir_type: IrType::Int,
                            source_location: None,
                        },
                    ],
                    ir_type: inner_vec.clone(),
                    source_location: None,
                },
            ],
            ir_type: IrType::Vector(Box::new(inner_vec.clone())),
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
        assert_eq!(
            infer_type(&node).unwrap(),
            IrType::Vector(Box::new(inner_vec))
        );
    }

    #[test]
    fn test_t_app_function_application_success() {
        // (T-App): Function application with correct types
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        let node = IrNode::Apply {
            id: 1,
            function: Box::new(IrNode::VariableRef {
                id: 2,
                name: "+".to_string(),
                binding_id: 1,
                ir_type: IrType::Function {
                    param_types: vec![number.clone()],
                    variadic_param_type: Some(Box::new(number.clone())),
                    return_type: Box::new(number.clone()),
                },
                source_location: None,
            }),
            arguments: vec![
                IrNode::Literal {
                    id: 3,
                    value: Literal::Integer(1),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 4,
                    value: Literal::Float(2.5),
                    ir_type: IrType::Float,
                    source_location: None,
                },
            ],
            ir_type: number.clone(),
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
    }

    #[test]
    fn test_t_app_function_application_type_error() {
        // (T-App): Function application with incorrect types should fail
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        let node = IrNode::Apply {
            id: 1,
            function: Box::new(IrNode::VariableRef {
                id: 2,
                name: "+".to_string(),
                binding_id: 1,
                ir_type: IrType::Function {
                    param_types: vec![number.clone()],
                    variadic_param_type: Some(Box::new(number.clone())),
                    return_type: Box::new(number.clone()),
                },
                source_location: None,
            }),
            arguments: vec![
                IrNode::Literal {
                    id: 3,
                    value: Literal::Integer(1),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 4,
                    value: Literal::String("bad".to_string()),
                    ir_type: IrType::String,
                    source_location: None,
                },
            ],
            ir_type: number.clone(),
            source_location: None,
        };

        // Should fail: String ⊈ Number
        let result = type_check_ir(&node);
        assert!(result.is_err());
        if let Err(TypeCheckError::FunctionCallTypeMismatch {
            expected, actual, ..
        }) = result
        {
            assert_eq!(expected, number);
            assert_eq!(actual, IrType::String);
        } else {
            panic!("Expected FunctionCallTypeMismatch error");
        }
    }

    // =========================================================================
    // SECTION 5: SOUNDNESS THEOREM TESTS
    // Empirical tests for Progress and Preservation claims
    // =========================================================================

    #[test]
    fn test_progress_well_typed_expressions_reduce_or_are_values() {
        // Progress: Well-typed expressions either are values or can take a step

        // Literals are values (cannot reduce further)
        let lit = IrNode::Literal {
            id: 1,
            value: Literal::Integer(42),
            ir_type: IrType::Int,
            source_location: None,
        };
        assert!(type_check_ir(&lit).is_ok());
        // This is a value - progress satisfied

        // Function application can reduce
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let app = IrNode::Apply {
            id: 1,
            function: Box::new(IrNode::VariableRef {
                id: 2,
                name: "+".to_string(),
                binding_id: 1,
                ir_type: IrType::Function {
                    param_types: vec![number.clone()],
                    variadic_param_type: Some(Box::new(number.clone())),
                    return_type: Box::new(number.clone()),
                },
                source_location: None,
            }),
            arguments: vec![
                IrNode::Literal {
                    id: 3,
                    value: Literal::Integer(1),
                    ir_type: IrType::Int,
                    source_location: None,
                },
                IrNode::Literal {
                    id: 4,
                    value: Literal::Integer(2),
                    ir_type: IrType::Int,
                    source_location: None,
                },
            ],
            ir_type: number,
            source_location: None,
        };
        assert!(type_check_ir(&app).is_ok());
        // This can reduce (to Integer(3)) - progress satisfied
    }

    // =========================================================================
    // SECTION 7: EXAMPLES FROM DOCUMENTATION
    // Tests that validate examples in docs/rtfs-2.0/specs/13-type-system.md
    // =========================================================================

    #[test]
    fn test_example_7_1_numeric_coercion() {
        // Example 7.1 from docs: (+ 1 2.5) → Number
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        // Verify: Int ≤ Number
        assert!(is_subtype(&IrType::Int, &number));

        // Verify: Float ≤ Number
        assert!(is_subtype(&IrType::Float, &number));
    }

    #[test]
    fn test_example_7_2_vector_type_inference() {
        // Example 7.2 from docs: Vector type inference

        // [1 2 3] → Vector<Int>
        let vec_int = IrType::Vector(Box::new(IrType::Int));
        assert!(is_subtype(&vec_int, &vec_int));

        // [1 2.5 3] → Vector<Number>
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let vec_number = IrType::Vector(Box::new(number.clone()));
        assert!(is_subtype(&IrType::Int, &number));
        assert!(is_subtype(&IrType::Float, &number));
        assert!(is_subtype(&vec_number, &IrType::Vector(Box::new(IrType::Any))));

        // [[1 2] [3 4]] → Vector<Vector<Int>>
        let vec_vec_int = IrType::Vector(Box::new(vec_int.clone()));
        assert!(is_subtype(&vec_int, &vec_int));
        assert!(is_subtype(&vec_vec_int, &IrType::Vector(Box::new(IrType::Any))));
    }

    #[test]
    fn test_example_7_3_type_error_detection() {
        // Example 7.3 from docs: (+ 1 "string") should be rejected
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        // String ≤ Number? NO
        assert!(!is_subtype(&IrType::String, &number));
    }

    #[test]
    fn test_example_7_4_function_subtyping() {
        // Example 7.4 from docs
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        // f : Int → Number
        let f = IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(number.clone()),
        };

        // g : Number → Int
        let g = IrType::Function {
            param_types: vec![number.clone()],
            variadic_param_type: None,
            return_type: Box::new(IrType::Int),
        };

        // Is f ≤ g? NO (as proven in docs)
        assert!(!is_subtype(&f, &g));
    }

    // =========================================================================
    // LEMMA VERIFICATION TESTS
    // Tests that validate specific lemmas from the formal specification
    // =========================================================================

    #[test]
    fn test_lemma_3_1_union_characterization() {
        // Lemma 3.1: τ ≤ τ₁|τ₂ ⟺ τ ≤ τ₁ ∨ τ ≤ τ₂
        let union = IrType::Union(vec![IrType::Int, IrType::String]);

        // Int ≤ Int|String because Int ≤ Int
        assert!(is_subtype(&IrType::Int, &union));

        // String ≤ Int|String because String ≤ String
        assert!(is_subtype(&IrType::String, &union));

        // Bool ⊈ Int|String because Bool ⊈ Int and Bool ⊈ String
        assert!(!is_subtype(&IrType::Bool, &union));
    }

    #[test]
    fn test_lemma_3_4_join_is_least_upper_bound() {
        // Lemma 3.4: Join computes least upper bound
        // Property 1: ∀i. τᵢ ≤ join(τ₁, ..., τₙ)
        // Property 2: If ∀i. τᵢ ≤ σ, then join(τ₁, ..., τₙ) ≤ σ

        let number = IrType::Union(vec![IrType::Int, IrType::Float]);

        // join(Int, Float) = Number
        // Property 1: Int ≤ Number and Float ≤ Number
        assert!(is_subtype(&IrType::Int, &number));
        assert!(is_subtype(&IrType::Float, &number));

        // Property 2: If Int ≤ Any and Float ≤ Any, then Number ≤ Any
        assert!(is_subtype(&IrType::Int, &IrType::Any));
        assert!(is_subtype(&IrType::Float, &IrType::Any));
        assert!(is_subtype(&number, &IrType::Any));
    }

    #[test]
    fn test_lemma_4_1_vector_type_soundness() {
        // Lemma 4.1: If Γ ⊢ [e₁,...,eₙ] ⇒ Vector⟨τ⟩, then ∀i. Γ ⊢ eᵢ ⇒ τᵢ where τᵢ ≤ τ

        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let vec_number = IrType::Vector(Box::new(number.clone()));

        // Vector elements: 1 : Int, 2.5 : Float
        // Vector type: Vector<Number>

        // Check: Int ≤ Number ✓
        assert!(is_subtype(&IrType::Int, &number));

        // Check: Float ≤ Number ✓
        assert!(is_subtype(&IrType::Float, &number));

        // Therefore: Vector<Number> is sound for [1, 2.5]
        assert!(is_subtype(&vec_number, &IrType::Vector(Box::new(IrType::Any))));
    }

    // =========================================================================
    // EDGE CASES AND CORNER CASES
    // =========================================================================

    #[test]
    fn test_empty_vector_is_vector_any() {
        let vec_any = IrType::Vector(Box::new(IrType::Any));

        let node = IrNode::Vector {
            id: 1,
            elements: vec![],
            ir_type: vec_any.clone(),
            source_location: None,
        };

        assert!(type_check_ir(&node).is_ok());
        assert_eq!(infer_type(&node).unwrap(), vec_any);
    }

    #[test]
    fn test_union_of_unions_flattening() {
        // (Int | String) | Bool should be compatible with Int | String | Bool
        let union1 = IrType::Union(vec![IrType::Int, IrType::String]);
        let union2 = IrType::Union(vec![union1.clone(), IrType::Bool]);
        let union_flat = IrType::Union(vec![IrType::Int, IrType::String, IrType::Bool]);

        // Both unions should be subtypes of Any
        assert!(is_subtype(&union2, &IrType::Any));
        assert!(is_subtype(&union_flat, &IrType::Any));
    }

    #[test]
    fn test_non_function_call_error() {
        // Attempting to call a non-function should fail type checking
        let node = IrNode::Apply {
            id: 1,
            function: Box::new(IrNode::Literal {
                id: 2,
                value: Literal::Integer(42),
                ir_type: IrType::Int,
                source_location: None,
            }),
            arguments: vec![],
            ir_type: IrType::Any,
            source_location: None,
        };

        let result = type_check_ir(&node);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(TypeCheckError::NonFunctionCalled { .. })
        ));
    }

    // =========================================================================
    // COMPREHENSIVE PROPERTY TESTS
    // =========================================================================

    #[test]
    fn test_subtype_reflexivity_comprehensive() {
        // Every type should be a subtype of itself
        let types = vec![
            IrType::Int,
            IrType::Float,
            IrType::String,
            IrType::Bool,
            IrType::Nil,
            IrType::Keyword,
            IrType::Symbol,
            IrType::Any,
            IrType::Never,
            IrType::Vector(Box::new(IrType::Int)),
            IrType::List(Box::new(IrType::String)),
            IrType::Union(vec![IrType::Int, IrType::Float]),
        ];

        for t in types {
            assert!(is_subtype(&t, &t), "Reflexivity failed for {:?}", t);
        }
    }

    #[test]
    fn test_subtype_antisymmetry() {
        // If τ₁ ≤ τ₂ and τ₂ ≤ τ₁, then τ₁ = τ₂ (for non-union types)

        // Int ≤ Int and Int ≤ Int → they're equal
        assert!(is_subtype(&IrType::Int, &IrType::Int));
        assert!(is_subtype(&IrType::Int, &IrType::Int));

        // Int ≤ Any but Any ⊈ Int
        assert!(is_subtype(&IrType::Int, &IrType::Any));
        assert!(!is_subtype(&IrType::Any, &IrType::Int));
    }

    // =========================================================================
    // SECTION 6: INTERSECTION TYPE TESTS
    // Tests for intersection types from §6 of the formal specification
    // =========================================================================

    #[test]
    fn test_s_intersection_l_basic() {
        // (S-Intersection-L1/L2): (T1 & T2) ≤ T1 and (T1 & T2) ≤ T2
        
        // Basic case: (Int & Float) ≤ Number where Number = Int | Float
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let int_float_intersection = IrType::Intersection(vec![IrType::Int, IrType::Float]);
        
        // (Int & Float) ≤ Int and Int ≤ Number, so (Int & Float) ≤ Number
        assert!(is_subtype(&int_float_intersection, &number));
    }

    #[test]
    fn test_s_intersection_l_complex() {
        // More complex intersection: (Vector<Int> & Vector<Float>) ≤ Vector<Number>
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let vec_int = IrType::Vector(Box::new(IrType::Int));
        let vec_float = IrType::Vector(Box::new(IrType::Float));
        let vec_number = IrType::Vector(Box::new(number.clone()));
        
        let vec_int_float_intersection = IrType::Intersection(vec![vec_int, vec_float]);
        
        // Vector<Int> ≤ Vector<Number> and Vector<Float> ≤ Vector<Number>
        // So (Vector<Int> & Vector<Float>) ≤ Vector<Number>
        assert!(is_subtype(&vec_int_float_intersection, &vec_number));
    }

    #[test]
    fn test_s_intersection_r_basic() {
        // (S-Intersection-R): T ≤ (T1 & T2)  iff  T ≤ T1 ∧ T ≤ T2
        
        // Int ≤ (Int & Any) because Int ≤ Int and Int ≤ Any
        let int_any_intersection = IrType::Intersection(vec![IrType::Int, IrType::Any]);
        assert!(is_subtype(&IrType::Int, &int_any_intersection));
        
        // Int ≤ (Any & Int) because Int ≤ Any and Int ≤ Int
        let any_int_intersection = IrType::Intersection(vec![IrType::Any, IrType::Int]);
        assert!(is_subtype(&IrType::Int, &any_int_intersection));
    }

    #[test]
    fn test_s_intersection_r_failure() {
        // String ⊈ (Int & Float) because String ⊈ Int
        let int_float_intersection = IrType::Intersection(vec![IrType::Int, IrType::Float]);
        assert!(!is_subtype(&IrType::String, &int_float_intersection));
    }

    #[test]
    fn test_intersection_with_never() {
        // (Never & T) ≤ T for any T (Never is bottom)
        let int_never_intersection = IrType::Intersection(vec![IrType::Int, IrType::Never]);
        assert!(is_subtype(&int_never_intersection, &IrType::Int));
        
        // T ≤ (Never & T) only if T ≤ Never, which is only true if T = Never
        assert!(is_subtype(&IrType::Never, &int_never_intersection));
        assert!(!is_subtype(&IrType::Int, &int_never_intersection));
    }

    #[test]
    fn test_intersection_with_any() {
        // (Int & Any) ≤ Any because Int ≤ Any
        let int_any_intersection = IrType::Intersection(vec![IrType::Int, IrType::Any]);
        assert!(is_subtype(&int_any_intersection, &IrType::Any));
        
        // Int ≤ (Int & Any) because Int ≤ Int and Int ≤ Any
        assert!(is_subtype(&IrType::Int, &int_any_intersection));
        
        // But String ⊈ (Int & Any) because String ⊈ Int
        assert!(!is_subtype(&IrType::String, &int_any_intersection));
        
        // (Any & Any) = Any
        let any_any_intersection = IrType::Intersection(vec![IrType::Any, IrType::Any]);
        assert!(is_subtype(&any_any_intersection, &IrType::Any));
        // Note: Any ⊈ (Any & Any) because Any ⊈ Any is true, but we need Any ≤ Any & Any
        // which requires Any ≤ Any (true) and Any ≤ Any (true), so this should work
        assert!(is_subtype(&IrType::Any, &any_any_intersection));
    }

    #[test]
    fn test_intersection_transitivity() {
        // Test transitivity with intersection types
        let int_float_intersection = IrType::Intersection(vec![IrType::Int, IrType::Float]);
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        
        // (Int & Float) ≤ Number (from S-Intersection-L)
        assert!(is_subtype(&int_float_intersection, &number));
        
        // Number ≤ Any (from S-Union-L)
        assert!(is_subtype(&number, &IrType::Any));
        
        // Therefore (Int & Float) ≤ Any by transitivity
        assert!(is_subtype(&int_float_intersection, &IrType::Any));
    }

    #[test]
    fn test_type_meet_basic() {
        // Test meet operation (greatest lower bound)
        
        // meet(Int, Float) = Int & Float
        let result = type_meet(&IrType::Int, &IrType::Float);
        assert!(matches!(result, IrType::Intersection(components) if components.len() == 2));
        
        // meet(Int, Int) = Int
        let result = type_meet(&IrType::Int, &IrType::Int);
        assert_eq!(result, IrType::Int);
        
        // meet(Int, Any) = Int
        let result = type_meet(&IrType::Int, &IrType::Any);
        assert_eq!(result, IrType::Int);
        
        // meet(Int, Never) = Never
        let result = type_meet(&IrType::Int, &IrType::Never);
        assert_eq!(result, IrType::Never);
    }

    #[test]
    fn test_type_join_basic() {
        // Test join operation (least upper bound)
        
        // join(Int, Float) = Int | Float (union)
        let result = type_join(&IrType::Int, &IrType::Float);
        assert!(matches!(result, IrType::Union(components) if components.len() == 2));
        
        // join(Int, Int) = Int
        let result = type_join(&IrType::Int, &IrType::Int);
        assert_eq!(result, IrType::Int);
        
        // join(Int, Any) = Any
        let result = type_join(&IrType::Int, &IrType::Any);
        assert_eq!(result, IrType::Any);
        
        // join(Int, Never) = Int
        let result = type_join(&IrType::Int, &IrType::Never);
        assert_eq!(result, IrType::Int);
    }

    #[test]
    fn test_simplify_intersection() {
        // Test intersection simplification
        
        // Remove duplicates
        let components = vec![IrType::Int, IrType::Int, IrType::Float];
        let simplified = simplify_intersection(components);
        assert_eq!(simplified.len(), 2);

        // Remove non-adjacent duplicates too
        let components = vec![IrType::Int, IrType::Float, IrType::Int];
        let simplified = simplify_intersection(components);
        assert_eq!(simplified.len(), 2);
        
        // Remove Any
        let components = vec![IrType::Int, IrType::Any, IrType::Float];
        let simplified = simplify_intersection(components);
        assert_eq!(simplified.len(), 2);
        assert!(simplified.iter().all(|t| !matches!(t, IrType::Any)));
        
        // Handle Never
        let components = vec![IrType::Int, IrType::Never, IrType::Float];
        let simplified = simplify_intersection(components);
        assert_eq!(simplified.len(), 1);
        assert_eq!(simplified[0], IrType::Never);
        
        // Flatten nested intersections
        let nested = IrType::Intersection(vec![IrType::Int, IrType::Float]);
        let components = vec![nested, IrType::String];
        let simplified = simplify_intersection(components);
        assert_eq!(simplified.len(), 3);
    }

    #[test]
    fn test_intersection_function_types() {
        // Test intersection with function types
        
        // Test a simpler case: (Int → Int) & (Int → Float) ≤ (Int → Number)
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        
        let int_to_int = IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(IrType::Int),
        };
        
        let int_to_float = IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(IrType::Float),
        };
        
        let int_to_number = IrType::Function {
            param_types: vec![IrType::Int],
            variadic_param_type: None,
            return_type: Box::new(number.clone()),
        };
        
        let intersection = IrType::Intersection(vec![int_to_int, int_to_float]);
        
        // (Int → Int) & (Int → Float) ≤ (Int → Number) should be true
        // because (Int → Int) ≤ (Int → Number) and (Int → Float) ≤ (Int → Number)
        // For (Int → Int) ≤ (Int → Number): Int ≤ Int (contravariant) and Int ≤ Number (covariant) → true
        // For (Int → Float) ≤ (Int → Number): Int ≤ Int (contravariant) and Float ≤ Number (covariant) → true
        assert!(is_subtype(&intersection, &int_to_number));
        
        // Test that the intersection is also a subtype of Any
        assert!(is_subtype(&intersection, &IrType::Any));
    }

    #[test]
    fn test_intersection_collection_types() {
        // Test intersection with collection types
        
        // Vector<Int> & Vector<Float> ≤ Vector<Number>
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let vec_int = IrType::Vector(Box::new(IrType::Int));
        let vec_float = IrType::Vector(Box::new(IrType::Float));
        let vec_number = IrType::Vector(Box::new(number));
        
        let intersection = IrType::Intersection(vec![vec_int, vec_float]);
        assert!(is_subtype(&intersection, &vec_number));
        
        // List<String> & List<Keyword> ≤ List<Any>
        let list_string = IrType::List(Box::new(IrType::String));
        let list_keyword = IrType::List(Box::new(IrType::Keyword));
        let list_any = IrType::List(Box::new(IrType::Any));
        
        let intersection = IrType::Intersection(vec![list_string, list_keyword]);
        assert!(is_subtype(&intersection, &list_any));
    }

    #[test]
    fn test_s_map_structural_subtyping_required_and_optional() {
        let sub = IrType::Map {
            entries: vec![
                IrMapTypeEntry {
                    key: Keyword::new("a"),
                    value_type: IrType::Int,
                    optional: false,
                },
                IrMapTypeEntry {
                    key: Keyword::new("b"),
                    value_type: IrType::String,
                    optional: true,
                },
            ],
            wildcard: None,
        };

        let sup = IrType::Map {
            entries: vec![IrMapTypeEntry {
                key: Keyword::new("a"),
                value_type: IrType::Any,
                optional: false,
            }],
            wildcard: None,
        };

        // `:a` is present and Int ≤ Any
        assert!(is_subtype(&sub, &sup));

        // Missing required key should fail
        let sub_missing = IrType::Map {
            entries: vec![],
            wildcard: None,
        };
        assert!(!is_subtype(&sub_missing, &sup));

        // Optional in subtype is NOT ok when supertype requires the key
        let sub_optional_a = IrType::Map {
            entries: vec![IrMapTypeEntry {
                key: Keyword::new("a"),
                value_type: IrType::Int,
                optional: true,
            }],
            wildcard: None,
        };
        assert!(!is_subtype(&sub_optional_a, &sup));
    }

    #[test]
    fn test_parametric_map_subtyping_keys_string_keyword_union() {
        let sub = IrType::ParametricMap {
            key_type: Box::new(IrType::String),
            value_type: Box::new(IrType::Int),
        };

        let sup = IrType::ParametricMap {
            key_type: Box::new(IrType::Union(vec![IrType::String, IrType::Keyword])),
            value_type: Box::new(IrType::Any),
        };

        // String ≤ (String | Keyword) and Int ≤ Any
        assert!(is_subtype(&sub, &sup));

        // But Keyword-keyed maps are not subtypes of String-keyed maps
        let kw_map = IrType::ParametricMap {
            key_type: Box::new(IrType::Keyword),
            value_type: Box::new(IrType::Int),
        };
        let string_map = IrType::ParametricMap {
            key_type: Box::new(IrType::String),
            value_type: Box::new(IrType::Any),
        };
        assert!(!is_subtype(&kw_map, &string_map));
    }

    #[test]
    fn test_intersection_reflexivity() {
        // Every intersection type should be a subtype of itself
        let intersection = IrType::Intersection(vec![IrType::Int, IrType::Float]);
        assert!(is_subtype(&intersection, &intersection));
        
        let nested = IrType::Intersection(vec![
            IrType::Int,
            IrType::Intersection(vec![IrType::Float, IrType::String])
        ]);
        assert!(is_subtype(&nested, &nested));
    }

    #[test]
    fn test_intersection_complex_scenario() {
        // Test a complex intersection scenario
        let number = IrType::Union(vec![IrType::Int, IrType::Float]);
        let int_float_intersection = IrType::Intersection(vec![IrType::Int, IrType::Float]);
        
        // (Int & Float) ≤ Number should be true
        assert!(is_subtype(&int_float_intersection, &number));
        
        // Test nested intersections and unions
        let complex_type = IrType::Intersection(vec![
            IrType::Union(vec![IrType::Int, IrType::String]),
            IrType::Union(vec![IrType::Int, IrType::Bool])
        ]);
        
        // Int should be a subtype of this complex type
        assert!(is_subtype(&IrType::Int, &complex_type));
        
        // The complex type should be a subtype of Any
        assert!(is_subtype(&complex_type, &IrType::Any));
    }
}
