import { useMemo, useState } from 'react';
import { ChevronRight, FileDiff } from 'lucide-react';
import { DiffView, parseUnifiedDiff, type DiffComment } from './DiffView';

export interface ChangedFile {
  id: string;
  title: string;
  diff: string;
}

interface Props {
  files: ChangedFile[];
  /** Forwarded to each expanded diff so a line can be commented on. */
  onComment?: (comment: DiffComment) => void | Promise<void>;
}

/**
 * One card for the whole run, the way Orca reads a turn back: how many files
 * changed and the net line delta, with each file expandable to its own diff.
 *
 * The alternative — a diff card per artifact — grows the transcript without
 * bound and repeats the same summary, so a session that touched six files ends
 * with six cards saying nothing about the run as a whole.
 */
export function ChangeRollup({ files, onComment }: Props) {
  const [openId, setOpenId] = useState<string | null>(null);

  const parsed = useMemo(
    () =>
      files.map((file) => {
        const p = parseUnifiedDiff(file.diff);
        const name = p.files[0] ?? file.title;
        return { ...file, name, additions: p.additions, deletions: p.deletions };
      }),
    [files],
  );

  const totalAdd = parsed.reduce((sum, f) => sum + f.additions, 0);
  const totalDel = parsed.reduce((sum, f) => sum + f.deletions, 0);

  if (parsed.length === 0) return null;

  return (
    <div className="rounded-lg border border-neutral-200 bg-white">
      <div className="flex items-center gap-2 border-b border-neutral-200 px-2.5 py-1.5">
        <FileDiff className="h-3.5 w-3.5 shrink-0 text-neutral-500" />
        <span className="text-[11.5px] font-medium text-neutral-700">
          {parsed.length} {parsed.length === 1 ? 'file changed' : 'files changed'}
        </span>
        <span className="ml-auto flex items-center gap-2 font-mono text-[11px]">
          <span className="text-emerald-700">+{totalAdd}</span>
          <span className="text-clay-700">−{totalDel}</span>
        </span>
      </div>

      <div className="flex flex-col">
        {parsed.map((file) => {
          const open = openId === file.id;
          return (
            <div key={file.id} className="border-b border-neutral-100 last:border-b-0">
              <button
                type="button"
                onClick={() => setOpenId(open ? null : file.id)}
                className="flex w-full items-center gap-2 px-2.5 py-1.5 text-left hover:bg-neutral-200"
              >
                <ChevronRight
                  className={`h-3 w-3 shrink-0 text-neutral-400 transition-transform ${open ? 'rotate-90' : ''}`}
                />
                <span className="min-w-0 flex-1 truncate font-mono text-[11px] text-neutral-700">
                  {file.name}
                </span>
                <span className="flex shrink-0 items-center gap-1.5 font-mono text-[10.5px]">
                  <span className="text-emerald-700">+{file.additions}</span>
                  <span className="text-clay-700">−{file.deletions}</span>
                </span>
              </button>
              {open && (
                <div className="px-2 pb-2">
                  <DiffView diff={file.diff} maxRows={40} onComment={onComment} />
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}
