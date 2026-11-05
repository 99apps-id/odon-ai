import React, { useMemo, useRef } from 'react';
import { TOKEN_CLASS, languageFamily, tokenizeLine } from '../lib/highlight';

interface CodeEditorProps {
  value: string;
  onChange?: (value: string) => void;
  /** File extension or language id; unknown values render as plain text. */
  language?: string;
  readOnly?: boolean;
  /** Ctrl/Cmd+S handler. Without it the shortcut is ignored. */
  onSave?: () => void;
  className?: string;
}

/** Tokenising a pasted log or minified bundle line-by-line on every keystroke
 *  is what turns an editor into a slideshow. Above this size the text stays
 *  editable but uncoloured. */
const MAX_HIGHLIGHT_LINES = 4_000;

/**
 * Editable code surface with syntax colours.
 *
 * A transparent textarea sits on top of a highlighted `<pre>`; both share the
 * same font, padding and line height, and scrolling is mirrored, so the caret
 * lands exactly on the coloured text. This keeps native editing (selection,
 * IME, undo, accessibility) instead of reimplementing a contentEditable.
 */
export function CodeEditor({
  value,
  onChange,
  language = 'text',
  readOnly = false,
  onSave,
  className = '',
}: CodeEditorProps) {
  const preRef = useRef<HTMLPreElement>(null);
  const gutterRef = useRef<HTMLDivElement>(null);
  const textareaRef = useRef<HTMLTextAreaElement>(null);
  const family = languageFamily(language);
  const lines = useMemo(() => value.split('\n'), [value]);
  const highlightable = lines.length <= MAX_HIGHLIGHT_LINES;

  const renderedLines = useMemo(() => {
    if (!highlightable) return [];
    return lines.map((line) => {
      if (!line) return null;
      return tokenizeLine(line, family).map((token, tokenIndex) => (
        <span key={tokenIndex} className={TOKEN_CLASS[token.kind]}>
          {token.text}
        </span>
      ));
    });
  }, [lines, family, highlightable]);

  const syncScroll = () => {
    const textarea = textareaRef.current;
    if (!textarea) return;
    if (preRef.current) {
      preRef.current.scrollTop = textarea.scrollTop;
      preRef.current.scrollLeft = textarea.scrollLeft;
    }
    if (gutterRef.current) {
      gutterRef.current.scrollTop = textarea.scrollTop;
    }
  };

  const handleKeyDown = (event: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
      event.preventDefault();
      onSave?.();
      return;
    }
    if (event.key === 'Tab' && !readOnly) {
      event.preventDefault();
      const target = event.currentTarget;
      const start = target.selectionStart;
      const end = target.selectionEnd;
      onChange?.(`${value.slice(0, start)}  ${value.slice(end)}`);
      // The value is controlled, so restore the caret after React re-renders.
      window.requestAnimationFrame(() => {
        target.selectionStart = start + 2;
        target.selectionEnd = start + 2;
      });
    }
  };

  return (
    <div
      className={`flex h-full min-h-0 w-full overflow-hidden bg-white font-mono text-[13px] leading-6 text-neutral-800 ${className}`}
      // Ligatures and tab width must be identical in the textarea and the
      // highlighted layer, or the caret drifts away from the coloured glyphs.
      style={{ fontVariantLigatures: 'none', tabSize: 2 }}
    >
      {highlightable && (
        <div
          ref={gutterRef}
          aria-hidden
          className="w-11 shrink-0 select-none overflow-hidden border-r border-neutral-200 bg-neutral-50 py-3 text-right text-[12px] leading-6 text-neutral-400"
        >
          {lines.map((_, index) => (
            <div key={index} className="min-h-6 pr-2">
              {index + 1}
            </div>
          ))}
        </div>
      )}

      <div className="relative min-w-0 flex-1">
        <pre
          ref={preRef}
          aria-hidden
          className="pointer-events-none absolute inset-0 m-0 overflow-hidden whitespace-pre px-3 py-3 font-mono text-[13px] leading-6"
        >
          {highlightable
            ? renderedLines.map((line, index) => (
                <div key={index} className="min-h-6">
                  {line ?? '\u00a0'}
                </div>
              ))
            : `${value}\n`}
        </pre>

        <textarea
          ref={textareaRef}
          value={value}
          onChange={(event) => onChange?.(event.target.value)}
          onScroll={syncScroll}
          onKeyDown={handleKeyDown}
          readOnly={readOnly}
          spellCheck={false}
          autoCapitalize="off"
          autoCorrect="off"
          wrap="off"
          aria-label="Code editor"
          className="relative block h-full w-full resize-none overflow-auto whitespace-pre bg-transparent px-3 py-3 font-mono text-[13px] leading-6 outline-none"
          style={{
            color: 'transparent',
            caretColor: '#1f2937',
          }}
        />
      </div>
    </div>
  );
}
