//! The graph engine behind ironweaver, in pure Rust.
//!
//! A [`Graph<N, E>`](Graph) is a directed multigraph whose nodes have unique
//! string ids and carry a payload `N`; edges carry a payload `E`. Nodes and
//! edges are addressed by [`NodeIx`] / [`EdgeIx`] handles, which stay valid
//! until that node or edge is removed (a removed handle never aliases a later
//! node or edge).
//!
//! The algorithms never look inside the payloads except through the
//! [`Attributes`] trait (edge weights, node coordinates, edge types), and
//! take user callbacks as closures returning `Result<_, X>`, so a caller can
//! thread its own error type through them. [`Record`] (an `attr` and a `meta`
//! map of [`Value`]s) is the ready-made payload for Rust users; the Python
//! bindings use their own.
//!
//! ```
//! use ironweaver_core::{Direction, Graph, GraphError, Record, Value};
//! use ironweaver_core::pathfinding::{find_path, PathQuery};
//!
//! let mut g: Graph<Record, Record> = Graph::new();
//! let a = g.add_node("a", Record::default())?;
//! let b = g.add_node("b", Record::default())?;
//! let c = g.add_node("c", Record::default())?;
//! g.add_edge(a, b, Record::with_attr([("weight", Value::from(5.0))]))?;
//! g.add_edge(a, c, Record::with_attr([("weight", Value::from(1.0))]))?;
//! g.add_edge(c, b, Record::with_attr([("weight", Value::from(1.0))]))?;
//!
//! let path = find_path::<_, _, GraphError>(&g, a, b, &mut PathQuery::dijkstra())?.unwrap();
//! let ids: Vec<&str> = path.nodes.iter().map(|&n| g.node(n).unwrap().id()).collect();
//! assert_eq!(ids, ["a", "c", "b"]);
//! assert_eq!(path.cost, 2.0);
//! # let _ = Direction::Out;
//! # Ok::<(), GraphError>(())
//! ```

pub mod direction;
pub mod error;
pub mod format;
pub mod graph;
pub mod pathfinding;
pub mod random_walks;
pub mod record;
pub mod traversal;
pub mod value;

pub use direction::Direction;
pub use error::GraphError;
pub use graph::{Edge, EdgeIx, Graph, Node, NodeIx};
pub use record::{Attributes, Attrs, Lookup, Record};
pub use value::Value;
