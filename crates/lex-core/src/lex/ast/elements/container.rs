//! Rich, type-safe containers for AST tree traversal and querying
//!
//! # Design Philosophy
//!
//! This module implements **rich containers** - child-bearing nodes that are self-sufficient
//! for all tree traversal and querying operations. This design makes container capabilities
//! orthogonal to element type: any element using a Container<P> automatically gets the full
//! query API without code duplication.
//!
//! ## Core Abstractions
//!
//! **Container<P>**: Generic container parameterized by a policy type P
//! - Stores children as Vec<ContentItem>
//! - Policy P determines nesting rules at compile time
//! - Provides rich traversal, querying, and search APIs
//! - Self-sufficient: all generic operations live here, not in element types
//!
//! **ContainerPolicy**: Trait defining what content is allowed
//! - SessionPolicy: allows Sessions (unlimited nesting)
//! - GeneralPolicy: no Sessions (prevents infinite nesting)
//! - ListPolicy: only ListItem variants
//! - VerbatimPolicy: only VerbatimLine nodes
//!
//! ## Container Types
//!
//! - **SessionContainer** = Container<SessionPolicy>
//!   - Used by: Document.root, Session.children
//!   - Allows: All ContentItem types including nested Sessions
//!
//! - **GeneralContainer** = Container<GeneralPolicy>
//!   - Used by: Definition.children, Annotation.children, ListItem.children
//!   - Allows: All ContentItem EXCEPT Sessions
//!
//! - **ListContainer** = Container<ListPolicy>
//!   - Used by: List.items
//!   - Allows: Only ListItem variants (homogeneous)
//!
//! - **VerbatimContainer** = Container<VerbatimPolicy>
//!   - Used by: VerbatimBlock.children
//!   - Allows: Only VerbatimLine nodes (homogeneous)
//!
//! ## Rich Query API
//!
//! All Container<P> types provide:
//!
//! **Direct child iteration:**
//! ```ignore
//! container.iter()                    // All immediate children
//! container.iter_paragraphs()         // Only paragraph children
//! container.iter_sessions()           // Only session children (if allowed by policy)
//! ```
//!
//! **Recursive traversal:**
//! ```ignore
//! container.iter_all_nodes()                  // Depth-first pre-order
//! container.iter_all_nodes_with_depth()       // With depth tracking
//! container.iter_paragraphs_recursive()       // All paragraphs at any depth
//! container.iter_sessions_recursive()         // All sessions at any depth
//! ```
//!
//! **Searching and filtering:**
//! ```ignore
//! container.find_nodes(|n| n.is_paragraph())              // Generic predicate
//! container.find_paragraphs(|p| p.text().contains("foo")) // Type-specific
//! container.find_nodes_at_depth(2)                        // By depth
//! container.find_nodes_in_depth_range(1, 3)               // Depth range
//! ```
//!
//! **Convenience methods:**
//! ```ignore
//! container.first_paragraph()     // Option<&Paragraph>
//! container.expect_paragraph()    // &Paragraph (panics if not found)
//! container.count_by_type()       // (paragraphs, sessions, lists, verbatim)
//! ```
//!
//! **Position-based queries:**
//! ```ignore
//! container.element_at(pos)               // Deepest element at position
//! container.find_nodes_at_position(pos)   // All nodes containing position
//! container.format_at_position(pos)       // Human-readable description
//! ```
//!
//! ## Benefits of This Design
//!
//! 1. **No code duplication**: Session, Definition, Annotation, ListItem all get the same
//!    rich API through their container field - no need to implement 400+ LOC in each.
//!
//! 2. **Type safety**: Policy system enforces nesting rules at compile time.
//!    Cannot accidentally put a Session in a Definition.
//!
//! 3. **Clear structure**: Container's job is child management + querying.
//!    Element's job is domain-specific behavior (title, subject, annotations, etc.).
//!
//! 4. **Uniform access**: Any code working with containers can use the same API
//!    regardless of whether it's a Session, Definition, or Annotation.
//!
//! ## Accessing Container Children
//!
//! The `.children` field is private. Use one of these access patterns:
//!
//! **Deref coercion (preferred for Vec operations):**
//! ```ignore
//! let session = Session::new(...);
//! for child in &session.children {  // Deref to &Vec<ContentItem>
//!     // process child
//! }
//! let count = session.children.len();  // Works via Deref
//! ```
//!
//! **ContentItem polymorphic access:**
//! ```ignore
//! fn process(item: &ContentItem) {
//!     if let Some(children) = item.children() {
//!         // Access children polymorphically
//!     }
//! }
//! ```
//!
//! **Container trait:**
//! ```ignore
//! fn process<T: Container>(container: &T) {
//!     let children = container.children();  // Returns &[ContentItem]
//! }
//! ```
//!
//! ## Implementation Notes
//!
//! - Macros generate repetitive iterator/finder methods (see bottom of file)
//! - All traversal builds on ContentItem::descendants() primitive
//! - Depth is 0-indexed from immediate children
//! - Position queries return deepest (most nested) matching element
//!
//! See `docs/architecture/type-safe-containers.md` for compile-time safety guarantees.

