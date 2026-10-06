#![doc = include_str!("../README.md")]
//!
//! ## Modules
//!
//! - [`graph`]: [`Graph`], handles, [`EdgeId`], labels and types ([`Symbols`]).
//! - [`ops`]: [`Op`], `Graph::apply` / `Graph::apply_all` (changes as data, with undo).
//! - [`expr`]: [`Expr`] filter expressions.
//! - [`projection`]: [`Projection`], the read-only snapshot the analytics run on.
//! - [`algo`]: analytics on a projection; [`batch`]: many shortest paths at once.
//! - [`query`]: pattern matching and variable-length paths.
//! - [`pathfinding`]: single shortest paths (BFS, Dijkstra, A*).
//! - [`traversal`], [`random_walks`]: walks over the graph.
//! - [`budget`]: [`Budget`] limits on visited nodes, examined edges and results for traversals, path expansion and walks.
//! - [`format`](mod@format): saving and loading ([`Record`] graphs), format version 2.
//! - [`value`], [`record`]: [`Value`], [`Record`] and the [`Attributes`] trait.
//! - [`heap_size`]: [`HeapSize`], payload memory for [`Graph::memory_usage`].

pub mod algo;
pub mod batch;
pub mod budget;
pub mod cancel;
pub mod direction;
pub mod error;
pub mod expr;
pub mod format;
pub mod graph;
pub mod heap_size;
pub mod index;
pub mod ops;
pub mod pathfinding;
pub mod projection;
pub mod query;
pub mod random_walks;
pub mod record;
pub mod temporal;
pub mod traversal;
pub mod value;

pub use budget::{Budget, Limited, OnLimit};
pub use direction::Direction;
pub use error::GraphError;
pub use expr::{CmpOp, Expr};
pub use graph::{Edge, EdgeId, EdgeIx, Graph, Node, NodeIx, Symbol, Symbols};
pub use heap_size::HeapSize;
pub use index::IndexBuild;
pub use index::IndexPlan;
pub use index::IndexStats;
pub use ops::{AttrPatch, Op};
pub use projection::Projection;
pub use record::{lookup, Attributes, Attrs, Lookup, Record};
pub use temporal::{Date, DateTime};
pub use value::{Key, Value};
