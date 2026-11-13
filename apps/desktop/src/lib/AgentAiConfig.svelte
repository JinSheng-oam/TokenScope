<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { icons } from "./icons";

  export let runtimeAvailable: boolean;
  export let showAiConfig: boolean;
  export let hasAiConfig = false;
  export let error: string;
  export let notice: string;

  type AiConfig = { api_url: string; api_key: string; model: string };
  const aiPresets = [
    { label: 'DeepSeek', url: 'https://api.deepseek.com/v1', defaultModel: 'deepseek-chat' },
    { label: 'OpenAI', url: 'https://api.openai.com/v1', defaultModel: 'gpt-4o-mini' },
    { label: 'SiliconFlow', url: 'https://api.siliconflow.cn/v1', defaultModel: 'deepseek-ai/DeepSeek-V3' },
    { label: '通义千问', url: 'https://dashscope.aliyuncs.com/compatible-mode/v1', defaultModel: 'qwen-plus' },
    { label: 'OpenRouter', url: 'https://openrouter.ai/api/v1', defaultModel: 'deepseek/deepseek-chat' },
    { label: 'Ollama', url: 'http://localhost:11434/v1', defaultModel: 'llama3' },
  ];

  let aiConfig: AiConfig = { api_url: '', api_key: '', model: '' };
  let availableModels: string[] = [];
  let fetchingModels = false;
  let aiSaving = false;

  $: hasAiConfig = Boolean(aiConfig.api_url.trim() && aiConfig.api_key.trim());

  function isPresetActive(preset: typeof aiPresets[0]): boolean {
    if (!aiConfig.api_url) return false;
    const cur = aiConfig.api_url.toLowerCase();
    return cur.includes(preset.url.toLowerCase()) ||
      (preset.label === 'DeepSeek' && cur.includes('deepseek')) ||
      (preset.label === 'OpenAI' && cur.includes('openai')) ||
      (preset.label === 'SiliconFlow' && cur.includes('siliconflow')) ||
      (preset.label === '通义千问' && (cur.includes('aliyuncs') || cur.includes('dashscope'))) ||
      (preset.label === 'OpenRouter' && cur.includes('openrouter')) ||
      (preset.label === 'Ollama' && (cur.includes('11434') || cur.includes('ollama')));
  }

  function normalizeApiUrl(raw: string): string {
    let s = raw.trim();
    if (!s) return '';
    s = s.replace(/\/chat\/completions$/, '').replace(/\/models$/, '').replace(/\/+$/, '');
    const lower = s.toLowerCase();
    if (lower === 'openai' || lower === 'api.openai.com' || lower === 'https://api.openai.com') return 'https://api.openai.com/v1';
    if (lower === 'deepseek' || lower === 'api.deepseek.com' || lower === 'https://api.deepseek.com') return 'https://api.deepseek.com/v1';
    if (lower === 'siliconflow' || lower === 'api.siliconflow.cn' || lower === 'https://api.siliconflow.cn') return 'https://api.siliconflow.cn/v1';
    if (lower === 'dashscope' || lower === 'dashscope.aliyuncs.com' || lower === 'https://dashscope.aliyuncs.com') return 'https://dashscope.aliyuncs.com/compatible-mode/v1';
    if (lower === 'openrouter' || lower === 'openrouter.ai' || lower === 'https://openrouter.ai') return 'https://openrouter.ai/api/v1';
    if (lower === 'groq' || lower === 'api.groq.com' || lower === 'https://api.groq.com') return 'https://api.groq.com/openai/v1';
    if (lower === 'moonshot' || lower === 'api.moonshot.cn' || lower === 'https://api.moonshot.cn') return 'https://api.moonshot.cn/v1';
    if (lower === 'zhipu' || lower === 'open.bigmodel.cn' || lower === 'https://open.bigmodel.cn') return 'https://open.bigmodel.cn/api/paas/v4';
    if (lower === 'ollama' || lower === 'localhost:11434' || lower === 'http://localhost:11434') return 'http://localhost:11434/v1';
    if (lower === 'lmstudio' || lower === 'localhost:1234' || lower === 'http://localhost:1234') return 'http://localhost:1234/v1';

    let withScheme = s;
    if (!s.startsWith('http://') && !s.startsWith('https://')) {
      withScheme = (s.startsWith('localhost') || s.startsWith('127.0.0.1')) ? `http://${s}` : `https://${s}`;
    }
    withScheme = withScheme.replace(/\/+$/, '');
    try {
      const u = new URL(withScheme);
      if (u.pathname === '' || u.pathname === '/') {
        return `${withScheme}/v1`;
      }
    } catch {
      // ignore
    }
    return withScheme;
  }

  async function loadAiConfig() {
    if (!runtimeAvailable) return;
    try {
      const cfg = await invoke<AiConfig>('get_ai_config');
      if (cfg) aiConfig = cfg;
    } catch {
      // ignore
    }
  }

  async function saveAiConfig() {
    aiSaving = true;
    error = '';
    notice = '';
    try {
      aiConfig.api_url = normalizeApiUrl(aiConfig.api_url);
      await invoke('save_ai_config', { config: aiConfig });
      notice = 'AI 助手配置已保存。';
      showAiConfig = false;
    } catch (e) {
      error = String(e);
    } finally {
      aiSaving = false;
    }
  }

  async function fetchModels() {
    aiConfig.api_url = normalizeApiUrl(aiConfig.api_url);
    if (!aiConfig.api_url) {
      error = '请先输入 API 地址';
      return;
    }
    fetchingModels = true;
    error = '';
    notice = '';
    try {
      const list = await invoke<string[]>('fetch_ai_models', {
        apiUrl: aiConfig.api_url,
        apiKey: aiConfig.api_key,
      });
      availableModels = list;
      if (list.length > 0 && !aiConfig.model) {
        aiConfig.model = list[0];
      }
      notice = `已成功拉取 ${list.length} 个模型。`;
    } catch (e) {
      error = String(e);
    } finally {
      fetchingModels = false;
    }
  }

  function applyAiPreset(preset: typeof aiPresets[0]) {
    aiConfig.api_url = preset.url;
    if (!aiConfig.model) {
      aiConfig.model = preset.defaultModel;
    }
  }

  onMount(() => { void loadAiConfig(); });
