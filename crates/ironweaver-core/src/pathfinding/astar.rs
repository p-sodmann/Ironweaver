// pathfinding/astar.rs

use super::{best_first, MethodKind, PathMethod, PathQuery, PathResult};
use crate::budget::Meter;
use crate::{Attributes, Graph, GraphError, NodeIx};

pub const METHOD: PathMethod = PathMethod {
    name: "astar",
    description: "Cheapest path by edge weight (A*), guided by node coordinates \
                  (heuristic='euclidean'|'manhattan', coords=...) or precomputed \
                  estimates (distances='<vertex.meta key>'). Estimates must not \
                  overestimate the remaining cost.",
    options: &["heuristic", "coords", "distances"],
    weighted: true,
    kind: MethodKind::AStar,
};

pub fn find<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    q: &mut PathQuery<'_, N, X>,
    meter: &mut Meter,
) -> Result<Option<PathResult>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    best_first::search(g, source, target, q, true, meter)
}
