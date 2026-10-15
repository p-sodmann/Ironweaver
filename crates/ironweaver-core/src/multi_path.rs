//! One shortest-path search between sets of endpoints with entry/exit costs.
use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::batch::{AStar, DensePath};
use crate::{GraphError, Projection};

/// A path including its selected real endpoints. `path.cost` includes entry
/// and exit costs; its nodes exclude the two virtual endpoints.
#[derive(Clone, Debug, PartialEq)]
pub struct MultiPath {
    pub path: DensePath,
    pub source: u32,
    pub target: u32,
}

/// Find the cheapest entry + graph path + exit cost with one search.
///
/// Endpoints are dense node indices paired with finite, nonnegative costs.
/// Duplicates use their lowest cost; an empty side returns `None` after
/// validation. Unweighted projections use unit edge costs. Infinite edges
/// remain impassable. `max_cost` includes both endpoint costs.
///
/// With `astar`, coordinates are required. Without `goal_coords`, the
/// estimate is the minimum scaled metric distance to a target plus its
/// exit cost. With a goal, it is the scaled metric distance to that goal.
/// For optimal results the estimate must be admissible: edge weights must
/// dominate scaled metric distances, and each target's exit cost must
/// dominate its scaled distance to the goal. Nodes may reopen.
///
/// Equal-cost exits prefer the lower target index. Inside the path, equal
/// costs over steps that increase the represented distance prefer the lower predecessor index (the
/// virtual source sorts after real nodes); zero edges retain the first
/// route to avoid cycles. Input and edge order do not decide ties.
#[allow(clippy::too_many_arguments)]
pub fn shortest_path_multi(
    p: &Projection,
    sources: &[(u32, f64)],
    targets: &[(u32, f64)],
    astar: Option<&AStar>,
    goal_coords: Option<&[f64]>,
    max_cost: Option<f64>,
) -> Result<Option<MultiPath>, GraphError> {
    crate::pathfinding::check_max_cost(max_cost)?;
    let stop = crate::cancel::stop();
    for (name, endpoints) in [("sources", sources), ("targets", targets)] {
        for &(u, cost) in endpoints {
            if stop.poll() {
                return Ok(None);
            }
            if u as usize >= p.node_count() {
                return Err(GraphError::InvalidArgument(format!("node index {u} out of range")));
            }
            if !cost.is_finite() || cost < 0.0 {
                return Err(GraphError::InvalidArgument(format!(
                    "{name} costs must be finite and non-negative, got {cost}"
                )));
            }
        }
    }
    if let Some(a) = astar {
        AStar::new(a.metric, a.scale)?;
        if p.coord_dim() == 0 {
            return Err(GraphError::InvalidArgument("astar needs a projection with node coordinates".into()));
        }
    } else if goal_coords.is_some() {
        return Err(GraphError::InvalidArgument("goal_coords is an option of method 'astar'".into()));
    }
    if let Some(goal) = goal_coords {
        if goal.len() != p.coord_dim() || goal.iter().any(|v| !v.is_finite()) {
            return Err(GraphError::InvalidArgument(
                "goal_coords must contain one finite value per coordinate dimension".into(),
            ));
        }
    }
    if sources.is_empty() || targets.is_empty() {
        return Ok(None);
    }
    let n = p.node_count();
    let mut exits = vec![f64::INFINITY; n];
    for &(u, cost) in targets {
        exits[u as usize] = exits[u as usize].min(cost + 0.0);
    }
    let h = |u: u32| {
        let Some(a) = astar.filter(|a| a.scale != 0.0) else {
            return 0.0;
        };
        let coords = p.coords(u).expect("coordinates checked");
        match goal_coords {
            Some(goal) => a.scale * a.metric.distance(coords, goal),
            None => targets
                .iter()
                .map(|&(v, exit)| a.scale * a.metric.distance(coords, p.coords(v).unwrap()) + exit)
                .fold(f64::INFINITY, f64::min),
        }
    };
    let none = u32::MAX;
    let mut dist = vec![f64::INFINITY; n];
    let mut parent = vec![none; n];
    // Seed in node order, independent of the caller's endpoint ordering.
    for &(u, cost) in sources {
        dist[u as usize] = dist[u as usize].min(cost + 0.0);
    }
    let mut heap = BinaryHeap::new();
    for (u, &d) in dist.iter().enumerate() {
        if d.is_finite() && max_cost.is_none_or(|limit| d <= limit) {
            heap.push((Reverse((d + h(u as u32)).to_bits()), d.to_bits(), Reverse(u as u32)));
        }
    }
    let (mut best, mut target, mut settled) = (f64::INFINITY, none, 0);
    let report = crate::cancel::progress();
    report.start("shortest path multi", None);
    while let Some((Reverse(fbits), dbits, Reverse(u))) = heap.pop() {
        if stop.poll() {
            break;
        }
        let (f, du) = (f64::from_bits(fbits), f64::from_bits(dbits));
        if f > best || max_cost.is_some_and(|limit| f > limit) {
            break;
        }
        if du != dist[u as usize] {
            continue;
        }
        settled += 1;
        report.add(1);
        let total = du + exits[u as usize];
        if total.is_finite()
            && max_cost.is_none_or(|limit| total <= limit)
            && (total < best || (total == best && u < target))
        {
            best = total;
            target = u;
        }
        let weights = p.out_weights(u);
        for (i, &v) in p.out_neighbors(u).iter().enumerate() {
            if stop.poll() {
                break;
            }
            let w = weights.map_or(1.0, |w| w[i]);
            let nd = du + w;
            if !nd.is_finite() || max_cost.is_some_and(|limit| nd > limit) {
                continue;
            }
            if nd < dist[v as usize] {
                dist[v as usize] = nd;
                parent[v as usize] = u;
                heap.push((Reverse((nd + h(v)).to_bits()), nd.to_bits(), Reverse(v)));
            } else if nd == dist[v as usize] && du < nd && u < parent[v as usize] {
                parent[v as usize] = u;
            }
        }
    }
    if target == none {
        return Ok(None);
    }
    let mut nodes = vec![target];
    while parent[*nodes.last().unwrap() as usize] != none {
        if stop.poll() {
            return Ok(None);
        }
        nodes.push(parent[*nodes.last().unwrap() as usize]);
    }
    nodes.reverse();
    Ok(Some(MultiPath { source: nodes[0], target, path: DensePath { nodes, cost: best, settled } }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{pathfinding::Metric, Direction};

    fn projection(weights: &[f64]) -> Projection {
        Projection::from_edges(
            ["a", "b", "c", "d", "isolated"].map(String::from).to_vec(),
            Direction::Out,
            &[0u32, 0, 1, 2],
            &[1u32, 2, 3, 3],
            Some(weights),
        )
        .unwrap()
        .with_coords(1, vec![0.0, 1.0, 1.0, 2.0, 9.0])
        .unwrap()
        .finish()
    }

    #[test]
    fn terminal_costs_limits_duplicates_and_overlap() {
        let p = projection(&[1.0, 1.0, 1.0, 1.0]);
        let run = |sources: &[(u32, f64)], targets: &[(u32, f64)], limit| {
            shortest_path_multi(&p, sources, targets, None, None, limit).unwrap()
        };
        let r = run(&[(0, 3.0), (0, 2.0), (4, 0.0)], &[(1, 100.0), (3, 1.0)], None).unwrap();
        assert_eq!((r.path.nodes, r.path.cost, r.source, r.target), (vec![0, 1, 3], 5.0, 0, 3));
        assert!(run(&[(0, 2.0)], &[(3, 1.0)], Some(4.99)).is_none());
        assert_eq!(run(&[(0, 2.0)], &[(3, 1.0)], Some(5.0)).unwrap().path.cost, 5.0);
        let r = run(&[(0, 0.0), (3, 10.0)], &[(0, 100.0), (3, 0.0)], None).unwrap();
        assert_eq!((r.source, r.target, r.path.cost), (0, 3, 2.0));
        assert!(run(&[], &[(3, 0.0)], None).is_none());
        assert!(run(&[(0, 0.0)], &[], None).is_none());
    }

    #[test]
    fn ties_and_zero_cycles_and_impassable_edges() {
        let p = Projection::from_edges(
            ["a", "b", "c", "d"].map(String::from).to_vec(),
            Direction::Out,
            &[0u32, 0, 1, 2, 1, 2],
            &[1u32, 2, 2, 1, 3, 3],
            Some(&[0.0, 0.0, 0.0, 0.0, f64::INFINITY, 1.0]),
        )
        .unwrap()
        .finish();
        let r = shortest_path_multi(&p, &[(0, -0.0)], &[(3, 0.0)], None, None, None).unwrap().unwrap();
        assert_eq!(r.path.nodes, [0, 2, 3]);
        assert_eq!(r.path.cost, 1.0);
        let r =
            shortest_path_multi(&p, &[(2, 0.0), (1, 0.0)], &[(2, 0.0), (1, 0.0)], None, None, None).unwrap().unwrap();
        assert_eq!((r.source, r.target), (1, 1));
        let p = projection(&[f64::MAX, f64::INFINITY, f64::MAX, 1.0]);
        assert!(shortest_path_multi(&p, &[(0, 0.0)], &[(3, 0.0)], None, None, None).unwrap().is_none());
    }

    #[test]
    fn astar_real_goal_and_target_estimate() {
        let p = projection(&[1.0, 1.0, 1.0, 1.0]);
        let a = AStar::new(Metric::Manhattan, 1.0).unwrap();
        for goal in [None, Some(&[3.0][..])] {
            let r = shortest_path_multi(&p, &[(0, 2.0), (4, 0.0)], &[(1, 10.0), (3, 1.0)], Some(&a), goal, None)
                .unwrap()
                .unwrap();
            assert_eq!((r.path.nodes, r.path.cost), (vec![0, 1, 3], 5.0));
            assert!(shortest_path_multi(&p, &[(0, 2.0)], &[(3, 1.0)], Some(&a), goal, Some(4.0)).unwrap().is_none());
        }
    }

    #[test]
    fn virtual_source_and_rounded_positive_weights() {
        let p = Projection::from_edges(
            ["a", "b"].map(String::from).to_vec(),
            Direction::Out,
            &[1u32],
            &[0u32],
            Some(&[1.0]),
        )
        .unwrap()
        .finish();
        let r = shortest_path_multi(&p, &[(0, 1.0), (1, 0.0)], &[(0, 0.0)], None, None, None).unwrap().unwrap();
        assert_eq!(r.path.nodes, [1, 0]);
        let p = Projection::from_edges(
            ["a", "b"].map(String::from).to_vec(),
            Direction::Out,
            &[0u32, 1, 0],
            &[1u32, 0, 0],
            Some(&[1.0; 3]),
        )
        .unwrap()
        .finish();
        let r = shortest_path_multi(&p, &[(0, 1e20)], &[(1, 0.0)], None, None, None).unwrap().unwrap();
        assert_eq!(r.path.nodes, [0, 1]);
    }

    #[test]
    fn long_path_and_cancellation() {
        let n = 10_000u32;
        let p = Projection::from_edges(
            (0..n).map(|i| i.to_string()).collect(),
            Direction::Out,
            &(0..n - 1).collect::<Vec<_>>(),
            &(1..n).collect::<Vec<_>>(),
            None,
        )
        .unwrap()
        .finish();
        let r = shortest_path_multi(&p, &[(0, 0.5)], &[(n - 1, 0.25)], None, None, None).unwrap().unwrap();
        assert_eq!(r.path.nodes.len(), n as usize);
        assert_eq!(r.path.cost, n as f64 - 0.25);
        use std::{cell::Cell, rc::Rc};
        let calls = Rc::new(Cell::new(0));
        let count = calls.clone();
        let hook = Rc::new(move || {
            count.set(count.get() + 1);
            count.get() >= 3
        });
        let result = crate::cancel::run_polling(&crate::cancel::Token::new(), hook, || {
            shortest_path_multi(&p, &[(0, 0.0)], &[(n - 1, 0.0)], None, None, None)
        });
        assert_eq!(result, Err(GraphError::Interrupted));
    }

    #[test]
    fn validation() {
        let p = projection(&[1.0; 4]);
        for cost in [-1.0, f64::INFINITY, f64::NAN] {
            for (s, t) in [(vec![(0, cost)], vec![(3, 0.0)]), (vec![(0, 0.0)], vec![(3, cost)])] {
                assert!(shortest_path_multi(&p, &s, &t, None, None, None).is_err());
            }
        }
        assert!(shortest_path_multi(&p, &[(5, 0.0)], &[], None, None, None).is_err());
        assert!(shortest_path_multi(&p, &[], &[], None, Some(&[1.0]), None).is_err());
        let a = AStar::default();
        for goal in [&[][..], &[f64::NAN][..], &[0.0, 1.0][..]] {
            assert!(shortest_path_multi(&p, &[], &[], Some(&a), Some(goal), None).is_err());
        }
    }
}
