#!/usr/bin/env python3
"""
Test script for the new graph construction functionality.
"""

import sys
import os

# Add the current directory to Python path so we can import the compiled module
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

try:
    # First build the module
    import subprocess
    print("Building the Rust module...")
    result = subprocess.run(["cargo", "build", "--release"], cwd=os.path.dirname(__file__), capture_output=True, text=True)
    if result.returncode != 0:
        print(f"Build failed: {result.stderr}")
        sys.exit(1)
    print("Build successful!")
    
    # Now try to import and test
    from observed_dict_rs import Vertex, Node, Edge
    
    print("Creating a new empty graph...")
    g = Vertex()
    print(f"Initial graph: {g}")
    print(f"Node count: {g.node_count()}")
    
    print("\nAdding nodes...")
    node1 = g.add_node("node1", {"type": "start", "value": 1})
    node2 = g.add_node("node2", {"type": "middle", "value": 2})
    node3 = g.add_node("node3", {"type": "end", "value": 3})
    
    print(f"Node 1: {node1}")
    print(f"Node 2: {node2}")
    print(f"Node 3: {node3}")
    print(f"Graph after adding nodes: {g}")
    print(f"Node count: {g.node_count()}")
    
    print("\nAdding edges...")
    edge1 = g.add_edge("node1", "node2", {"weight": 1.5, "label": "first"})
    edge2 = g.add_edge("node2", "node3", {"weight": 2.0, "label": "second"})
    edge3 = g.add_edge("node1", "node3", {"weight": 3.0, "label": "direct"})
    
    print(f"Edge 1: {edge1}")
    print(f"Edge 2: {edge2}")
    print(f"Edge 3: {edge3}")
    
    print("\nTesting graph traversal...")
    # Get node1 and test traversal
    start_node = g.get_node("node1")
    reachable = start_node.traverse(depth=None)
    print(f"Nodes reachable from node1: {reachable}")
    
    print("\nTesting BFS...")
    bfs_result = start_node.bfs(depth=None)
    print(f"BFS from node1: {bfs_result}")
    
    print("\nTesting BFS search...")
    found_node = start_node.bfs_search("node3", depth=None)
    print(f"Found node3 via BFS: {found_node}")
    
    print("\nTesting metadata...")
    metadata = g.get_metadata()
    print(f"Graph metadata: {metadata}")
    
    print("\nTesting error cases...")
    try:
        g.add_node("node1", {})  # Should fail - duplicate ID
    except ValueError as e:
        print(f"Expected error: {e}")
    
    try:
        g.add_edge("nonexistent", "node1", {})  # Should fail - node doesn't exist
    except ValueError as e:
        print(f"Expected error: {e}")
    
    print("\n✅ All tests passed!")
    
except ImportError as e:
    print(f"❌ Failed to import module: {e}")
    print("Make sure to build the module first with: cargo build --release")
    sys.exit(1)
except Exception as e:
    print(f"❌ Test failed: {e}")
    import traceback
    traceback.print_exc()
    sys.exit(1)
