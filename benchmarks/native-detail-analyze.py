"""Descriptive native attribution only. No optimization acceptance or inferred URL ownership."""
import argparse, collections, json, statistics
from pathlib import Path

def summarize(rows):
    spans=collections.defaultdict(list); pending=collections.defaultdict(list)
    snapshots=collections.defaultdict(lambda: {'requested':False,'complete':False,'failed':False,'inventory':{},'views':collections.defaultdict(set)})
    lifecycle=[]; counts=[]
    for row in rows:
        event=row['event']; tab=row.get('tab_id') or ''; view=row.get('view') or ''
        if event.startswith(('configure_', 'blocker_')) and event.endswith('_started'):
            pending[(event[:-8],tab)].append(float(row['elapsed_ms']))
        elif event.startswith(('configure_', 'blocker_')) and event.endswith('_finished'):
            key=(event[:-9],tab)
            if pending[key]: spans[event[:-9]].append(float(row['elapsed_ms'])-pending[key].pop())
            if event=='configure_filter_registration_finished':counts.append({'tab_id':tab,'successful_registrations':int(row['count'])})
        if event in ('blocker_payload_finished','blocker_pattern_count','blocker_pattern_preparation_finished'):
            counts.append({'event':event,'tab_id':tab,'count':int(row['count'])})
        if event.startswith('chrome_host_') or event in ('chrome_controller_result','chrome_configure_started','chrome_configure_finished'):
            lifecycle.append({k:row.get(k) for k in ('event','elapsed_ms','view','count')})
        if event.startswith('chrome_snapshot_') or event.startswith('chrome_renderer_') or event.startswith('renderer_private_inventory'):
            snapshot=snapshots[row.get('count') or 'missing']
            if event=='chrome_snapshot_requested':snapshot['requested']=True
            if event=='chrome_snapshot_completed':snapshot['complete']=True
            if event in ('chrome_snapshot_failed','chrome_snapshot_unsupported') or 'unavailable' in event:snapshot['failed']=True
            if event=='renderer_private_inventory':snapshot['inventory'][int(row['pid'])]=int(row['private_bytes'])
            if event=='chrome_renderer_private':snapshot['views'][view].add(int(row['pid']))
    maps=[]
    for ident,snapshot in snapshots.items():
        known=set().union(*snapshot['views'].values()) if snapshot['views'] else set()
        missing=sorted(known-set(snapshot['inventory']))
        eligible=bool(snapshot['requested'] and snapshot['complete'] and not snapshot['failed'] and snapshot['inventory'] and known and not missing)
        maps.append({'snapshot_id':ident,'eligible':eligible,'requested':snapshot['requested'],'completed':snapshot['complete'],'failed':snapshot['failed'],'mapped_pids_missing_inventory':missing,'renderer_private_by_pid':snapshot['inventory'],'fixed_view_pids':{v:sorted(p) for v,p in snapshot['views'].items()},'deduplicated_fixed_chrome_private_bytes':sum(snapshot['inventory'][pid] for pid in known) if eligible else None,'all_renderer_private_bytes':sum(snapshot['inventory'].values()) if eligible else None,'note':'One PID can host multiple views. Do not sum per-view bytes; unrecognized frame sources are not logged or assumed to be content. Invalid snapshot totals are null; observed per-PID counters remain retained.'})
    return {'configure_spans_ms':{key:{'count':len(vals),'median':statistics.median(vals),'min':min(vals),'max':max(vals)} for key,vals in spans.items()},'successful_filter_registrations':counts,'lifecycle':lifecycle,'renderer_snapshots':maps,'unmatched_started_spans':sum(map(len,pending.values()))}

def main():
    p=argparse.ArgumentParser();p.add_argument('directory',type=Path);a=p.parse_args()
    rows=[json.loads(p.read_text(encoding='utf-8-sig')) for p in sorted(a.directory.glob('candidate_detail_on-*.json'))]
    rows=[row for row in rows if isinstance(row,dict) and 'id' in row]
    result={'scope':'Short diagnostic attribution pilot, not accepted speed/RAM gains. Detail-on/off overhead is measured separately; no subtraction. Native private commit, not working set or PSS. Snapshots are asynchronous and are not settled resource endpoints.','samples':[{'id':r['id'],'valid':r['valid'],'expected_failure':r['expected_failure'],**summarize(r.get('events') or [])} for r in rows if r['arm']=='candidate_detail_on']}
    (a.directory/'native-detail.json').write_text(json.dumps(result,indent=2)+'\n')
if __name__=='__main__':main()