use super::super::range::{Position, Range};
use super::super::traits::{AstNode, Visitor};
use super::annotation::Annotation;
use super::content_item::ContentItem;
use super::definition::Definition;
use super::list::{List, ListItem};
use super::paragraph::Paragraph;
use super::session::Session;
use super::typed_content::{ContentElement, ListContent, SessionContent, VerbatimContent};
use super::verbatim::Verbatim;
use std::fmt;
use std::marker::PhantomData;

// ============================================================================
// MACROS FOR GENERATING REPETITIVE ITERATOR/FINDER METHODS
// ============================================================================

/// Macro to generate recursive iterator methods for different AST node types
macro_rules! impl_recursive_iterator {
    ($method_name:ident, $type:ty, $as_method:ident, $doc:expr) => {
        #[doc = $doc]
        pub fn $method_name(&self) -> Box<dyn Iterator<Item = &$type> + '_> {
            Box::new(self.iter_all_nodes().filter_map(|item| item.$as_method()))
        }
    };
}

/// Macro to generate "first" convenience methods
macro_rules! impl_first_method {
    ($method_name:ident, $type:ty, $iter_method:ident, $doc:expr) => {
        #[doc = $doc]
        pub fn $method_name(&self) -> Option<&$type> {
            self.$iter_method().next()
        }
    };
}

/// Macro to generate predicate-based finder methods
macro_rules! impl_find_method {
    ($method_name:ident, $type:ty, $iter_method:ident, $doc:expr) => {
        #[doc = $doc]
        pub fn $method_name<F>(&self, predicate: F) -> Vec<&$type>
        where
            F: Fn(&$type) -> bool,
        {
            self.$iter_method().filter(|x| predicate(x)).collect()
        }
    };
}

// ============================================================================
// CONTAINER POLICY TRAITS
// ============================================================================

/// Policy trait defining what content is allowed in a container.
///
/// This trait provides compile-time information about nesting rules.
/// Each policy type defines which element types can be contained.
pub trait ContainerPolicy: 'static {
    /// The typed content variant this policy accepts
    type ContentType: Into<ContentItem> + Clone;

    /// Whether this container allows Session elements
    const ALLOWS_SESSIONS: bool;

    /// Whether this container allows Annotation elements
    const ALLOWS_ANNOTATIONS: bool;

    /// Human-readable name for error messages
    const POLICY_NAME: &'static str;

    /// Validate that an item is allowed in this container
    ///
    /// Returns Ok(()) if the item is valid, or an error message if not.
    fn validate(item: &ContentItem) -> Result<(), String>;
}

/// Policy for Session containers - allows all elements including Sessions
///
/// Used by: Document.root, Session.children
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy;

impl ContainerPolicy for SessionPolicy {
    type ContentType = SessionContent;

    const ALLOWS_SESSIONS: bool = true;
    const ALLOWS_ANNOTATIONS: bool = true;
    const POLICY_NAME: &'static str = "SessionPolicy";

    fn validate(_item: &ContentItem) -> Result<(), String> {
        // SessionPolicy allows all content types
        Ok(())
    }
}

/// Policy for general containers - allows all elements EXCEPT Sessions
///
/// Used by: Definition.children, Annotation.children, ListItem.children
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GeneralPolicy;

impl ContainerPolicy for GeneralPolicy {
    type ContentType = ContentElement;

    const ALLOWS_SESSIONS: bool = false;
    const ALLOWS_ANNOTATIONS: bool = true;
    const POLICY_NAME: &'static str = "GeneralPolicy";

    fn validate(item: &ContentItem) -> Result<(), String> {
        match item {
            ContentItem::Session(_) => Err("GeneralPolicy does not allow Sessions".to_string()),
            ContentItem::ListItem(_) => {
                Err("GeneralPolicy does not allow ListItems (use List instead)".to_string())
            }
            _ => Ok(()),
        }
    }
}

/// Policy for list containers - only allows ListItem elements
///
/// Used by: List.items
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListPolicy;

impl ContainerPolicy for ListPolicy {
    type ContentType = ListContent;

    const ALLOWS_SESSIONS: bool = false;
    const ALLOWS_ANNOTATIONS: bool = false;
    const POLICY_NAME: &'static str = "ListPolicy";

    fn validate(item: &ContentItem) -> Result<(), String> {
        match item {
            ContentItem::ListItem(_) => Ok(()),
            _ => Err(format!(
                "ListPolicy only allows ListItems, found {}",
                item.node_type()
            )),
        }
    }
}

/// Policy for verbatim containers - only allows VerbatimLine elements
///
/// Used by: VerbatimBlock.children
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VerbatimPolicy;

impl ContainerPolicy for VerbatimPolicy {
    type ContentType = VerbatimContent;