</script>

  {#if showAiConfig}
    <div class="ai-config-panel" role="region" aria-label="AI 助手配置">
      <div class="ai-config-head">
        <div class="ai-config-title">
          <span class="ai-config-icon">{@html icons.sparkle}</span>
          <strong>AI 助手服务配置</strong>
          {#if hasAiConfig}
            <span class="ai-saved-pill">已配置</span>
          {:else}
            <span class="ai-unsaved-pill">待配置</span>
          {/if}
        </div>
        <button class="btn-compact btn-close" onclick={() => showAiConfig = false} title="收起配置面板">✕ 收起</button>
      </div>

      <div class="ai-presets-bar">
        <span class="presets-label">快捷服务商：</span>
        <div class="preset-chips">
          {#each aiPresets as p}
            {@const active = isPresetActive(p)}
            <button
              type="button"
              class="ai-preset-chip"
              class:selected={active}
              onclick={() => applyAiPreset(p)}
            >
              {p.label}
              {#if active}<span class="preset-check">✓</span>{/if}
            </button>
          {/each}
        </div>
      </div>

      <div class="ai-config-grid">
        <label class="ai-field">
          <span class="field-title">API 接口地址 <small class="text-muted">(自动补全 /v1)</small></span>
          <input
            type="text"
            bind:value={aiConfig.api_url}
            onblur={() => aiConfig.api_url = normalizeApiUrl(aiConfig.api_url)}
            placeholder="例如 https://api.deepseek.com/v1"
          />
        </label>

        <label class="ai-field">
          <span class="field-title">API Key</span>
          <input
            type="password"
            bind:value={aiConfig.api_key}
            placeholder="sk-..."
          />
        </label>

        <label class="ai-field ai-model-field">
          <span class="field-title">模型名称</span>
          <div class="ai-model-group">
            <input
              list="models-datalist"
              bind:value={aiConfig.model}
              placeholder="例如 deepseek-chat 或 gpt-4o-mini"
            />
            <datalist id="models-datalist">
              {#each availableModels as m}
                <option value={m}></option>
              {/each}
            </datalist>
            <button
              type="button"
              class="btn-fetch-models"
              disabled={fetchingModels || !aiConfig.api_url.trim()}
              onclick={() => void fetchModels()}
              title="连接 API 接口并获取当前可用模型列表"
            >
              {#if fetchingModels}
                <span class="spinner-small" aria-hidden="true"></span> 拉取中…
              {:else}
                拉取模型
              {/if}
            </button>
          </div>
        </label>
      </div>

      <div class="ai-config-footer">
        <span class="ai-security-tip">密钥仅保存在本机桌面端受保护的配置库，不经过第三方中间服务</span>
        <div class="ai-footer-buttons">
          <button class="btn-secondary btn-sm" onclick={() => showAiConfig = false}>取消</button>
          <button class="btn-primary btn-sm" disabled={aiSaving} onclick={() => void saveAiConfig()}>
            {aiSaving ? '保存中…' : '保存配置'}
          </button>
        </div>
      </div>
    </div>
  {/if}

<style>
  /* AI Config Drawer Panel */
  .ai-config-panel {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px 18px;
    background: var(--bg-card);
    border: 1px solid var(--border-default);
    border-radius: var(--radius-md);
    box-shadow: 0 4px 16px rgba(0, 0, 0, 0.05);
    animation: fadeIn 0.15s ease-out;
  }

  @keyframes fadeIn {
    from { opacity: 0; transform: translateY(-4px); }
    to { opacity: 1; transform: translateY(0); }
  }

  .ai-config-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    min-height: 30px;
  }

  .ai-config-title {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: var(--text-primary);
  }

  .ai-config-icon {
    display: inline-flex;
    width: 16px;
    height: 16px;
    color: var(--brand-primary);
  }

  .ai-config-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .ai-saved-pill, .ai-unsaved-pill {
    padding: 1px 7px;
    font-size: 10.5px;
    font-weight: 600;
    border-radius: 999px;
  }

  .ai-saved-pill {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border: 1px solid var(--color-exact-border);
  }

  .ai-unsaved-pill {
    background: var(--bg-secondary);
    color: var(--text-muted);
    border: 1px solid var(--border-subtle);
  }

  .btn-close {
    background: transparent;
    border: none;
    color: var(--text-muted);
    cursor: pointer;
    font-size: 11px;
    padding: 2px 6px;
  }

  .btn-close:hover {
    color: var(--text-primary);
  }

  .ai-presets-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    font-size: 11.5px;
  }

  .presets-label {
    color: var(--text-muted);
    flex-shrink: 0;
  }

  .preset-chips {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }

  .ai-preset-chip {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 10px;
    font: inherit;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .ai-preset-chip:hover {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .ai-preset-chip.selected {
    color: var(--brand-strong);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
    font-weight: 600;
  }

  .preset-check {
    font-size: 10px;
  }

  .ai-config-grid {
    display: grid;
    grid-template-columns: minmax(260px, 1.3fr) minmax(180px, 1fr) minmax(240px, 1.2fr);
    gap: 12px;
  }

  .ai-field {
    display: flex;
    flex-direction: column;
    gap: 5px;
  }

  .field-title {
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text-secondary);
  }

  .ai-field input {
    height: 32px;
    padding: 0 10px;
    font: inherit;
    font-size: 12px;
    color: var(--text-primary);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
    transition: all var(--transition-fast);
  }

  .ai-field input:focus {
    border-color: var(--brand-primary);
    box-shadow: 0 0 0 2px var(--brand-subtle);
    background: var(--bg-card);
    outline: none;
  }

  .ai-model-group {
    display: flex;
    gap: 4px;
  }

  .ai-model-group input {
    flex: 1;
    min-width: 0;
  }

  .btn-fetch-models {
    height: 32px;
    padding: 0 10px;
    font: inherit;
    font-size: 11.5px;
    font-weight: 500;
    color: var(--text-primary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    white-space: nowrap;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    transition: all var(--transition-fast);
  }

  .btn-fetch-models:hover:not(:disabled) {
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .btn-fetch-models:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .spinner-small {
    width: 11px;
    height: 11px;
    border: 2px solid var(--border-subtle);
    border-top-color: var(--brand-primary);
    border-radius: 50%;
    animation: spin 0.8s linear infinite;
  }

  @keyframes spin {
    to { transform: rotate(360deg); }
  }

  .ai-config-footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding-top: 8px;
    border-top: 1px solid var(--border-subtle);
    font-size: 11.5px;
  }

  .ai-security-tip {
    color: var(--text-muted);
    font-size: 11px;
  }

  .ai-footer-buttons {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  @media (max-width: 900px) {
    .ai-config-grid {
      grid-template-columns: 1fr;
    }
  }
</style>
