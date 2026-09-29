// query/paths.rs
//
// Variable-length paths: every path of `min..=max` edges from a node,
// following edges in a direction, with a uniqueness rule (walk: anything
// goes; trail: no edge twice; path: no node twice). Enumerated depth-first
// with an explicit stack, streamed to a visitor that can stop early.

use crate::{Direction, Edge, EdgeIx, Graph, GraphError, NodeIx};

/// What may repeat along a variable-length path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Uniqueness {
    /// Nodes and edges may repeat (needs a maximum length).
    Walk,
    /// No edge twice (Cypher's rule); nodes may repeat.
    Trail,
    /// No node twice (a simple path).
    Path,
}

impl std::str::FromStr for Uniqueness {
    type Err = GraphError;

    fn from_str(s: &str) -> Result<Self, GraphError> {
        match s {
            "walk" => Ok(Uniqueness::Walk),
            "trail" => Ok(Uniqueness::Trail),
            "path" => Ok(Uniqueness::Path),
            other => Err(GraphError::InvalidArgument(format!(
                "uniqueness must be 'walk', 'trail' or 'path', got '{}'",
                other
            ))),
        }
    }
}

/// Path length bounds, in edges.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Hops {
    pub min: usize,
    /// No limit if `None` (not allowed for walks).
    pub max: Option<usize>,
}

impl Hops {
    pub fn exactly(n: usize) -> Self {
        Hops { min: n, max: Some(n) }
    }
}

/// Edges one step from `ix` in `direction`, as `(edge, neighbour)`:
/// outgoing edges first, then incoming ones. With `Both`, a self-loop is
/// listed once.
pub fn steps<N, E>(g: &Graph<N, E>, ix: NodeIx, direction: Direction) -> impl Iterator<Item = (EdgeIx, NodeIx)> + '_ {
    let node = g.node(ix);
    let out: &[EdgeIx] = match node {
        Some(n) if direction != Direction::In => n.out_edges(),
        _ => &[],
    };
    let inc: &[EdgeIx] = match node {
        Some(n) if direction != Direction::Out => n.in_edges(),
        _ => &[],
    };
    let both = direction == Direction::Both;
    out.iter()
        .map(move |&e| (e, g.edge_ref(e).target()))
        .chain(inc.iter().map(move |&e| (e, g.edge_ref(e).source())).filter(move |&(_, n)| !(both && n == ix)))
}

/// Every path from `start` with `hops.min..=hops.max` edges, following
/// edges in `direction` for which `edge_ok` is true, under `uniqueness`.
/// `visit` gets each path's edges and nodes (`nodes[0] == start`) in
/// depth-first order, and returns false to stop.
pub fn expand_paths<N, E, X>(
    g: &Graph<N, E>,
    start: NodeIx,
    direction: Direction,
    hops: Hops,
    uniqueness: Uniqueness,
    mut edge_ok: impl FnMut(EdgeIx, &Edge<E>) -> Result<bool, X>,
    mut visit: impl FnMut(&[EdgeIx], &[NodeIx]) -> Result<bool, X>,
) -> Result<(), X>
where
    X: From<GraphError>,
{
    check_hops(hops, uniqueness)?;
    let mut nodes = vec![start];
    let mut edges: Vec<EdgeIx> = Vec::new();
    if hops.min == 0 && !visit(&edges, &nodes)? {
        return Ok(());
    }
    if hops.max == Some(0) || g.node(start).is_none() {
        return Ok(());
    }
    // One frame per path length: the candidate steps and the next one to try
    let mut stack: Vec<(Vec<(EdgeIx, NodeIx)>, usize)> = vec![(steps(g, start, direction).collect(), 0)];
    let stop = crate::cancel::stop();
    while let Some((candidates, next)) = stack.last_mut() {
        if stop.poll() {
            return Ok(());
        }
        let Some(&(e, n)) = candidates.get(*next) else {
            stack.pop();
            if !stack.is_empty() {
                edges.pop();
                nodes.pop();
            }
            continue;
        };
        *next += 1;
        let repeated = match uniqueness {
            Uniqueness::Walk => false,
            Uniqueness::Trail => edges.contains(&e),
            Uniqueness::Path => nodes.contains(&n),
        };
        if repeated || !edge_ok(e, g.edge_ref(e))? {
            continue;
        }
        edges.push(e);
        nodes.push(n);
        if edges.len() >= hops.min && !visit(&edges, &nodes)? {
            return Ok(());
        }
        if hops.max.is_none_or(|max| edges.len() < max) {
            stack.push((steps(g, n, direction).collect(), 0));
        } else {
            edges.pop();
            nodes.pop();
        }
    }
    Ok(())
}