    const ALLOWS_SESSIONS: bool = false;
    const ALLOWS_ANNOTATIONS: bool = false;
    const POLICY_NAME: &'static str = "VerbatimPolicy";

    fn validate(item: &ContentItem) -> Result<(), String> {
        match item {
            ContentItem::VerbatimLine(_) => Ok(()),
            _ => Err(format!(
                "VerbatimPolicy only allows VerbatimLines, found {}",
                item.node_type()
            )),
        }
    }
}

// ============================================================================
// CONTAINER TYPES
// ============================================================================

/// Generic container with compile-time policy enforcement
///
/// The policy type parameter P determines what content is allowed in this container.
/// See the ContainerPolicy trait for available policies.
#[derive(Debug, Clone, PartialEq)]
pub struct Container<P: ContainerPolicy> {
    children: Vec<ContentItem>,
    pub location: Range,
    _policy: PhantomData<P>,
}

// ============================================================================
// TYPE ALIASES
// ============================================================================

/// SessionContainer allows any ContentItem including nested Sessions
///
/// Used for document-level containers where unlimited Session nesting is allowed.
pub type SessionContainer = Container<SessionPolicy>;

/// GeneralContainer allows any ContentItem EXCEPT Sessions
///
/// Used for Definition, Annotation, and ListItem children where Session nesting
/// is prohibited.
pub type GeneralContainer = Container<GeneralPolicy>;

/// ListContainer is a homogeneous container for ListItem variants only
///
/// Used by List.items to enforce that lists only contain list items.
pub type ListContainer = Container<ListPolicy>;

/// VerbatimContainer is a homogeneous container for VerbatimLine nodes only
///
/// Used by VerbatimBlock.children to enforce that verbatim blocks only contain
/// verbatim lines (content from other formats).
pub type VerbatimContainer = Container<VerbatimPolicy>;

// ============================================================================
// GENERIC CONTAINER IMPLEMENTATION
// ============================================================================

impl<P: ContainerPolicy> Container<P> {
    /// Create a type-safe container from typed content
    ///
    /// This is the preferred way to create containers as it enforces nesting rules
    /// at compile time via the policy's ContentType.
    ///
    /// # Status
    ///
    /// Element constructors (Session::new, Definition::new, Annotation::new) now accept
    /// typed content directly. This helper remains useful for tests or manual AST
    /// construction where callers want explicit control over container policies.
    pub fn from_typed(children: Vec<P::ContentType>) -> Self {
        Self {
            children: children.into_iter().map(|c| c.into()).collect(),
            location: Range::default(),
            _policy: PhantomData,
        }
    }

    /// Create an empty container
    pub fn empty() -> Self {
        Self {
            children: Vec::new(),
            location: Range::default(),
            _policy: PhantomData,
        }
    }

    /// Set the location for this container (builder pattern)
    pub fn at(mut self, location: Range) -> Self {
        self.location = location;
        self
    }

    /// Get the number of children
    pub fn len(&self) -> usize {
        self.children.len()
    }

    /// Check if the container is empty
    pub fn is_empty(&self) -> bool {
        self.children.is_empty()
    }

    /// Add a child to the container (type-safe, preferred method)
    ///
    /// This method accepts the policy's typed content, ensuring compile-time safety.
    ///
    /// # Example
    /// ```ignore
    /// let mut container = GeneralContainer::empty();
    /// let para = Paragraph::from_line("text".to_string());
    /// container.push_typed(ContentElement::Paragraph(para));
    /// ```
    pub fn push_typed(&mut self, item: P::ContentType) {
        self.children.push(item.into());
    }

    /// Add a child to the container with runtime validation
    ///
    /// This method accepts any ContentItem and validates it against the policy at runtime.
    /// Use this when you need polymorphic access to containers.
    ///
    /// # Panics
    /// Panics if the item violates the container's policy (e.g., adding a Session to a GeneralContainer).
    ///
    /// # Example
    /// ```ignore
    /// let mut container = GeneralContainer::empty();
    /// let para = Paragraph::from_line("text".to_string());
    /// container.push(ContentItem::Paragraph(para));  // OK
    ///
    /// let session = Session::with_title("Title".to_string());
    /// container.push(ContentItem::Session(session));  // Panics!
    /// ```
    pub fn push(&mut self, item: ContentItem) {
        P::validate(&item)
            .unwrap_or_else(|err| panic!("Invalid item for {}: {}", P::POLICY_NAME, err));
        self.children.push(item);
    }

    /// Extend the container with multiple items (with validation)
    ///
    /// # Panics
    /// Panics if any item violates the container's policy.
    pub fn extend<I>(&mut self, items: I)
    where
        I: IntoIterator<Item = ContentItem>,
    {
        for item in items {
            self.push(item);
        }
    }

    /// Get a specific child by index
    pub fn get(&self, index: usize) -> Option<&ContentItem> {
        self.children.get(index)
    }

