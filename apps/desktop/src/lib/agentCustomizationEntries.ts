export type Provider = 'codex' | 'claude' | 'antigravity' | 'deepseek' | 'pi';
export type Id = { provider: Provider; root: string; name: string };
export type Entry = {
  id: Id;
  path: string;
  kind: 'skill' | 'instruction';
  exists: boolean;
  editable: boolean;
  builtin?: boolean;
  builtin_description?: string;
  linked: boolean;
  enabled: boolean;
  instructionEntry: boolean;
};
export type InstructionInfo = {
  label: string;
  detail: string;
  tone: 'primary' | 'entry' | 'warning' | 'auxiliary';
};

export function instructionRootLabel(rootId: string): string {
  const labels: Record<string, string> = {
    'codex-instructions': 'Codex 全局目录',
    'claude-instructions': 'Claude Code 全局目录',
    'gemini-instructions': 'Antigravity 用户目录',
    'gemini-config-instructions': 'Antigravity 配置目录',
    'dsh-instructions': 'DeepSeek Harness 用户目录',
    'dsh-shared-instructions': '共享 Agent 目录',
    'pi-instructions': 'Pi Agent 全局目录',
    'pi-skills': 'Pi Agent 用户目录',
    'pi-shared-skills': '共享 Skill 目录',
    'claude-builtin-catalog': 'Claude Code 官方目录',
    'dsh-office-builtin': 'DeepSeek Harness 安装包',
    'antigravity-app-builtin': 'Antigravity App 自带',
    'antigravity-cli-builtin': 'Antigravity CLI 自带',
    'antigravity-cli-skills': 'Antigravity CLI 用户目录',
    'antigravity-cli-legacy-skills': 'Antigravity CLI 兼容目录',
    'codex-system-skills': 'Codex 自带',
    'shared-skills': '共享 Skill 目录',
    'antigravity-skills': 'Antigravity 配置目录',
    'codex-skills': 'Codex 全局目录',
    'claude-skills': 'Claude Code 全局目录',
    'gemini-skills': 'Antigravity 用户目录',
    'gemini-config-skills': 'Antigravity 配置目录',
    'dsh-skills': 'DeepSeek Harness 用户目录',
    'dsh-shared-skills': '共享 Skill 目录',
    'agents-skills': '通用 Agent 目录',
  };
  return labels[rootId] ?? rootId;
}

export function instructionInfo(entry: Entry, instructions: Entry[]): InstructionInfo {
  if (!entry.instructionEntry) {
    return {
      label: entry.exists ? '辅助文件' : '新辅助文件',
      detail: entry.id.provider === 'pi'
        ? 'Pi 不会仅因该文件位于全局配置目录就自动加载它。'
        : '这个文件名不在工具预设的指令入口中；仅放在此目录通常不会自动加载。可通过下方开关直接以 @ 语法引用进主入口文件。',
      tone: 'auxiliary',
    };
  }

  if (entry.id.root === 'pi-instructions') {
    const name = entry.id.name;
    if (name === 'SYSTEM.md' || name === 'APPEND_SYSTEM.md') {
      return {
        label: name === 'SYSTEM.md' ? '系统提示词覆盖' : '系统提示词追加',
        detail: '这是 Pi 的全局系统提示词配置；同名的可信项目配置优先。',
        tone: 'entry',
      };
    }
    const priority = ['AGENTS.override.md', 'AGENTS.md', 'AGENTS.MD', 'CLAUDE.md', 'CLAUDE.MD'];
    const index = priority.indexOf(name);
    const higher = priority.slice(0, index).find((candidate) =>
      instructions.some((item) => item.id.root === entry.id.root && item.id.name === candidate && item.exists)
    );
    return {
      label: higher ? '被更高优先级入口覆盖' : entry.exists ? '全局上下文入口' : '入口文件未创建',
      detail: higher ? 'Pi 优先读取 ' + higher + '。' : 'Pi 在全局配置目录按优先级读取第一个存在的上下文文件。',
      tone: higher ? 'warning' : 'entry',
    };
  }

  if (entry.id.provider === 'codex' && entry.id.root === 'codex-instructions') {
    const overrideExists = instructions.some(
      (item) => item.id.root === entry.id.root && item.id.name === 'AGENTS.override.md' && item.exists
    );
    if (entry.id.name === 'AGENTS.override.md') {
      return entry.exists
        ? {
            label: '优先级最高的入口',
            detail: 'Codex 全局层优先检查 AGENTS.override.md。TokenScope 显示文件存在，但不会验证正在运行的 Codex 会话是否已重新加载。',
            tone: 'primary',
          }
        : {
            label: '可选覆盖入口',
            detail: '创建后会优先于 AGENTS.md；未创建时不影响默认入口。',
            tone: 'entry',
          };
    }
    if (entry.id.name === 'AGENTS.md') {
      if (overrideExists) {
        return {
          label: '可能被覆盖',
          detail: 'AGENTS.override.md 已存在并优先。若覆盖文件为空，Codex 可能继续回退到此文件；TokenScope 不会验证会话的实际加载结果。',
          tone: 'warning',
        };
      }
      return entry.exists
        ? {
            label: '默认全局入口',
            detail: '这是 Codex 默认的全局指令文件。修改后需由 Codex 新会话重新读取；TokenScope 不会控制其会话缓存。',
            tone: 'primary',
          }
        : {
            label: '默认入口未创建',
            detail: '创建 AGENTS.md 可作为 Codex 默认的全局指令文件。',
            tone: 'entry',
          };
    }
  }

  return entry.exists
    ? {
        label: '工具预设入口',
        detail: '此文件名和目录是 TokenScope 为该工具登记的指令入口。TokenScope 未启动对应工具验证运行时是否已加载。',
        tone: 'entry',
      }
    : {
        label: '入口文件未创建',
        detail: '这是 TokenScope 为该工具登记的入口文件，目前尚未创建。',
        tone: 'entry',
      };
}

