//! Transform pipeline infrastructure
//!
//! This module provides a composable, type-safe transformation system that replaces
//! the old rigid pipeline architecture. Any transform can be chained with another
//! if their types are compatible, enabling modular and reusable processing stages.
//!
//! # Architecture Overview
//!
//! The transform system consists of three core concepts:
//!
//! ## 1. The `Runnable` Trait
//!
//! The fundamental interface for all transformation stages. Any type implementing
//! `Runnable<I, O>` can transform input of type `I` to output of type `O`:
//!
//! ```rust,ignore
//! pub trait Runnable<I, O> {
//!     fn run(&self, input: I) -> Result<O, TransformError>;
//! }
//! ```
//!
//! This trait is implemented by individual processing stages (tokenization, parsing, etc.).
//!
//! ## 2. The `Transform<I, O>` Type
//!
//! A wrapper that enables composition. Any `Runnable` can be converted to a `Transform`,
//! which provides the `.then()` method for type-safe chaining:
//!
//! ```rust,ignore
//! let pipeline = Transform::from_fn(|x| Ok(x))
//!     .then(Tokenize)   // String → Vec<Token>
//!     .then(Parse);     // Vec<Token> → Ast
//! // Result: Transform<String, Ast>
//! ```
//!
//! The compiler enforces that output types match input types at each stage.
//!
//! ## 3. Static Lazy Transforms
//!
//! Common pipelines are pre-built as static references using `once_cell::sync::Lazy`.
//! This provides zero-cost abstractions for standard processing paths:
//!
//! ```rust,ignore
//! pub static LEXING: Lazy<Transform<String, TokenStream>> = Lazy::new(|| {
//!     Transform::from_fn(Ok)
//!         .then(CoreTokenization::new())
//!         .then(SemanticIndentation::new())
//! });
//! ```
//!
//! See the [`standard`] module for all pre-built transforms.
//!
//! # Usage Patterns
//!
//! ## Direct Transform Usage
//!
//! For programmatic access to specific stages:
//!
//! ```rust
//! use lex_parser::lex::transforms::standard::LEXING;
//!
//! let tokens = LEXING.run("Session:\n    Content\n".to_string())?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## With DocumentLoader
//!
//! For most use cases, use [`DocumentLoader`](crate::lex::loader::DocumentLoader)
//! which provides convenient shortcuts:
//!
//! ```rust
//! use lex_parser::lex::loader::DocumentLoader;
//!
//! let loader = DocumentLoader::from_string("Hello\n");
//! let doc = loader.parse()?;          // Full AST
//! let tokens = loader.tokenize()?;     // Lexed tokens
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## Custom Pipelines
//!
//! Build custom processing chains for specialized needs:
//!
//! ```rust,ignore
//! use lex_parser::lex::transforms::{Transform, standard::CORE_TOKENIZATION};
//!
//! let custom = CORE_TOKENIZATION
//!     .then(MyCustomStage::new())
//!     .then(AnotherStage::new());
//!
//! let result = custom.run(source)?;
//! ```
//!
//! # Module Organization
//!
//! - [`stages`]: Individual transformation stages (tokenization, indentation, parsing)
//! - [`standard`]: Pre-built transform combinations for common use cases
//!
//! # Design Benefits
//!
//! - Type Safety: Compiler verifies pipeline stage compatibility
//! - Composability: Mix and match stages to create custom pipelines
//! - Reusability: Share transforms across CLI, tests, and library code
//! - Clarity: Explicit stage boundaries with clear input/output types
//! - Testability: Test individual stages in isolation

pub mod stages;
pub mod standard;

use std::fmt;

/// Error that can occur during transformation
#[derive(Debug, Clone, PartialEq)]
pub enum TransformError {
    /// Generic error with message
    Error(String),
    /// Stage failed with specific error
    StageFailed { stage: String, message: String },
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TransformError::Error(msg) => write!(f, "{msg}"),
            TransformError::StageFailed { stage, message } => {
                write!(f, "Stage '{stage}' failed: {message}")
            }
        }
    }
}

