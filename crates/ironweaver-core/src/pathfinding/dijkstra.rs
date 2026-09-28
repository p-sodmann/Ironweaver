// pathfinding/dijkstra.rs

use super::{best_first, MethodKind, PathMethod, PathQuery, PathResult};
use crate::{Attributes, Graph, GraphError, NodeIx};

pub const METHOD: PathMethod = PathMethod {
    name: "dijkstra",
    description: "Cheapest path by edge weight (Dijkstra). Weights must be non-negative.",
    options: &[],
    weighted: true,
    kind: MethodKind::Dijkstra,
};

pub fn find<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    q: &mut PathQuery<'_, N, X>,
) -> Result<Option<PathResult>, X>
where
    N: Attributes,
    E: Attributes,
    X: From<GraphError> + From<N::Error> + From<E::Error>,
{
    best_first::search(g, source, target, q, false)
}