    /// Get a mutable reference to a specific child
    ///
    /// Note: This allows mutation of the child itself, but not replacement with a different type.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut ContentItem> {
        self.children.get_mut(index)
    }

    /// Remove all children from the container
    pub fn clear(&mut self) {
        self.children.clear();
    }

    /// Remove and return the child at the specified index
    ///
    /// # Panics
    /// Panics if index is out of bounds.
    pub fn remove(&mut self, index: usize) -> ContentItem {
        self.children.remove(index)
    }

    /// Get an iterator over the children
    pub fn iter(&self) -> std::slice::Iter<'_, ContentItem> {
        self.children.iter()
    }

    /// Get a mutable iterator over the children
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, ContentItem> {
        self.children.iter_mut()
    }

    /// Get mutable access to the underlying Vec for advanced operations
    ///
    /// # Safety
    /// This method is intended for internal use by assembler stages that need
    /// Vec-specific operations like `retain()`. Direct manipulation bypasses
    /// policy validation - use with caution.
    ///
    /// Prefer `push()`, `push_typed()`, or other validated methods when possible.
    pub fn as_mut_vec(&mut self) -> &mut Vec<ContentItem> {
        &mut self.children
    }

    // ========================================================================
    // DIRECT CHILD ITERATION (non-recursive, immediate children only)
    // ========================================================================

    /// Iterate over immediate paragraph children
    pub fn iter_paragraphs(&self) -> impl Iterator<Item = &Paragraph> {
        self.children.iter().filter_map(|item| item.as_paragraph())
    }

    /// Iterate over immediate session children
    pub fn iter_sessions(&self) -> impl Iterator<Item = &Session> {
        self.children.iter().filter_map(|item| item.as_session())
    }

    /// Iterate over immediate list children
    pub fn iter_lists(&self) -> impl Iterator<Item = &List> {
        self.children.iter().filter_map(|item| item.as_list())
    }

    /// Iterate over immediate verbatim block children
    pub fn iter_verbatim_blocks(&self) -> impl Iterator<Item = &Verbatim> {
        self.children
            .iter()
            .filter_map(|item| item.as_verbatim_block())
    }

    /// Iterate over immediate definition children
    pub fn iter_definitions(&self) -> impl Iterator<Item = &Definition> {
        self.children.iter().filter_map(|item| item.as_definition())
    }

    /// Iterate over immediate annotation children
    pub fn iter_annotations(&self) -> impl Iterator<Item = &Annotation> {
        self.children.iter().filter_map(|item| item.as_annotation())
    }

    // ========================================================================
    // RECURSIVE TRAVERSAL
    // ========================================================================

    /// Iterate all nodes in the container tree (depth-first pre-order traversal)
    pub fn iter_all_nodes(&self) -> Box<dyn Iterator<Item = &ContentItem> + '_> {
        Box::new(
            self.children
                .iter()
                .flat_map(|item| std::iter::once(item).chain(item.descendants())),
        )
    }

    /// Iterate all nodes with their depth (0 = immediate children)
    pub fn iter_all_nodes_with_depth(
        &self,
    ) -> Box<dyn Iterator<Item = (&ContentItem, usize)> + '_> {
        Box::new(
            self.children
                .iter()
                .flat_map(|item| std::iter::once((item, 0)).chain(item.descendants_with_depth(1))),
        )
    }

    impl_recursive_iterator!(
        iter_paragraphs_recursive,
        Paragraph,
        as_paragraph,
        "Recursively iterate all paragraphs at any depth in the container"
    );
    impl_recursive_iterator!(
        iter_sessions_recursive,
        Session,
        as_session,
        "Recursively iterate all sessions at any depth in the container"
    );
    impl_recursive_iterator!(
        iter_lists_recursive,
        List,
        as_list,
        "Recursively iterate all lists at any depth in the container"
    );
    impl_recursive_iterator!(
        iter_verbatim_blocks_recursive,
        Verbatim,
        as_verbatim_block,
        "Recursively iterate all verbatim blocks at any depth in the container"
    );
    impl_recursive_iterator!(
        iter_list_items_recursive,
        ListItem,
        as_list_item,
        "Recursively iterate all list items at any depth in the container"
    );
    impl_recursive_iterator!(
        iter_definitions_recursive,
        Definition,
        as_definition,
        "Recursively iterate all definitions at any depth in the container"
    );
    impl_recursive_iterator!(
        iter_annotations_recursive,
        Annotation,
        as_annotation,
        "Recursively iterate all annotations at any depth in the container"
    );

    // ========================================================================
    // CONVENIENCE "FIRST" METHODS
    // ========================================================================

    impl_first_method!(
        first_paragraph,
        Paragraph,
        iter_paragraphs_recursive,
        "Get the first paragraph in the container (returns None if not found)"
    );
    impl_first_method!(
        first_session,
        Session,
        iter_sessions_recursive,
        "Get the first session in the container tree (returns None if not found)"
    );
    impl_first_method!(
        first_list,
        List,
        iter_lists_recursive,
        "Get the first list in the container (returns None if not found)"
    );
    impl_first_method!(
        first_definition,
        Definition,
        iter_definitions_recursive,
        "Get the first definition in the container (returns None if not found)"
    );
    impl_first_method!(
        first_annotation,
        Annotation,
        iter_annotations_recursive,
        "Get the first annotation in the container (returns None if not found)"
    );
    impl_first_method!(
        first_verbatim,
        Verbatim,
        iter_verbatim_blocks_recursive,
        "Get the first verbatim block in the container (returns None if not found)"
    );

    // ========================================================================
    // "EXPECT" METHODS (panic if not found)
    // ========================================================================

    /// Get the first paragraph, panicking if none found
    pub fn expect_paragraph(&self) -> &Paragraph {
        self.first_paragraph()
            .expect("No paragraph found in container")
    }

    /// Get the first session, panicking if none found
    pub fn expect_session(&self) -> &Session {
        self.first_session()
            .expect("No session found in container tree")
    }

    /// Get the first list, panicking if none found
    pub fn expect_list(&self) -> &List {
        self.first_list().expect("No list found in container")
    }

    /// Get the first definition, panicking if none found
    pub fn expect_definition(&self) -> &Definition {
        self.first_definition()
            .expect("No definition found in container")
    }

    /// Get the first annotation, panicking if none found
    pub fn expect_annotation(&self) -> &Annotation {
        self.first_annotation()
            .expect("No annotation found in container")
    }

    /// Get the first verbatim block, panicking if none found
    pub fn expect_verbatim(&self) -> &Verbatim {
        self.first_verbatim()
            .expect("No verbatim block found in container")
    }

    // ========================================================================
    // PREDICATE-BASED SEARCH
    // ========================================================================

    impl_find_method!(
        find_paragraphs,
        Paragraph,
        iter_paragraphs_recursive,
        "Find all paragraphs matching a predicate"
    );
    impl_find_method!(
        find_sessions,
        Session,
        iter_sessions_recursive,
        "Find all sessions matching a predicate"
    );
    impl_find_method!(
        find_lists,
        List,
        iter_lists_recursive,
        "Find all lists matching a predicate"
    );
    impl_find_method!(
        find_definitions,
        Definition,
        iter_definitions_recursive,
        "Find all definitions matching a predicate"
    );
    impl_find_method!(
        find_annotations,
        Annotation,
        iter_annotations_recursive,
        "Find all annotations matching a predicate"
    );

    /// Find all nodes matching a generic predicate
    pub fn find_nodes<F>(&self, predicate: F) -> Vec<&ContentItem>
    where
        F: Fn(&ContentItem) -> bool,
    {
        self.iter_all_nodes().filter(|n| predicate(n)).collect()
    }

    // ========================================================================
    // DEPTH-BASED QUERIES
    // ========================================================================

    /// Find all nodes at a specific depth (0 = immediate children)
    pub fn find_nodes_at_depth(&self, target_depth: usize) -> Vec<&ContentItem> {
        self.iter_all_nodes_with_depth()
            .filter(|(_, depth)| *depth == target_depth)
            .map(|(node, _)| node)
            .collect()
    }

    /// Find all nodes within a depth range (inclusive)
    pub fn find_nodes_in_depth_range(
        &self,
        min_depth: usize,
        max_depth: usize,
    ) -> Vec<&ContentItem> {
        self.iter_all_nodes_with_depth()
            .filter(|(_, depth)| *depth >= min_depth && *depth <= max_depth)
            .map(|(node, _)| node)
            .collect()
    }

    /// Find nodes at a specific depth matching a predicate
    pub fn find_nodes_with_depth<F>(&self, target_depth: usize, predicate: F) -> Vec<&ContentItem>
    where
        F: Fn(&ContentItem) -> bool,
    {
        self.iter_all_nodes_with_depth()
            .filter(|(node, depth)| *depth == target_depth && predicate(node))
            .map(|(node, _)| node)
            .collect()
    }

    // ========================================================================
    // COUNTING AND STATISTICS
    // ========================================================================

    /// Count immediate children by type (paragraphs, sessions, lists, verbatim)
    pub fn count_by_type(&self) -> (usize, usize, usize, usize) {
        let paragraphs = self.iter_paragraphs().count();
        let sessions = self.iter_sessions().count();
        let lists = self.iter_lists().count();
        let verbatim_blocks = self.iter_verbatim_blocks().count();
        (paragraphs, sessions, lists, verbatim_blocks)
    }

    // ========================================================================
    // POSITION-BASED QUERIES
    // ========================================================================

    /// Returns the deepest (most nested) element that contains the position
    pub fn element_at(&self, pos: Position) -> Option<&ContentItem> {
        for item in &self.children {
            if let Some(result) = item.element_at(pos) {
                return Some(result);
            }
        }
        None
    }

    /// Returns the visual line element at the given position
    ///
    /// Returns the element representing a source line (TextLine, ListItem, VerbatimLine,
    /// BlankLineGroup, or header nodes like Session/Definition).
    pub fn visual_line_at(&self, pos: Position) -> Option<&ContentItem> {
        for item in &self.children {
            if let Some(result) = item.visual_line_at(pos) {
                return Some(result);
            }
        }
        None
    }

    /// Returns the block element at the given position
    ///
    /// Returns the shallowest block-level container element (Session, Definition, List,
    /// Paragraph, Annotation, VerbatimBlock) that contains the position.
    pub fn block_element_at(&self, pos: Position) -> Option<&ContentItem> {
        for item in &self.children {
            if let Some(result) = item.block_element_at(pos) {
                return Some(result);
            }
        }
        None
    }

    /// Returns the path of nodes at the given position
    pub fn node_path_at_position(&self, pos: Position) -> Vec<&ContentItem> {
        for item in &self.children {
            let path = item.node_path_at_position(pos);
            if !path.is_empty() {
                return path;
            }
        }
        Vec::new()
    }

    /// Returns the deepest AST node at the given position, if any
    pub fn find_nodes_at_position(&self, position: Position) -> Vec<&dyn AstNode> {
        if let Some(item) = self.element_at(position) {
            vec![item as &dyn AstNode]
        } else {
            Vec::new()
        }
    }

    /// Formats information about nodes located at a given position
    pub fn format_at_position(&self, position: Position) -> String {
        let nodes = self.find_nodes_at_position(position);
        if nodes.is_empty() {
            "No AST nodes at this position".to_string()
        } else {
            nodes
                .iter()
                .map(|node| format!("- {}: {}", node.node_type(), node.display_label()))
                .collect::<Vec<_>>()
                .join("\n")
        }
    }
}

