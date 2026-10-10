import json
from ironweaver import Vertex

g = Vertex()
for i in range(40):
    g.add_node(f"n{i}", {"x": i % 5})
for i in range(40):
    for step in (1, 3, 7):
        g.add_edge(f"n{i}", f"n{(i + step) % 40}", {"weight": 1.0 + (i % 3)}, type="link")
p = g.project(direction="both", weight="weight")
out = {}
for seed in (0, 1, 7, 42):
    out[seed] = {
        "betweenness": p.betweenness_centrality(k=10, seed=seed),
        "leiden": p.leiden(seed=seed),
        "fastrp": p.fastrp(8, seed=seed),
        "node2vec": p.node2vec_walks(10, 3, p=0.5, q=2.0, seed=seed),
        "walks": g.random_walks("n0", 10, 100, seed=seed),
        "stratified": g.random_walks(None, 10, 100, stratified=True, seed=seed),
    }
print(json.dumps(out, sort_keys=True, separators=(",", ":")))
