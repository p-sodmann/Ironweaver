"""Analyze retained process pairs; export pointwise CIs and two PNGs."""
import argparse
import csv
import json
import math
from pathlib import Path
import statistics

from scipy.stats import t


def stats(before, after):
    n=len(before)
    logs=[math.log(a/b) for b,a in zip(before,after)]
    mean=statistics.mean(logs)
    half=t.ppf(.975,n-1)*statistics.stdev(logs)/math.sqrt(n)
    diffs=[a-b for b,a in zip(before,after)]
    delta=statistics.mean(diffs)
    dh=t.ppf(.975,n-1)*statistics.stdev(diffs)/math.sqrt(n)
    lo,hi=[100*math.expm1(x) for x in (mean-half,mean+half)]
    status='unchanged' if all(a==b for a,b in zip(before,after)) else 'improved' if hi<0 else 'regression' if lo>0 else 'inconclusive'
    return dict(pairs=n,before=statistics.median(before),after=statistics.median(after),change=100*math.expm1(mean),ci_low=lo,ci_high=hi,status=status,delta=delta,delta_low=delta-dh,delta_high=delta+dh)


def extract(raw, repeat=False):
    rows=[]
    def add(suite,name,pairs,unit='seconds',scale=1):
        b=[statistics.median(p['baseline'])*scale for p in pairs]
        a=[statistics.median(p['candidate'])*scale for p in pairs]
        rows.append(dict(suite=suite,name=name,unit=unit,**stats(b,a)))
    prefix='repeat-' if repeat else ''
    for path in sorted(raw.glob(prefix+'construction*.json')):
        data=json.loads(path.read_text())
        for name,r in data['results'].items(): add('construction',name,r['raw'])
    for path in sorted(raw.glob(prefix+'traversal*.json')):
        data=json.loads(path.read_text())
        for name in data['raw'][0]['baseline']:
            pairs=[dict(baseline=[p['baseline'][name]['seconds']],candidate=[p['candidate'][name]['seconds']]) for p in data['raw']]
            add('traversal',name,pairs)
    for path in sorted(raw.glob(prefix+'deletion*.json')):
        for r in json.loads(path.read_text())['results']:
            add('deletion',f"{r['shape']}/{r['edges']}",r['raw_pairs'],scale=1e-9)
    for path in sorted(raw.glob(prefix+'binding-focus*.json')):
        for n,pairs in json.loads(path.read_text())['cases'].items():
            if not pairs: continue
            for name in pairs[0]['baseline']['metrics']:
                add('python-focus',name+'/'+n,[{revision:pair[revision]['metrics'][name] for revision in ('baseline','candidate')} for pair in pairs])
    for path in sorted(raw.glob(prefix+'suite*.json')):
        for case,pairs in json.loads(path.read_text())['cases'].items():
            if not pairs: continue
            for name in pairs[0]['baseline']['metrics']:
                samples=[{revision:p[revision]['metrics'][name]['samples'] for revision in ('baseline','candidate')} for p in pairs]
                add(case.split(':')[0],case.replace(':','/')+'/'+name,samples,pairs[0]['baseline']['metrics'][name]['unit'])
    return rows


def short(name):
    return (name.replace('networkx/','').replace('libraries/','').replace('memory/','')
        .replace('/threads=1','').replace('/threads=2',' / 2 threads')
        .replace('Weakly connected components','WCC').replace('Strongly connected components','SCC')
        .replace('Communities (Leiden / Louvain)','Leiden').replace('Dijkstra from one node (weighted)','Dijkstra')
        .replace('Minimum spanning forest (weight)','MSF').replace('Loop shortest paths (control)','Loop paths')
        .replace('subgraph-small','edgeless copy (3 nodes)').replace('None','full').replace('Some(3)','depth 3').replace('false','unreserved').replace('true','reserved'))


