// query/mod.rs
//
// Query primitives for a query engine on top of the graph: variable-length
// path expansion (`paths`), graph patterns with a Cypher-like text form
// (`pattern`) and a backtracking pattern matcher (`matcher`). Conditions on
// nodes and edges are `Expr`s, so they are evaluated without callbacks.

pub mod matcher;
pub mod paths;
pub mod pattern;

pub use matcher::{find_matches, for_each_match, Bound, Match};
pub use paths::{expand_paths, expand_paths_limited, steps, Hops, Uniqueness};
pub use pattern::{EdgePattern, NodePattern, Pattern};
