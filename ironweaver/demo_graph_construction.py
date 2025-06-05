#!/usr/bin/env python3
"""
Demonstration of the new graph construction functionality.

This script shows how to:
1. Create an empty graph
2. Add nodes with attributes
3. Add edges between nodes
4. Traverse the graph
5. Save and load graphs
"""

from ironweaver import Vertex

def main():
    print("🚀 Graph Construction Demo")
    print("=" * 50)
    
    # Create an empty graph
    print("\n1. Creating an empty graph...")
    g = Vertex()
    print(f"   Empty graph: {g}")
    print(f"   Node count: {g.node_count()}")
    
    # Add nodes with attributes
    print("\n2. Adding nodes with attributes...")
    
    # Add a start node
    start_node = g.add_node("start", {
        "type": "entry_point", 
        "color": "green", 
        "value": 1
    })
    print(f"   Added: {start_node}")
    
    # Add processing nodes
    process1 = g.add_node("process1", {
        "type": "processor", 
        "color": "blue", 
        "operation": "transform",
        "value": 10
    })
    print(f"   Added: {process1}")
    
    process2 = g.add_node("process2", {
        "type": "processor", 
        "color": "blue", 
        "operation": "filter",
        "value": 20
    })
    print(f"   Added: {process2}")
    
    # Add an end node
    end_node = g.add_node("end", {
        "type": "exit_point", 
        "color": "red", 
        "value": 100
    })
    print(f"   Added: {end_node}")
    
    print(f"   Graph now: {g}")
    print(f"   Node count: {g.node_count()}")
    
    # Add edges between nodes
    print("\n3. Adding edges between nodes...")
    
    # Connect start to both processors
    edge1 = g.add_edge("start", "process1", {
        "weight": 0.8, 
        "label": "primary_path",
        "type": "data_flow"
    })
    print(f"   Added: {edge1}")
    
    edge2 = g.add_edge("start", "process2", {
        "weight": 0.3, 
        "label": "secondary_path",
        "type": "data_flow"
    })
    print(f"   Added: {edge2}")
    
    # Connect processors to end
    edge3 = g.add_edge("process1", "end", {
        "weight": 1.0, 
        "label": "output",
        "type": "result"
    })
    print(f"   Added: {edge3}")
    
    edge4 = g.add_edge("process2", "end", {
        "weight": 0.7, 
        "label": "filtered_output",
        "type": "result"
    })
    print(f"   Added: {edge4}")
    
    # Add a cross-connection
    edge5 = g.add_edge("process1", "process2", {
        "weight": 0.5, 
        "label": "cross_connect",
        "type": "internal"
    })
    print(f"   Added: {edge5}")
    
    # Show graph metadata
    print("\n4. Graph analysis...")
    metadata = g.get_metadata()
    print(f"   Metadata: {metadata}")
    
    # Traverse the graph
    print("\n5. Graph traversal...")
    
    # Get the start node and traverse
    start = g.get_node("start")
    
    # Depth-first traversal
    print("   Depth-first traversal from start:")
    dfs_result = start.traverse()
    print(f"   DFS result: {dfs_result}")
    
    # Breadth-first traversal
    print("   Breadth-first traversal from start:")
    bfs_result = start.bfs()
    print(f"   BFS result: {bfs_result}")
    
    # Search for specific node
    print("   Searching for 'end' node:")
    found = start.bfs_search("end")
    print(f"   Found: {found}")
    
    # Limited depth traversal
    print("   Limited depth traversal (depth=1):")
    limited = start.bfs(depth=1)
    print(f"   Depth-1 result: {limited}")
    
    # Save the graph
    print("\n6. Saving and loading the graph...")
    
    # Save to JSON
    print("   Saving to JSON...")
    g.save_to_json("demo_graph.json")
    print("   ✅ Saved to demo_graph.json")
    
    # Save to binary
    print("   Saving to binary...")
    g.save_to_binary("demo_graph.bin")
    print("   ✅ Saved to demo_graph.bin")
    
    # Load from JSON
    print("   Loading from JSON...")
    loaded_graph = Vertex.load_from_json("demo_graph.json")
    print(f"   Loaded graph: {loaded_graph}")
    
    # Verify the loaded graph
    loaded_metadata = loaded_graph.get_metadata()
    print(f"   Loaded metadata: {loaded_metadata}")
    
    print("\n7. Advanced features...")
    
    # Check node existence
    print(f"   Has 'start' node: {g.has_node('start')}")
    print(f"   Has 'nonexistent' node: {g.has_node('nonexistent')}")
    
    # Get node keys
    print(f"   All node IDs: {g.keys()}")
    
    # Access node by key
    node = g["start"]
    print(f"   Retrieved node 'start': {node}")
    
    print("\n✨ Demo completed successfully!")
    print("\nThe graph construction API provides:")
    print("   • g = Vertex() - Create empty graph")
    print("   • g.add_node(id, attrs) - Add nodes with attributes")
    print("   • g.add_edge(from_id, to_id, attrs) - Add edges between nodes")
    print("   • g.get_node(id) - Retrieve nodes by ID")
    print("   • g.has_node(id) - Check if node exists")
    print("   • g.node_count() - Get number of nodes")
    print("   • node.traverse() / node.bfs() - Graph traversal")
    print("   • g.save_to_json() / g.load_from_json() - Persistence")
    print("   • g.get_metadata() - Graph statistics")

if __name__ == "__main__":
    main()
