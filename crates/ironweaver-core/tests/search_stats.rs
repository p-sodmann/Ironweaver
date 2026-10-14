use ironweaver_core::batch;
use ironweaver_core::pathfinding::{find_path, find_path_limited, PathQuery};
use ironweaver_core::{Budget, Direction, Graph, GraphError, Projection, Record};

#[test]
fn path_counts_agree_with_the_budget_meter_and_batch_counters() {
    let mut graph = Graph::<Record, Record>::new();
    let nodes: Vec<_> = (0..4).map(|i| graph.add_node(i.to_string(), Record::default()).unwrap()).collect();
    for pair in nodes.windows(2) {
        graph.add_edge(pair[0], pair[1], Record::default()).unwrap();
    }
    for mut query in
        [PathQuery::bfs(), PathQuery::dijkstra(), PathQuery::astar(ironweaver_core::pathfinding::Heuristic::Zero)]
    {
        let limited =
            find_path_limited::<_, _, GraphError>(&graph, nodes[0], nodes[3], &mut query, Budget::UNLIMITED).unwrap();
        let result = limited.value.unwrap();
        assert_eq!(result.expanded, Some(limited.visited));
        let single = find_path::<_, _, GraphError>(&graph, nodes[0], nodes[3], &mut query).unwrap().unwrap();
        assert_eq!(single.expanded, result.expanded);
        if query.method.weighted {
            assert_eq!(result.expanded, Some(4));
        } else {
            assert_eq!(result.expanded, Some(3));
        }
    }
    let projection = Projection::build::<_, _, GraphError>(
        &graph,
        Direction::Out,
        &ironweaver_core::pathfinding::EdgeCost::weighted(None, None),
    )
    .unwrap();
    for weighted in [false, true] {
        let paths = batch::shortest_paths(&projection, &[(0, 3), (0, 0), (3, 0)], weighted, None).unwrap();
        assert_eq!(paths[0].as_ref().unwrap().settled, 4);
        assert_eq!(paths[1].as_ref().unwrap().settled, 1);
        assert!(paths[2].is_none());
    }
}
