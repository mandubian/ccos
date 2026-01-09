//! Type Unification for RTFS
//!
//! This module implements a simple unification algorithm for type inference.
//! The algorithm handles type variables and concrete types, enabling parametric
//! polymorphism in RTFS.

use std::collections::HashMap;
use crate::ir::core::IrType;

/// A substitution maps type variables to concrete types
pub type Substitution = HashMap<String, IrType>;

/// Type unification error
#[derive(Debug, Clone, PartialEq)]
pub enum UnificationError {
    /// Occurs when trying to unify incompatible types
    IncompatibleTypes(IrType, IrType),
    /// Occurs when a type variable is bound to itself (circular)
    CircularVariable(String),
    /// Occurs when trying to unify with an unsupported type
    UnsupportedType(String),
}

impl std::fmt::Display for UnificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UnificationError::IncompatibleTypes(t1, t2) => {
                write!(f, "Cannot unify types {:?} and {:?}", t1, t2)
            }
            UnificationError::CircularVariable(name) => {
                write!(f, "Circular type variable: {}", name)
            }
            UnificationError::UnsupportedType(msg) => {
                write!(f, "Unsupported type for unification: {}", msg)
            }
        }
    }
}

impl std::error::Error for UnificationError {}

/// Unify two types, returning a substitution if successful
pub fn unify(t1: &IrType, t2: &IrType) -> Result<Substitution, UnificationError> {
    unify_with_substitution(t1, t2, &Substitution::new())
}

/// Unify two types with an existing substitution
fn unify_with_substitution(
    t1: &IrType,
    t2: &IrType,
    subst: &Substitution,
) -> Result<Substitution, UnificationError> {
    match (apply_substitution(t1, subst), apply_substitution(t2, subst)) {
        // Both types are the same - no substitution needed
        (t1_applied, t2_applied) if t1_applied == t2_applied => Ok(Substitution::new()),

        // Type variable cases
        (IrType::TypeVar(name), t) => bind_variable(&name, &t),
        (t, IrType::TypeVar(name)) => bind_variable(&name, &t),

        // Concrete type cases - check compatibility
        (IrType::Int, IrType::Float) | (IrType::Float, IrType::Int) => {
            // Numeric types can unify to Number (union type)
            Ok(Substitution::new())
        }

        // Collection types
        (IrType::Vector(elem1), IrType::Vector(elem2)) => {
            unify_with_substitution(&elem1, &elem2, subst)
        }
        (IrType::List(elem1), IrType::List(elem2)) => {
            unify_with_substitution(&elem1, &elem2, subst)
        }

        // Parametric maps
        (IrType::ParametricMap { key_type: k1, value_type: v1 }, 
         IrType::ParametricMap { key_type: k2, value_type: v2 }) => {
            let key_subst = unify_with_substitution(&k1, &k2, subst)?;
            let value_subst = unify_with_substitution(&v1, &v2, subst)?;
            merge_substitutions(key_subst, value_subst)
        }

        // Unsupported cases
        (t1, t2) => Err(UnificationError::IncompatibleTypes(t1, t2)),
    }
}

/// Bind a type variable to a concrete type
fn bind_variable(name: &str, t: &IrType) -> Result<Substitution, UnificationError> {
    // Check for circular binding (variable bound to itself)
    if let IrType::TypeVar(var_name) = t {
        if var_name == name {
            return Err(UnificationError::CircularVariable(name.to_string()));
        }
    }

    // Check if the type contains the variable (occurs check)
    if contains_variable(t, name) {
        return Err(UnificationError::CircularVariable(name.to_string()));
    }

    let mut subst = Substitution::new();
    subst.insert(name.to_string(), t.clone());
    Ok(subst)
}

