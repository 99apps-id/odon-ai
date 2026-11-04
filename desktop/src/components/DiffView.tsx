import { Fragment, useMemo, useState } from 'react';
import { ChevronDown, ChevronRight, FileDiff, MessageSquarePlus, Send, X } from 'lucide-react';

/** A comment anchored to one line of a rendered diff. */
export interface DiffComment {
  /** File the line belongs to, when the diff names one. */
  file?: string;
  /** New-file line number, the anchor a reviewer usually wants. */
  line: number | null;
  /** Old-file line number, set for removed lines. */
  oldLine: number | null;
  kind: DiffRow['kind'];
  /** The raw line text, so the agent sees exactly what was commented on. */
  text: string;
  comment: string;
}

export type DiffRow = {
  kind: 'meta' | 'hunk' | 'add' | 'del' | 'ctx';
  oldNo: number | null;
  newNo: number | null;
  text: string;
  /** File this row belongs to, carried from the nearest file header. */
  file?: string;
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
  let currentFile: string | undefined;

  for (const line of diff.split('\n')) {
    if (line.startsWith('diff --git ') || line.startsWith('Index: ')) {
      const m = /b\/(.+)$/.exec(line) ?? /Index: (.+)$/.exec(line);
      if (m) {
        currentFile = m[1].trim();
        if (!files.includes(currentFile)) files.push(currentFile);
      }
      rows.push({ kind: 'meta', oldNo: null, newNo: null, text: line, file: currentFile });
      continue;
    }
    if (line.startsWith('+++ ') || line.startsWith('--- ')) {
      const p = line.slice(4).trim().replace(/^b\//, '').replace(/^a\//, '');
      if (p && p !== '/dev/null') {
        currentFile = p;
        if (!files.includes(p)) files.push(p);
      }
      rows.push({ kind: 'meta', oldNo: null, newNo: null, text: line, file: currentFile });
      continue;
    }
    const hunk = HUNK_RE.exec(line);
    if (hunk) {
      oldNo = Number(hunk[1]);
      newNo = Number(hunk[2]);
      rows.push({ kind: 'hunk', oldNo: null, newNo: null, text: line, file: currentFile });
      continue;
    }
    if (line.startsWith('+')) {
      additions += 1;
      rows.push({ kind: 'add', oldNo: null, newNo: newNo++, text: line.slice(1), file: currentFile });
      continue;
    }
    if (line.startsWith('-')) {
      deletions += 1;
      rows.push({ kind: 'del', oldNo: oldNo++, newNo: null, text: line.slice(1), file: currentFile });
      continue;
    }
    if (line.startsWith('\\')) {
      rows.push({ kind: 'meta', oldNo: null, newNo: null, text: line, file: currentFile });
      continue;
    }
    if (line.startsWith(' ') || line === '') {
      if (line === '' ) continue;
      rows.push({ kind: 'ctx', oldNo: oldNo++, newNo: newNo++, text: line.slice(1), file: currentFile });
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
  /**
   * When provided, every line gets a comment affordance. The returned value is
   * awaited; a rejection is surfaced next to the composer so a failed steer is
   * never silently dropped.
   */
  onComment?: (comment: DiffComment) => void | Promise<void>;
}

export function DiffView({ diff, dense = false, collapsible = true, maxRows = 18, label, onComment }: DiffViewProps) {
  const parsed = useMemo(() => parseUnifiedDiff(diff), [diff]);
  const [expanded, setExpanded] = useState(!collapsible);
  const [commentRow, setCommentRow] = useState<number | null>(null);
  const [commentText, setCommentText] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const truncated = !expanded && parsed.rows.length > maxRows;
  const rows = truncated ? parsed.rows.slice(0, maxRows) : parsed.rows;
  const hidden = parsed.rows.length - rows.length;
  const title = label ?? parsed.files[0] ?? 'diff';
  const extraFiles = parsed.files.length > 1 ? ` +${parsed.files.length - 1}` : '';

  if (parsed.rows.length === 0) return null;

  async function submitComment(row: DiffRow) {
    const body = commentText.trim();
    if (!body || !onComment) return;
    setSending(true);
    setError(null);
    try {
      await onComment({
        file: row.file,
        line: row.newNo,
        oldLine: row.oldNo,
        kind: row.kind,
        text: row.text,
        comment: body,
      });
      setCommentRow(null);
      setCommentText('');
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Comment could not be sent');
    } finally {
      setSending(false);
    }
  }

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
            {rows.map((row, i) => {
              const commentable = Boolean(onComment) && (row.kind === 'add' || row.kind === 'del' || row.kind === 'ctx');
              return (
                <Fragment key={i}>
                  <tr className={`group ${ROW_STYLE[row.kind]}`}>
                    <td className="w-9 select-none text-right pr-1.5 text-[10.5px] text-[#b9b3a5] align-top pl-1">
                      {row.oldNo ?? ''}
                    </td>
                    <td className="w-9 select-none text-right pr-2 text-[10.5px] text-[#b9b3a5] align-top">
                      {row.newNo ?? ''}
                    </td>
                    <td className={`w-4 select-none text-center align-top ${MARK_STYLE[row.kind]}`}>{MARK[row.kind]}</td>
                    <td className="whitespace-pre-wrap break-words pr-1 align-top">
                      {row.kind === 'hunk' || row.kind === 'meta' ? (
                        <span className="text-[#6f6a5d]">{row.text}</span>
                      ) : (
                        row.text
                      )}
                    </td>
                    <td className="w-6 align-top pr-1">
                      {commentable && (
                        <button
                          type="button"
                          onClick={() => {
                            setCommentRow(commentRow === i ? null : i);
                            setCommentText('');
                            setError(null);
                          }}
                          title="Comment on this line"
                          aria-label="Comment on this line"
                          className="mt-0.5 rounded p-0.5 text-[#8a8577] opacity-0 transition-opacity group-hover:opacity-100 focus-visible:opacity-100 hover:bg-[#e9e5dc] hover:text-[#3f5b70] cursor-pointer"
                        >
                          <MessageSquarePlus className="w-3 h-3" />
                        </button>
                      )}
                    </td>
                  </tr>
                  {commentRow === i && (
                    <tr className="bg-[#f7f5ef]">
                      <td colSpan={5} className="px-2 py-1.5">
                        <div className="flex items-start gap-1.5">
                          <textarea
                            autoFocus
                            value={commentText}
                            onChange={(e) => setCommentText(e.target.value)}
                            onKeyDown={(e) => {
                              if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
                                e.preventDefault();
                                void submitComment(row);
                              }
                              if (e.key === 'Escape') setCommentRow(null);
                            }}
                            rows={2}
                            placeholder="Ask the agent to change this line…"
                            className="min-w-0 flex-1 resize-y rounded-md border border-[#d8d3c8] bg-white px-2 py-1 font-sans text-[11.5px] text-[#2f332e] placeholder:text-[#a8a294] focus:border-[#3f5b70] focus:outline-none"
                          />
                          <button
                            type="button"
                            onClick={() => void submitComment(row)}
                            disabled={sending || commentText.trim().length === 0}
                            className="rounded-md bg-[#3f5b70] p-1.5 text-white transition-colors hover:bg-[#33495a] disabled:opacity-40 cursor-pointer disabled:cursor-not-allowed"
                            title="Send as steer (Ctrl+Enter)"
                          >
                            <Send className="w-3 h-3" />
                          </button>
                          <button
                            type="button"
                            onClick={() => setCommentRow(null)}
                            className="rounded-md p-1.5 text-[#8a8577] transition-colors hover:bg-[#e9e5dc] cursor-pointer"
                            title="Cancel"
                          >
                            <X className="w-3 h-3" />
                          </button>
                        </div>
                        {error && <p className="mt-1 font-sans text-[11px] text-[#8f3b32]">{error}</p>}
                      </td>
                    </tr>
                  )}
                </Fragment>
              );
            })}
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
