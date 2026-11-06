import { describe, expect, it } from 'vitest';
import {
  formatCountdown,
  formatQuotaPoolName,
  quotaWindowLabel,
  resolveScopedBreakdown,
  resolveScopedTotals,
  resolveScopedDaily,
  resolveSessionActivityState,
  buildDashboardProjection,
} from './dashboardView';
import type { DashboardSnapshot, UsageTotals } from './types';

describe('dashboardView quota utilities', () => {
  it('formats pool names into readable display strings', () => {
    expect(formatQuotaPoolName('claude_gpt')).toBe('Claude / GPT');
    expect(formatQuotaPoolName('gemini_4_argon')).toBe('Gemini 4 Argon');
    expect(formatQuotaPoolName('gemini')).toBe('Gemini');
    expect(formatQuotaPoolName('codex')).toBe('Codex');
  });

  it('formats reset countdown with days when >= 24h', () => {
    const baseNow = 1_000_000_000;
    // 166 hours, 1 minute, 55 seconds = (166 * 3600 + 1 * 60 + 55) * 1000 ms
    const diffMs = (166 * 3600 + 1 * 60 + 55) * 1000;
    expect(formatCountdown(baseNow + diffMs, baseNow)).toBe('6天 22小时 后重置');
  });

  it('formats reset countdown with standard timer when < 24h', () => {
    const baseNow = 1_000_000_000;
    // 3 hours, 1 minute, 55 seconds
    const diffMs = (3 * 3600 + 1 * 60 + 55) * 1000;
    expect(formatCountdown(baseNow + diffMs, baseNow)).toBe('03:01:55 后重置');
  });

  it('handles expired and null countdowns gracefully', () => {
    expect(formatCountdown(null)).toBe('未提供重置时间');
    expect(formatCountdown(100, 200)).toBe('即将重置');
  });

  it('labels standard quota window durations', () => {
    expect(quotaWindowLabel(300)).toBe('5 小时周期');
    expect(quotaWindowLabel(10_080)).toBe('每周周期');
  });

  it('keeps actual device components, reasoning, pricing and precision', () => {
    const snapshot = devices();
    expect(resolveScopedTotals(snapshot,'all')).toMatchObject({total_tokens:1050,reasoning_tokens:9,exact_records:1,estimated_records:1,aggregate_records:1});
    expect(resolveScopedTotals(snapshot,'A')).toMatchObject({total_tokens:100,input_tokens:100,output_tokens:0,exact_records:0,estimated_records:1});
    expect(resolveScopedTotals(snapshot,'B')).toMatchObject({total_tokens:900,input_tokens:0,output_tokens:900,reasoning_tokens:9,aggregate_records:1,priced_records:0});
    expect(resolveScopedTotals(snapshot,'missing')?.total_tokens).toBe(0);
    expect(resolveScopedBreakdown([],snapshot.remote_by_tool,'all')[0]).toMatchObject({total_tokens:1000,exact_records:0,aggregate_records:1,estimated_records:1});
  });

  it('uses the same device scope for cards, curves and heatmaps, with empty days', () => {
    const snapshot = devices();
    for (const [scope,total] of [['local',50],['all',1050],['A',100],['B',900]] as const) {
      const view = buildDashboardProjection({snapshot,heatmapSnapshot:snapshot,deviceScope:scope,todayIso:'2026-11-05',hoveredCurveIndex:null,remoteSelectedDevice:'all',modelSortBy:'tokens',range:'all',customStart:'',customEnd:''});
      expect(view.local?.total_tokens).toBe(total);
      expect(view.chartValues.reduce((sum,d)=>sum+d.total_tokens,0)).toBe(total);
      expect(resolveScopedDaily(snapshot,scope)[1].total_tokens).toBe(0);
      expect(view.heatmapCalendar).toEqual(buildDashboardProjection({snapshot,heatmapSnapshot:null,deviceScope:scope,todayIso:'2026-11-05',hoveredCurveIndex:null,remoteSelectedDevice:'all',modelSortBy:'tokens',range:'all',customStart:'',customEnd:''}).heatmapCalendar);
    }
  });

  it('keeps a running turn during quiet output and clears it after completion', () => {
    const now=2_000_000_000_000;
    const session={tool_id:'codex',session_id:'same',display_name:'Demo',last_event_at_ms:now-300_000,activity_observed_at_ms:null};
    expect(resolveSessionActivityState(session,[{tool_id:'codex',session_id:'same',display_name:'Demo',model_id:null,state:'generating',observed_at_ms:now-120_000}],now)).toBe('generating');
    expect(resolveSessionActivityState(session,[],now)).toBeNull();
  });
});

function totals(patch: Partial<UsageTotals>): UsageTotals {
  return {total_tokens:0,input_tokens:0,cache_read_tokens:0,cache_write_tokens:0,output_tokens:0,unclassified_tokens:0,reasoning_tokens:0,reasoning_known_records:0,record_count:1,estimated_cost_microusd:null,priced_records:0,unpriced_records:1,exact_records:0,aggregate_records:0,estimated_records:0,unknown_precision_records:0,...patch};
}
function devices(): DashboardSnapshot {
  const a=totals({total_tokens:100,input_tokens:100,estimated_records:1});
  const b=totals({total_tokens:900,output_tokens:900,reasoning_tokens:9,reasoning_known_records:1,aggregate_records:1});
  return {
    local:totals({total_tokens:50,input_tokens:50,exact_records:1}),
    remote:totals({total_tokens:1000,input_tokens:100,output_tokens:900,reasoning_tokens:9,reasoning_known_records:1,record_count:2,unpriced_records:2,estimated_records:1,aggregate_records:1}),
    remote_by_device:[{owner_device_id:'A',total_tokens:100,record_count:1,estimated_cost_microusd:null,usage:a},{owner_device_id:'B',total_tokens:900,record_count:1,estimated_cost_microusd:null,usage:b}],
    by_tool:[],by_model:[],by_project:[],remote_by_model:[],remote_by_project:[],
    remote_by_tool:[a,b].map((usage,i)=>({owner_device_id:i===0?'A':'B',key:'codex',label:'Codex',total_tokens:usage.total_tokens,record_count:1,estimated_cost_microusd:null,exact_records:usage.exact_records,aggregate_records:usage.aggregate_records,estimated_records:usage.estimated_records,unknown_precision_records:0,untimed_records:0})),
    daily:[{date:'2026-11-04',total_tokens:50,input_tokens:50,output_tokens:0},{date:'2026-11-05',total_tokens:0,input_tokens:0,output_tokens:0}],
    remote_daily:[{owner_device_id:'A',date:'2026-11-04',total_tokens:100,input_tokens:100,output_tokens:0},{owner_device_id:'B',date:'2026-11-04',total_tokens:900,input_tokens:0,output_tokens:900}],
  } as unknown as DashboardSnapshot;
}

it('does not transfer live state to another session with the same title', () => {
  const now=2_000_000_000_000;
  const activity=[{tool_id:'antigravity',session_id:'active',display_name:'Demo',model_id:null,state:'generating' as const,observed_at_ms:now}];
  const session={tool_id:'antigravity',session_id:'different',display_name:'Demo',last_event_at_ms:now-600_000,activity_observed_at_ms:null};
  expect(resolveSessionActivityState(session,activity,now)).toBeNull();
  expect(resolveSessionActivityState({...session,session_id:'active'},activity,now)).toBe('generating');
});