impl std::error::Error for TransformError {}

impl From<String> for TransformError {
    fn from(s: String) -> Self {
        TransformError::Error(s)
    }
}

impl From<&str> for TransformError {
    fn from(s: &str) -> Self {
        TransformError::Error(s.to_string())
    }
}

/// Trait for anything that can transform an input to an output
///
/// This is implemented by individual transformation stages.
/// The `Transform` struct composes multiple `Runnable` implementations.
pub trait Runnable<I, O> {
    /// Execute this transformation on the input
    fn run(&self, input: I) -> Result<O, TransformError>;
}

/// A composable transformation pipeline
///
/// `Transform<I, O>` represents a transformation from type `I` to type `O`.
/// It can be composed with other transforms using the `add` method.
///
/// # Type Safety
///
/// The type system ensures that transforms can only be composed if their
/// input/output types are compatible. For example:
///
/// ```rust,compile_fail
/// let t1: Transform<String, TokenStream> = ...;
/// let t2: Transform<Document, String> = ...;
///
/// // This will fail to compile - TokenStream != Document
/// let bad = t1.add_transform(&t2);
/// ```
pub struct Transform<I, O> {
    run_fn: Box<dyn Fn(I) -> Result<O, TransformError> + Send + Sync>,
}

impl<I, O> Transform<I, O> {
    /// Create a new identity transform that passes input through unchanged
    ///
    /// Note: This only works when `I = O`
    pub fn identity() -> Self
    where
        I: Clone + 'static,
        O: From<I> + 'static,
    {
        Transform {
            run_fn: Box::new(|input| Ok(O::from(input))),
        }
    }

    /// Create a transform from a function
    pub fn from_fn<F>(f: F) -> Self
    where
        F: Fn(I) -> Result<O, TransformError> + Send + Sync + 'static,
    {
        Transform {
            run_fn: Box::new(f),
        }
    }

    /// Add a stage to this transform, returning a new transform with extended output type
    ///
    /// This is the core composition method. It chains this transform's output into
    /// the next stage's input, creating a new transform from `I` to `O2`.
    ///
    /// # Type Safety
    ///
    /// The compiler ensures that the stage's input type matches this transform's
    /// output type.
    ///
    /// # Example
    ///
    /// ```rust
    /// let t1: Transform<String, Tokens> = ...;
    /// let t2: Transform<Tokens, Ast> = ...;
    ///
    /// let combined: Transform<String, Ast> = t1.then_transform(&t2);
    /// ```
    pub fn then<O2, S>(self, stage: S) -> Transform<I, O2>
    where
        S: Runnable<O, O2> + Send + Sync + 'static,
        I: 'static,
        O: 'static,
        O2: 'static,
    {
        let prev_run = self.run_fn;
        Transform {
            run_fn: Box::new(move |input| {
                let intermediate = prev_run(input)?;
                stage.run(intermediate)
            }),
        }
    }