export function isReferenceLine(line: string, targetPath: string, targetName: string): boolean {
  const trimmed = line.trim();
  if (!trimmed.startsWith('@')) return false;

  const targetPathForward = targetPath.replace(/\\/g, '/');
  const targetPathBack = targetPath.replace(/\//g, '\\');

  let ref = trimmed.slice(1).trim();
  if (
    (ref.startsWith('"') && ref.endsWith('"')) ||
    (ref.startsWith("'") && ref.endsWith("'"))
  ) {
    ref = ref.slice(1, -1).trim();
  }

  if (
    ref === targetPath ||
    ref === targetPathForward ||
    ref === targetPathBack ||
    ref === targetName ||
    ref === `./${targetName}` ||
    ref === `.\\${targetName}`
  ) {
    return true;
  }

  const linkMatch = trimmed.match(/^@\[.*?\]\((.*?)\)$/);
  if (linkMatch) {
    let linkPath = linkMatch[1].trim();
    if (
      (linkPath.startsWith('"') && linkPath.endsWith('"')) ||
      (linkPath.startsWith("'") && linkPath.endsWith("'"))
    ) {
      linkPath = linkPath.slice(1, -1).trim();
    }
    if (
      linkPath === targetPath ||
      linkPath === targetPathForward ||
      linkPath === targetPathBack ||
      linkPath === targetName ||
      linkPath === `./${targetName}` ||
      linkPath === `.\\${targetName}`
    ) {
      return true;
    }
  }

  return false;
}

export function isReferenced(fileContent: string, entry: Entry): boolean {
  const lines = fileContent.split(/\r?\n/);
  return lines.some((line) => isReferenceLine(line, entry.path, entry.id.name));
}

export function addReference(fileContent: string, refLine: string): string {
  const trimmed = fileContent.trimEnd();
  if (!trimmed) return `${refLine}\n`;
  return `${trimmed}\n\n${refLine}\n`;
}

export function removeReference(fileContent: string, entry: Entry): string {
  const lines = fileContent.split(/\r?\n/);
  const remaining = lines.filter((line) => !isReferenceLine(line, entry.path, entry.id.name));
  const joined = remaining.join('\n').trimEnd();
  return joined ? `${joined}\n` : '';
}

export function getMainEntry(auxiliaryEntry: Entry, instructions: Entry[]): Entry | null {
  const rootInstructions = instructions.filter(
    (item) => item.id.root === auxiliaryEntry.id.root && item.instructionEntry
  );
  if (rootInstructions.length === 0) return null;
  const override = rootInstructions.find((e) => e.id.name === 'AGENTS.override.md' && e.exists);
  const preferred = rootInstructions.find((e) => e.exists && (e.id.name === 'AGENTS.md' || e.id.name === 'CLAUDE.md'));
  const anyExist = rootInstructions.find((e) => e.exists);
  const defaultName = rootInstructions.find((e) => e.id.name === 'AGENTS.md' || e.id.name === 'CLAUDE.md');
  return override || preferred || anyExist || defaultName || rootInstructions[0];
}

