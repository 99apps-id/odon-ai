import { CheckCircle2, ChevronDown, ChevronRight, Loader2, Wrench } from 'lucide-react';
import { useState } from 'react';

interface ToolActivityBlockProps {
  toolName: string;
  finished?: boolean;
}

export function ToolActivityBlock({ toolName, finished = false }: ToolActivityBlockProps) {
  const [open, setOpen] = useState(false);

  return (
    <section className="tool-activity-block ml-7 max-w-full min-w-0 overflow-hidden rounded-lg border border-neutral-200 bg-white/75">
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-neutral-50"
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        )}
        {finished ? (
          <CheckCircle2 className="h-3.5 w-3.5 shrink-0 text-emerald-600" />
        ) : (
          <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-blue-600" />
        )}
        <Wrench className="h-3.5 w-3.5 shrink-0 text-neutral-500" />
        <span className="text-[11px] font-medium text-neutral-700">Task</span>
        <code className="min-w-0 flex-1 truncate text-[10.5px] text-neutral-500">
          {toolName}
        </code>
        <span className="text-[9px] font-mono uppercase tracking-wide text-neutral-400">
          {finished ? 'Done' : 'Running'}
        </span>
      </button>
      {open && (
        <div className="border-t border-neutral-200 bg-neutral-50/70 px-3 py-2 text-[10.5px] text-neutral-500">
          {finished ? 'Tool task completed.' : 'ODON is executing this task…'}
        </div>
      )}
    </section>
  );
}

export default ToolActivityBlock;
