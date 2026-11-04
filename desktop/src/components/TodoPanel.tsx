import { useState } from 'react';
import { Check, ChevronDown, ChevronRight, Circle, Loader2 } from 'lucide-react';
import type { TodoItem } from '../api';

interface TodoPanelProps {
  items: TodoItem[];
  live?: boolean;
}

const STATUS_LABEL: Record<TodoItem['status'], string> = {
  pending: 'Pending',
  in_progress: 'In progress',
  completed: 'Done',
};

/**
 * Single place todos are rendered in chat. Tool calls named todo_* are filtered
 * out of the tool rows so the list never appears twice.
 */
export function TodoPanel({ items, live = false }: TodoPanelProps) {
  const [open, setOpen] = useState(true);

  if (!items || items.length === 0) return null;

  const done = items.filter((i) => i.status === 'completed').length;
  const total = items.length;
  const pct = total === 0 ? 0 : Math.round((done / total) * 100);
  const active = items.find((i) => i.status === 'in_progress');

  return (
    <div className="my-2 max-w-full min-w-0">
      <div className="rounded-xl border border-neutral-200 bg-sand-50/60 overflow-hidden shadow-2xs">
        <button
          type="button"
          onClick={() => setOpen((v) => !v)}
          aria-expanded={open}
          className="w-full px-3 py-2 flex items-center gap-2 text-left hover:bg-sand-100/60 transition-colors"
        >
          {open ? (
            <ChevronDown className="w-3.5 h-3.5 text-neutral-400 shrink-0" />
          ) : (
            <ChevronRight className="w-3.5 h-3.5 text-neutral-400 shrink-0" />
          )}
          <span className="text-[11.5px] font-semibold text-neutral-700">Tasks</span>
          <span className="text-[10.5px] font-mono text-neutral-500 tabular-nums">
            {done}/{total}
          </span>
          {live && active && (
            <span className="text-[10.5px] text-blue-700 truncate min-w-0 flex-1">
              {active.title}
            </span>
          )}
          <span className="ml-auto text-[10px] font-mono text-neutral-500 tabular-nums">
            {pct}%
          </span>
        </button>

        <div className="h-0.5 bg-neutral-200/70">
          <div
            className="h-full bg-blue-600 transition-all duration-300"
            style={{ width: `${pct}%` }}
            role="progressbar"
            aria-valuenow={done}
            aria-valuemin={0}
            aria-valuemax={total}
          />
        </div>

        {open && (
          <ul className="py-1">
            {items.map((item) => {
              const isDone = item.status === 'completed';
              const isActive = item.status === 'in_progress';
              return (
                <li
                  key={item.id}
                  className="flex items-start gap-2 px-3 py-1.5 text-[11.5px] leading-snug"
                >
                  <span className="mt-[2px] shrink-0">
                    {isDone ? (
                      <Check className="w-3.5 h-3.5 text-blue-600" />
                    ) : isActive ? (
                      <Loader2 className="w-3.5 h-3.5 text-clay-700 animate-spin" />
                    ) : (
                      <Circle className="w-3.5 h-3.5 text-neutral-300" />
                    )}
                  </span>
                  <span
                    className={
                      isDone
                        ? 'text-neutral-400 line-through decoration-neutral-300 min-w-0 break-words'
                        : isActive
                          ? 'text-neutral-800 font-medium min-w-0 break-words'
                          : 'text-neutral-600 min-w-0 break-words'
                    }
                  >
                    {item.title}
                  </span>
                  <span className="ml-auto pl-2 shrink-0 text-[9.5px] uppercase tracking-wide text-neutral-400">
                    {STATUS_LABEL[item.status] ?? item.status}
                  </span>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
}
