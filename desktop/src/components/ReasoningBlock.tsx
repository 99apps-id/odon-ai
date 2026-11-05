import { BrainCircuit, ChevronDown, ChevronRight } from 'lucide-react';
import { useEffect, useRef, useState } from 'react';
import { MarkdownView } from './MarkdownView';

interface ReasoningBlockProps {
  content: string;
  live?: boolean;
}

export function ReasoningBlock({ content, live = false }: ReasoningBlockProps) {
  const [open, setOpen] = useState(live);
  const bodyRef = useRef<HTMLDivElement>(null);
  // Follow the newest reasoning token, but stop the moment the operator
  // scrolls up: yanking the viewport back down while they read is worse than
  // a stale frame.
  const stickToBottomRef = useRef(true);

  useEffect(() => {
    const el = bodyRef.current;
    if (!el || !open || !stickToBottomRef.current) return;
    el.scrollTop = el.scrollHeight;
  }, [content, open]);

  return (
    <section
      className={`reasoning-block ml-7 max-w-full min-w-0 overflow-hidden rounded-xl border ${
        live
          ? 'border-blue-200/80 bg-blue-50/45'
          : 'border-neutral-200/80 bg-neutral-50/80'
      }`}
      aria-label={live ? 'ODON reasoning' : 'Reasoning and thought process'}
    >
      <button
        type="button"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        className="flex w-full items-center gap-2 border-b border-inherit px-3 py-2 text-left transition-colors hover:bg-white/50"
      >
        {open ? (
          <ChevronDown className="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        ) : (
          <ChevronRight className="h-3.5 w-3.5 shrink-0 text-neutral-400" />
        )}
        <span className={`odon-reasoning-mark ${live ? 'is-live' : ''}`}>
          <BrainCircuit className="h-3.5 w-3.5" />
        </span>
        <span className="text-[12.5px] font-semibold text-neutral-700">
          {live ? 'ODON is reasoning…' : 'Reasoning & thought process'}
        </span>
        {live && (
          <span className="ml-auto rounded-full bg-blue-100/80 px-1.5 py-0.5 text-[9px] font-mono uppercase tracking-wide text-blue-700">
            Live
          </span>
        )}
      </button>
      {open && (
        <div
          ref={bodyRef}
          onScroll={(event) => {
            const el = event.currentTarget;
            stickToBottomRef.current =
              el.scrollHeight - el.scrollTop - el.clientHeight < 24;
          }}
          className="max-h-80 overflow-y-auto bg-white/45 px-3 py-2 text-[12.5px] leading-relaxed text-neutral-600 select-text break-words"
        >
          {content ? (
            <MarkdownView content={content} dense preserveLineBreaks />
          ) : (
            <span className="text-neutral-400">Thinking through the next step…</span>
          )}
        </div>
      )}
    </section>
  );
}

export default ReasoningBlock;
