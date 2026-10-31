"""Independent encoding controls and numeric-heavy JSON loading control.

Run in a fresh process for each frozen artifact, alternating revision order.
"""
import json,statistics,sys,timeit
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from compare_binary_decode import make_graph
from ironweaver import Vertex
rows=[]
# No decode operations before these encoding controls.
for n,profile,count in [(1,'scalar',100000),(1000,'scalar',100),(10,'unicode',10000)]:
 g=make_graph(n,profile)
 samples=[v/count*1e6 for v in timeit.repeat(g.to_binary,number=count,repeat=7)]
 rows.append({'name':f'encode/{n}/{profile}','median_us':statistics.median(samples),'samples_us':samples})
g=make_graph(10,'embedding');text=g.save_to_json()
samples=[v/100*1e6 for v in timeit.repeat(lambda:Vertex.load_from_json(text),number=100,repeat=7)]
rows.append({'name':'decode_json/10/embedding','median_us':statistics.median(samples),'samples_us':samples})
print(json.dumps(rows))
