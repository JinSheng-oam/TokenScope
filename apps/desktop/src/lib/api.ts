import { invoke } from '@tauri-apps/api/core';
import type { AgentStatus, DashboardRange, DashboardResponse, DashboardSnapshot, PairHubResult, RemoteDeviceList, SessionAnalysisPage, SessionAnalysisResponse } from './types';

export function isTauriRuntime(): boolean {
  return typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;
}

export async function startAgent(timeZone: string): Promise<AgentStatus> {
  return invoke<AgentStatus>('start_agent', { timeZone });
}

export async function getAgentStatus(): Promise<AgentStatus> {
  return invoke<AgentStatus>('get_agent_status');
}

export async function getDashboardSnapshot(
  range: DashboardRange,
  sessionOffset = 0,
  startDate?: string,
  endDate?: string
): Promise<DashboardSnapshot> {
  const response = await invoke<DashboardResponse>('get_dashboard_snapshot', {
    range,
    sessionOffset,
    startDate: startDate ?? null,
    endDate: endDate ?? null,
  });
  if (response.protocol_version !== 2) throw new Error('Agent IPC 协议版本不兼容，请更新桌面端和 Agent');
  return response.snapshot;
}

export async function getSessionAnalysis(
  range: DashboardRange,
  sessionOffset: number,
  filters: { search: string; toolId: string; modelId: string; projectId: string },
  startDate?: string,
  endDate?: string
): Promise<SessionAnalysisPage> {
  const response = await invoke<SessionAnalysisResponse>('get_session_analysis', {
    range,
    sessionOffset,
    search: filters.search.trim() || null,
    toolId: filters.toolId || null,
    modelId: filters.modelId || null,
    projectId: filters.projectId || null,
    startDate: startDate ?? null,
    endDate: endDate ?? null,
  });
  if (response.protocol_version !== 1) throw new Error('会话查询协议版本不兼容，请更新桌面端和 Agent');
  return response.analysis;
}

export async function deleteSession(toolId: string, sessionId: string): Promise<number> {
  const response = await invoke<{ deletedRecords: number }>('delete_session', { toolId, sessionId });
  return response.deletedRecords;
}

export async function getAutostartEnabled(): Promise<boolean> {
  return invoke<boolean>('get_autostart_enabled');
}

export async function setAutostartEnabled(enabled: boolean): Promise<boolean> {
  return invoke<boolean>('set_autostart_enabled', { enabled });
}


export async function pairHubDevice(hubUrl: string, pairingCode: string, displayName: string): Promise<PairHubResult> {
  return invoke<PairHubResult>('pair_hub_device', { hubUrl, pairingCode, displayName });
}

export async function listHubDevices(hubUrl: string): Promise<RemoteDeviceList> {
  const response = await invoke<RemoteDeviceList>('list_hub_devices', { hubUrl });
  if (response.protocol_version !== 1) throw new Error('Hub API 协议版本不兼容');
  return response;
}

export async function renameHubDevice(hubUrl: string, displayName: string): Promise<string> {
  return invoke<string>('rename_hub_device', { hubUrl, displayName });
}

export async function rotateHubDeviceToken(hubUrl: string): Promise<number> {
  return invoke<number>('rotate_hub_device_token', { hubUrl });
}

export async function disconnectHubDevice(hubUrl: string): Promise<boolean> {
  return invoke<boolean>('disconnect_hub_device', { hubUrl });
}
