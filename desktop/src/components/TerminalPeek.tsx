import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, SquareTerminal, ArrowUpRight } from 'lucide-react';

export interface TerminalEntry {
  cmd: string;
  output: string;
  isError: boolean;
}

const MAX_PREVIEW_LINES = 8;

/** Terminal tools whose output is worth peeking at inside the transcript. */
export const TERMINAL_TOOL_NAMES = [
  'terminal_exec',
  'bash_run',
  'shell_exec',
  'exec_background',
  'exec_stream_stdout',
  'ssh_exec',
  'dev_server_start',
];

export function isTerminalTool(name: string): boolean {
  return TERMINAL_TOOL_NAMES.includes(name);
}

/** Turn an arbitrary tool output value into readable terminal text. */
export function outputToText(output: unknown): string {
  if (output == null) return '';
  if (typeof output === 'string') return output;
  if (typeof output === 'object') {
    const rec = output as Record<string, unknown>;
    for (const key of ['stdout', 'output', 'text', 'result', 'content']) {
      const v = rec[key];
      if (typeof v === 'string' && v.trim()) {
        const stderr = typeof rec.stderr === 'string' ? rec.stderr : '';
        return stderr ? `${v}\n${stderr}` : v;
      }
    }
    try {
      return JSON.stringify(output, null, 2);
    } catch {
      return String(output);
    }
  }
  return String(output);
}

export function commandOf(args: unknown): string {
  if (!args || typeof args !== 'object') return '';
  const rec = args as Record<string, unknown>;
  for (const key of ['command', 'cmd', 'command_line', 'script']) {
    const v = rec[key];
    if (typeof v === 'string') return v;
  }
  return '';
}

export interface TerminalPeekProps {
  entries: TerminalEntry[];
  onOpenTerminal?: () => void;
  dense?: boolean;
}

/**
 * Compact terminal peek shown inside the chat stream: the command the agent ran
 * and the tail of its real output, with a jump to the full terminal panel.
 */
export function TerminalPeek({ entries, onOpenTerminal, dense = false }: TerminalPeekProps) {
  const [expanded, setExpanded] = useState(false);
  const last = entries[entries.length - 1];
  const totals = useMemo(() => {
    const lines = entries.reduce((acc, e) => acc + e.output.split('\n').length, 0);
    return { lines, failed: entries.filter((e) => e.isError).length };
  }, [entries]);

  if (!last) return null;

  const visible = entries.slice(expanded ? -4 : -1);
  const tail = (text: string) => {
    const lines = text.split('\n').filter((l, i, arr) => !(i === arr.length - 1 && l === ''));
    return expanded ? lines.slice(-40) : lines.slice(-MAX_PREVIEW_LINES);
  };

  return (
    <div className={`${dense ? 'my-1.5' : 'my-2'} rounded-xl overflow-hidden border border-neutral-300 bg-[#22211e] text-[#e8e4da] font-mono text-[11px] shadow-2xs`}>
      <div className="flex items-center justify-between gap-2 px-2.5 py-1.5 bg-[#2c2b27] border-b border-neutral-700">
        <div className="flex items-center gap-1.5 min-w-0">
          <SquareTerminal className="w-3 h-3 shrink-0 text-[#a9c3b3]" />
          <span className="truncate text-[#d8d3c6]" title={last.cmd}>
            {last.cmd || 'terminal'}
          </span>
        </div>
        <div className="flex items-center gap-1.5 shrink-0">
          {entries.length > 1 && (
            <span className="text-[10px] text-[#8f897a]">{entries.length} cmds</span>
          )}
          {totals.failed > 0 && (
            <span className="text-[10px] text-[#d99a8a]">{totals.failed} failed</span>
          )}
          <button
            onClick={() => setExpanded((v) => !v)}
            className="flex items-center gap-0.5 rounded px-1 py-0.5 text-[#b9b3a5] hover:bg-[#3a3934] transition-colors cursor-pointer"
            title={expanded ? 'Collapse terminal peek' : `Expand (${totals.lines} lines)`}
          >
            {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
            <span className="text-[10px]">{totals.lines}</span>
          </button>
          {onOpenTerminal && (
            <button
              onClick={onOpenTerminal}
              className="flex items-center gap-0.5 rounded px-1 py-0.5 text-[#b9b3a5] hover:bg-[#3a3934] transition-colors cursor-pointer"
              title="Open the full terminal panel"
            >
              <ArrowUpRight className="w-3 h-3" />
            </button>
          )}
        </div>
      </div>

      <div className={`px-2.5 py-1.5 ${expanded ? 'max-h-72' : 'max-h-24'} overflow-y-auto scrollbar-thin`}>
        {visible.map((entry, i) => {
          const lines = tail(entry.output);
          const hidden = expanded ? 0 : entry.output.split('\n').length - lines.length;
          return (
            <div key={i} className={i > 0 ? 'mt-2 pt-2 border-t border-neutral-700/70' : ''}>
              {expanded && entries.length > 1 && (
                <div className="text-[10px] text-[#8f897a] truncate mb-0.5">$ {entry.cmd || '(no command)'}</div>
              )}
              <pre className={`whitespace-pre-wrap break-words ${entry.isError ? 'text-[#e3a99b]' : 'text-[#d5d1c6]'}`}>
                {lines.join('\n') || '(no output)'}
              </pre>
              {hidden > 0 && (
                <button
                  onClick={() => setExpanded(true)}
                  className="mt-0.5 text-[10px] text-[#9aa79d] hover:text-[#c8d3c9] cursor-pointer"
                >
                  +{hidden} more line{hidden === 1 ? '' : 's'}
                </button>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