impl<P: ContainerPolicy> AstNode for Container<P> {
    fn node_type(&self) -> &'static str {
        P::POLICY_NAME
    }

    fn display_label(&self) -> String {
        format!("{} items", self.children.len())
    }

    fn range(&self) -> &Range {
        &self.location
    }

    fn accept(&self, visitor: &mut dyn Visitor) {
        // Container itself doesn't have a visit method
        // It delegates to its children
        super::super::traits::visit_children(visitor, &self.children);
    }
}

// Implement Deref for ergonomic read-only access to the children
impl<P: ContainerPolicy> std::ops::Deref for Container<P> {
    type Target = [ContentItem];

    fn deref(&self) -> &Self::Target {
        &self.children
    }
}

// DerefMut is intentionally NOT implemented to prevent bypassing type safety.
// Use explicit methods like push(), push_typed(), get_mut(), etc. instead.

impl<P: ContainerPolicy> fmt::Display for Container<P> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}({} items)", P::POLICY_NAME, self.children.len())
    }
}

// Implement IntoIterator to allow for loops over Container
impl<'a, P: ContainerPolicy> IntoIterator for &'a Container<P> {
    type Item = &'a ContentItem;
    type IntoIter = std::slice::Iter<'a, ContentItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.children.iter()
    }
}