pub(crate) fn check_hops(hops: Hops, uniqueness: Uniqueness) -> Result<(), GraphError> {
    if hops.max.is_some_and(|max| max < hops.min) {
        return Err(GraphError::InvalidArgument(format!(
            "max_hops ({}) is less than min_hops ({})",
            hops.max.unwrap_or_default(),
            hops.min
        )));
    }
    if hops.max.is_none() && uniqueness == Uniqueness::Walk {
        return Err(GraphError::InvalidArgument("walks can repeat forever: give max_hops".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Record;

    type G = Graph<Record, Record>;

    /// a -> b -> c -> a, c -> d, and a self-loop on d.
    fn graph() -> (G, Vec<NodeIx>) {
        let mut g = G::new();
        let ix: Vec<NodeIx> =
            ["a", "b", "c", "d"].iter().map(|id| g.add_node(*id, Record::default()).unwrap()).collect();
        for (s, t) in [(0, 1), (1, 2), (2, 0), (2, 3), (3, 3)] {
            g.add_edge(ix[s], ix[t], Record::default()).unwrap();
        }
        (g, ix)
    }

    fn ids(g: &G, hops: Hops, dir: Direction, u: Uniqueness) -> Vec<String> {
        let mut out = Vec::new();
        expand_paths::<_, _, GraphError>(
            g,
            g.node_ix("a").unwrap(),
            dir,
            hops,
            u,
            |_, _| Ok(true),
            |_, nodes| {
                out.push(nodes.iter().map(|&n| g.node(n).unwrap().id()).collect::<String>());
                Ok(true)
            },
        )
        .unwrap();
        out
    }

    #[test]
    fn uniqueness_rules() {
        let (g, _) = graph();
        let h = |min, max| Hops { min, max };
        assert_eq!(ids(&g, h(1, None), Direction::Out, Uniqueness::Path), ["ab", "abc", "abcd"]);
        assert_eq!(ids(&g, h(0, Some(1)), Direction::Out, Uniqueness::Path), ["a", "ab"]);
        // Trails may revisit nodes (back to a, around d's loop) but not edges
        assert_eq!(ids(&g, h(3, None), Direction::Out, Uniqueness::Trail), ["abca", "abcd", "abcdd"]);
        assert_eq!(ids(&g, Hops::exactly(5), Direction::Out, Uniqueness::Walk), ["abcabc", "abcddd"]);
        // Undirected: the self-loop once per visit
        assert_eq!(ids(&g, Hops::exactly(1), Direction::Both, Uniqueness::Trail), ["ab", "ac"]);
        assert_eq!(ids(&g, Hops::exactly(2), Direction::In, Uniqueness::Path), ["acb"]);
        assert!(expand_paths::<_, _, GraphError>(
            &g,
            g.node_ix("a").unwrap(),
            Direction::Out,
            h(1, None),
            Uniqueness::Walk,
            |_, _| Ok(true),
            |_, _| Ok(true)
        )
        .is_err());
        assert!(check_hops(h(3, Some(2)), Uniqueness::Trail).is_err());
    }

    #[test]
    fn filters_and_stopping() {
        let (g, ix) = graph();
        let mut seen = 0;
        expand_paths::<_, _, GraphError>(
            &g,
            ix[0],
            Direction::Out,
            Hops { min: 1, max: None },
            Uniqueness::Trail,
            |_, e| Ok(e.target() != ix[3]),
            |edges, _| {
                seen += 1;
                Ok(edges.len() < 2)
            },
        )
        .unwrap();
        assert_eq!(seen, 2); // a-b, then a-b-c stops it
    }
}
