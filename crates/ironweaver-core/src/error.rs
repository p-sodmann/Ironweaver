// error.rs

use std::fmt;

/// Errors raised by the graph and its algorithms.
///
/// The `Display` text is the user-facing message (the Python bindings pass
/// it on unchanged).
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum GraphError {
    /// A node with this id already exists.
    DuplicateNode(String),
    /// No node has this id.
    NodeNotFound(String),
    /// An edge with this id already exists.
    DuplicateEdge(u64),
    /// No edge has this id.
    EdgeNotFound(u64),
    /// A `NodeIx` / `EdgeIx` whose node or edge has been removed.
    Stale,
    /// An argument or option has an invalid value.
    InvalidArgument(String),
    /// A value (edge weight, coordinate, option, ...) has the wrong type.
    InvalidType(String),
    /// Encoding or decoding a graph document failed.
    Format(String),
    /// The computation was cancelled (see [`cancel`](crate::cancel)).
    Interrupted,
    /// A size limit was reached (more than `u32::MAX` node or edge slots).
    Capacity(String),
    /// A [`Budget`](crate::Budget) limit was reached: `visited` nodes had
    /// been entered and `results` produced when the search stopped.
    BudgetExceeded { visited: usize, results: usize },
    /// An internal invariant failed (a bug). After this error from
    /// [`Graph::apply_all`](crate::Graph::apply_all) the graph may be
    /// inconsistent: reload it.
    Internal(String),
}

impl fmt::Display for GraphError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            GraphError::DuplicateNode(id) => write!(f, "Node with id '{}' already exists", id),
            GraphError::NodeNotFound(id) => write!(f, "Node with id '{}' not found", id),
            GraphError::DuplicateEdge(id) => write!(f, "Edge with id {} already exists", id),
            GraphError::EdgeNotFound(id) => write!(f, "Edge with id {} not found", id),
            GraphError::Stale => f.write_str("node or edge was removed from its graph"),
            GraphError::Interrupted => f.write_str("interrupted"),
            GraphError::BudgetExceeded { visited, results } => {
                write!(f, "budget exceeded after visiting {} nodes and producing {} results", visited, results)
            }
            GraphError::Internal(msg) => write!(f, "internal error: {}", msg),
            GraphError::InvalidArgument(msg)
            | GraphError::InvalidType(msg)
            | GraphError::Format(msg)
            | GraphError::Capacity(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for GraphError {}
