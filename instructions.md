# Ironweaver LLM Coding Instructions

Use these core APIs first:

- `Vertex()` — create a graph.
- `Vertex.add_node(id, attr={})` — add a node.
- `Vertex.add_edge(from_id, to_id, attr={})` — add an edge.
- `Vertex.get_node(id)` / `Vertex.has_node(id)` / `Vertex.node_count()` — retrieve, check existence, and count nodes.
- `Vertex.filter(...)` — create a filtered subgraph.
- `Vertex.expand(source, depth=1)` — expand subgraph neighborhood.
- `Vertex.shortest_path_bfs(start, end, max_depth=None)` — shortest path by BFS.
- `Vertex.random_walks(...)` — sample random walks.
- `Node.traverse(depth=None)` / `Node.bfs(depth=None)` / `Node.bfs_search(target_id, depth=None)` — node-level traversal.
- `Vertex.to_networkx()` — convert to NetworkX.
- `Vertex.save_to_json(...)`, `save_to_binary(...)`, `save_to_binary_f16(...)` — persist graph.
- `Vertex.load_from_json(...)`, `Vertex.load_from_binary(...)` — load graph.
- `parse_lgf(text)` / `parse_lgf_file(path)` — parse LGF into a `Vertex`.
