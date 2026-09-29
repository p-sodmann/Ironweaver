// pathfinding/bfs.rs

use super::{MethodKind, PathMethod, PathQuery, PathResult};
use crate::traversal::bidirectional_bfs;
use crate::{Graph, NodeIx};

pub const METHOD: PathMethod = PathMethod {
    name: "bfs",
    description: "Fewest edges (bidirectional breadth-first search); ignores weights. \
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
) -> Result<Option<PathResult>, X> {
    // max_cost counts edges here, so it is a depth limit too.
    let max_depth = match (q.max_depth, q.max_cost) {
        (d, None) => d,
        (None, Some(c)) => Some(c.floor() as usize),
        (Some(d), Some(c)) => Some(d.min(c.floor() as usize)),
    };
    let path = bidirectional_bfs(g, source, target, max_depth, q.direction, |_, _| Ok(true))?;
    Ok(path.map(|nodes| {
        let cost = (nodes.len() - 1) as f64;
        PathResult { nodes, cost, expanded: None }
    }))
}
