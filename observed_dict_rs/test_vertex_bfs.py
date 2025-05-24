#!/usr/bin/env python3
"""
Test script for the new Vertex.bfs method
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

def test_vertex_bfs():
    """Test the Vertex.bfs method with different scenarios"""
    
    # Create a new vertex (graph)
    vertex = observed_dict_rs.Vertex()
    
    # Add nodes
    node_a = vertex.add_node("A", {"label": "Node A"})
    node_b = vertex.add_node("B", {"label": "Node B"})
    node_c = vertex.add_node("C", {"label": "Node C"})
    node_d = vertex.add_node("D", {"label": "Node D"})
    node_e = vertex.add_node("E", {"label": "Node E"})
    
    # Add edges to create a graph structure:
    # A -> B -> D
    # A -> C -> E
    vertex.add_edge("A", "B", {"weight": 1})
    vertex.add_edge("A", "C", {"weight": 2})
    vertex.add_edge("B", "D", {"weight": 3})
    vertex.add_edge("C", "E", {"weight": 4})
    
    print("Created graph with structure:")
    print("A -> B -> D")
    print("A -> C -> E")
    print()
    
    # Test 1: BFS from A with no target (should return all reachable nodes)
    print("Test 1: BFS from A (no target, unlimited depth)")
    try:
        result = vertex.bfs("A")
        print(f"Found nodes: {result.keys()}")
        expected = {"A", "B", "C", "D", "E"}
        if set(result.keys()) == expected:
            print("✓ SUCCESS: Found all expected nodes")
        else:
            print(f"✗ FAILURE: Expected {expected}, got {set(result.keys())}")
    except Exception as e:
        print(f"✗ ERROR: {e}")
    print()
    
    # Test 2: BFS from A with target D
    print("Test 2: BFS from A targeting node D")
    try:
        result = vertex.bfs("A", target_node_id="D")
        print(f"Found nodes: {result.keys()}")
        # Should find A, B, C, D (stops when D is found)
        if "D" in result.keys() and "A" in result.keys():
            print("✓ SUCCESS: Found target D and includes starting node A")
        else:
            print(f"✗ FAILURE: Expected to find D and A in result")
    except Exception as e:
        print(f"✗ ERROR: {e}")
    print()
    
    # Test 3: BFS with depth limit
    print("Test 3: BFS from A with max_depth=1")
    try:
        result = vertex.bfs("A", max_depth=1)
        print(f"Found nodes: {result.keys()}")
        # With max_depth=1, should find A, B, C (but not D, E)
        expected = {"A", "B", "C"}
        if set(result.keys()) == expected:
            print("✓ SUCCESS: Depth limit working correctly")
        else:
            print(f"✗ FAILURE: Expected {expected}, got {set(result.keys())}")
    except Exception as e:
        print(f"✗ ERROR: {e}")
    print()
    
    # Test 4: BFS from non-existent root
    print("Test 4: BFS from non-existent root node")
    try:
        result = vertex.bfs("Z")
        print("✗ FAILURE: Should have raised ValueError")
    except ValueError as e:
        print(f"✓ SUCCESS: Correctly raised ValueError: {e}")
    except Exception as e:
        print(f"✗ ERROR: Unexpected exception: {e}")
    print()
    
    # Test 5: Search for non-existent target
    print("Test 5: BFS searching for non-existent target")
    try:
        result = vertex.bfs("A", target_node_id="Z")
        print("✗ FAILURE: Should have raised ValueError")
    except ValueError as e:
        print(f"✓ SUCCESS: Correctly raised ValueError: {e}")
    except Exception as e:
        print(f"✗ ERROR: Unexpected exception: {e}")
    print()

if __name__ == "__main__":
    test_vertex_bfs()
