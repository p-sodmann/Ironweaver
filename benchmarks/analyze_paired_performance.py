"""Summarize independent process pairs and export PNGs with paired 95% CIs."""
from __future__ import annotations
import argparse
import csv
import json
import math
from pathlib import Path
import re
import statistics

import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
from matplotlib.lines import Line2D
from scipy.stats import t


def load(raw):
    data = {}
    for path in sorted(raw.glob('final-*-*-*.jsonl')):
        match = re.fullmatch(r'final-(\w+)-(\d+)-(before|after)\.jsonl', path.name)
        if not match or not path.with_suffix('.done').exists():
            continue
        suite, pair, variant = match.groups()
        if suite == 'focused':
            rows = [dict(name=r[0], value=float(r[1]), unit='seconds') for r in csv.reader(path.open())]
        else:
            rows = [json.loads(line) for line in path.read_text().splitlines()]
            labels = {r['name']: r['value'] for r in rows if r['unit'] == 'labels'}
            for row in rows:
                m = re.fullmatch(r'networkx/(\d+)/(\d+)', row['name'])
                if m:
                    n, i = m.groups()
                    row['name'] = f'networkx/{n}/{labels[f"networkx/{n}/labels"][int(i)-1]}'
        for row in rows:
            if not isinstance(row['value'], (int, float)):
                continue
            key = (suite, row['name'], row['unit'])
            data.setdefault(key, {}).setdefault(int(pair), {})[variant] = row['value']
    return data


def interval(values):
    m = statistics.mean(values)
    half = t.ppf(.975, len(values)-1) * statistics.stdev(values) / math.sqrt(len(values)) if len(values)>1 else math.inf
    return m, m-half, m+half


def stats(pairs, unit):
    b = [p['before'] for p in pairs]
    a = [p['after'] for p in pairs]
    dm, dl, dh = interval([x-y for x,y in zip(a,b)])
    if all(x>0 for x in a+b):
        m, lo, hi = interval([math.log(x/y) for x,y in zip(a,b)])
        pct, low, high = [100*math.expm1(v) for v in (m, lo, hi)]
    else:
        pct = low = high = None
    status = 'unchanged' if a==b else 'improved' if dh<0 else 'regression' if dl>0 else 'inconclusive'
    if unit == 'seconds' and low is not None and a != b:
        status = 'improved' if high<0 else 'regression' if low>0 else 'inconclusive'
    return dict(n=len(pairs),before=statistics.mean(b),after=statistics.mean(a),change_percent=pct,ci_low=low,ci_high=high,status=status,absolute_diff=dm,absolute_ci_low=dl,absolute_ci_high=dh)


def summarize(data):
    rows=[]
    for (suite,name,unit), runs in data.items():
        primary=[p for i,p in sorted(runs.items()) if i<6 and len(p)==2]
        repeat=[p for i,p in sorted(runs.items()) if 6<=i<20 and len(p)==2]
        late=[p for i,p in sorted(runs.items()) if 20<=i<23 and len(p)==2]
        late_repeat=[p for i,p in sorted(runs.items()) if 23<=i<26 and len(p)==2]
        late_primary = len(primary)<2 and len(late)>=2
        if late_primary:
            primary, repeat = late, late_repeat
        elif suite=='memory' and not repeat:
            repeat = late
        if len(primary)<2:
            continue
        row=dict(suite=suite,name=name,unit=unit,series='late primary (peak reset enabled)' if late_primary else 'primary',**stats(primary,unit))
        if repeat:
            row.update({'repeat_'+k:v for k,v in stats(repeat,unit).items()})
        rows.append(row)
    return rows


COLORS={'improved':'#16856b','regression':'#c44343','inconclusive':'#838893','unchanged':'#838893'}


def shown(row):
    # Report independent repeat separately; display it for initial regression signals.
    use_repeat=row['status']=='regression' and 'repeat_status' in row
    return ({k.removeprefix('repeat_'):v for k,v in row.items() if k.startswith('repeat_')} if use_repeat else row), use_repeat


def short(name):
    s=name.replace('Some(3)','depth 3').replace('None','full').replace('create_edges','build nodes+edges').replace('create_nodes','build nodes').replace('duplicate_nodes','duplicate errors')
    s=s.replace('Weakly connected components','WCC').replace('Strongly connected components','SCC').replace('Communities (Leiden / Louvain)','Leiden').replace('Dijkstra from one node (weighted)','Dijkstra').replace('Weakly connected components','WCC').replace('Strongly connected components','SCC')
    s=s.replace('/false', '/unreserved').replace('/true', '/reserved')
    return s.replace('networkx/','').replace('libraries/','').replace('memory/','').replace('files/','files ')