def plots(rows,raw,out):
    import matplotlib
    matplotlib.use('Agg')
    import matplotlib.pyplot as plt
    from matplotlib.lines import Line2D
    colors=dict(improved='#167455',regression='#bc3540',inconclusive='#74808d',unchanged='#74808d')
    def selected(row): return row.get('repeat',row)
    def point(ax,y,row,memory=False):
        s=selected(row); state=s['status']; color=colors[state]
        val,lo,hi=([s[k]/2**20 for k in ('delta','delta_low','delta_high')] if memory else [s[k] for k in ('change','ci_low','ci_high')])
        ax.errorbar(val,y,xerr=[[max(0,val-lo)],[max(0,hi-val)]],fmt='D' if state=='regression' else 'o',mfc='white' if state in ('inconclusive','unchanged') else color,color=color,capsize=2.5,ms=4,lw=1)
    names=[('construction',f'{case}/{n}/threads=1') for n in (1000,20000,100000) for case in ('subgraph','subgraph-small')]
    names += [('construction','random/20000/threads=1'),('construction','nodes/20000/threads=1'),('construction','neighbors/20000/threads=1')]
    names += [('construction','neighbors/1000/threads=1'),('deletion','parallel-out/4000'),('deletion','mixed/16000')]
    names += [('traversal','bfs/20000/random/None'),('traversal','dfs/20000/random/None')]
    names += [('python-focus',f'{case}/{n}') for n in (1000,20000,100000) for case in ('filter-3-isolated','bfs-isolated')]
    names += [('networkx',f'networkx/20000/100000/{case}') for case in ('Build graph','BFS (full)','DFS traversal')]
    lookup={(r['suite'],r['name']):r for r in rows}
    picked=[lookup[k] for k in names if k in lookup]
    fig,(ax,threads)=plt.subplots(1,2,figsize=(18,12),gridspec_kw={'width_ratios':[2.1,1]})
    for y,row in enumerate(picked):
        s=selected(row)
        ax.plot([0,s['change']],[y,y],color='#d3d8df',lw=2)
        ax.plot(0,y,'s',color='#356dad',ms=5)
        point(ax,y,row)
    labels=[]
    for row in picked:
        s=selected(row)
        labels.append(f"{short(row['name'])}{' †' if 'repeat' in row else ''}\n{s['before']*1000:.4g} → {s['after']*1000:.4g} ms · {s['status']}")
    ax.set_yticks(range(len(picked)),labels,fontsize=9);ax.invert_yaxis()
    ax.axvline(0,color='#356dad',ls='--',lw=1);ax.set_xscale('symlog',linthresh=20)
    ax.set_title('Before / after: subgraph creation and traversal',loc='left',fontsize=14)
    ax.set_xlabel('Paired runtime change (%)\n← faster   slower →')
    data=json.loads((raw/'threads.json').read_text())
    for y,r in enumerate(data['raw']):
        s=r['summary'];val=s['change'];lo,hi=s['ci95']
        state='improved' if hi<0 else 'regression' if lo>0 else 'inconclusive'
        threads.errorbar(val,y,xerr=[[val-lo],[hi-val]],fmt='o',color=colors[state],mfc='white' if state=='inconclusive' else colors[state],capsize=4,ms=7)
    threads.set_yticks(range(len(data['raw'])),[f"{r['n']:,} nodes / graph\n{r['summary']['before']*1000:.4g} → {r['summary']['after']*1000:.4g} ms" for r in data['raw']],fontsize=10)
    threads.invert_yaxis();threads.axvline(0,color='#333',lw=1)
    threads.set_title('Concurrency control: 1 → 2 threads',loc='left',fontsize=13)
    threads.set_xlabel('Paired runtime change (%)\nFour independent graphs; frozen candidate')
    for axis in (ax,threads):
        axis.spines[['top','right']].set_visible(False);axis.grid(axis='x',alpha=.15)
    fig.suptitle('Ironweaver — measured graph creation, traversal and concurrency (draft)',x=.02,ha='left',fontsize=18)
    fig.text(.02,.025,'Pointwise 95% Student-t CIs on independent paired log ratios. Labels show medians of process summaries.\nGray / hollow = inconclusive or exactly unchanged. † = independent repeat shown; primary and repeat remain in the CSV.\nRed marks retain slowdown signals; see the report. Concurrency excludes node setup/destruction and does not enable Python graph mutation.',fontsize=10)
    fig.tight_layout(rect=(0,.1,1,.94));fig.savefig(out/'focused-comparison.png',dpi=180);plt.close(fig)
    groups=[('construction', ['construction','python-focus','deletion']),('traversal',['traversal']),('general',['networkx']),('libraries',['libraries']),('memory',['memory'])]
    fig,axes=plt.subplots(1,5,figsize=(36,24),gridspec_kw={'width_ratios':[1,1.1,1.25,1.2,.9]})
    for axis,(title,suites) in zip(axes,groups):
        subset=[r for r in rows if r['suite'] in suites]
        for y,row in enumerate(subset): point(axis,y,row,title=='memory')
        labels=[('delete/' if r['suite']=='deletion' else '')+short(r['name'])+(' †' if 'repeat' in r else '') for r in subset]
        axis.set_yticks(range(len(subset)),labels,fontsize=6.7)
        axis.set_ylim(len(subset)-.2,-1);axis.axvline(0,color='#333',lw=.8);axis.grid(axis='x',alpha=.15)
        axis.spines[['top','right']].set_visible(False)
        axis.set_title(dict(construction='Construction + deletion controls',traversal='Every core traversal fixture',general='Every general operation',libraries='Every applicable library workload',memory='Memory + file sizes')[title],loc='left',fontsize=12)
        if title!='memory': axis.set_xscale('symlog',linthresh=20)
        axis.set_xlabel('After − before (MiB)' if title=='memory' else 'Paired runtime change (%)\n← faster   slower →',fontsize=10)
    legend=[Line2D([0],[0],marker='D' if s=='regression' else 'o',color=colors[s],mfc='white' if s=='inconclusive' else colors[s],lw=0,label=s.capitalize()) for s in ('improved','regression','inconclusive')]
    fig.legend(handles=legend,loc='upper right',ncols=3,frameon=False,fontsize=12)
    fig.suptitle(f'Full-suite regression overview — all {len(rows)} applicable revision workloads',x=.012,ha='left',fontsize=22)
    fig.text(.012,.018,'All retained measurements from this session on one machine; matched compiler, lockfile, release flags, bindings, affinity and threads. Builds, tests and benchmark workers ran serially.\nPointwise 95% paired CIs; gray/hollow marks inconclusive or exactly unchanged results. † displays the independent repeat; primary and repeat are preserved separately.\nRuntime axes use symmetric log beyond ±20%. Memory intervals use paired absolute differences. Existing directed-only and exact-betweenness budget exclusions are retained.\nConcurrency thread scaling is an additional focused control, shown in the focused image. See README and summary.csv for sample counts, unresolved signals and limitations.',fontsize=11)
    fig.tight_layout(rect=(0,.075,1,.965),w_pad=2);fig.savefig(out/'suite-regression-overview.png',dpi=170);plt.close(fig)


