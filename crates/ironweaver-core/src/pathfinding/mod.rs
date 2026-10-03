// pathfinding/mod.rs
//
// Shortest-path algorithms behind one entry point, `find_path`. Each
// algorithm is a `PathMethod` in `METHODS`; front ends (the Python
// `Vertex.shortest_path`) resolve the method by name with `resolve`, check
// its options with `check_options`, build a `PathQuery` and call `find_path`.
//
// Adding an algorithm:
//   1. Write `my_algo.rs` with a `pub const METHOD: PathMethod` (a new
//      `MethodKind` variant) and a `find` function returning
//      `Ok(Some(PathResult))`, or `Ok(None)` if the target is not reachable.
//      Reuse `EdgeCost`, `Heuristic` and `best_first::search` where they fit.
//   2. Add `mod my_algo;` below, `my_algo::METHOD` to `METHODS` and a match
//      arm in `find_path`.
//   3. List its options in `METHOD.options`; if it needs a new one, add a
//      field to `PathQuery` and parse it in the bindings
//      (src/vertex/pathfinding.rs).
//   4. Update the `.pyi` stubs, docs/traversal.md, llms.txt and add tests.

mod astar;
mod best_first;
mod bfs;
mod cost;
mod dijkstra;
mod heuristic;

pub use cost::EdgeCost;
pub use heuristic::{Coords, Heuristic, Metric};

use crate::budget::{Budget, Limited, Meter};
use crate::{Attributes, Direction, Graph, GraphError, NodeIx};

/// Which algorithm a `PathMethod` runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MethodKind {
    Bfs,
    Dijkstra,
    AStar,
}

/// A registered shortest-path algorithm.
#[derive(Debug)]
pub struct PathMethod {
    /// Value of the `method` argument.
    pub name: &'static str,
    /// One-line description (`Vertex.path_methods()`).
    pub description: &'static str,
    /// Algorithm-specific options it accepts.
    pub options: &'static [&'static str],
    /// Whether it uses edge weights (`weight` / `default_weight`).
    pub weighted: bool,
    pub kind: MethodKind,
}

/// All algorithms, in the order `path_methods()` lists them.
pub const METHODS: &[PathMethod] = &[bfs::METHOD, dijkstra::METHOD, astar::METHOD];

/// Options whose presence makes `method=None` pick A*.
const ASTAR_OPTIONS: &[&str] = &["heuristic", "coords", "distances"];

/// `'bfs', 'dijkstra', 'astar'`, for error messages.
pub fn method_names() -> String {
    METHODS.iter().map(|m| format!("'{}'", m.name)).collect::<Vec<_>>().join(", ")
}

/// Look a method up by name. With `None`, pick A* if any of its options is
/// given, Dijkstra if a weight attribute is, BFS otherwise.
pub fn resolve(
    method: Option<&str>,
    weight_given: bool,
    option_names: &[&str],
) -> Result<&'static PathMethod, GraphError> {
    let name = match method {
        Some(m) => m,
        None if option_names.iter().any(|n| ASTAR_OPTIONS.contains(n)) => "astar",
        None if weight_given => "dijkstra",
        None => "bfs",
    };
    METHODS.iter().find(|m| m.name == name).ok_or_else(|| {
        GraphError::InvalidArgument(format!("unknown path method '{}'; available: {}", name, method_names()))
    })
}

/// Reject options `method` does not accept.
pub fn check_options(method: &PathMethod, option_names: &[&str]) -> Result<(), GraphError> {
    for name in option_names {
        if !method.options.contains(name) {
            let accepted = if method.options.is_empty() { "none".to_string() } else { method.options.join(", ") };
            return Err(GraphError::InvalidType(format!(
                "method '{}' does not accept option '{}' (accepted options: {})",
                method.name, name, accepted
            )));
        }
    }
    Ok(())
}

/// The edge cost `method` uses: `Weighted` for weighted methods; unweighted
/// methods refuse `weight` / `default_weight`.
pub fn edge_cost(
    method: &PathMethod,
    weight: Option<String>,
    default_weight: Option<f64>,
) -> Result<EdgeCost, GraphError> {
    if method.weighted {
        return Ok(EdgeCost::weighted(weight, default_weight));
    }
    if weight.is_some() || default_weight.is_some() {
        return Err(GraphError::InvalidArgument(format!(
            "method '{}' ignores edge weights; use method='dijkstra' or 'astar' for weighted paths",
            method.name
        )));
    }
    Ok(EdgeCost::Unit)
}

