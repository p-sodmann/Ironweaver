"""Benchmark serial vs parallel BFS implementations.

This script constructs linear graphs of varying sizes and records
execution times for both the existing serial BFS (via ``Node.bfs``)
and the new ``Vertex.parallel_bfs`` method. Results are written to
``performance_results/parallel_bfs_comparison.csv``.
"""

import csv
import datetime
import time

from ironweaver import Vertex


def build_graph(size: int) -> Vertex:
    g = Vertex()
    for i in range(size):
        g.add_node(f"n{i}", {})
    for i in range(size - 1):
        g.add_edge(f"n{i}", f"n{i+1}", {})
    return g


def run_benchmark(sizes):
    results = []
    for size in sizes:
        g = build_graph(size)
        root = g.get_node("n0")

        # Serial BFS using Node.bfs
        start = time.perf_counter()
        root.bfs(depth=None)
        serial_time = time.perf_counter() - start

        # Parallel BFS using Vertex.parallel_bfs
        start = time.perf_counter()
        g.parallel_bfs("n0")
        parallel_time = time.perf_counter() - start

        timestamp = datetime.datetime.now().isoformat()
        results.append({
            "timestamp": timestamp,
            "graph_size": size,
            "serial_time": serial_time,
            "parallel_time": parallel_time,
        })
    return results


def save_csv(results, path):
    with open(path, "w", newline="") as f:
        writer = csv.DictWriter(
            f,
            fieldnames=["timestamp", "graph_size", "serial_time", "parallel_time"],
        )
        writer.writeheader()
        for row in results:
            writer.writerow(row)


if __name__ == "__main__":
    sizes = [100, 500, 1000]
    results = run_benchmark(sizes)
    save_csv(results, "parallel_bfs_comparison.csv")
    for r in results:
        print(r)