def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--raw',type=Path,default=Path('work/raw'));p.add_argument('--out',type=Path,required=True)
    p.add_argument('--no-plots',action='store_true')
    a=p.parse_args();a.out.mkdir(parents=True,exist_ok=True)
    rows=extract(a.raw)
    repeats={(r['suite'],r['name']):r for r in extract(a.raw,True)}
    for row in rows:
        if (row['suite'],row['name']) in repeats: row['repeat']=repeats[(row['suite'],row['name'])]
    (a.out/'summary.json').write_text(json.dumps(rows,indent=2)+'\n')
    flat=[]
    for row in rows:
        f={k:v for k,v in row.items() if k!='repeat'}
        if 'repeat' in row: f.update({'repeat_'+k:v for k,v in row['repeat'].items() if k not in ('suite','name','unit')})
        flat.append(f)
    keys=list(dict.fromkeys(k for f in flat for k in f))
    with (a.out/'summary.csv').open('w') as f:
        w=csv.DictWriter(f,fieldnames=keys);w.writeheader();w.writerows(flat)
    suspects=[r for r in rows if r['status']=='regression']
    (a.out/'suspected-regressions.json').write_text(json.dumps(suspects,indent=2)+'\n')
    print(json.dumps(dict(workloads=len(rows),regressions=len(suspects),confirmed=sum(r.get('repeat',{}).get('status')=='regression' for r in suspects))))
    if not a.no_plots: plots(rows,a.raw,a.out)


if __name__=='__main__': main()
