"""Ctrl+C stops long computations: each call below would run for a very long
time; a simulated Ctrl+C must raise KeyboardInterrupt soon after, and the
graph must stay usable.

Computations that release the GIL are interrupted from another thread
(``_thread.interrupt_main``). Searches that hold the GIL keep that thread
from running, so they get a real signal (SIGALRM, handled like SIGINT) as a
Ctrl+C would; ``setitimer`` doesn't exist on Windows."""

import _thread
import signal
import threading
import time

import pytest

import ironweaver as iw


def complete(n):
    v = iw.Vertex()
    ids = [f"n{i}" for i in range(n)]
    v.add_nodes(ids)
    v.add_edges([(a, b, {"w": 1.0, "to": b}) for a in ids for b in ids if a != b])
    return v


def interrupted_after(delay, fn):
    """Run fn, simulating Ctrl+C from another thread after *delay* seconds;
    the seconds between the interrupt and fn raising KeyboardInterrupt."""
    fired = []

    def fire():
        fired.append(time.perf_counter())
        _thread.interrupt_main()

    timer = threading.Timer(delay, fire)
    timer.start()
    try:
        fn()
    except KeyboardInterrupt:
        return time.perf_counter() - fired[0]
    # Finished first: absorb the interrupt before failing
    try:
        timer.join()
        time.sleep(1)
    except KeyboardInterrupt:
        pass
    pytest.fail("the computation ended before the interrupt")


def signalled_after(delay, fn):
    """Like interrupted_after, with a real signal (SIGALRM raising
    KeyboardInterrupt)."""
    previous = signal.signal(signal.SIGALRM, signal.default_int_handler)
    try:
        signal.setitimer(signal.ITIMER_REAL, delay)
        start = time.perf_counter()
        try:
            fn()
        except KeyboardInterrupt:
            return time.perf_counter() - start - delay
        signal.setitimer(signal.ITIMER_REAL, 0)
        pytest.fail("the computation ended before the interrupt")
    finally:
        signal.signal(signal.SIGALRM, previous)


@pytest.fixture(scope="module")
def big():
    return complete(1200)


def test_projection_algorithms(big):
    p = big.project()
    lag = interrupted_after(0.2, lambda: p.pagerank(tol=0.0, max_iter=10**12))
    assert lag < 2
    weighted = big.project(weight="w")  # Dijkstra from every node: seconds
    lag = interrupted_after(0.2, lambda: weighted.betweenness_centrality())
    assert lag < 2
    # Still usable, and gives the uninterrupted answer
    small = complete(5).project()
    assert small.pagerank() == pytest.approx({f"n{i}": 0.2 for i in range(5)})


def test_batch_queries(big):
    ids = [f"n{i}" for i in range(1200)]
    lag = interrupted_after(0.2, lambda: big.distances(ids, weight="w"))
    assert lag < 2


@pytest.mark.skipif(not hasattr(signal, "setitimer"), reason="needs setitimer")
def test_searches_holding_the_gil():
    v = complete(30)
    # No match (the last edge never qualifies), after ~30^7 attempts; the
    # filters keep results from piling up in memory meanwhile
    never = {"r": iw.attr("to") == "none"}
    lag = signalled_after(0.2, lambda: v.match("(a)-->(b)-->(c)-->(d)-->(e)-->(f)-[r]->(g)", where=never))
    assert lag < 2
    # Paths of 29 edges avoiding n29 don't exist; there are 28! to try
    start = v.get_node("n0")
    lag = signalled_after(0.2, lambda: start.paths(29, 29, uniqueness="path", where=iw.attr("to") != "n29"))
    assert lag < 2
    # The graph is intact and not left borrowed
    assert v.node_count() == 30
    assert len(v.match("(a)-->(b)", ids={"a": "n0"}, limit=3)) == 3
    v.add_node("extra")
    assert v.has_node("extra")
