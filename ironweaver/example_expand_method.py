#!/usr/bin/env python3
"""
Test script for the new expand method
"""

from ironweaver import Node, Edge, Vertex

def test_expand_method():
    """Test the new expand method with the example from the user"""
    print("Testing the new expand method:")
    
    # Create the full graph (g)
    g = Vertex()
    node1 = g.add_node('node1', {'value': 1})
    node2 = g.add_node('node2', {'value': 2})
    node3 = g.add_node('node3', {'value': 3})
    node4 = g.add_node('node4', {'value': 4})
    node5 = g.add_node('node5', {'value': 5})

    edge1 = g.add_edge('node1', 'node2', {'weight': 1.0})
    edge2 = g.add_edge('node2', 'node3', {'weight': 2.0})
    edge3 = g.add_edge('node1', 'node3', {'weight': 3.0})
    edge4 = g.add_edge('node3', 'node4', {'weight': 4.0})
    edge5 = g.add_edge('node4', 'node5', {'weight': 4.0})
    
    print(f"Original graph has nodes: {g.keys()}")
    
    # Create a path (using shortest_path_bfs as in the example)
    path_vertex = g.shortest_path_bfs('node1', 'node3')
    print(f"Path vertex contains nodes: {path_vertex.keys()}")
    
    # Test expand with depth=1 (default)
    expanded_path = path_vertex.expand(g, depth=1)
    print(f"Expanded path (depth=1) contains nodes: {expanded_path.keys()}")
    
    # Test expand with depth=2
    expanded_path_d2 = path_vertex.expand(g, depth=2)
    print(f"Expanded path (depth=2) contains nodes: {expanded_path_d2.keys()}")
    
    # Verify the expansion worked correctly
    print("\nVerification:")
    print(f"Original path: {sorted(path_vertex.keys())}")
    print(f"Expanded (d=1): {sorted(expanded_path.keys())}")
    print(f"Expanded (d=2): {sorted(expanded_path_d2.keys())}")
    
    # Expected for depth=1: original nodes (node1, node3) + their direct neighbors (node2, node4)
    expected_d1 = {'node1', 'node2', 'node3', 'node4'}
    actual_d1 = set(expanded_path.keys())
    
    print(f"\nExpected nodes for depth=1: {sorted(expected_d1)}")
    print(f"Actual nodes for depth=1: {sorted(actual_d1)}")
    print(f"Depth=1 test {'PASSED' if actual_d1 == expected_d1 else 'FAILED'}")
    
    # Expected for depth=2: all nodes since the graph is small
    expected_d2 = {'node1', 'node2', 'node3', 'node4', 'node5'}
    actual_d2 = set(expanded_path_d2.keys())
    
    print(f"\nExpected nodes for depth=2: {sorted(expected_d2)}")
    print(f"Actual nodes for depth=2: {sorted(actual_d2)}")
    print(f"Depth=2 test {'PASSED' if actual_d2 == expected_d2 else 'FAILED'}")

def test_expand_edge_filtering():
    """Test that edges are properly filtered in the expanded vertex"""
    print("\n" + "="*50)
    print("Testing edge filtering in expanded vertex:")
    
    # Create a simple graph
    g = Vertex()
    g.add_node('A', {'value': 1})
    g.add_node('B', {'value': 2})
    g.add_node('C', {'value': 3})
    g.add_node('D', {'value': 4})
    
    g.add_edge('A', 'B', {'weight': 1})
    g.add_edge('B', 'C', {'weight': 2})
    g.add_edge('A', 'C', {'weight': 3})
    g.add_edge('C', 'D', {'weight': 4})
    
    # Create a subset with just node A
    subset = Vertex()
    subset.add_node('A', {'value': 1})
    
    # Expand by depth 1
    expanded = subset.expand(g, depth=1)
    print(f"Expanded subset contains: {expanded.keys()}")
    
    # Check edges in the expanded vertex
    for node_id in expanded.keys():
        node = expanded.get_node(node_id)
        edges = node.edges
        print(f"Node {node_id} has {len(edges)} edges:")
        for edge in edges:
            to_id = edge.to.id
            print(f"  -> {to_id}")

if __name__ == "__main__":
    test_expand_method()
    test_expand_edge_filtering()
