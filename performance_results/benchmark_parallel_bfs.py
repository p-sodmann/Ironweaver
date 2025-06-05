import time
from ironweaver import Vertex


def build_linear_graph(n):
    v = Vertex()
    for i in range(n):
        v.add_node(f"n{i}", {"value": i})
    for i in range(n - 1):
        v.add_edge(f"n{i}", f"n{i+1}", None)
    return v


def run_bench(size=5000):
    v = build_linear_graph(size)
    start_id = "n0"
    target_id = f"n{size-1}"

    t0 = time.perf_counter()
    v.shortest_path_bfs(start_id, target_id)
    serial_time = time.perf_counter() - t0

    t0 = time.perf_counter()
    v.parallel_bfs(start_id, target_id)
    parallel_time = time.perf_counter() - t0

    print(f"Graph size: {size}")
    print(f"serial bfs:   {serial_time:.6f}s")
    print(f"parallel bfs: {parallel_time:.6f}s")


if __name__ == "__main__":
    run_bench()
