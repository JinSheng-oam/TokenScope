<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

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
    <div>
      <h1>Agent 配置与技能管理</h1>
      <p>管理全局指令、Skill 启停与更新；支持第三方 Skill 纳管与 AI 智能摘要分析。</p>
    </div>
    <div class="head-right-actions">
      <button class="btn-secondary" onclick={() => showAiConfig = !showAiConfig}>
        ⚙️ AI 助手配置
      </button>
      <span class="local-label">仅本机</span>
    </div>
  </div>

  {#if showAiConfig}
    <div class="ai-config-panel">
      <div class="ai-config-head">
        <strong>🤖 AI 助手配置（用于 Skill 智能分析与摘要生成）</strong>
        <button class="btn-compact" onclick={() => showAiConfig = false}>✕ 关闭</button>
      </div>
      <div class="ai-presets-row">
        <span class="text-muted">快捷服务商：</span>
        {#each aiPresets as p}
          <button class="ai-preset-chip" onclick={() => applyAiPreset(p)}>{p.label}</button>
        {/each}
      </div>
      <div class="ai-config-grid">
        <label>
          <span>API 接口地址 (支持自动补全)：</span>
          <input
            bind:value={aiConfig.api_url}
            onblur={() => aiConfig.api_url = normalizeApiUrl(aiConfig.api_url)}
            placeholder="例如 https://api.openai.com/v1 或 https://api.deepseek.com"
          />
        </label>
        <label>
          <span>API Key：</span>
          <input
            type="password"
            bind:value={aiConfig.api_key}
            placeholder="sk-..."
          />
        </label>
        <label class="ai-model-field">
          <span>模型名称：</span>
          <div class="ai-model-input-group">
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
              class="btn-secondary"
              disabled={fetchingModels || !aiConfig.api_url.trim()}
              onclick={() => void fetchModels()}
            >
              {fetchingModels ? '拉取中…' : '拉取模型'}
            </button>
          </div>
        </label>
      </div>
      <div class="ai-config-actions">
        <button class="btn-primary" disabled={aiSaving} onclick={() => void saveAiConfig()}>
          {aiSaving ? '保存中…' : '保存配置'}
        </button>
      </div>
    </div>
  {/if}

  <p class="privacy-note">内容直接读写工具原有文件，不进入 TokenScope 数据库或同步。支持第三方 Skill 纳管更新；Skill 停用后编码工具将不再扫描载入。</p>
  <div class="provider-tabs" role="group" aria-label="选择工具">
    {#each providers as item}
      <button class="btn-secondary" class:selected={provider === item.id} aria-pressed={provider === item.id} disabled={busy} onclick={() => void switchProvider(item.id)}>{item.label}</button>
    {/each}
  </div>

  {#if !runtimeAvailable}
    <div class="empty-box">请在 TokenScope 桌面应用中管理本地文件。</div>
  {:else}
    {#if error}<p class="message error" role="alert">{error}</p>{/if}
    {#if notice}<p class="message" role="status">{notice}</p>{/if}
    <div class="workspace">
      <section class="catalog" aria-label="文件列表">
        <div class="catalog-head">
          <strong>文件列表</strong>
          <button class="btn-secondary" disabled={busy} onclick={() => void refresh()}>刷新</button>
        </div>

        <div class="group-title instructions-head">
          <span>全局指令 · {instructions.length}</span>
          <button class="btn-secondary" disabled={busy} onclick={beginCreateInstruction}>新建指令</button>
        </div>
        {#each instructions as entry}
          <button class="file-row" class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name} disabled={busy || !entry.editable} onclick={() => void select(entry)}>
            <strong>{entry.id.name}</strong><span>{entry.exists ? entry.id.root : '尚未创建'}</span>
          </button>
        {/each}

        <div class="group-title skills-head">
          <span>Skills · {skills.length}</span>
          <div class="catalog-actions">
            <button class="btn-secondary" disabled={busy} onclick={beginImport}>添加</button>
            <button class="btn-secondary" disabled={busy} onclick={beginCreate}>新建</button>
          </div>
        </div>
        {#each skills as entry}
          <button class="file-row" class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name} class:row-disabled={!entry.enabled} disabled={busy || !entry.editable} title={entry.editable ? entry.path : '符号链接请在原位置管理'} onclick={() => void select(entry)}>
            <div class="skill-row-title">
              <strong>{entry.id.name}</strong>
              {#if !entry.enabled}
                <span class="status-badge disabled">已停用</span>
              {/if}
              {#if entry.linked}
                <span class="link-badge">软链接</span>
              {/if}
            </div>
            <span>{entry.editable ? entry.id.root : '无法访问目标文件'}</span>
          </button>
        {:else}
          <p class="empty-list">尚未发现全局 skill。</p>
        {/each}
      </section>

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
              <div>
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
            <div>
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
            <div>
              <button class="btn-primary" disabled={busy} onclick={() => void createSkill()}>开始编辑</button>
              <button class="btn-secondary" onclick={() => creating = false}>取消</button>
            </div>
          </div>

        {:else if selected}
          <div class="editor-head">
            <div>
              <div class="editor-title-row">
                <h2>{selected.id.name}{selected.kind === 'skill' ? ' / SKILL.md' : ''}</h2>
                {#if selected.kind === 'skill'}
                  <span class="status-badge" class:disabled={!selected.enabled} class:enabled={selected.enabled}>
                    {selected.enabled ? '已启用' : '已停用'}
                  </span>
                {/if}
                {#if selected.linked}
                  <span class="link-badge">软链接目标</span>
                {/if}
              </div>
              <p title={selected.path}>{selected.path}</p>
            </div>

            <div class="editor-actions">
              {#if selected.kind === 'skill' && selected.exists}
                <button
                  class="btn-secondary"
                  disabled={busy}
                  onclick={() => void toggleSkill()}
                  title={selected.enabled ? '停用此 Skill，Agent 将不再载入' : '启用此 Skill'}
                >
                  {selected.enabled ? '停用 Skill' : '启用 Skill'}
                </button>
                <button
                  class="btn-secondary"
                  disabled={busy || analyzingSkill}
                  onclick={() => void generateSkillSummary()}
                  title="使用 AI 深度分析此 Skill，给出中文核心功能摘要与调用指南"
                >
                  {analyzingSkill ? 'AI 分析中…' : '✨ AI 智能分析'}
                </button>
                <button
                  class="btn-secondary"
                  disabled={busy || dirty}
                  onclick={() => void checkUpdate()}
                >
                  检查更新
                </button>
              {/if}

              {#if selected.exists}
                <button
                  class="btn-secondary"
                  disabled={busy}
                  onclick={() => void renameCustomization()}
                  title="重命名文件或 Skill 目录"
                >
                  改名
                </button>
              {/if}

              <button
                class="btn-primary"
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
                  <span>💡 AI 智能摘要分析</span>
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
                    <h4>功能定位（这个 Skill 是干什么的）：</h4>
                    <p>{skillSummary.overview}</p>
                  </div>
                  {#if skillSummary.usage_guide}
                    <div class="summary-section">
                      <h4>推荐使用方式与调用指南：</h4>
                      <pre class="summary-pre">{skillSummary.usage_guide}</pre>
                    </div>
                  {/if}
                </div>
              {/if}
            </div>
          {/if}

          <label class="sr-only" for="agent-editor">文件内容</label>
          <textarea id="agent-editor" spellcheck="false" bind:value={content} disabled={busy}></textarea>
          <div class="editor-foot">
            <span>{dirty ? '有未保存的更改' : selected.exists ? '已保存' : '文件尚未创建'}</span>
            <span>UTF-8 · 最大 1 MiB</span>
          </div>
        {:else}
          <div class="editor-empty">
            <h2>选择文件开始编辑</h2>
            <p>在左侧选择全局指令或 skill，也可新建指令、新建 skill 或添加来自 GitHub 的第三方 skill。</p>
          </div>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .custom-page { max-width: 1500px; margin: 0 auto; }
  .custom-heading,.catalog-head,.editor-head,.skills-head,.instructions-head,.editor-foot { display:flex; align-items:center; justify-content:space-between; gap:16px; }
  .head-right-actions { display:flex; align-items:center; gap:10px; }
  h1 { font-size:21px; margin:0 0 5px; color:var(--text-primary); } h2 { font-size:14px; margin:0; color:var(--text-primary); }
  p { color:var(--text-secondary); } .custom-heading p { margin:0; font-size:12px; }
  .local-label { color:var(--text-secondary); border:1px solid var(--border-subtle); border-radius:20px; padding:4px 9px; font-size:11px; white-space:nowrap; }
  .privacy-note { margin:16px 0; font-size:12px; line-height:1.5; }
  .provider-tabs { display:flex; gap:8px; flex-wrap:wrap; margin-bottom:16px; }
  .workspace { display:grid; grid-template-columns:270px minmax(0,1fr); border:1px solid var(--border-subtle); border-radius:var(--radius-sm); background:var(--bg-card); min-height:540px; overflow:hidden; }
  .catalog { border-right:1px solid var(--border-subtle); padding:14px; max-height:75vh; overflow:auto; }
  .catalog-head { font-size:13px; color:var(--text-primary); } .catalog-head button,.skills-head button,.instructions-head button { min-height:26px; padding:2px 8px; font-size:11px; }
  .group-title { margin:19px 0 7px; font-size:11px; color:var(--text-muted); font-weight:600; }
  .file-row { display:flex; flex-direction:column; align-items:flex-start; width:100%; gap:3px; text-align:left; border:1px solid transparent; border-radius:5px; background:transparent; color:var(--text-primary); padding:8px; cursor:pointer; }
  .file-row:hover,.file-row.active { background:var(--bg-secondary); border-color:var(--border-subtle); } .file-row:disabled { opacity:.55; cursor:not-allowed; }
  .file-row.row-disabled { opacity:.7; border-left:3px solid var(--text-muted); }
  .file-row strong { font-size:12px; overflow-wrap:anywhere; } .file-row span,.empty-list { font-size:11px; color:var(--text-muted); overflow-wrap:anywhere; }
  .skill-row-title { display:flex; align-items:center; gap:6px; flex-wrap:wrap; }
  .link-badge { border:1px solid var(--border-subtle); border-radius:4px; padding:1px 4px; font-size:10px; font-weight:400; color:var(--text-muted); }
  .status-badge { display:inline-block; border-radius:4px; padding:1px 5px; font-size:10px; font-weight:500; }
  .status-badge.disabled { background:rgba(239, 68, 68, 0.12); color:#ef4444; border:1px solid rgba(239, 68, 68, 0.25); }
  .status-badge.enabled { background:rgba(16, 185, 129, 0.12); color:#10b981; border:1px solid rgba(16, 185, 129, 0.25); }
  .catalog-actions,.editor-actions { display:flex; gap:6px; flex-shrink:0; flex-wrap:wrap; }
  .editor-title-row { display:flex; align-items:center; gap:8px; }
  .update-panel { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:10px 16px; border-bottom:1px solid var(--border-subtle); background:rgba(59, 130, 246, 0.04); color:var(--text-secondary); font-size:12px; overflow-wrap:anywhere; }
  .update-actions { display:flex; gap:8px; flex-shrink:0; }
  .source-hint { font-size:11px; line-height:1.5; overflow-wrap:anywhere; }
  details { color:var(--text-secondary); } summary { cursor:pointer; } pre { max-height:220px; overflow:auto; white-space:pre-wrap; overflow-wrap:anywhere; font:11px/1.5 var(--font-mono, monospace); }
  .editor { display:flex; min-width:0; flex-direction:column; } .editor-head { padding:16px; border-bottom:1px solid var(--border-subtle); }
  .editor-head p { margin:5px 0 0; font-size:11px; overflow-wrap:anywhere; } .editor-head button { flex-shrink:0; }
  textarea { flex:1; min-height:360px; resize:vertical; padding:16px; border:0; outline-offset:-3px; color:var(--text-primary); background:transparent; font:12px/1.6 var(--font-mono, monospace); }
  .editor-foot { padding:10px 16px; border-top:1px solid var(--border-subtle); font-size:11px; color:var(--text-muted); }
  .editor-empty { margin:auto; text-align:center; padding:30px; } .editor-empty p { font-size:12px; }
  .create-form { display:grid; gap:10px; max-width:480px; padding:20px; color:var(--text-secondary); font-size:12px; }
  input,select { color:var(--text-primary); background:var(--bg-secondary); border:1px solid var(--border-subtle); border-radius:5px; padding:8px; font:inherit; }
  .message { font-size:12px; padding:10px 12px; border:1px solid var(--border-subtle); border-radius:5px; margin-bottom:12px; } .message.error { color:var(--color-danger); }
  .sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; border:0; }

  /* AI config styling */
  .ai-config-panel { margin-bottom:16px; padding:14px 18px; border:1px solid var(--border-subtle); border-radius:var(--radius-sm); background:var(--bg-card); display:grid; gap:12px; }
  .ai-config-head { display:flex; justify-content:space-between; align-items:center; }
  .ai-presets-row { display:flex; align-items:center; gap:6px; flex-wrap:wrap; font-size:11px; }
  .ai-preset-chip { background:var(--bg-secondary); border:1px solid var(--border-subtle); border-radius:12px; padding:2px 8px; font-size:11px; cursor:pointer; color:var(--text-primary); }
  .ai-preset-chip:hover { border-color:var(--text-primary); }
  .ai-config-grid { display:grid; grid-template-columns:repeat(auto-fit, minmax(260px, 1fr)); gap:12px; font-size:12px; }
  .ai-config-grid label { display:flex; flex-direction:column; gap:4px; }
  .ai-model-input-group { display:flex; gap:6px; }
  .ai-model-input-group input { flex:1; }
  .ai-config-actions { display:flex; justify-content:flex-end; }

  /* Adopt panel styling */
  .adopt-panel { padding:14px 18px; background:rgba(245, 158, 11, 0.05); border-bottom:1px solid var(--border-subtle); display:grid; gap:8px; font-size:12px; }
  .adopt-title { display:flex; justify-content:space-between; align-items:center; }
  .adopt-desc { margin:0; font-size:11px; color:var(--text-muted); }
  .adopt-form { display:grid; gap:8px; max-width:540px; margin-top:4px; }
  .adopt-buttons { display:flex; gap:8px; }

  /* Summary card styling */
  .summary-card { margin:12px 16px; border:1px solid var(--border-subtle); border-radius:6px; background:var(--bg-secondary); overflow:hidden; font-size:12px; }
  .summary-header { display:flex; justify-content:space-between; align-items:center; padding:8px 12px; border-bottom:1px solid var(--border-subtle); background:rgba(0,0,0,0.02); }
  .summary-title { display:flex; align-items:center; gap:8px; font-weight:600; color:var(--text-primary); }
  .summary-meta { font-size:10px; color:var(--text-muted); font-weight:normal; }
  .summary-actions { display:flex; gap:6px; }
  .summary-body { padding:12px; display:grid; gap:10px; }
  .summary-section h4 { margin:0 0 4px; font-size:12px; color:var(--text-primary); }
  .summary-section p { margin:0; line-height:1.5; color:var(--text-secondary); }
  .summary-pre { margin:0; padding:8px; background:rgba(0,0,0,0.04); border-radius:4px; font-size:11px; line-height:1.5; color:var(--text-primary); white-space:pre-wrap; }

  .btn-compact { min-height:24px; padding:2px 8px; font-size:11px; border:1px solid var(--border-subtle); border-radius:4px; background:var(--bg-secondary); color:var(--text-primary); cursor:pointer; }
  .btn-compact:hover { background:var(--border-subtle); }

  @media(max-width:760px) { .workspace { grid-template-columns:1fr; } .catalog { border-right:0; border-bottom:1px solid var(--border-subtle); max-height:260px; } }
</style>