impl<'a, P: ContainerPolicy> IntoIterator for &'a mut Container<P> {
    type Item = &'a mut ContentItem;
    type IntoIter = std::slice::IterMut<'a, ContentItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.children.iter_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::super::list::ListItem;
    use super::super::paragraph::Paragraph;
    use super::super::session::Session;
    use super::super::typed_content::{ContentElement, ListContent, SessionContent};
    use super::*;

    // ========================================================================
    // BASIC CONTAINER CREATION
    // ========================================================================

    #[test]
    fn test_session_container_creation() {
        let container = SessionContainer::empty();
        assert_eq!(container.len(), 0);
        assert!(container.is_empty());
    }

    #[test]
    fn test_general_container_creation() {
        let container = GeneralContainer::empty();
        assert_eq!(container.len(), 0);
        assert!(container.is_empty());
    }

    #[test]
    fn test_list_container_creation() {
        let container = ListContainer::empty();
        assert_eq!(container.len(), 0);
        assert!(container.is_empty());
    }

    #[test]
    fn test_verbatim_container_creation() {
        let container = VerbatimContainer::empty();
        assert_eq!(container.len(), 0);
        assert!(container.is_empty());
    }

    #[test]
    fn test_container_with_items() {
        let para = Paragraph::from_line("Test".to_string());
        let container = SessionContainer::from_typed(vec![SessionContent::Element(
            ContentElement::Paragraph(para),
        )]);
        assert_eq!(container.len(), 1);
        assert!(!container.is_empty());
    }

    #[test]
    fn test_container_push() {
        let mut container = GeneralContainer::empty();
        let para = Paragraph::from_line("Test".to_string());
        container.push(ContentItem::Paragraph(para));
        assert_eq!(container.len(), 1);
    }

    #[test]
    fn test_container_deref() {
        let list_item = ListItem::new("-".to_string(), "Item".to_string());
        let container = ListContainer::from_typed(vec![ListContent::ListItem(list_item)]);
        // Should be able to use Vec methods directly via Deref
        assert_eq!(container.len(), 1);
        assert!(!container.is_empty());
    }

