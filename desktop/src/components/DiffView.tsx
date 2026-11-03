import { useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, FileDiff } from 'lucide-react';

export type DiffRow = {
  kind: 'meta' | 'hunk' | 'add' | 'del' | 'ctx';
  oldNo: number | null;
  newNo: number | null;
  text: string;
};

export interface ParsedDiff {
  files: string[];
  rows: DiffRow[];
  additions: number;
  deletions: number;
}

const HUNK_RE = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/;

/** Parse unified diff text into renderable rows with line numbers. */
export function parseUnifiedDiff(diff: string): ParsedDiff {
  const files: string[] = [];
  const rows: DiffRow[] = [];
  let additions = 0;
  let deletions = 0;
  let oldNo = 0;
  let newNo = 0;

  for (const line of diff.split('\n')) {
    if (line.startsWith('diff --git ') || line.startsWith('Index: ')) {
      const m = /b\/(.+)$/.exec(line) ?? /Index: (.+)$/.exec(line);
      if (m) files.push(m[1].trim());
      rows.push({ kind: 'meta', oldNo: null, newNo: null, text: line });
      continue;
    }
    if (line.startsWith('+++ ') || line.startsWith('--- ')) {
      const p = line.slice(4).trim().replace(/^b\//, '').replace(/^a\//, '');
      if (p && p !== '/dev/null' && !files.includes(p)) files.push(p);
      rows.push({ kind: 'meta', oldNo: null, newNo: null, text: line });
      continue;
    }
    const hunk = HUNK_RE.exec(line);
    if (hunk) {
      oldNo = Number(hunk[1]);
      newNo = Number(hunk[2]);
      rows.push({ kind: 'hunk', oldNo: null, newNo: null, text: line });
      continue;
    }
    if (line.startsWith('+')) {
      additions += 1;
      rows.push({ kind: 'add', oldNo: null, newNo: newNo++, text: line.slice(1) });
      continue;
    }
    if (line.startsWith('-')) {
      deletions += 1;
      rows.push({ kind: 'del', oldNo: oldNo++, newNo: null, text: line.slice(1) });
      continue;
    }
    if (line.startsWith('\\')) {
      rows.push({ kind: 'meta', oldNo: null, newNo: null, text: line });
      continue;
    }
    if (line.startsWith(' ') || line === '') {
      if (line === '' ) continue;
      rows.push({ kind: 'ctx', oldNo: oldNo++, newNo: newNo++, text: line.slice(1) });
    }
  }
  return { files, rows, additions, deletions };
}

const ROW_STYLE: Record<DiffRow['kind'], string> = {
  meta: 'text-[#6f6a5d] bg-[#f4f2ec]',
  hunk: 'text-[#3f5b70] bg-[#eef2f5]',
  add: 'text-[#2f332e] bg-[#edf5ee]',
  del: 'text-[#2f332e] bg-[#fbeeee]',
  ctx: 'text-[#2f332e] bg-white',
};

const MARK: Record<DiffRow['kind'], string> = {
  meta: ' ',
  hunk: ' ',
  add: '+',
  del: '-',
  ctx: ' ',
};

const MARK_STYLE: Record<DiffRow['kind'], string> = {
  meta: 'text-[#8a8577]',
  hunk: 'text-[#3f5b70]',
  add: 'text-[#2f6b3a] font-semibold',
  del: 'text-[#8f3b32] font-semibold',
  ctx: 'text-[#b9b3a5]',
};

export interface DiffViewProps {
  diff: string;
  dense?: boolean;
  /** Collapse long diffs behind a toggle. */
  collapsible?: boolean;
  maxRows?: number;
  label?: string;
}

export function DiffView({ diff, dense = false, collapsible = true, maxRows = 18, label }: DiffViewProps) {
  const parsed = useMemo(() => parseUnifiedDiff(diff), [diff]);
  const [expanded, setExpanded] = useState(!collapsible);
  const truncated = !expanded && parsed.rows.length > maxRows;
  const rows = truncated ? parsed.rows.slice(0, maxRows) : parsed.rows;
  const hidden = parsed.rows.length - rows.length;
  const title = label ?? parsed.files[0] ?? 'diff';
  const extraFiles = parsed.files.length > 1 ? ` +${parsed.files.length - 1}` : '';

  if (parsed.rows.length === 0) return null;

  return (
    <div className={`${dense ? 'my-1.5 rounded-xl' : 'my-3 rounded-2xl'} overflow-hidden border border-[#d8d3c8] bg-white shadow-2xs`}>
      <div className="flex items-center justify-between gap-3 px-3.5 py-2 bg-[#f4f2ec] border-b border-[#d8d3c8]">
        <div className="flex items-center gap-2 min-w-0">
          <FileDiff className="w-3.5 h-3.5 shrink-0 text-[#5b4636]" />
          <span className="font-mono text-[11.5px] font-semibold text-[#2f332e] truncate" title={parsed.files.join('\n')}>
            {title}
            <span className="text-[#6f6a5d]">{extraFiles}</span>
          </span>
        </div>
        <div className="flex items-center gap-2 shrink-0 font-mono text-[11px]">
          <span className="text-[#2f6b3a]">+{parsed.additions}</span>
          <span className="text-[#8f3b32]">-{parsed.deletions}</span>
          {collapsible && (
            <button
              onClick={() => setExpanded((v) => !v)}
              className="flex items-center gap-0.5 rounded-md px-1.5 py-0.5 text-[#5a5a52] hover:bg-[#e9e5dc] transition-colors cursor-pointer"
              title={expanded ? 'Collapse diff' : 'Expand full diff'}
            >
              {expanded ? <ChevronDown className="w-3 h-3" /> : <ChevronRight className="w-3 h-3" />}
              <span>{parsed.rows.length}</span>
            </button>
          )}
        </div>
      </div>

      <div className="overflow-x-auto max-w-full">
        <table className="w-full border-collapse font-mono text-[11.5px] leading-[1.55]">
          <tbody>
            {rows.map((row, i) => (
              <tr key={i} className={ROW_STYLE[row.kind]}>
                <td className="w-9 select-none text-right pr-1.5 text-[10.5px] text-[#b9b3a5] align-top pl-1">
                  {row.oldNo ?? ''}
                </td>
                <td className="w-9 select-none text-right pr-2 text-[10.5px] text-[#b9b3a5] align-top">
                  {row.newNo ?? ''}
                </td>
                <td className={`w-4 select-none text-center align-top ${MARK_STYLE[row.kind]}`}>{MARK[row.kind]}</td>
                <td className="whitespace-pre-wrap break-words pr-3 align-top">
                  {row.kind === 'hunk' || row.kind === 'meta' ? (
                    <span className="text-[#6f6a5d]">{row.text}</span>
                  ) : (
                    row.text
                  )}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {hidden > 0 && (
        <button
          onClick={() => setExpanded(true)}
          className="w-full px-3 py-1.5 bg-[#f7f5ef] border-t border-[#d8d3c8] text-[11px] text-[#5a5a52] hover:bg-[#efece4] transition-colors cursor-pointer"
        >
          Show {hidden} more line{hidden === 1 ? '' : 's'}
        </button>
      )}
    </div>
  );
}

/** Heuristic: does this text look like a unified diff worth rendering as one? */
export function looksLikeUnifiedDiff(text: string): boolean {
  return /^(diff --git |--- )/m.test(text) && /^\+\+\+ /m.test(text) && /^@@ /m.test(text);
}