/// `max_cost` must be non-negative (and not NaN).
pub fn check_max_cost(max_cost: Option<f64>) -> Result<(), GraphError> {
    match max_cost {
        Some(c) if c.is_nan() || c < 0.0 => {
            Err(GraphError::InvalidArgument(format!("max_cost must be non-negative, got {}", c)))
        }
        _ => Ok(()),
    }
}

/// Everything an algorithm needs besides the endpoints.
pub struct PathQuery<'h, N, X> {
    pub method: &'static PathMethod,
    pub direction: Direction,
    pub cost: EdgeCost,
    /// Ignore paths more expensive than this (for BFS: with more edges).
    pub max_cost: Option<f64>,
    /// BFS only: maximum number of edges.
    pub max_depth: Option<usize>,
    /// A* only: estimate of the remaining cost.
    pub heuristic: Heuristic<'h, N, X>,
}

impl<'h, N, X> PathQuery<'h, N, X> {
    /// A query for `method` with its defaults (outgoing edges, edge
    /// attribute `weight` with default 1.0 for weighted methods, no limits,
    /// no heuristic).
    pub fn new(method: &'static PathMethod) -> Self {
        PathQuery {
            method,
            direction: Direction::Out,
            cost: if method.weighted { EdgeCost::weighted(None, None) } else { EdgeCost::Unit },
            max_cost: None,
            max_depth: None,
            heuristic: Heuristic::Zero,
        }
    }

    pub fn bfs() -> Self {
        Self::new(&bfs::METHOD)
    }

    pub fn dijkstra() -> Self {
        Self::new(&dijkstra::METHOD)
    }

    pub fn astar(heuristic: Heuristic<'h, N, X>) -> Self {
        PathQuery { heuristic, ..Self::new(&astar::METHOD) }
    }
}

/// A found path: nodes in source -> target order and its total cost (the
/// number of edges for BFS).
#[derive(Clone, Debug, PartialEq)]
pub struct PathResult {
    pub nodes: Vec<NodeIx>,
    pub cost: f64,
    /// Nodes the algorithm settled, if it keeps count.
    pub expanded: Option<usize>,
}

/// Run `query.method` from `source` to `target`. `Ok(None)` means the target
/// is not reachable within the query's limits.
pub fn find_path<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    query: &mut PathQuery<'_, N, X>,
) -> Result<Option<PathResult>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    find_metered(g, source, target, query, &mut Meter::new(Budget::UNLIMITED))
}

/// [`find_path`] under a [`Budget`]. Dijkstra and A* count each node they
/// settle as visited and each edge they relax as examined (before its cost
/// and the heuristic are computed); BFS counts the frontier nodes of both
/// ends whose edges it lists as visited and those edges as examined. A path
/// found is a result. Every method polls cancellation once per edge. With a
/// truncated search the value is `None`: no path was found within the
/// budget (`truncated` tells it from an unreachable target).
pub fn find_path_limited<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    query: &mut PathQuery<'_, N, X>,
    budget: Budget,
) -> Result<Limited<Option<PathResult>>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    let mut meter = Meter::new(budget);
    let path = find_metered(g, source, target, query, &mut meter)?;
    Ok(meter.finish(path)?)
}

fn find_metered<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    query: &mut PathQuery<'_, N, X>,
    meter: &mut Meter,
) -> Result<Option<PathResult>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    check_max_cost(query.max_cost)?;
    if g.node(source).is_none() || g.node(target).is_none() {
        return Err(GraphError::Stale.into());
    }
    match query.method.kind {
        MethodKind::Bfs => bfs::find(g, source, target, query, meter),
        MethodKind::Dijkstra => dijkstra::find(g, source, target, query, meter),
        MethodKind::AStar => astar::find(g, source, target, query, meter),
    }
}

/// The error for an unreachable target.
pub fn not_reachable(
    method: &PathMethod,
    source_id: &str,
    target_id: &str,
    max_depth: Option<usize>,
    max_cost: Option<f64>,
) -> GraphError {
    let mut msg = format!("Target node '{}' not reachable from '{}'", target_id, source_id);
    if let Some(d) = max_depth {
        msg += &format!(" within max_depth {}", d);
    }
    if let Some(c) = max_cost {
        msg += &format!(" within max_cost {}", c);
    }
    msg += &format!(" (method '{}')", method.name);
    GraphError::InvalidArgument(msg)
}