    // ========================================================================
    // RECURSIVE ITERATION
    // ========================================================================

    #[test]
    fn test_iter_paragraphs_recursive() {
        let mut inner_session = Session::with_title("Inner".to_string());
        inner_session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Nested 2".to_string(),
            )));

        let mut outer_session = Session::with_title("Outer".to_string());
        outer_session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Nested 1".to_string(),
            )));
        outer_session
            .children
            .push(ContentItem::Session(inner_session));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Top".to_string(),
        )));
        container.push(ContentItem::Session(outer_session));

        assert_eq!(container.iter_paragraphs().count(), 1);
        let paragraphs: Vec<_> = container.iter_paragraphs_recursive().collect();
        assert_eq!(paragraphs.len(), 3);
    }

    #[test]
    fn test_iter_sessions_recursive() {
        let inner_session = Session::with_title("Inner".to_string());
        let mut outer_session = Session::with_title("Outer".to_string());
        outer_session
            .children
            .push(ContentItem::Session(inner_session));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Session(outer_session));

        assert_eq!(container.iter_sessions().count(), 1);
        assert_eq!(container.iter_sessions_recursive().count(), 2);
    }

    #[test]
    fn test_iter_all_nodes_with_depth() {
        let mut inner_session = Session::with_title("Inner".to_string());
        inner_session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Deep".to_string(),
            )));

        let mut outer_session = Session::with_title("Outer".to_string());
        outer_session
            .children
            .push(ContentItem::Session(inner_session));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Top".to_string(),
        )));
        container.push(ContentItem::Session(outer_session));

        let nodes_with_depth: Vec<_> = container.iter_all_nodes_with_depth().collect();
        assert_eq!(nodes_with_depth.len(), 6);
        assert_eq!(nodes_with_depth[0].1, 0);
        assert!(nodes_with_depth[0].0.is_paragraph());
        assert_eq!(nodes_with_depth[1].1, 1);
        assert!(nodes_with_depth[1].0.is_text_line());
    }

    // ========================================================================
    // PREDICATE-BASED SEARCH
    // ========================================================================

    #[test]
    fn test_find_paragraphs_with_predicate() {
        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Hello, world!".to_string(),
        )));
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Goodbye, world!".to_string(),
        )));
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Hello again!".to_string(),
        )));

        let hello_paras = container.find_paragraphs(|p| p.text().starts_with("Hello"));
        assert_eq!(hello_paras.len(), 2);

        let goodbye_paras = container.find_paragraphs(|p| p.text().contains("Goodbye"));
        assert_eq!(goodbye_paras.len(), 1);
    }

    #[test]
    fn test_find_sessions_with_predicate() {
        let mut session1 = Session::with_title("Chapter 1: Introduction".to_string());
        session1
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Intro".to_string(),
            )));
        let session2 = Session::with_title("Chapter 2: Advanced".to_string());
        let section = Session::with_title("Section 1.1".to_string());
        session1.children.push(ContentItem::Session(section));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Session(session1));
        container.push(ContentItem::Session(session2));

        let chapters = container.find_sessions(|s| s.title.as_string().contains("Chapter"));
        assert_eq!(chapters.len(), 2);
    }

    #[test]
    fn test_find_nodes_generic_predicate() {
        let mut session = Session::with_title("Test".to_string());
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Child 1".to_string(),
            )));
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Child 2".to_string(),
            )));
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Child 3".to_string(),
            )));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Top".to_string(),
        )));
        container.push(ContentItem::Session(session));

        let big_sessions = container.find_nodes(|node| {
            matches!(node, ContentItem::Session(_))
                && node.children().map(|c| c.len() > 2).unwrap_or(false)
        });
        assert_eq!(big_sessions.len(), 1);
    }

    // ========================================================================
    // DEPTH-BASED QUERIES
    // ========================================================================

    #[test]
    fn test_find_nodes_at_depth() {
        let mut inner = Session::with_title("Inner".to_string());
        inner
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Deep".to_string(),
            )));
        let mut outer = Session::with_title("Outer".to_string());
        outer.children.push(ContentItem::Session(inner));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Top".to_string(),
        )));
        container.push(ContentItem::Session(outer));

        assert_eq!(container.find_nodes_at_depth(0).len(), 2);
        assert!(!container.find_nodes_at_depth(1).is_empty());
    }

    #[test]
    fn test_find_sessions_at_depth() {
        let mut level2 = Session::with_title("Level 2".to_string());
        level2
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Leaf".to_string(),
            )));
        let mut level1 = Session::with_title("Level 1".to_string());
        level1.children.push(ContentItem::Session(level2));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Session(level1));

        let level_0: Vec<_> = container
            .find_nodes_at_depth(0)
            .into_iter()
            .filter_map(|n| n.as_session())
            .collect();
        assert_eq!(level_0.len(), 1);

        let level_1: Vec<_> = container
            .find_nodes_at_depth(1)
            .into_iter()
            .filter_map(|n| n.as_session())
            .collect();
        assert_eq!(level_1.len(), 1);
    }

    #[test]
    fn test_find_nodes_in_depth_range() {
        let mut deep = Session::with_title("Deep".to_string());
        deep.children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Very deep".to_string(),
            )));
        let mut mid = Session::with_title("Mid".to_string());
        mid.children.push(ContentItem::Session(deep));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Root".to_string(),
        )));
        container.push(ContentItem::Session(mid));

        assert!(!container.find_nodes_in_depth_range(0, 1).is_empty());
        assert!(!container.find_nodes_in_depth_range(1, 2).is_empty());
    }

    #[test]
    fn test_find_nodes_with_depth_and_predicate() {
        let mut session = Session::with_title("Test Session".to_string());
        session
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Hello from nested".to_string(),
            )));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Hello from top".to_string(),
        )));
        container.push(ContentItem::Session(session));

        let depth_0_hello = container.find_nodes_with_depth(0, |node| {
            node.as_paragraph()
                .map(|p| p.text().contains("Hello"))
                .unwrap_or(false)
        });
        assert_eq!(depth_0_hello.len(), 1);
    }

    // ========================================================================
    // COMPREHENSIVE QUERY EXAMPLES
    // ========================================================================

    #[test]
    fn test_comprehensive_query_api() {
        let mut chapter1 = Session::with_title("Chapter 1: Introduction".to_string());
        chapter1
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Hello, this is the intro.".to_string(),
            )));

        let mut section1_1 = Session::with_title("Section 1.1".to_string());
        section1_1
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Nested content here.".to_string(),
            )));
        chapter1.children.push(ContentItem::Session(section1_1));

        let mut chapter2 = Session::with_title("Chapter 2: Advanced".to_string());
        chapter2
            .children
            .push(ContentItem::Paragraph(Paragraph::from_line(
                "Advanced topics.".to_string(),
            )));

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Preamble".to_string(),
        )));
        container.push(ContentItem::Session(chapter1));
        container.push(ContentItem::Session(chapter2));

        assert_eq!(container.iter_paragraphs_recursive().count(), 4);
        assert_eq!(container.iter_sessions_recursive().count(), 3);

        let hello_paragraphs: Vec<_> = container
            .iter_paragraphs_recursive()
            .filter(|p| p.text().contains("Hello"))
            .collect();
        assert_eq!(hello_paragraphs.len(), 1);

        let nested_sessions: Vec<_> = container
            .iter_all_nodes_with_depth()
            .filter(|(node, depth)| node.is_session() && *depth >= 1)
            .collect();
        assert_eq!(nested_sessions.len(), 1);
    }

    // ========================================================================
    // FIRST/EXPECT METHODS
    // ========================================================================

    #[test]
    fn test_first_methods() {
        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "First para".to_string(),
        )));

        assert!(container.first_paragraph().is_some());
        assert!(container.first_session().is_none());
    }

    #[test]
    fn test_expect_paragraph() {
        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        )));

        let para = container.expect_paragraph();
        assert_eq!(para.text(), "Test");
    }

    #[test]
    #[should_panic(expected = "No paragraph found in container")]
    fn test_expect_paragraph_panics() {
        let container = SessionContainer::empty();
        container.expect_paragraph();
    }

    // ========================================================================
    // COUNT BY TYPE
    // ========================================================================

    #[test]
    fn test_count_by_type() {
        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Para 1".to_string(),
        )));
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Para 2".to_string(),
        )));
        container.push(ContentItem::Session(Session::with_title(
            "Session".to_string(),
        )));

        let (paragraphs, sessions, lists, verbatim) = container.count_by_type();
        assert_eq!(paragraphs, 2);
        assert_eq!(sessions, 1);
        assert_eq!(lists, 0);
        assert_eq!(verbatim, 0);
    }

    // ========================================================================
    // ELEMENT_AT ERROR PATHS
    // ========================================================================

    #[test]
    fn test_element_at_position_outside_document() {
        use crate::lex::ast::range::Position;

        let mut container = SessionContainer::empty();
        container.push(ContentItem::Paragraph(Paragraph::from_line(
            "Test".to_string(),
        )));

        let far_position = Position::new(1000, 1000);
        assert!(container.element_at(far_position).is_none());
    }

    #[test]
    fn test_element_at_empty_container() {
        use crate::lex::ast::range::Position;

        let container = SessionContainer::empty();
        let position = Position::new(1, 1);
        assert!(container.element_at(position).is_none());
    }

    #[test]
    fn test_find_nodes_at_position_no_results() {
        use crate::lex::ast::range::Position;

        let container = SessionContainer::empty();
        let position = Position::new(1, 1);
        let nodes = container.find_nodes_at_position(position);
        assert!(nodes.is_empty());
    }

    #[test]
    fn test_format_at_position_no_nodes() {
        use crate::lex::ast::range::Position;

        let container = SessionContainer::empty();
        let position = Position::new(1, 1);
        let output = container.format_at_position(position);
        assert_eq!(output, "No AST nodes at this position");
    }
}
