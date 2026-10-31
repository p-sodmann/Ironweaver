"""Frozen-artifact worker for binary/JSON decode latency and unaffected encoding.

Run each revision in a separate Python process; orchestration should alternate
revision order. Values include result destruction (normal handler lifecycle).
"""
import gc
import hashlib
import importlib
import json
from pathlib import Path
import statistics
import sys
import timeit

from ironweaver import Vertex


def make_graph(n, profile):
    g = Vertex()
    def attrs(i):
        values = {'name': f'Node {i}', 'score': i/10, 'tags': ['a', 'b']}
        if profile == 'unique':
            values.update({f'unique-key-{i}-{j}': f'unique-value-{i}-{j}' for j in range(32)})
        elif profile == 'nested':
            values['details'] = {'settings': {'active': True}, 'items': [{'k': j} for j in range(8)]}
        elif profile == 'embedding':
            values['embedding'] = [i/10+j/1000 for j in range(384)]
        elif profile == 'unicode':
            values['κλειδί'] = '日本語🙂'
        return values
    g.add_nodes([(str(i), attrs(i)) for i in range(n)], labels=['Item'])
    g.add_edges([(str(i),str((i+d)%n),{'weight':1.25}) for i in range(n) for d in [1,2]], type='next')
    if profile == 'indexed':
        g.create_index('score')
    return g


def main():
    extension=Path(importlib.import_module('ironweaver._ironweaver').__file__)
    results={'python':sys.version,'extension_sha256':hashlib.sha256(extension.read_bytes()).hexdigest(),'cases':[]}
    fixtures=[(n,'scalar') for n in [0,1,10,100,1000,10000]]+[(10,p) for p in ['unique','nested','embedding','indexed','unicode']]
    for n,profile in fixtures:
        g=make_graph(n,profile)
        blob=g.to_binary()
        text=g.save_to_json()
        for load in [lambda:Vertex.from_binary(blob),lambda:Vertex.load_from_json(text)]:
            loaded=load()
            assert len(loaded)==n
            if n:
                assert loaded['0'].attr==g['0'].attr
                assert len(loaded['0'].edges)==2
            if profile=='indexed':
                assert [v.id for v in loaded.find('score',0)]==['0']
        number=max(1,min(1000,10000//max(1,n)))
        row={'nodes':n,'profile':profile,'number':number,'bytes':len(blob)}
        for name,fn in [('decode_binary',lambda:Vertex.from_binary(blob)),('decode_json',lambda:Vertex.load_from_json(text)),
                        ('encode_binary_control',g.to_binary)]:
            gc.collect()
            values=[v/number*1e6 for v in timeit.repeat(fn,number=number,repeat=5)]
            row[name]={'median_us':statistics.median(values),'samples_us':values}
        results['cases'].append(row)
    print(json.dumps(results,indent=2))


if __name__=='__main__':
    main()
