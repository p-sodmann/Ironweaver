// pathfinding/bfs.rs

use super::{EdgeCost, MethodKind, PathMethod, PathQuery, PathResult};
use crate::budget::Meter;
use crate::traversal::bidirectional_metered;
use crate::{Attributes, Graph, GraphError, NodeIx};

pub const METHOD: PathMethod = PathMethod {
    name: "bfs",
    description: "Fewest edges (bidirectional breadth-first search); skips infinite weights when supplied. \
                  max_cost or max_depth limit the number of edges.",
    options: &["max_depth"],
    weighted: false,
    kind: MethodKind::Bfs,
};

pub fn find<N, E, X>(
    g: &Graph<N, E>,
    source: NodeIx,
    target: NodeIx,
    q: &PathQuery<'_, N, X>,
    meter: &mut Meter,
) -> Result<Option<PathResult>, X>
where
    E: Attributes,
    X: From<GraphError> + From<E::Error>,
{
    // max_cost counts edges here, so it is a depth limit too.
    let max_depth = match (q.max_depth, q.max_cost) {
        (d, None) => d,
        (None, Some(c)) => Some(c.floor() as usize),
        (Some(d), Some(c)) => Some(d.min(c.floor() as usize)),
    };
    // Keep the default hop search independent of payloads: the unit-cost
    // specialization has no weight validation branch in its inner loops.
    let path = match &q.cost {
        EdgeCost::Unit => {
            bidirectional_metered(g, source, target, max_depth, q.direction, meter, |_, _| Ok::<_, X>(true))?
        }
        _ => bidirectional_metered(g, source, target, max_depth, q.direction, meter, |_, edge| {
            Ok::<_, X>(q.cost.cost::<E, X>(&edge.data)?.is_finite())
        })?,
    };
    Ok(path.map(|nodes| {
        let cost = (nodes.len() - 1) as f64;
        PathResult { nodes, cost, expanded: Some(meter.visited()) }
    }))
}