def focus_plot(rows,out):
    names=[f'{walk}/20000/{shape}/None' for shape in ['chain','random','hub'] for walk in ['bfs','dfs']]
    names+=['bfs/100000/random/None','dfs/100000/random/None','bfs/100000/chain/Some(3)','dfs/100000/isolated/None','create_nodes/20000/false','create_nodes/20000/true']
    picked=[next(r for r in rows if r['suite']=='focused' and r['name']==name) for name in names]
    fig,ax=plt.subplots(figsize=(12,7.7))
    labels=[]
    for y,row in enumerate(picked):
        s,repeated=shown(row)
        lo,hi=s['ci_low']+100,s['ci_high']+100
        after=s['change_percent']+100
        ax.plot([100,after],[y,y],color='#cbd0d6',lw=2)
        ax.plot(100,y,'s',color='#4679bd',ms=6)
        ax.errorbar(after,y,xerr=[[after-lo],[hi-after]],fmt='o',color=COLORS[s['status']],capsize=4,ms=7)
        state=s['status']+(' †' if repeated else '')
        labels.append(f"{short(row['name'])}\n{s['before']*1000:.4g} → {s['after']*1000:.4g} ms · {state}")
    ax.set_yticks(range(len(labels)),labels,fontsize=9)
    ax.invert_yaxis(); ax.axvline(100,color='#4679bd',ls='--',alpha=.6)
    ax.set_xlabel('Paired geometric runtime ratio (%; before = 100%; lower is faster)')
    ax.set_title('Focused before/after: graph traversal and construction controls',loc='left',fontsize=15,pad=18)
    ax.grid(axis='x',alpha=.17)
    ax.spines[['top','right']].set_visible(False)
    fig.text(.02,.02,'Labels: arithmetic mean times. Points/CIs: paired geometric ratios (6 independent pairs).\nGray = inconclusive/unchanged. † = independent repeat shown for an initial regression signal.',fontsize=9)
    fig.tight_layout(rect=(0,.075,1,1)); fig.savefig(out/'focused-comparison.png',dpi=200); plt.close(fig)


def overview(rows,out):
    fig,axes=plt.subplots(1,4,figsize=(29,18),gridspec_kw={'width_ratios':[1.1,1.15,1.05,.9]})
    suites=['focused','networkx','libraries','memory']
    for ax,suite in zip(axes,suites):
        selected=[r for r in rows if r['suite']==suite]
        for y,row in enumerate(selected):
            s,repeated=shown(row)
            if suite=='memory':
                m,lo,hi=[s[k]/2**20 for k in ['absolute_diff','absolute_ci_low','absolute_ci_high']]
            else:
                m,lo,hi=[s[k] for k in ['change_percent','ci_low','ci_high']]
            ax.errorbar(m,y,xerr=[[m-lo],[hi-m]],fmt='o' if s['status']=='improved' else 'D' if s['status']=='regression' else 'o',mfc=COLORS[s['status']] if s['status'] in ['improved','regression'] else 'white',color=COLORS[s['status']],ms=3,capsize=2,lw=.8)
        labels=[short(r['name'])+(' †' if shown(r)[1] else '') for r in selected]
        ax.set_yticks(range(len(selected)),labels,fontsize=6.8)
        ax.set_ylim(len(selected)-.2,-1);ax.axvline(0,color='#333',lw=.7);ax.grid(axis='x',alpha=.13)
        ax.spines[['top','right']].set_visible(False)
        ax.set_title({'focused':'Core focus + controls','networkx':'General operations (both sizes)','libraries':'Analytics (all default datasets)','memory':'Memory + file size (both sizes)'}[suite],fontsize=11,loc='left')
        if suite != 'memory':
            ax.set_xscale('symlog', linthresh=20)
        ax.set_xlabel('After − before (MiB)' if suite=='memory' else 'Paired runtime change (%)\n← faster    slower →',fontsize=9)
    legend=[Line2D([0],[0],marker='o',color=COLORS[s],mfc='white' if s in ['inconclusive','unchanged'] else COLORS[s],lw=0,label=s.capitalize()) for s in ['improved','regression','inconclusive']]
    fig.legend(handles=legend,loc='upper right',ncols=3,frameon=False,fontsize=11)
    fig.suptitle('Full-suite regression overview — every applicable Ironweaver workload',x=.015,ha='left',fontsize=19)
    fig.text(.015,.012,'Pointwise 95% paired CIs; 6 focused pairs and 3 full-suite pairs. Regression signals get independent repeat pairs; n is recorded in summary.csv.\n† shows the repeat estimate (primary + repeat data remain in summary.csv). Empty gray markers explicitly mean inconclusive/unchanged.\nExact betweenness on GitHub/Kronecker is skipped by the existing benchmark’s nodes×edges budget; Facebook is measured. Construction timings include destruction. Runtime axes use symmetric log beyond ±20%.',fontsize=11)
    fig.tight_layout(rect=(0,.05,1,.97),w_pad=2);fig.savefig(out/'suite-regression-overview.png',dpi=170);plt.close(fig)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--raw',type=Path,default=Path('work/raw'));p.add_argument('--out',type=Path,default=Path('work/report'));p.add_argument('--no-plots',action='store_true');args=p.parse_args()
    args.out.mkdir(parents=True,exist_ok=True)
    rows=summarize(load(args.raw))
    keys=list(dict.fromkeys(k for row in rows for k in row))
    with (args.out/'summary.csv').open('w') as f:
        writer=csv.DictWriter(f,fieldnames=keys);writer.writeheader();writer.writerows(rows)
    (args.out/'summary.json').write_text(json.dumps(rows,indent=2))
    suspects=[r for r in rows if r['status']=='regression']
    (args.out/'suspected-regressions.json').write_text(json.dumps(suspects,indent=2))
    print(json.dumps({'workloads':len(rows),'primary_regressions':len(suspects),'suites_to_repeat':sorted({r['suite'] for r in suspects})}))
    if not args.no_plots:
        focus_plot(rows,args.out);overview(rows,args.out)