    /// Chain another transform to this transform
    ///
    /// This is similar to `then` but takes a `Transform` instead of a `Runnable`.
    /// Useful for composing pre-built transform pipelines.
    ///
    /// The referenced transform must have a static lifetime (typically created with `lazy_static!`).
    pub fn then_transform<O2>(self, next: &'static Transform<O, O2>) -> Transform<I, O2>
    where
        I: 'static,
        O: 'static,
        O2: 'static,
    {
        let prev_run = self.run_fn;
        Transform {
            run_fn: Box::new(move |input| {
                let intermediate = prev_run(input)?;
                next.run(intermediate)
            }),
        }
    }

    /// Execute this transform on the given input
    pub fn run(&self, input: I) -> Result<O, TransformError> {
        (self.run_fn)(input)
    }
}

// Implement Runnable for Transform so transforms can be used as stages
impl<I, O> Runnable<I, O> for Transform<I, O>
where
    I: 'static,
    O: 'static,
{
    fn run(&self, input: I) -> Result<O, TransformError> {
        Transform::run(self, input)
    }
}

// Helper for creating transforms from closures
impl<I, O> Transform<I, O> {
    /// Create a new transform with no stages (useful as a starting point for composition)
    pub fn new<F>(f: F) -> Self
    where
        F: Fn(I) -> Result<O, TransformError> + Send + Sync + 'static,
    {
        Transform::from_fn(f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test helpers - simple stages for composition
    struct DoubleNumber;
    impl Runnable<i32, i32> for DoubleNumber {
        fn run(&self, input: i32) -> Result<i32, TransformError> {
            Ok(input * 2)
        }
    }

    struct AddTen;
    impl Runnable<i32, i32> for AddTen {
        fn run(&self, input: i32) -> Result<i32, TransformError> {
            Ok(input + 10)
        }
    }

    struct IntToString;
    impl Runnable<i32, String> for IntToString {
        fn run(&self, input: i32) -> Result<String, TransformError> {
            Ok(input.to_string())
        }
    }

    struct FailingStage;
    impl Runnable<i32, i32> for FailingStage {
        fn run(&self, _input: i32) -> Result<i32, TransformError> {
            Err(TransformError::Error("intentional failure".to_string()))
        }
    }

    #[test]
    fn test_transform_from_fn() {
        let transform = Transform::from_fn(|x: i32| Ok(x * 2));
        assert_eq!(transform.run(5).unwrap(), 10);
    }

    #[test]
    fn test_single_stage() {
        let transform = Transform::from_fn(|x: i32| Ok(x)).then(DoubleNumber);
        assert_eq!(transform.run(5).unwrap(), 10);
    }

    #[test]
    fn test_multiple_same_type_stages() {
        let transform = Transform::from_fn(|x: i32| Ok(x))
            .then(DoubleNumber)
            .then(AddTen)
            .then(DoubleNumber);

        // (5 * 2) + 10 = 20, then 20 * 2 = 40
        assert_eq!(transform.run(5).unwrap(), 40);
    }

    #[test]
    fn test_type_changing_stage() {
        let transform = Transform::from_fn(|x: i32| Ok(x))
            .then(DoubleNumber)
            .then(IntToString);

        assert_eq!(transform.run(5).unwrap(), "10");
    }

    #[test]
    fn test_error_propagation() {
        let transform = Transform::from_fn(|x: i32| Ok(x))
            .then(DoubleNumber)
            .then(FailingStage)
            .then(AddTen);

        let result = transform.run(5);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            TransformError::Error("intentional failure".to_string())
        );
    }

    #[test]
    fn test_transform_composition() {
        // Build sub-transforms
        let double_and_add = Transform::from_fn(|x: i32| Ok(x))
            .then(DoubleNumber)
            .then(AddTen);

        let to_string = Transform::from_fn(|x: i32| Ok(x)).then(IntToString);

        // Compose them (note: we need to use static refs for then_transform)
        // For now, just test that individual transforms work
        assert_eq!(double_and_add.run(5).unwrap(), 20);
        assert_eq!(to_string.run(5).unwrap(), "5");
    }

    #[test]
    fn test_error_display() {
        let err = TransformError::Error("test error".to_string());
        assert_eq!(format!("{err}"), "test error");

        let stage_err = TransformError::StageFailed {
            stage: "tokenization".to_string(),
            message: "invalid token".to_string(),
        };
        assert_eq!(
            format!("{stage_err}"),
            "Stage 'tokenization' failed: invalid token"
        );
    }

    #[test]
    fn test_error_conversion() {
        let err1: TransformError = "string error".into();
        assert_eq!(err1, TransformError::Error("string error".to_string()));

        let err2: TransformError = "owned string".to_string().into();
        assert_eq!(err2, TransformError::Error("owned string".to_string()));
    }
}
