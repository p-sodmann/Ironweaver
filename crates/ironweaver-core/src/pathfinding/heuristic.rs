// pathfinding/heuristic.rs
//
// Estimates of the remaining cost from a node to the target, for A*.
// `best_first::search` asks for each node's estimate once, when the node is
// first reached.

use crate::{Attributes, Graph, GraphError, Lookup, Node, NodeIx};

/// Distance between two coordinate vectors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Metric {
    Euclidean,
    Manhattan,
}

impl Metric {
    /// `"euclidean"` (the default for `None`) or `"manhattan"`.
    pub fn parse(name: Option<&str>) -> Result<Self, GraphError> {
        match name.unwrap_or("euclidean") {
            "euclidean" => Ok(Metric::Euclidean),
            "manhattan" => Ok(Metric::Manhattan),
            other => Err(GraphError::InvalidArgument(format!(
                "unknown heuristic '{}'; use 'euclidean' or 'manhattan' (with coords=...), \
                 or distances='<meta key>' for precomputed estimates",
                other
            ))),
        }
    }

    pub fn distance(self, a: &[f64], b: &[f64]) -> f64 {
        match self {
            Metric::Euclidean => a.iter().zip(b).map(|(x, y)| (x - y) * (x - y)).sum::<f64>().sqrt(),
            Metric::Manhattan => a.iter().zip(b).map(|(x, y)| (x - y).abs()).sum(),
        }
    }
}

/// Where a node's coordinates live. A path is an attribute name followed by
/// nested keys (`"pos.lat"` -> attr["pos"]["lat"]).
#[derive(Clone, Debug, PartialEq)]
pub enum Coords {
    /// One path to a sequence of numbers (`coords="pos"`).
    Sequence(Vec<String>),
    /// One path per dimension (`coords=["x", "y"]`).
    PerDimension(Vec<Vec<String>>),
}

fn split_path(path: &str) -> Vec<String> {
    path.split('.').map(str::to_string).collect()
}

fn show(path: &[String]) -> String {
    path.join(".")
}

impl Default for Coords {
    /// Attributes `x` and `y`.
    fn default() -> Self {
        Coords::PerDimension(vec![vec!["x".into()], vec!["y".into()]])
    }
}

impl Coords {
    /// One dotted path to a sequence of numbers.
    pub fn sequence(path: &str) -> Self {
        Coords::Sequence(split_path(path))
    }

    /// One dotted path per dimension.
    pub fn per_dimension<S: AsRef<str>>(paths: &[S]) -> Result<Self, GraphError> {
        if paths.is_empty() {
            return Err(GraphError::InvalidArgument("coords must name at least one dimension".into()));
        }
        Ok(Coords::PerDimension(paths.iter().map(|p| split_path(p.as_ref())).collect()))
    }

    fn describe(&self) -> String {
        match self {
            Coords::Sequence(p) => format!("'{}'", show(p)),
            Coords::PerDimension(ps) => {
                format!("[{}]", ps.iter().map(|p| format!("'{}'", show(p))).collect::<Vec<_>>().join(", "))
            }
        }
    }

    /// The node's coordinates, or `None` if any part is missing.
    pub fn read<N, X>(&self, node: &Node<N>) -> Result<Option<Vec<f64>>, X>
    where
        N: Attributes,
        X: From<GraphError> + From<N::Error>,
    {
        match self {
            Coords::Sequence(path) => match node.data.numbers(path)? {
                Lookup::Missing => Ok(None),
                Lookup::Found(v) => Ok(Some(v)),
                Lookup::Invalid => Err(GraphError::InvalidType(format!(
                    "coordinates at '{}' of node '{}' must be a sequence of numbers",
                    show(path),
                    node.id()
                ))
                .into()),
            },
            Coords::PerDimension(paths) => {
                let mut out = Vec::with_capacity(paths.len());
                for path in paths {
                    match node.data.number(path)? {
                        Lookup::Missing => return Ok(None),
                        Lookup::Found(v) => out.push(v),
                        Lookup::Invalid => {
                            return Err(GraphError::InvalidType(format!(
                                "coordinate '{}' of node '{}' must be a number",
                                show(path),
                                node.id()
                            ))
                            .into())
                        }
                    }
                }
                Ok(Some(out))
            }
        }
    }
}

/// Estimate function supplied by the caller.
pub type EstimateFn<'h, N, X> = Box<dyn FnMut(&Node<N>) -> Result<f64, X> + 'h>;

/// A* heuristic: an estimate of the remaining cost from a node to the target.
/// Estimates must not overestimate, or the path may not be the cheapest.
pub enum Heuristic<'h, N, X> {
    /// No estimate (Dijkstra).
    Zero,
    /// Distance between node and target coordinates; nodes without
    /// coordinates count as 0.
    Coords { metric: Metric, coords: Coords, target: Vec<f64> },
    /// Any estimate, e.g. a precomputed table.
    Custom(EstimateFn<'h, N, X>),
}

impl<'h, N, X> Heuristic<'h, N, X>
where
    N: Attributes,
    X: From<GraphError> + From<N::Error>,
{
    /// Coordinate heuristic towards `target`, whose coordinates must be set.
    pub fn coords<E>(g: &Graph<N, E>, target: NodeIx, metric: Metric, coords: Coords) -> Result<Self, X> {
        let node = g.node(target).ok_or(GraphError::Stale)?;
        let target = coords.read::<N, X>(node)?.ok_or_else(|| {
            GraphError::InvalidArgument(format!(
                "target node '{}' has no coordinates at coords={}",
                node.id(),
                coords.describe()
            ))
        })?;
        Ok(Heuristic::Coords { metric, coords, target })
    }

    /// Estimated cost from `node` to the target (0 when unknown).
    pub fn estimate(&mut self, node: &Node<N>) -> Result<f64, X> {
        match self {
            Heuristic::Zero => Ok(0.0),
            Heuristic::Coords { metric, coords, target } => match coords.read::<N, X>(node)? {
                None => Ok(0.0),
                Some(c) if c.len() != target.len() => Err(GraphError::InvalidArgument(format!(
                    "node '{}' has {} coordinates but the target has {}",
                    node.id(),
                    c.len(),
                    target.len()
                ))
                .into()),
                Some(c) => Ok(metric.distance(&c, target)),
            },
            Heuristic::Custom(f) => f(node),
        }
    }
}
