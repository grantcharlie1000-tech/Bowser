"""Read retained native production samples. Never fills absent/invalid measurements with zero."""
from __future__ import annotations
import argparse, collections, datetime, json, math, random, statistics
from pathlib import Path

def dist(values):
    a=sorted(x for x in values if x is not None and math.isfinite(x))
    return dict(count=len(a),median=statistics.median(a) if a else None,p95=a[max(0,math.ceil(len(a)*.95)-1)] if a else None,min=a[0] if a else None,max=a[-1] if a else None)

def cpu_delta(rows):
    if len(rows)<2 or any(row.get('CoverageErrors') for row in rows): return None
    last=dict(rows[0].get('CpuByPid',{})); total=0
    for row in rows[1:]:
        for ident,value in row.get('CpuByPid',{}).items():
            if value is None: continue
            value=float(value); old=last.get(ident)
            total+=max(0,value-float(old)) if old is not None else max(0,value)
            last[ident]=value
    return total

def native_private(row):
    return row.get('NativePrivateBytes') if row.get('PrivateSource')=='native' and not row.get('CoverageErrors') else None

def metrics(row,freq):
    def ms(a,b): return (float(b)-float(a))*1000/freq if a is not None and b is not None and b>=a else None
    out={'launch_to_fixture_ack_ms':ms(row.get('launch_qpc'),row.get('ready_qpc')),'launch_to_title_proxy_ms':ms(row.get('launch_qpc'),row.get('title_ready_qpc')),'launch_to_window_handle_proxy_ms':ms(row.get('launch_qpc'),row.get('input_ready_qpc')),'shutdown_ms':row.get('shutdown_ms'),'tab_switch_title_proxy_ms':row.get('tab_switch_proxy_ms'),'profile_disk_growth_bytes':row.get('disk_after',0)-row.get('disk_before',0)}
    inp=row.get('input') or {}
    out['ui_enter_to_fixture_ack_with_focus_overhead_ms']=ms(inp.get('start_qpc'),inp.get('end_qpc')) if inp.get('ready') else None
    events=row.get('events') or []
    for name in ('html_toolbar_ready','html_chrome_shown','chrome_painted'):
        hits=[int(e['qpc_ticks']) for e in events if e['event']==name]
        out['launch_to_'+name+'_ms']=ms(row.get('launch_qpc'),hits[0]) if hits else None
    snapshots=row.get('snapshots') or []
    private=[native_private(s) for s in snapshots]; private=[x for x in private if x is not None]
    out['recorded_window_sampled_peak_private_bytes']=max(private) if private and len(private)==len(snapshots) else None
    counts=[s.get('ProcessCount') for s in snapshots if s.get('ProcessCount') is not None]
    out['recorded_window_sampled_peak_process_count']=max(counts) if counts else None
    # Warm-profile helper lifetime CPU is not the current workload's CPU budget.
    # Keep the raw number unchanged in its retained sample, but exclude the metric from analysis.
    out['sampled_tree_cpu_core_seconds']=None if (row.get('condition')=='reused' and not row.get('tree_exit_confirmed')) or any(s.get('CoverageErrors') for s in snapshots) else (row.get('cpu') or {}).get('CoreSeconds')
    for phase in ('idle','minimized'):
        phase_rows=row.get(phase) or []
        pv=[native_private(s) for s in phase_rows]; pv=[x for x in pv if x is not None]
        out[phase+'_median_private_bytes']=statistics.median(pv) if pv and len(pv)==len(phase_rows) else None
        out[phase+'_cpu_core_seconds']=cpu_delta(phase_rows)
        out[phase+'_observed_endpoint_seconds']=None
        if len(phase_rows)>1:
            first=phase_rows[0].get('SampledAtUtc'); last=phase_rows[-1].get('SampledAtUtc')
            if first and last:
                seconds=(datetime.datetime.fromisoformat(last.replace('Z','+00:00'))-datetime.datetime.fromisoformat(first.replace('Z','+00:00'))).total_seconds()
                out[phase+'_observed_endpoint_seconds']=seconds if seconds>=0 else None
    return out

