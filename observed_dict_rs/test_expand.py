#!/usr/bin/env python3
"""
Test script for the new Vertex.expand method
"""

import sys
import os
sys.path.insert(0, os.path.join(os.path.dirname(__file__), 'target/wheels'))

try:
    import observed_dict_rs
except ImportError:
    print("Building the Python module first...")
    os.system("cd /home/inexen/Documents/python/cpyg/cpyg/observed_dict_rs && maturin develop")
    import observed_dict_rs

def test_expand_method():
    """Test the Vertex.expand method with the user's example"""
    print("Testing Vertex.expand method")
    print("=" * 50)
    
    # Create the full graph (g)
    g = observed_dict_rs.Vertex()
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
    
    print(f"Full graph has nodes: {g.keys()}")
    print(f"Full graph node count: {g.node_count()}")
    
    # Get a path vertex (subset of the graph)
    path_vertex = g.shortest_path_bfs('node1', 'node3')
    print(f"\nPath vertex from node1 to node3 has nodes: {path_vertex.keys()}")
    print(f"Path vertex node count: {path_vertex.node_count()}")
    
    # Test expanding the path vertex by depth=1
    print(f"\nExpanding path vertex by depth=1:")
    expanded_path = path_vertex.expand(g, depth=1)
    print(f"Expanded vertex has nodes: {expanded_path.keys()}")
    print(f"Expanded vertex node count: {expanded_path.node_count()}")
    
    # Test expanding with default depth (should be 1)
    print(f"\nExpanding path vertex with default depth:")
    expanded_default = path_vertex.expand(g)
    print(f"Expanded (default) vertex has nodes: {expanded_default.keys()}")
    print(f"Expanded (default) vertex node count: {expanded_default.node_count()}")
    
    # Test expanding with depth=2
    print(f"\nExpanding path vertex by depth=2:")
    expanded_depth2 = path_vertex.expand(g, depth=2)
    print(f"Expanded (depth=2) vertex has nodes: {expanded_depth2.keys()}")
    print(f"Expanded (depth=2) vertex node count: {expanded_depth2.node_count()}")
    
    # Verify that edges are preserved correctly
    print(f"\nChecking edges in expanded vertex (depth=1):")
    for node_id in expanded_path.keys():
        node = expanded_path.get_node(node_id)
        print(f"Node {node_id} edges:")
        for edge in node.edges:
            to_node_id = edge.to.id
            print(f"  -> {to_node_id}")
    
    print("\n" + "=" * 50)
    print("✓ Expand method test completed successfully!")

if __name__ == "__main__":
    test_expand_method()