/// Apply a substitution to a type
pub fn apply_substitution(t: &IrType, subst: &Substitution) -> IrType {
    match t {
        IrType::TypeVar(name) => {
            if let Some(t) = subst.get(name) {
                apply_substitution(t, subst)
            } else {
                t.clone()
            }
        }
        IrType::Vector(elem) => {
            IrType::Vector(Box::new(apply_substitution(elem, subst)))
        }
        IrType::List(elem) => {
            IrType::List(Box::new(apply_substitution(elem, subst)))
        }
        IrType::ParametricMap { key_type, value_type } => {
            IrType::ParametricMap {
                key_type: Box::new(apply_substitution(key_type, subst)),
                value_type: Box::new(apply_substitution(value_type, subst)),
            }
        }
        // For other types, no substitution needed
        _ => t.clone(),
    }
}

/// Check if a type contains a specific variable
fn contains_variable(t: &IrType, var_name: &str) -> bool {
    match t {
        IrType::TypeVar(name) => name == var_name,
        IrType::Vector(elem) => contains_variable(elem, var_name),
        IrType::List(elem) => contains_variable(elem, var_name),
        IrType::ParametricMap { key_type, value_type } => {
            contains_variable(key_type, var_name) || contains_variable(value_type, var_name)
        }
        _ => false,
    }
}

/// Merge two substitutions
fn merge_substitutions(mut s1: Substitution, s2: Substitution) -> Result<Substitution, UnificationError> {
    for (var, ty) in s2 {
        if let Some(existing) = s1.get(&var) {
            // Check that the existing binding is compatible with the new one
            let subst = unify(existing, &ty)?;
            if !subst.is_empty() {
                // There's a conflict - apply the substitution to resolve it
                s1 = apply_substitution_to_all(existing, &subst, s1)?;
            }
        }
        s1.insert(var, ty);
    }
    Ok(s1)
}

/// Apply substitution to all bindings in a substitution
fn apply_substitution_to_all(
    _original: &IrType,
    _subst: &Substitution,
    mut s: Substitution,
) -> Result<Substitution, UnificationError> {
    // Apply substitution to all values in the substitution
    for (_, ty) in s.iter_mut() {
        *ty = apply_substitution(ty, subst);
    }
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ir::core::IrType;

    #[test]
    fn test_unify_same_types() {
        let result = unify(&IrType::Int, &IrType::Int);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_unify_type_variable() {
        let result = unify(&IrType::TypeVar("T".to_string()), &IrType::Int);
        assert!(result.is_ok());
        let subst = result.unwrap();
        assert_eq!(subst.get("T"), Some(&IrType::Int));
    }

    #[test]
    fn test_unify_circular_variable() {
        let result = unify(
            &IrType::TypeVar("T".to_string()),
            &IrType::TypeVar("T".to_string())
        );
        assert!(matches!(result, Err(UnificationError::CircularVariable(_))));
    }

    #[test]
    fn test_unify_incompatible_types() {
        let result = unify(&IrType::Int, &IrType::String);
        assert!(matches!(result, Err(UnificationError::IncompatibleTypes(_, _))));
    }

    #[test]
    fn test_unify_vector_types() {
        let result = unify(
            &IrType::Vector(Box::new(IrType::Int)),
            &IrType::Vector(Box::new(IrType::TypeVar("T".to_string())))
        );
        assert!(result.is_ok());
        let subst = result.unwrap();
        assert_eq!(subst.get("T"), Some(&IrType::Int));
    }

    #[test]
    fn test_apply_substitution() {
        let mut subst = Substitution::new();
        subst.insert("T".to_string(), IrType::Int);
        
        let result = apply_substitution(&IrType::TypeVar("T".to_string()), &subst);
        assert_eq!(result, IrType::Int);
    }

    #[test]
    fn test_parametric_map_unification() {
        let map1 = IrType::ParametricMap {
            key_type: Box::new(IrType::String),
            value_type: Box::new(IrType::Int),
        };
        let map2 = IrType::ParametricMap {
            key_type: Box::new(IrType::String),
            value_type: Box::new(IrType::TypeVar("T".to_string())),
        };
        
        let result = unify(&map1, &map2);
        assert!(result.is_ok());
        let subst = result.unwrap();
        assert_eq!(subst.get("T"), Some(&IrType::Int));
    }
}
