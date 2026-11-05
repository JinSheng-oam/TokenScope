<script lang="ts">
  import { onMount } from 'svelte';
  import { invoke } from '@tauri-apps/api/core';

  export let runtimeAvailable: boolean;
  export let dirty = false;
  type Provider = 'codex' | 'claude' | 'antigravity' | 'deepseek';
  type Id = { provider: Provider; root: string; name: string };
  type Entry = { id: Id; path: string; kind: 'skill' | 'instruction'; exists: boolean; editable: boolean; linked: boolean };
  type Source = { url: string; branch: string | null; subpath: string | null; skill: string | null };
  type Inspection = { source: Source; revision: string; candidates: { path: string; name: string; preview: string }[] };
  type UpdateInfo = { kind: string; source: string | null; status: string; detail: string };
  const providers: { id: Provider; label: string; roots: { id: string; label: string }[] }[] = [
    { id: 'codex', label: 'Codex', roots: [{ id: 'shared-skills', label: '共享 skills' }, { id: 'codex-skills', label: 'Codex skills' }] },
    { id: 'claude', label: 'Claude Code', roots: [{ id: 'claude-skills', label: 'Claude skills' }] },
    { id: 'antigravity', label: 'Antigravity', roots: [{ id: 'antigravity-skills', label: 'Antigravity skills' }] },
    { id: 'deepseek', label: 'DeepSeek Harness', roots: [{ id: 'dsh-skills', label: 'DeepSeek skills' }, { id: 'dsh-shared-skills', label: '共享 skills' }] },
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
  let importing = false;
  let sourceInput = '';
  let inspection: Inspection | null = null;
  let candidatePath = '';
  let updateInfo: UpdateInfo | null = null;
  let skillName = '';
  let skillRoot = providers[0].roots[0].id;
  let request = 0;
  $: dirty = selected !== null && content !== (original ?? '');
  $: instructions = entries.filter((entry) => entry.kind === 'instruction');
  $: skills = entries.filter((entry) => entry.kind === 'skill');
  $: currentProvider = providers.find((p) => p.id === provider)!;

  function canLeave(): boolean {
    return !dirty || window.confirm('当前文件有未保存的更改，放弃更改？');
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
    provider = next; selected = null; content = ''; original = null; creating = false; importing = false; updateInfo = null; notice = '';
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
      selected = entry; original = value; content = value ?? ''; creating = false; importing = false; updateInfo = null;
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
  async function createSkill() {
    if (!/^[a-z0-9]+(?:-[a-z0-9]+)*$/.test(skillName) || skillName.length > 64) {
      error = '名称使用小写字母、数字和连字符，最长 64 个字符。'; return;
    }
    const id: Id = { provider, root: skillRoot, name: skillName };
    busy = true; error = '';
    try {
      const existing = await invoke<string | null>('read_customization', { id });
      if (existing !== null) { error = '这个 skill 已存在，请在列表中打开。'; return; }
      selected = { id, path: `${currentProvider.roots.find((r) => r.id === skillRoot)?.label} / ${skillName} / SKILL.md`, kind: 'skill', exists: false, editable: true, linked: false };
      original = null;
      content = `---\nname: ${skillName}\ndescription: 描述何时使用此 skill。\n---\n\n# ${skillName}\n\n`;
      creating = false; notice = '编辑后点击保存，才会创建文件。';
    } catch (e) { error = String(e); }
    finally { busy = false; }
  }
  function beginCreate() {
    if (!canLeave()) return;
    selected = null; original = null; content = ''; skillName = ''; creating = true; importing = false; updateInfo = null; error = ''; notice = '';
  }
  function beginImport() {
    if (!canLeave()) return;
    selected = null; original = null; content = ''; sourceInput = ''; inspection = null; candidatePath = ''; skillName = ''; importing = true; creating = false; updateInfo = null; notice = ''; error = '';
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
  async function checkUpdate() {
    if (!selected || dirty) return;
    busy = true; error = ''; notice = ''; updateInfo = null;
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
  onMount(() => { void refresh(); });
</script>

<div class="custom-page">
  <div class="custom-heading">
    <div><h1>Agent 配置</h1><p>管理本机编码工具的全局指令和 skills。</p></div>
    <span class="local-label">仅本机</span>
  </div>
  <p class="privacy-note">内容直接读写工具原有文件，不进入 TokenScope 数据库或同步。软链接会编辑其实际目标，多个工具共用该目标时请先确认影响范围。</p>
  <div class="provider-tabs" role="group" aria-label="选择工具">
    {#each providers as item}
      <button class="btn-secondary" class:selected={provider === item.id} aria-pressed={provider === item.id} disabled={busy} onclick={() => void switchProvider(item.id)}>{item.label}</button>
    {/each}
  </div>
  {#if !runtimeAvailable}<div class="empty-box">请在 TokenScope 桌面应用中管理本地文件。</div>
  {:else}
    {#if error}<p class="message error" role="alert">{error}</p>{/if}
    {#if notice}<p class="message" role="status">{notice}</p>{/if}
    <div class="workspace">
      <section class="catalog" aria-label="文件列表">
        <div class="catalog-head"><strong>文件</strong><button class="btn-secondary" disabled={busy} onclick={() => void refresh()}>刷新</button></div>
        <div class="group-title">全局指令</div>
        {#each instructions as entry}
          <button class="file-row" class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name} disabled={busy || !entry.editable} onclick={() => void select(entry)}>
            <strong>{entry.id.name}</strong><span>{entry.exists ? entry.id.root : '尚未创建'}</span>
          </button>
        {/each}
        <div class="group-title skills-head"><span>Skills · {skills.length}</span><div class="catalog-actions"><button class="btn-secondary" disabled={busy} onclick={beginImport}>添加</button><button class="btn-secondary" disabled={busy} onclick={beginCreate}>新建</button></div></div>
        {#each skills as entry}
          <button class="file-row" class:active={selected?.id.root === entry.id.root && selected?.id.name === entry.id.name} disabled={busy || !entry.editable} title={entry.editable ? entry.path : '符号链接请在原位置管理'} onclick={() => void select(entry)}>
            <strong>{entry.id.name}{#if entry.linked} <span class="link-badge">软链接</span>{/if}</strong><span>{entry.editable ? entry.id.root : '无法访问目标文件'}</span>
          </button>
        {:else}<p class="empty-list">尚未发现全局 skill。</p>{/each}
      </section>
      <section class="editor" aria-label="文件编辑器">
        {#if importing}
          <div class="editor-head"><div><h2>从来源添加 skill</h2><p>支持 skills.sh 页面和 GitHub 仓库或子目录；只下载文件，不执行脚本。</p></div></div>
          <div class="create-form">
            <label for="source-url">来源地址</label><input id="source-url" bind:value={sourceInput} oninput={() => inspection = null} placeholder="https://skills.sh/owner/repo/skill" />
            <div><button class="btn-secondary" disabled={busy || !sourceInput.trim()} onclick={() => void inspectSource()}>{busy ? '读取中…' : '预览来源'}</button></div>
            {#if inspection}
              <label for="source-skill">选择 skill</label><select id="source-skill" bind:value={candidatePath} onchange={chooseCandidate}>{#each inspection.candidates as item}<option value={item.path}>{item.name} · {item.path || '仓库根目录'}</option>{/each}</select>
              <details><summary>查看 SKILL.md 预览</summary><pre>{inspection.candidates.find((c) => c.path === candidatePath)?.preview}</pre></details>
              <label for="import-root">安装位置</label><select id="import-root" bind:value={skillRoot}>{#each currentProvider.roots as root}<option value={root.id}>{root.label}</option>{/each}</select>
              <label for="import-name">安装名称</label><input id="import-name" bind:value={skillName} maxlength="64" />
              <p class="source-hint">{inspection.source.url} · {inspection.revision.slice(0, 12)}。请确认来源可信，skill 可能包含脚本。</p>
              <div><button class="btn-primary" disabled={busy} onclick={() => void installSource()}>安装选中 skill</button> <button class="btn-secondary" onclick={() => importing = false}>取消</button></div>
            {:else}<div><button class="btn-secondary" onclick={() => importing = false}>取消</button></div>{/if}
          </div>
        {:else if creating}
          <div class="editor-head"><div><h2>新建 skill</h2><p>创建一个包含 SKILL.md 的目录。</p></div></div>
          <div class="create-form">
            <label for="skill-root">保存位置</label><select id="skill-root" bind:value={skillRoot}>{#each currentProvider.roots as root}<option value={root.id}>{root.label}</option>{/each}</select>
            <label for="skill-name">名称</label><input id="skill-name" bind:value={skillName} placeholder="例如 code-review" maxlength="64" />
            <div><button class="btn-primary" disabled={busy} onclick={() => void createSkill()}>开始编辑</button> <button class="btn-secondary" onclick={() => creating = false}>取消</button></div>
          </div>
        {:else if selected}
          <div class="editor-head"><div><h2>{selected.id.name}{selected.kind === 'skill' ? ' / SKILL.md' : ''}{#if selected.linked} <span class="link-badge">软链接目标</span>{/if}</h2><p title={selected.path}>{selected.path}</p></div><div class="editor-actions">{#if selected.kind === 'skill' && selected.exists}<button class="btn-secondary" disabled={busy || dirty} onclick={() => void checkUpdate()}>检查更新</button>{/if}<button class="btn-primary" disabled={busy || (!dirty && selected.exists)} onclick={() => void save()}>{busy ? '处理中…' : selected.exists ? '保存更改' : '创建文件'}</button></div></div>
          {#if updateInfo}<div class="update-panel"><span>{updateInfo.detail}{#if updateInfo.source} · {updateInfo.source}{/if}</span>{#if updateInfo.kind === 'manager' || updateInfo.status === 'available'}<button class="btn-secondary" disabled={busy} onclick={() => void applyUpdate()}>更新 skill</button>{/if}</div>{/if}
          <label class="sr-only" for="agent-editor">文件内容</label>
          <textarea id="agent-editor" spellcheck="false" bind:value={content} disabled={busy}></textarea>
          <div class="editor-foot"><span>{dirty ? '有未保存的更改' : selected.exists ? '已保存' : '文件尚未创建'}</span><span>UTF-8 · 最大 1 MiB</span></div>
        {:else}
          <div class="editor-empty"><h2>选择文件开始编辑</h2><p>在左侧选择全局指令或 skill，也可以新建 skill。</p></div>
        {/if}
      </section>
    </div>
  {/if}
</div>

<style>
  .custom-page { max-width: 1500px; margin: 0 auto; }
  .custom-heading,.catalog-head,.editor-head,.skills-head,.editor-foot { display:flex; align-items:center; justify-content:space-between; gap:16px; }
  h1 { font-size:21px; margin:0 0 5px; color:var(--text-primary); } h2 { font-size:14px; margin:0; color:var(--text-primary); }
  p { color:var(--text-secondary); } .custom-heading p { margin:0; font-size:12px; }
  .local-label { color:var(--text-secondary); border:1px solid var(--border-subtle); border-radius:20px; padding:4px 9px; font-size:11px; white-space:nowrap; }
  .privacy-note { margin:16px 0; font-size:12px; line-height:1.5; }
  .provider-tabs { display:flex; gap:8px; flex-wrap:wrap; margin-bottom:16px; }
  .workspace { display:grid; grid-template-columns:260px minmax(0,1fr); border:1px solid var(--border-subtle); border-radius:var(--radius-sm); background:var(--bg-card); min-height:520px; overflow:hidden; }
  .catalog { border-right:1px solid var(--border-subtle); padding:14px; max-height:75vh; overflow:auto; }
  .catalog-head { font-size:13px; color:var(--text-primary); } .catalog-head button,.skills-head button { min-height:26px; padding:2px 8px; font-size:11px; }
  .group-title { margin:19px 0 7px; font-size:11px; color:var(--text-muted); font-weight:600; }
  .file-row { display:flex; flex-direction:column; align-items:flex-start; width:100%; gap:3px; text-align:left; border:1px solid transparent; border-radius:5px; background:transparent; color:var(--text-primary); padding:8px; cursor:pointer; }
  .file-row:hover,.file-row.active { background:var(--bg-secondary); border-color:var(--border-subtle); } .file-row:disabled { opacity:.55; cursor:not-allowed; }
  .file-row strong { font-size:12px; overflow-wrap:anywhere; } .file-row span,.empty-list { font-size:11px; color:var(--text-muted); overflow-wrap:anywhere; }
  .file-row .link-badge { display:inline-block; border:1px solid var(--border-subtle); border-radius:4px; padding:1px 4px; font-size:10px; font-weight:400; }
  .link-badge { color:var(--text-muted); font-size:10px; font-weight:400; }
  .catalog-actions,.editor-actions { display:flex; gap:6px; flex-shrink:0; }
  .update-panel { display:flex; justify-content:space-between; align-items:center; gap:12px; padding:10px 16px; border-bottom:1px solid var(--border-subtle); color:var(--text-secondary); font-size:12px; overflow-wrap:anywhere; }
  .source-hint { font-size:11px; line-height:1.5; overflow-wrap:anywhere; }
  details { color:var(--text-secondary); } summary { cursor:pointer; } pre { max-height:220px; overflow:auto; white-space:pre-wrap; overflow-wrap:anywhere; font:11px/1.5 var(--font-mono, monospace); }
  .editor { display:flex; min-width:0; flex-direction:column; } .editor-head { padding:16px; border-bottom:1px solid var(--border-subtle); }
  .editor-head p { margin:5px 0 0; font-size:11px; overflow-wrap:anywhere; } .editor-head button { flex-shrink:0; }
  textarea { flex:1; min-height:420px; resize:vertical; padding:16px; border:0; outline-offset:-3px; color:var(--text-primary); background:transparent; font:12px/1.6 var(--font-mono, monospace); }
  .editor-foot { padding:10px 16px; border-top:1px solid var(--border-subtle); font-size:11px; color:var(--text-muted); }
  .editor-empty { margin:auto; text-align:center; padding:30px; } .editor-empty p { font-size:12px; }
  .create-form { display:grid; gap:10px; max-width:440px; padding:20px; color:var(--text-secondary); font-size:12px; }
  input,select { color:var(--text-primary); background:var(--bg-secondary); border:1px solid var(--border-subtle); border-radius:5px; padding:8px; font:inherit; }
  .message { font-size:12px; padding:10px 12px; border:1px solid var(--border-subtle); border-radius:5px; } .message.error { color:var(--color-danger); }
  .sr-only { position:absolute; width:1px; height:1px; padding:0; margin:-1px; overflow:hidden; clip:rect(0,0,0,0); white-space:nowrap; border:0; }
  @media(max-width:760px) { .workspace { grid-template-columns:1fr; } .catalog { border-right:0; border-bottom:1px solid var(--border-subtle); max-height:260px; } }
</style>
