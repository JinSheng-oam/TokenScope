<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';
  import { icons } from './icons';

  export let runtimeAvailable: boolean;
  export let dirty = false;
  type Provider = 'codex' | 'claude' | 'antigravity' | 'deepseek';
  type Id = { provider: Provider; root: string; name: string };
  type Entry = {
    id: Id;
    path: string;
    kind: 'skill' | 'instruction';
    exists: boolean;
    editable: boolean;
    linked: boolean;
    enabled: boolean;
  };
  type Source = { url: string; branch: string | null; subpath: string | null; skill: string | null };
  type Inspection = { source: Source; revision: string; candidates: { path: string; name: string; preview: string }[] };
  type UpdateInfo = { kind: string; source: string | null; status: string; detail: string };
  type AiConfig = { api_url: string; api_key: string; model: string };
  type SkillSummary = {
    skill_name: string;
    overview: string;
    usage_guide: string;
    raw_response: string;
    model: string;
    updated_at_ms: number;
  };

  const providers: { id: Provider; label: string; roots: { id: string; label: string }[] }[] = [
    { id: 'codex', label: 'Codex', roots: [{ id: 'shared-skills', label: '共享 skills' }, { id: 'codex-skills', label: 'Codex skills' }] },
    { id: 'claude', label: 'Claude Code', roots: [{ id: 'claude-skills', label: 'Claude skills' }] },
    { id: 'antigravity', label: 'Antigravity', roots: [{ id: 'antigravity-skills', label: 'Antigravity skills' }] },
    { id: 'deepseek', label: 'DeepSeek Harness', roots: [{ id: 'dsh-skills', label: 'DeepSeek skills' }, { id: 'dsh-shared-skills', label: '共享 skills' }] },
  ];

  const aiPresets = [
    { label: 'DeepSeek', url: 'https://api.deepseek.com/v1', defaultModel: 'deepseek-chat' },
    { label: 'OpenAI', url: 'https://api.openai.com/v1', defaultModel: 'gpt-4o-mini' },
    { label: 'SiliconFlow', url: 'https://api.siliconflow.cn/v1', defaultModel: 'deepseek-ai/DeepSeek-V3' },
    { label: '通义千问', url: 'https://dashscope.aliyuncs.com/compatible-mode/v1', defaultModel: 'qwen-plus' },
    { label: 'OpenRouter', url: 'https://openrouter.ai/api/v1', defaultModel: 'deepseek/deepseek-chat' },
    { label: 'Ollama', url: 'http://localhost:11434/v1', defaultModel: 'llama3' },
  ];

  let provider: Provider = 'codex';
  let entries: Entry[] = [];
  let selected: Entry | null = null;
  let content = '';
  let original: string | null = null;
  let busy = false;
  let error = '';
  let notice = '';
  let creating = false;
  let creatingInstruction = false;
  let importing = false;
  let sourceInput = '';
  let inspection: Inspection | null = null;
  let candidatePath = '';
  let updateInfo: UpdateInfo | null = null;
  let skillName = '';
  let skillRoot = providers[0].roots[0].id;
  let instructionName = '';
  let instructionRoot = '';
  let request = 0;

  // AI assistant state
  let showAiConfig = false;
  let aiConfig: AiConfig = { api_url: '', api_key: '', model: '' };
  let availableModels: string[] = [];
  let fetchingModels = false;
  let aiSaving = false;
  let skillSummary: SkillSummary | null = null;
  let analyzingSkill = false;
  let showSummaryCard = true;

  // Adopt state
  let adopting = false;
  let adoptSourceInput = '';
  let adoptingBusy = false;

    let filterQuery = '';
  let copiedPath = false;

  $: filteredInstructions = instructions.filter(
    (entry) => !filterQuery.trim() || entry.id.name.toLowerCase().includes(filterQuery.toLowerCase().trim())
  );
  $: filteredSkills = skills.filter(
    (entry) => !filterQuery.trim() || entry.id.name.toLowerCase().includes(filterQuery.toLowerCase().trim())
  );
  $: hasAiConfig = Boolean(aiConfig.api_url.trim() && aiConfig.api_key.trim());
  $: editorLines = content ? content.split('\n').length : 0;
  $: editorChars = content ? content.length : 0;

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

  async function copyPath(path: string) {
    try {
      await navigator.clipboard.writeText(path);
      copiedPath = true;
      setTimeout(() => (copiedPath = false), 2000);
    } catch {
      // fallback
    }
  }

  $: dirty = selected !== null && content !== (original ?? '');
  $: instructions = entries.filter((entry) => entry.kind === 'instruction');
  $: skills = entries.filter((entry) => entry.kind === 'skill');
  $: currentProvider = providers.find((p) => p.id === provider)!;

  function canLeave(): boolean {
    return !dirty || window.confirm('当前文件有未保存的更改，放弃更改？');
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

  async function loadSkillSummary(entry: Entry) {
    if (entry.kind !== 'skill' || !entry.exists) {
      skillSummary = null;
      return;
    }
    try {
      skillSummary = await invoke<SkillSummary | null>('get_skill_summary', { id: entry.id });
    } catch {
      skillSummary = null;
    }
  }

  async function generateSkillSummary() {
    if (!selected || selected.kind !== 'skill' || !selected.exists) return;
    analyzingSkill = true;
    error = '';
    notice = '';
    try {
      skillSummary = await invoke<SkillSummary>('generate_skill_summary', { id: selected.id });
      notice = `AI 分析完成 (${skillSummary.model})。`;
      showSummaryCard = true;
    } catch (e) {
      error = String(e);
    } finally {
      analyzingSkill = false;
    }
  }

  async function refresh() {
    if (!runtimeAvailable) return;
    const current = ++request;
    busy = true; error = '';
    try {
      const result = await invoke<Entry[]>('list_customizations', { provider });
      if (current === request) entries = result;
    } catch (e) { if (current === request) error = String(e); }
    finally { if (current === request) busy = false; }
  }

  async function switchProvider(next: Provider) {
    if (provider === next || !canLeave()) return;
    provider = next;
    selected = null;
    content = '';
    original = null;
    creating = false;
    creatingInstruction = false;
    importing = false;
    adopting = false;
    updateInfo = null;
    skillSummary = null;
    notice = '';
    skillRoot = providers.find((p) => p.id === next)!.roots[0].id;
    await refresh();
  }

  async function select(entry: Entry) {
    if (!canLeave() || !entry.editable) return;
    const current = ++request;
    busy = true; error = ''; notice = '';
    try {
      const value = await invoke<string | null>('read_customization', { id: entry.id });
      if (current !== request) return;
      selected = entry;
      original = value;
      content = value ?? '';
      creating = false;
      creatingInstruction = false;
      importing = false;
      adopting = false;
      updateInfo = null;
      void loadSkillSummary(entry);
    } catch (e) { if (current === request) error = String(e); }
    finally { if (current === request) busy = false; }
  }

  async function save() {
    if (!selected || busy) return;
    busy = true; error = ''; notice = '';
    try {
      await invoke('save_customization', { id: selected.id, expected: original, content });
      original = content; selected = { ...selected, exists: true };
      notice = '已保存到工具的本地文件。';
      const result = await invoke<Entry[]>('list_customizations', { provider });
      entries = result;
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function renameCustomization() {
    if (!selected || !selected.exists || busy) return;
    const isInstruction = selected.kind === 'instruction';
    const promptText = isInstruction
      ? '请输入新的指令文件名（必须以 .md 结尾）：'
      : '请输入新的 Skill 名称（仅允许小写字母、数字和连字符）：';
    const newName = window.prompt(promptText, selected.id.name);
    if (!newName || newName.trim() === '' || newName.trim() === selected.id.name) return;
    busy = true; error = ''; notice = '';
    try {
      const updated = await invoke<Entry>('rename_customization', {
        id: selected.id,
        newName: newName.trim(),
      });
      notice = `已成功重命名为 ${updated.id.name}`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
      selected = updated;
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function toggleSkill() {
    if (!selected || selected.kind !== 'skill' || busy) return;
    busy = true; error = ''; notice = '';
    try {
      const nextEnabled = !selected.enabled;
      await invoke('toggle_skill_enabled', { id: selected.id, enabled: nextEnabled });
      selected = { ...selected, enabled: nextEnabled };
      notice = nextEnabled
        ? `已启用 Skill: ${selected.id.name}`
        : `已停用 Skill: ${selected.id.name}（Agent 将不再识别和载入）`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function beginCreateInstruction() {
    if (!canLeave()) return;
    selected = null;
    original = null;
    content = '';
    instructionName = '';
    const instRoots = entries.filter((e) => e.kind === 'instruction').map((e) => e.id.root);
    instructionRoot = instRoots[0] || `${provider}-instructions`;
    creatingInstruction = true;
    creating = false;
    importing = false;
    adopting = false;
    updateInfo = null;
    skillSummary = null;
    error = '';
    notice = '';
  }

  async function createInstruction() {
    let name = instructionName.trim();
    if (!name.toLowerCase().endsWith('.md')) {
      name = `${name}.md`;
    }
    if (name.length < 4 || name.length > 64) {
      error = '请输入有效的文件名（必须以 .md 结尾）';
      return;
    }
    busy = true;
    error = '';
    try {
      const newEntry = await invoke<Entry>('create_instruction', {
        provider,
        root: instructionRoot,
        name,
        content: `# ${name}\n\n`,
      });
      creatingInstruction = false;
      entries = await invoke<Entry[]>('list_customizations', { provider });
      selected = newEntry;
      original = `# ${name}\n\n`;
      content = original;
      notice = '已创建新全局指令文件，可直接在此编辑保存。';
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  async function createSkill() {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillName) || skillName.length > 64) {
      error = '名称使用小写字母、数字和连字符，最长 64 个字符。'; return;
    }
    const id: Id = { provider, root: skillRoot, name: skillName };
    busy = true; error = '';
    try {
      const existing = await invoke<string | null>('read_customization', { id });
      if (existing !== null) { error = '这个 skill 已存在，请在列表中打开。'; return; }
      selected = { id, path: `${currentProvider.roots.find((r) => r.id === skillRoot)?.label} / ${skillName} / SKILL.md`, kind: 'skill', exists: false, editable: true, linked: false, enabled: true };
      original = null;
      content = `---\nname: ${skillName}\ndescription: 描述何时使用此 skill。\n---\n\n# ${skillName}\n\n`;
      creating = false; notice = '编辑后点击保存，才会创建文件。';
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  function beginCreate() {
    if (!canLeave()) return;
    selected = null; original = null; content = ''; skillName = ''; creating = true; creatingInstruction = false; importing = false; adopting = false; updateInfo = null; error = ''; notice = '';
  }

  function beginImport() {
    if (!canLeave()) return;
    selected = null; original = null; content = ''; sourceInput = ''; inspection = null; candidatePath = ''; skillName = ''; importing = true; creating = false; creatingInstruction = false; adopting = false; updateInfo = null; notice = ''; error = '';
  }

  async function inspectSource() {
    busy = true; error = ''; inspection = null;
    try {
      inspection = await invoke<Inspection>('inspect_skill_source', { input: sourceInput });
      candidatePath = inspection.candidates[0].path;
      skillName = inspection.candidates[0].name;
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  function chooseCandidate() { skillName = inspection?.candidates.find((c) => c.path === candidatePath)?.name ?? ''; }

  async function installSource() {
    if (!inspection || !/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillName) || skillName.length > 64) { error = '名称使用小写字母、数字和连字符，最长 64 个字符。'; return; }
    busy = true; error = '';
    try {
      await invoke('install_skill_source', { id: { provider, root: skillRoot, name: skillName }, source: inspection.source, revision: inspection.revision, skillPath: candidatePath });
      entries = await invoke<Entry[]>('list_customizations', { provider });
      importing = false; notice = '安装成功；选择左侧 skill 可查看或检查更新。';
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function beginAdoptSkill() {
    if (!selected || selected.kind !== 'skill') return;
    adopting = true;
    error = '';
    notice = '';
    adoptSourceInput = '';
    try {
      const detected = await invoke<string | null>('detect_skill_source', { id: selected.id });
      if (detected) {
        adoptSourceInput = detected;
        notice = `已自动检测到 Git 来源: ${detected}，请确认后点击纳管。`;
      }
    } catch {
      // ignore
    }
  }

  async function confirmAdoptSkill() {
    if (!selected || !adoptSourceInput.trim()) {
      error = '请输入 GitHub 仓库或 skills.sh 来源地址';
      return;
    }
    adoptingBusy = true;
    error = '';
    notice = '';
    try {
      const info = await invoke<UpdateInfo>('adopt_skill_source', {
        id: selected.id,
        sourceInput: adoptSourceInput.trim(),
        candidatePath: null,
      });
      updateInfo = info;
      adopting = false;
      notice = `纳管成功！来源已绑定，当前状态：${info.detail}`;
      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) {
      error = String(e);
    } finally {
      adoptingBusy = false;
    }
  }

  async function checkUpdate() {
    if (!selected || dirty) return;
    busy = true; error = ''; notice = ''; updateInfo = null; adopting = false;
    try { updateInfo = await invoke<UpdateInfo>('check_skill_update', { id: selected.id }); }
    catch (e) { error = String(e); }
    finally { busy = false; }
  }

  async function applyUpdate() {
    if (!selected || !updateInfo || dirty) return;
    const msg = updateInfo.kind === 'manager' ? 'Skills Manager 更新可能覆盖该 skill 的本地修改，也会影响引用同一目标的其他工具。继续更新？' : '将从来源更新此 skill；更新前会再次检查本地文件是否被修改。继续？';
    if (!window.confirm(msg)) return;
    busy = true; error = ''; notice = '';
    try {
      notice = await invoke<string>('update_skill_source', { id: selected.id });
      const value = await invoke<string | null>('read_customization', { id: selected.id });
      original = value; content = value ?? ''; updateInfo = null;
      entries = await invoke<Entry[]>('list_customizations', { provider });
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }

  onMount(() => {
    void refresh();
    void loadAiConfig();
  });
</script>

<div class="custom-page">
  <div class="custom-heading">
    <div class="heading-text">
      <h1>Agent 配置与技能管理</h1>
      <p>管理全局指令、Skill 启停与更新；支持第三方 Skill 纳管与 AI 智能摘要分析。</p>
    </div>
    <div class="head-right-actions">
      <button
        class="ai-toggle-btn"
        class:active={showAiConfig}
        onclick={() => showAiConfig = !showAiConfig}
        title="配置用于 Skill 智能分析的 AI 模型 API"
      >
        <span class="ai-status-dot" class:configured={hasAiConfig}></span>
        <span class="ai-btn-icon">{@html icons.sparkle}</span>
        <span>AI 助手配置</span>
      </button>
      <span class="local-badge" title="所有指令与 Skill 均只读写本机环境文件，不上传云端">
        {@html icons.shield}
        <span>仅操作本机文件</span>
      </span>
    </div>
  </div>

  {#if showAiConfig}
    <div class="ai-config-panel" role="region" aria-label="AI 助手配置">
      <div class="ai-config-head">
        <div class="ai-config-title">
          <span class="ai-config-icon">{@html icons.sparkle}</span>
          <strong>AI 助手服务配置</strong>
          <span class="ai-config-sub">用于 Skill 代码智能分析与中文使用指南生成</span>
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

  <div class="privacy-banner">
    <span class="banner-icon">{@html icons.info}</span>
    <p>内容直接读写工具原有文件，不进入 TokenScope 数据库或同步。支持第三方 Skill 纳管更新；Skill 停用后编码工具将不再扫描载入。</p>
  </div>

  <div class="provider-tabs-wrap">
    <div class="provider-tabs" role="group" aria-label="选择工具">
      {#each providers as item}
        <button
          type="button"
          class="provider-tab-btn"
          class:selected={provider === item.id}
          aria-pressed={provider === item.id}
          disabled={busy}
          onclick={() => void switchProvider(item.id)}
        >
          {item.label}
        </button>
      {/each}
    </div>
  </div>

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理本地文件。</div>
  {:else}
    {#if error}<p class="message error" role="alert">{error}</p>{/if}
    {#if notice}<p class="message success" role="status">{notice}</p>{/if}

    <div class="workspace">
      <!-- Catalog (Left Column) -->
      <section class="catalog" aria-label="文件列表">
        <div class="catalog-head">
          <div class="catalog-head-left">
            <strong>文件列表</strong>
            <span class="catalog-total-pill">{instructions.length + skills.length} 项</span>
          </div>
          <button
            class="btn-icon-refresh"
            disabled={busy}
            onclick={() => void refresh()}
            title="刷新本地文件与 Skill 状态"
            aria-label="刷新"
          >
            <span class:spin={busy}>{@html icons.refresh}</span>
          </button>
        </div>

        <div class="catalog-search-box">
          <input
            type="search"
            class="catalog-search-input"
            bind:value={filterQuery}
            placeholder="搜索指令或 Skill..."
            aria-label="搜索指令或 Skill"
          />
          {#if filterQuery}
            <button
              type="button"
              class="search-clear-btn"
              onclick={() => filterQuery = ''}
              title="清除搜索"
              aria-label="清除搜索"
            >✕</button>
          {/if}
        </div>

        <div class="group-title instructions-head">
          <div class="group-title-left">
            <span>全局指令</span>
            <span class="count-badge">{filteredInstructions.length}</span>
          </div>
          <button class="btn-ghost-sm" disabled={busy} onclick={beginCreateInstruction}>+ 新建指令</button>
        </div>

        {#each filteredInstructions as entry}
          <button
            type="button"
            class="file-row"
            class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name}
            disabled={busy || !entry.editable}
            onclick={() => void select(entry)}
          >
            <div class="file-row-main">
              <span class="file-type-icon">📄</span>
              <strong class="file-name" title={entry.id.name}>{entry.id.name}</strong>
            </div>
            <span class="file-sub-root">{entry.exists ? entry.id.root : '尚未创建'}</span>
          </button>
        {:else}
          {#if filterQuery}
            <p class="empty-list">未匹配到指令</p>
          {/if}
        {/each}

        <div class="group-title skills-head">
          <div class="group-title-left">
            <span>Skills</span>
            <span class="count-badge">{filteredSkills.length}</span>
          </div>
          <div class="catalog-actions">
            <button class="btn-ghost-sm" disabled={busy} onclick={beginImport}>添加</button>
            <button class="btn-ghost-sm" disabled={busy} onclick={beginCreate}>新建</button>
          </div>
        </div>

        {#each filteredSkills as entry}
          <button
            type="button"
            class="file-row"
            class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name}
            class:row-disabled={!entry.enabled}
            disabled={busy || !entry.editable}
            title={entry.editable ? entry.path : '符号链接请在原位置管理'}
            onclick={() => void select(entry)}
          >
            <div class="file-row-main">
              <span class="file-type-icon">🧩</span>
              <div class="skill-name-col">
                <strong class="file-name" title={entry.id.name}>{entry.id.name}</strong>
                <div class="skill-tags-row">
                  {#if !entry.enabled}
                    <span class="status-badge disabled">已停用</span>
                  {/if}
                  {#if entry.linked}
                    <span class="link-badge">软链接</span>
                  {/if}
                </div>
              </div>
            </div>
            <span class="file-sub-root">{entry.editable ? entry.id.root : '无法访问目标文件'}</span>
          </button>
        {:else}
          <p class="empty-list">{filterQuery ? '未匹配到相关 Skill' : '尚未发现全局 skill。'}</p>
        {/each}
      </section>

      <!-- Editor Area (Right Column) -->
      <section class="editor" aria-label="文件编辑器">
        {#if importing}
          <div class="editor-head">
            <div>
              <h2>从来源添加 skill</h2>
              <p>支持 skills.sh 页面和 GitHub 仓库或子目录；只下载文件，不执行脚本。</p>
            </div>
          </div>
          <div class="create-form">
            <label for="source-url">来源地址</label>
            <input id="source-url" bind:value={sourceInput} oninput={() => inspection = null} placeholder="https://skills.sh/owner/repo/skill 或 GitHub URL" />
            <div><button class="btn-secondary" disabled={busy || !sourceInput.trim()} onclick={() => void inspectSource()}>{busy ? '读取中…' : '预览来源'}</button></div>
            {#if inspection}
              <label for="source-skill">选择 skill</label>
              <select id="source-skill" bind:value={candidatePath} onchange={chooseCandidate}>
                {#each inspection.candidates as item}
                  <option value={item.path}>{item.name} · {item.path || '仓库根目录'}</option>
                {/each}
              </select>
              <details><summary>查看 SKILL.md 预览</summary><pre>{inspection.candidates.find((c) => c.path === candidatePath)?.preview}</pre></details>
              <label for="import-root">安装位置</label>
              <select id="import-root" bind:value={skillRoot}>
                {#each currentProvider.roots as root}
                  <option value={root.id}>{root.label}</option>
                {/each}
              </select>
              <label for="import-name">安装名称</label>
              <input id="import-name" bind:value={skillName} maxlength="64" />
              <p class="source-hint">{inspection.source.url} · {inspection.revision.slice(0, 12)}。请确认来源可信，skill 可能包含脚本。</p>
              <div class="form-btn-row">
                <button class="btn-primary" disabled={busy} onclick={() => void installSource()}>安装选中 skill</button>
                <button class="btn-secondary" onclick={() => importing = false}>取消</button>
              </div>
            {:else}
              <div><button class="btn-secondary" onclick={() => importing = false}>取消</button></div>
            {/if}
          </div>

        {:else if creatingInstruction}
          <div class="editor-head">
            <div>
              <h2>新建全局指令文件</h2>
              <p>创建自定义全局指令或规则文件（如 RULES.md、CUSTOM.md）。</p>
            </div>
          </div>
          <div class="create-form">
            <label for="instr-root">保存目录</label>
            <select id="instr-root" bind:value={instructionRoot}>
              {#each instructions.map((e) => e.id.root).filter((v, i, a) => a.indexOf(v) === i) as rootId}
                <option value={rootId}>{rootId}</option>
              {/each}
            </select>
            <label for="instr-name">指令文件名 (必须以 .md 结尾)</label>
            <input id="instr-name" bind:value={instructionName} placeholder="例如 CUSTOM_RULES.md" maxlength="64" />
            <div class="form-btn-row">
              <button class="btn-primary" disabled={busy || !instructionName.trim()} onclick={() => void createInstruction()}>立即创建</button>
              <button class="btn-secondary" onclick={() => creatingInstruction = false}>取消</button>
            </div>
          </div>

        {:else if creating}
          <div class="editor-head">
            <div>
              <h2>新建 skill</h2>
              <p>创建一个包含 SKILL.md 的目录。</p>
            </div>
          </div>
          <div class="create-form">
            <label for="skill-root">保存位置</label>
            <select id="skill-root" bind:value={skillRoot}>
              {#each currentProvider.roots as root}
                <option value={root.id}>{root.label}</option>
              {/each}
            </select>
            <label for="skill-name">名称</label>
            <input id="skill-name" bind:value={skillName} placeholder="例如 code-review" maxlength="64" />
            <div class="form-btn-row">
              <button class="btn-primary" disabled={busy} onclick={() => void createSkill()}>开始编辑</button>
              <button class="btn-secondary" onclick={() => creating = false}>取消</button>
            </div>
          </div>

        {:else if selected}
          <div class="editor-head">
            <div class="editor-meta">
              <div class="editor-title-row">
                <h2 class="editor-file-title" title={selected.id.name}>
                  {selected.id.name}{selected.kind === 'skill' ? ' / SKILL.md' : ''}
                </h2>
                <div class="editor-badges">
                  {#if selected.kind === 'skill'}
                    <span class="status-badge" class:disabled={!selected.enabled} class:enabled={selected.enabled}>
                      <span class="status-dot"></span>
                      {selected.enabled ? '已启用' : '已停用'}
                    </span>
                  {/if}
                  {#if selected.linked}
                    <span class="link-badge">软链接目标</span>
                  {/if}
                </div>
              </div>

              <div class="editor-path-row">
                <span class="editor-path" title={selected.path}>{selected.path}</span>
                <button
                  type="button"
                  class="copy-path-btn"
                  onclick={() => { if (selected) void copyPath(selected.path); }}
                  title="复制文件绝对路径"
                >
                  {#if copiedPath}
                    <span class="copy-success">已复制</span>
                  {:else}
                    <span>复制路径</span>
                  {/if}
                </button>
              </div>
            </div>

            <div class="editor-actions">
              {#if selected.kind === 'skill' && selected.exists}
                <button
                  class="btn-secondary btn-action"
                  disabled={busy}
                  onclick={() => void toggleSkill()}
                  title={selected.enabled ? '停用此 Skill，Agent 将不再载入' : '启用此 Skill'}
                >
                  {selected.enabled ? '停用 Skill' : '启用 Skill'}
                </button>
                <button
                  class="btn-secondary btn-action ai-action-btn"
                  disabled={busy || analyzingSkill}
                  onclick={() => void generateSkillSummary()}
                  title="使用 AI 深度分析此 Skill，给出中文核心功能摘要与调用指南"
                >
                  {#if analyzingSkill}
                    <span class="spinner-small" aria-hidden="true"></span> 分析中…
                  {:else}
                    <span class="ai-sparkle-icon">{@html icons.sparkle}</span> AI 智能分析
                  {/if}
                </button>
                <button
                  class="btn-secondary btn-action"
                  disabled={busy || dirty}
                  onclick={() => void checkUpdate()}
                >
                  检查更新
                </button>
              {/if}

              {#if selected.exists}
                <button
                  class="btn-secondary btn-action"
                  disabled={busy}
                  onclick={() => void renameCustomization()}
                  title="重命名文件或 Skill 目录"
                >
                  改名
                </button>
              {/if}

              <button
                class="btn-primary btn-save"
                disabled={busy || (!dirty && selected.exists)}
                onclick={() => void save()}
              >
                {busy ? '处理中…' : selected.exists ? '保存更改' : '创建文件'}
              </button>
            </div>
          </div>

          {#if updateInfo}
            <div class="update-panel">
              <div class="update-info-text">
                <strong>更新状态：</strong>{updateInfo.detail}
                {#if updateInfo.source} · <span>{updateInfo.source}</span>{/if}
              </div>
              <div class="update-actions">
                {#if updateInfo.status === 'unsupported' && selected.kind === 'skill'}
                  <button class="btn-primary btn-compact" disabled={busy} onclick={() => void beginAdoptSkill()}>
                    纳入更新管理
                  </button>
                {/if}
                {#if updateInfo.kind === 'manager' || updateInfo.status === 'available'}
                  <button class="btn-secondary btn-compact" disabled={busy} onclick={() => void applyUpdate()}>
                    更新 skill
                  </button>
                {/if}
              </div>
            </div>
          {/if}

          {#if adopting}
            <div class="adopt-panel">
              <div class="adopt-title">
                <strong>🔗 纳入更新管理 (绑定来源仓库)</strong>
                <button class="btn-compact" onclick={() => adopting = false}>✕</button>
              </div>
              <p class="adopt-desc">将其他工具安装或本地存在的 Skill 纳入 TokenScope 统一管理，绑定来源后即可自动检查版本并一键更新。</p>
              <div class="adopt-form">
                <label for="adopt-source">GitHub 仓库或 skills.sh 地址：</label>
                <input
                  id="adopt-source"
                  bind:value={adoptSourceInput}
                  placeholder="https://github.com/owner/repo"
                />
                <div class="adopt-buttons">
                  <button
                    class="btn-primary"
                    disabled={adoptingBusy || !adoptSourceInput.trim()}
                    onclick={() => void confirmAdoptSkill()}
                  >
                    {adoptingBusy ? '正在拉取验证并绑定…' : '确认纳管'}
                  </button>
                  <button class="btn-secondary" onclick={() => adopting = false}>取消</button>
                </div>
              </div>
            </div>
          {/if}

          {#if skillSummary && selected.kind === 'skill'}
            <div class="summary-card">
              <div class="summary-header">
                <div class="summary-title">
                  <span class="summary-sparkle">{@html icons.sparkle}</span>
                  <span>AI 智能摘要分析</span>
                  <span class="summary-meta">模型: {skillSummary.model}</span>
                </div>
                <div class="summary-actions">
                  <button class="btn-compact" onclick={() => showSummaryCard = !showSummaryCard}>
                    {showSummaryCard ? '收起' : '展开'}
                  </button>
                  <button
                    class="btn-compact"
                    disabled={analyzingSkill}
                    onclick={() => void generateSkillSummary()}
                  >
                    {analyzingSkill ? '生成中…' : '重新分析'}
                  </button>
                </div>
              </div>
              {#if showSummaryCard}
                <div class="summary-body">
                  <div class="summary-section">
                    <h4>💡 功能定位（这个 Skill 是干什么的）：</h4>
                    <p>{skillSummary.overview}</p>
                  </div>
                  {#if skillSummary.usage_guide}
                    <div class="summary-section">
                      <h4>📖 推荐使用方式与调用指南：</h4>
                      <pre class="summary-pre">{skillSummary.usage_guide}</pre>
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}

          <div class="editor-textarea-wrap">
            <label class="sr-only" for="agent-editor">文件内容</label>
            <textarea
              id="agent-editor"
              spellcheck="false"
              bind:value={content}
              disabled={busy}
              placeholder="文件为空，在此输入文本内容..."
            ></textarea>
          </div>

          <div class="editor-foot">
            <div class="editor-foot-left">
              {#if dirty}
                <span class="foot-status dirty">
                  <span class="status-dot dirty"></span> 有未保存的更改
                </span>
              {:else if selected.exists}
                <span class="foot-status saved">
                  <span class="status-dot saved"></span> 已保存
                </span>
              {:else}
                <span class="foot-status unsaved">
                  <span class="status-dot unsaved"></span> 文件尚未创建
                </span>
              {/if}
              <span class="foot-divider">·</span>
              <span class="foot-metrics">{editorLines} 行 · {editorChars} 字符</span>
            </div>
            <div class="editor-foot-right">
              <span>UTF-8</span>
              <span class="foot-divider">·</span>
              <span>最大 1 MiB</span>
            </div>
          </div>
        {:else}
          <div class="editor-empty">
            <div class="empty-icon-box">{@html icons.tools}</div>
            <h2>选择文件开始编辑</h2>
            <p>在左侧选择全局指令或 Skill 进行查看与修改，亦可新建指令、新建 Skill 或一键安装 GitHub 开源技能。</p>
          </div>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .custom-page {
    max-width: 1440px;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  .custom-heading {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: 16px;
    flex-wrap: wrap;
  }

  .heading-text h1 {
    font-size: 20px;
    font-weight: 700;
    letter-spacing: -0.02em;
    margin: 0 0 4px;
    color: var(--text-primary);
  }

  .heading-text p {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  .head-right-actions {
    display: flex;
    align-items: center;
    gap: 10px;
    flex-shrink: 0;
  }

  .ai-toggle-btn {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
    background: var(--bg-card);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .ai-toggle-btn:hover {
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .ai-toggle-btn.active {
    color: var(--brand-strong);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .ai-status-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-muted);
  }

  .ai-status-dot.configured {
    background: var(--color-exact);
    box-shadow: 0 0 6px var(--color-exact);
  }

  .ai-btn-icon {
    display: inline-flex;
    width: 14px;
    height: 14px;
  }

  .ai-btn-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .local-badge {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 26px;
    padding: 0 9px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 999px;
    white-space: nowrap;
  }

  .local-badge :global(svg) {
    width: 12px;
    height: 12px;
    color: var(--color-exact);
  }

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
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
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

  .ai-config-sub {
    font-size: 11.5px;
    font-weight: normal;
    color: var(--text-muted);
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

  .btn-sm {
    height: 28px;
    padding: 0 12px;
    font-size: 11.5px;
  }

  /* Privacy Banner */
  .privacy-banner {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--text-muted);
  }

  .banner-icon {
    display: inline-flex;
    width: 14px;
    height: 14px;
    flex-shrink: 0;
    color: var(--text-secondary);
  }

  .banner-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .privacy-banner p {
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* Provider Segmented Capsule */
  .provider-tabs-wrap {
    display: flex;
    align-items: center;
    margin-bottom: 2px;
  }

  .provider-tabs {
    display: inline-flex;
    align-items: center;
    gap: 3px;
    padding: 3px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-sm);
  }

  .provider-tab-btn {
    height: 28px;
    padding: 0 14px;
    border: none;
    background: transparent;
    border-radius: 4px;
    font: inherit;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-secondary);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .provider-tab-btn:hover:not(:disabled) {
    color: var(--text-primary);
  }

  .provider-tab-btn.selected {
    background: var(--bg-card);
    color: var(--brand-strong);
    font-weight: 600;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.08);
  }

  .provider-tab-btn:disabled {
    opacity: 0.6;
    cursor: not-allowed;
  }

  /* Workspace Layout */
  .workspace {
    display: grid;
    grid-template-columns: 290px minmax(0, 1fr);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-md);
    background: var(--bg-card);
    min-height: 560px;
    overflow: hidden;
    box-shadow: 0 1px 3px rgba(0, 0, 0, 0.02);
  }

  /* Catalog (Left Column) */
  .catalog {
    border-right: 1px solid var(--border-subtle);
    padding: 12px;
    max-height: 76vh;
    overflow-y: auto;
    scrollbar-width: thin;
    scrollbar-color: var(--border-strong) transparent;
    display: flex;
    flex-direction: column;
    gap: 8px;
    background: var(--bg-card);
  }

  .catalog-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding-bottom: 8px;
    border-bottom: 1px solid var(--border-subtle);
  }

  .catalog-head-left {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 12.5px;
    color: var(--text-primary);
  }

  .catalog-total-pill {
    padding: 1px 6px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border-radius: 999px;
  }

  .btn-icon-refresh {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-muted);
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-icon-refresh:hover:not(:disabled) {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .btn-icon-refresh :global(svg) {
    width: 13px;
    height: 13px;
  }

  .btn-icon-refresh :global(.spin) {
    animation: spin 0.8s linear infinite;
  }

  .catalog-search-box {
    position: relative;
    width: 100%;
  }

  .catalog-search-input {
    width: 100%;
    height: 28px;
    padding: 0 24px 0 8px;
    font: inherit;
    font-size: 11.5px;
    color: var(--text-primary);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    transition: all var(--transition-fast);
  }

  .catalog-search-input:focus {
    outline: none;
    border-color: var(--brand-primary);
    background: var(--bg-card);
  }

  .search-clear-btn {
    position: absolute;
    right: 4px;
    top: 50%;
    transform: translateY(-50%);
    width: 18px;
    height: 18px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 10px;
    border: none;
    background: transparent;
    color: var(--text-muted);
    cursor: pointer;
  }

  .search-clear-btn:hover {
    color: var(--text-primary);
  }

  .group-title {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-top: 8px;
    margin-bottom: 2px;
    font-size: 11px;
    color: var(--text-muted);
    font-weight: 600;
  }

  .group-title-left {
    display: flex;
    align-items: center;
    gap: 5px;
  }

  .count-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    height: 15px;
    padding: 0 4px;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-secondary);
    background: var(--bg-secondary);
    border-radius: 999px;
  }

  .btn-ghost-sm {
    height: 22px;
    padding: 0 6px;
    font: inherit;
    font-size: 10.5px;
    font-weight: 500;
    color: var(--brand-strong);
    background: var(--brand-subtle);
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
    white-space: nowrap;
  }

  .btn-ghost-sm:hover:not(:disabled) {
    background: var(--brand-primary);
    color: #fff;
    border-color: var(--brand-primary);
  }

  .catalog-actions {
    display: flex;
    align-items: center;
    gap: 4px;
  }

  .file-row {
    position: relative;
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    width: 100%;
    gap: 2px;
    text-align: left;
    border: 1px solid transparent;
    border-radius: var(--radius-xs);
    background: transparent;
    color: var(--text-primary);
    padding: 7px 8px;
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .file-row:hover {
    background: var(--bg-secondary);
    border-color: var(--border-subtle);
  }

  .file-row.active {
    background: var(--bg-secondary);
    border-color: var(--brand-border);
  }

  .file-row.active::before {
    content: '';
    position: absolute;
    left: 0;
    top: 6px;
    bottom: 6px;
    width: 3px;
    background: var(--brand-primary);
    border-radius: 0 2px 2px 0;
  }

  .file-row:disabled {
    opacity: 0.55;
    cursor: not-allowed;
  }

  .file-row.row-disabled {
    opacity: 0.65;
  }

  .file-row-main {
    display: flex;
    align-items: flex-start;
    gap: 6px;
    width: 100%;
    min-width: 0;
  }

  .file-type-icon {
    font-size: 12px;
    flex-shrink: 0;
    line-height: 1.4;
  }

  .file-name {
    font-size: 12px;
    font-weight: 500;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: 1;
    min-width: 0;
    line-height: 1.4;
  }

  .file-sub-root {
    font-size: 10.5px;
    color: var(--text-muted);
    padding-left: 18px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    width: 100%;
  }

  .skill-name-col {
    display: flex;
    flex-direction: column;
    gap: 2px;
    flex: 1;
    min-width: 0;
  }

  .skill-tags-row {
    display: flex;
    align-items: center;
    gap: 4px;
    flex-wrap: wrap;
    margin-top: 1px;
  }

  .empty-list {
    font-size: 11px;
    color: var(--text-muted);
    padding: 10px 4px;
    text-align: center;
    margin: 0;
  }

  /* Badges - CRITICAL BUG FIX: white-space: nowrap */
  .status-badge, .link-badge {
    white-space: nowrap !important;
    flex-shrink: 0 !important;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 18px;
    padding: 0 6px;
    border-radius: 999px;
    font-size: 10px;
    font-weight: 500;
    line-height: 1;
    letter-spacing: 0;
  }

  .status-badge.enabled {
    background: var(--color-exact-bg);
    color: var(--color-exact);
    border: 1px solid var(--color-exact-border);
  }

  .status-badge.disabled {
    background: rgba(239, 68, 68, 0.1);
    color: #ef4444;
    border: 1px solid rgba(239, 68, 68, 0.25);
  }

  .status-dot {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: currentColor;
  }

  .link-badge {
    border: 1px solid var(--border-subtle);
    background: var(--bg-secondary);
    color: var(--text-muted);
  }

  /* Editor Area (Right Column) */
  .editor {
    display: flex;
    min-width: 0;
    flex-direction: column;
    background: var(--bg-card);
  }

  .editor-head {
    display: flex;
    justify-content: space-between;
    align-items: flex-start;
    gap: 16px;
    padding: 14px 18px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--bg-card);
  }

  .editor-meta {
    flex: 1;
    min-width: 0;
  }

  .editor-title-row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex-wrap: wrap;
    min-width: 0;
  }

  .editor-file-title {
    font-size: 15px;
    font-weight: 700;
    letter-spacing: -0.01em;
    color: var(--text-primary);
    margin: 0;
    word-break: break-all;
    line-height: 1.3;
  }

  .editor-badges {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
  }

  .editor-path-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 4px;
    min-width: 0;
  }

  .editor-path {
    font-family: var(--font-mono, monospace);
    font-size: 11px;
    color: var(--text-muted);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 520px;
    line-height: 1.4;
  }

  .copy-path-btn {
    display: inline-flex;
    align-items: center;
    height: 20px;
    padding: 0 6px;
    font: inherit;
    font-size: 10px;
    color: var(--text-muted);
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    cursor: pointer;
    transition: all var(--transition-fast);
    flex-shrink: 0;
    white-space: nowrap;
  }

  .copy-path-btn:hover {
    color: var(--text-primary);
    border-color: var(--border-default);
  }

  .copy-success {
    color: var(--color-exact);
    font-weight: 600;
  }

  .editor-actions {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-shrink: 0;
    flex-wrap: wrap;
    justify-content: flex-end;
  }

  .btn-action {
    height: 28px;
    padding: 0 10px;
    font-size: 11.5px;
    font-weight: 500;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    white-space: nowrap;
  }

  .ai-action-btn {
    color: var(--brand-strong);
    background: var(--brand-subtle);
    border-color: var(--brand-border);
  }

  .ai-action-btn:hover:not(:disabled) {
    background: var(--brand-primary);
    color: #fff;
  }

  .ai-sparkle-icon {
    display: inline-flex;
    width: 12px;
    height: 12px;
  }

  .ai-sparkle-icon :global(svg) {
    width: 100%;
    height: 100%;
  }

  .btn-save {
    height: 28px;
    padding: 0 14px;
    font-size: 11.5px;
    font-weight: 600;
    white-space: nowrap;
  }

  /* Forms & Create/Import Panels */
  .create-form {
    display: flex;
    flex-direction: column;
    gap: 10px;
    max-width: 520px;
    padding: 20px;
    font-size: 12px;
    color: var(--text-secondary);
  }

  .create-form input, .create-form select {
    height: 32px;
    padding: 0 10px;
    font: inherit;
    font-size: 12px;
    color: var(--text-primary);
    background: var(--bg-input);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
  }

  .create-form input:focus, .create-form select:focus {
    border-color: var(--brand-primary);
    outline: none;
  }

  .form-btn-row {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 6px;
  }

  .source-hint {
    font-size: 11px;
    color: var(--text-muted);
    line-height: 1.5;
    margin: 4px 0;
  }

  details {
    color: var(--text-secondary);
    font-size: 11.5px;
  }

  summary {
    cursor: pointer;
    color: var(--brand-strong);
  }

  pre {
    max-height: 220px;
    overflow: auto;
    white-space: pre-wrap;
    font: 11px/1.5 var(--font-mono, monospace);
    background: var(--bg-secondary);
    padding: 8px;
    border-radius: var(--radius-xs);
    border: 1px solid var(--border-subtle);
  }

  /* Update and Adopt Panels */
  .update-panel {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: 12px;
    padding: 8px 18px;
    border-bottom: 1px solid var(--border-subtle);
    background: var(--brand-subtle);
    color: var(--brand-strong);
    font-size: 11.5px;
  }

  .update-actions {
    display: flex;
    gap: 6px;
    flex-shrink: 0;
  }

  .adopt-panel {
    padding: 12px 18px;
    background: rgba(245, 158, 11, 0.06);
    border-bottom: 1px solid rgba(245, 158, 11, 0.2);
    display: flex;
    flex-direction: column;
    gap: 6px;
    font-size: 12px;
  }

  .adopt-title {
    display: flex;
    justify-content: space-between;
    align-items: center;
    color: #d97706;
  }

  .adopt-desc {
    margin: 0;
    font-size: 11px;
    color: var(--text-muted);
  }

  .adopt-form {
    display: flex;
    flex-direction: column;
    gap: 6px;
    max-width: 520px;
    margin-top: 4px;
  }

  .adopt-form input {
    height: 30px;
    padding: 0 8px;
    font: inherit;
    font-size: 12px;
    background: var(--bg-card);
    border: 1px solid var(--border-input);
    border-radius: var(--radius-sm);
  }

  .adopt-buttons {
    display: flex;
    gap: 8px;
  }

  /* Summary Card */
  .summary-card {
    margin: 12px 18px;
    border: 1px solid var(--brand-border);
    border-radius: var(--radius-sm);
    background: var(--bg-card);
    overflow: hidden;
    font-size: 12px;
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.03);
  }

  .summary-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 8px 12px;
    border-bottom: 1px solid var(--brand-border);
    background: var(--brand-subtle);
  }

  .summary-title {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 600;
    color: var(--brand-strong);
  }

  .summary-sparkle {
    display: inline-flex;
    width: 14px;
    height: 14px;
  }

  .summary-sparkle :global(svg) {
    width: 100%;
    height: 100%;
  }

  .summary-meta {
    font-size: 10.5px;
    color: var(--text-muted);
    font-weight: normal;
  }

  .summary-actions {
    display: flex;
    gap: 6px;
  }

  .summary-body {
    padding: 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .summary-section h4 {
    margin: 0 0 4px;
    font-size: 12px;
    color: var(--text-primary);
  }

  .summary-section p {
    margin: 0;
    line-height: 1.55;
    color: var(--text-secondary);
  }

  .summary-pre {
    margin: 0;
    padding: 8px 10px;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    font-size: 11px;
    line-height: 1.5;
    color: var(--text-primary);
    white-space: pre-wrap;
  }

  /* Textarea Editor Box */
  .editor-textarea-wrap {
    flex: 1;
    min-height: 380px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }

  textarea {
    flex: 1;
    width: 100%;
    height: 100%;
    min-height: 380px;
    resize: none;
    padding: 16px;
    border: none;
    outline: none;
    color: var(--text-primary);
    background: transparent;
    font: 12.5px/1.65 var(--font-mono, 'JetBrains Mono', 'Cascadia Code', Consolas, monospace);
    tab-size: 2;
  }

  /* Editor Footer Status Bar */
  .editor-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 7px 18px;
    border-top: 1px solid var(--border-subtle);
    font-size: 11px;
    color: var(--text-muted);
    background: var(--bg-card);
  }

  .editor-foot-left, .editor-foot-right {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .foot-status {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    font-weight: 500;
  }

  .foot-status.dirty {
    color: #f59e0b;
  }

  .foot-status.saved {
    color: var(--color-exact);
  }

  .foot-status .status-dot.dirty {
    background: #f59e0b;
  }

  .foot-status .status-dot.saved {
    background: var(--color-exact);
  }

  .foot-divider {
    color: var(--border-default);
  }

  .editor-empty {
    margin: auto;
    text-align: center;
    padding: 48px 20px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
  }

  .empty-icon-box {
    width: 44px;
    height: 44px;
    display: flex;
    align-items: center;
    justify-content: center;
    background: var(--bg-secondary);
    border: 1px solid var(--border-subtle);
    border-radius: 50%;
    color: var(--text-muted);
    margin-bottom: 4px;
  }

  .empty-icon-box :global(svg) {
    width: 20px;
    height: 20px;
  }

  .editor-empty h2 {
    font-size: 14px;
    color: var(--text-primary);
    margin: 0;
  }

  .editor-empty p {
    font-size: 11.5px;
    color: var(--text-muted);
    max-width: 42ch;
    margin: 0;
    line-height: 1.5;
  }

  .btn-compact {
    min-height: 22px;
    padding: 0 8px;
    font-size: 10.5px;
    border: 1px solid var(--border-subtle);
    border-radius: var(--radius-xs);
    background: var(--bg-secondary);
    color: var(--text-primary);
    cursor: pointer;
    transition: all var(--transition-fast);
  }

  .btn-compact:hover {
    border-color: var(--border-default);
    background: var(--bg-card-hover);
  }

  .message {
    font-size: 11.5px;
    padding: 8px 14px;
    border-radius: var(--radius-sm);
    margin-bottom: 8px;
    border: 1px solid var(--border-subtle);
  }

  .message.error {
    color: var(--color-danger);
    background: var(--color-danger-bg);
    border-color: var(--color-danger-border);
  }

  .message.success {
    color: var(--color-exact);
    background: var(--color-exact-bg);
    border-color: var(--color-exact-border);
  }

  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  @media (max-width: 900px) {
    .ai-config-grid {
      grid-template-columns: 1fr;
    }
    .workspace {
      grid-template-columns: 1fr;
    }
    .catalog {
      border-right: none;
      border-bottom: 1px solid var(--border-subtle);
      max-height: 280px;
    }
  }
</style>