def paired(samples,arm,reference,metric):
    a={x['pair']:x['metrics'].get(metric) for x in samples if x['arm']==arm}
    b={x['pair']:x['metrics'].get(metric) for x in samples if x['arm']==reference}
    ratios=[]; deltas=[]; missing=[]
    for pair in sorted(a.keys()|b.keys()):
        av=a.get(pair);bv=b.get(pair)
        if av is None or bv is None: missing.append(pair); continue
        deltas.append(av-bv)
        if bv>0: ratios.append(av/bv)
    rng=random.Random(20261018)
    boot=sorted(statistics.median(rng.choices(ratios,k=len(ratios))) for _ in range(2000)) if ratios else []
    return {'arm':arm,'reference':reference,'metric':metric,'valid_pair_count':len(deltas),'missing_pairs':missing,'paired_absolute_delta':dist(deltas),'paired_ratio':dist(ratios),'ratio_median_bootstrap95':[boot[49],boot[1949]] if boot else None,'ratio_numerically_at_or_below_0_80_not_acceptance':statistics.median(ratios)<=.8 if ratios else None,'minimum_primary_pair_count_met':len(deltas)>=10,'note':'Bootstrap intervals are descriptive and do not remove hosted-runner bias; success requires named-metric noise and correctness review, not this Boolean alone.'}

def main():
    parser=argparse.ArgumentParser(); parser.add_argument('directory',type=Path); parser.add_argument('--out',type=Path);args=parser.parse_args()
    env=json.loads((args.directory/'environment.json').read_text(encoding='utf-8-sig'));freq=float(env['qpc_frequency']); samples=[];invalid=[]; negatives=[]
    for path in sorted(args.directory.glob('*.json')):
        if path.name in ('environment.json','summary.json'): continue
        r=json.loads(path.read_text(encoding='utf-8-sig'))
        if not isinstance(r,dict) or 'id' not in r: continue
        if not r.get('valid'): invalid.append({'id':r['id'],'error':r.get('error')});continue
        if r.get('expected_failure'): negatives.append({'id':r['id'],'unexpected_ready':r.get('ready_qpc') is not None,'diagnostic':r.get('diagnostic_readiness')});continue
        r['metrics']=metrics(r,freq);samples.append(r)
    groups=collections.defaultdict(list)
    for r in samples: groups[(r['fixture'],r['condition'])].append(r)
    summaries=[]; comparisons=[]
    for (fixture,condition),group in sorted(groups.items()):
        names=sorted({name for r in group for name in r['metrics']})
        for arm in sorted({r['arm'] for r in group}):
            members=[r for r in group if r['arm']==arm]
            summaries.append({'fixture':fixture,'condition':condition,'arm':arm,'valid_samples':len(members),'metrics':{name:dist([r['metrics'].get(name) for r in members]) for name in names},'native_counter_coverage_errors':sum(len(s.get('CoverageErrors') or []) for r in members for s in r.get('snapshots') or [])})
        for arm,ref in [('diagnostic_off','baseline'),('diagnostic_on','diagnostic_off'),('candidate_on','baseline_on')]:
            for name in names:
                c=paired(group,arm,ref,name)
                if c['valid_pair_count']:comparisons.append(dict(fixture=fixture,condition=condition,**c))
    metric_exclusions=[{'id':r['id'],'metric':'sampled_tree_cpu_core_seconds','reason':'reused cumulative CPU not accepted: surviving helper lifetime CPU can predate current root; raw value retained'} for r in samples if r.get('condition')=='reused' and not r.get('tree_exit_confirmed')]
    report={'schema_version':2,'measurement_mode':env.get('measurement_mode','legacy-pilot'),'metric_exclusions':metric_exclusions,'scope':'Matched native production measurements; descriptive statistics, not automatic gain acceptance or the full audit matrix.','invalid_samples':invalid,'negative_fixtures':negatives,'summary':summaries,'paired_comparisons':comparisons,'limitations':env.get('noise',[])+['Resource peaks cover only retained sampling windows; no peak is inferred for gaps.','chrome_painted is the recorded native paint event, not measured display presentation.','Endpoint-sampled CPU excludes unobserved short-lived processes and post-last-observation work.','Primary claims require ten valid matched pairs per condition, successful HTML/native module activation and first-use regression review; tails from small samples are exploratory.']}
    out=args.out or args.directory/'analysis.json';out.write_text(json.dumps(report,indent=2)+'\n'); print(json.dumps({'output':str(out),'valid_positive_samples':len(samples),'invalid_samples':len(invalid),'negative_samples':len(negatives),'comparisons':len(comparisons)}))
if __name__=='__main__': main()
