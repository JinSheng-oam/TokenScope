export type Provider = 'codex' | 'claude' | 'antigravity' | 'deepseek';
export type Id = { provider: Provider; root: string; name: string };
export type Entry = {
  id: Id;
  path: string;
  kind: 'skill' | 'instruction';
  exists: boolean;
  editable: boolean;
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
  };
  return labels[rootId] ?? rootId;
}

export function instructionInfo(entry: Entry, instructions: Entry[]): InstructionInfo {
  if (!entry.instructionEntry) {
    return {
      label: entry.exists ? '辅助文件' : '新辅助文件',
      detail: '这个文件名不在工具预设的指令入口中；仅放在此目录通常不会自动加载。需要时，请在工具实际读取的入口文件中引用它。',
      tone: 'auxiliary',
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

