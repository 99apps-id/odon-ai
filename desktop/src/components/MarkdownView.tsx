import React, { useState } from 'react';
import { api } from '../api';
import {
  Check,
  Copy,
  Download,
  ExternalLink,
  Image as ImageIcon,
  Play,
  Terminal,
  Globe,
  Info,
  Sparkles,
  AlertCircle,
  AlertTriangle,
  ShieldAlert,
  ChevronRight,
  Code2,
} from 'lucide-react';

interface MarkdownViewProps {
  content: string;
  className?: string;
  dense?: boolean;
  onRunTerminal?: (cmd: string) => void;
  onOpenBrowser?: (url: string) => void;
}

export const MarkdownView: React.FC<MarkdownViewProps> = ({
  content,
  className = '',
  dense = false,
  onRunTerminal,
  onOpenBrowser,
}) => {
  const triggerRunTerminal = (cmd: string) => {
    if (onRunTerminal) {
      onRunTerminal(cmd);
    } else {
      window.dispatchEvent(new CustomEvent('odon:run-terminal', { detail: { command: cmd, focusPanel: true } }));
    }
  };

  const triggerOpenBrowser = (url: string) => {
    if (onOpenBrowser) {
      onOpenBrowser(url);
    } else {
      window.dispatchEvent(new CustomEvent('odon:open-browser', { detail: { url } }));
    }
  };

  // High-fidelity Markdown & GFM Block Parser
  const renderFormatted = (text: string) => {
    if (!text) return null;

    const rawLines = text.split('\n');
    const elements: React.ReactNode[] = [];

    let i = 0;
    while (i < rawLines.length) {
      const line = rawLines[i];
      const trimmed = line.trim();

      // 1. Code Blocks (```)
      if (trimmed.startsWith('```')) {
        const lang = trimmed.replace(/^```/, '').trim() || 'text';
        const codeLines: string[] = [];
        i++;
        while (i < rawLines.length && !rawLines[i].trim().startsWith('```')) {
          codeLines.push(rawLines[i]);
          i++;
        }
        i++; // skip closing ```
        const codeString = codeLines.join('\n');
        elements.push(
          <CodeBlock
            key={`code-${i}`}
            language={lang}
            code={codeString}
            dense={dense}
            onRunTerminal={triggerRunTerminal}
            onOpenBrowser={triggerOpenBrowser}
          />
        );
        continue;
      }

      // 2. Standalone Markdown Image ![alt](url)
      const imgMatch = trimmed.match(/^!\[(.*?)\]\((.*?)\)$/);
      if (imgMatch) {
        elements.push(<MarkdownImage key={`img-${i}`} alt={imgMatch[1]} src={imgMatch[2]} />);
        i++;
        continue;
      }

      // 3. Horizontal Rule (---, ***, ___)
      if (/^(\*\*\*|---|___)$/.test(trimmed)) {
        elements.push(<hr key={`hr-${i}`} className={dense ? "my-1.5 border-neutral-200/60" : "my-3.5 border-neutral-200/80"} />);
        i++;
        continue;
      }

      // 4. Tables (| col 1 | col 2 |)
      if (trimmed.startsWith('|') && trimmed.endsWith('|') && i + 1 < rawLines.length && rawLines[i + 1].includes('---')) {
        const tableLines: string[] = [];
        while (i < rawLines.length && rawLines[i].trim().startsWith('|') && rawLines[i].trim().endsWith('|')) {
          tableLines.push(rawLines[i].trim());
          i++;
        }
        elements.push(<TableBlock key={`table-${i}`} lines={tableLines} dense={dense} onOpenBrowser={triggerOpenBrowser} />);
        continue;
      }

      // 5. GitHub Alert Banners (> [!NOTE], > [!TIP], > [!IMPORTANT], > [!WARNING], > [!CAUTION])
      const alertMatch = trimmed.match(/^>\s*\[!(NOTE|TIP|IMPORTANT|WARNING|CAUTION)\]/i);
      if (alertMatch) {
        const alertType = alertMatch[1].toUpperCase();
        const alertLines: string[] = [];
        i++;
        while (i < rawLines.length && rawLines[i].trim().startsWith('>')) {
          alertLines.push(rawLines[i].trim().replace(/^>\s*/, ''));
          i++;
        }
        elements.push(
          <AlertBlock
            key={`alert-${i}`}
            type={alertType as any}
            content={alertLines.join(' ')}
            dense={dense}
            onOpenBrowser={triggerOpenBrowser}
          />
        );
        continue;
      }

      // 6. Standard Blockquotes (> text)
      if (trimmed.startsWith('>')) {
        const quoteLines: string[] = [];
        while (i < rawLines.length && rawLines[i].trim().startsWith('>')) {
          quoteLines.push(rawLines[i].trim().replace(/^>\s*/, ''));
          i++;
        }
        elements.push(
          <blockquote
            key={`quote-${i}`}
            className={
              dense
                ? "border-l-2 border-neutral-300 dark:border-neutral-600 pl-2.5 py-0.5 my-1 bg-neutral-100/40 dark:bg-neutral-800/30 rounded-r-lg italic text-[10.5px] text-neutral-600 dark:text-neutral-400 leading-snug"
                : "border-l-3 border-neutral-300 dark:border-neutral-600 pl-3.5 py-1 my-2 bg-neutral-50/70 dark:bg-neutral-800/40 rounded-r-xl italic text-[13px] text-neutral-700 dark:text-neutral-300 leading-relaxed"
            }
          >
            <span dangerouslySetInnerHTML={{ __html: formatInline(quoteLines.join(' '), triggerOpenBrowser, dense) }} />
          </blockquote>
        );
        continue;
      }

      // 7. Headings (#, ##, ###, ####)
      if (trimmed.startsWith('#')) {
        const hMatch = trimmed.match(/^(#{1,6})\s+(.*)$/);
        if (hMatch) {
          const level = hMatch[1].length;
          const textContent = hMatch[2];
          if (level === 1) {
            elements.push(
              <h1 key={`h1-${i}`} className={dense ? "text-[12px] font-semibold text-neutral-800 dark:text-neutral-200 mt-2 mb-1 tracking-tight" : "text-[15.5px] font-semibold text-neutral-900 dark:text-neutral-100 mt-3.5 mb-1.5 tracking-tight border-b border-neutral-100 pb-1"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h1>
            );
          } else if (level === 2) {
            elements.push(
              <h2 key={`h2-${i}`} className={dense ? "text-[11.5px] font-semibold text-neutral-800 dark:text-neutral-200 mt-1.5 mb-0.5 tracking-tight" : "text-[14.5px] font-semibold text-neutral-900 dark:text-neutral-100 mt-3 mb-1 tracking-tight"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h2>
            );
          } else if (level === 3) {
            elements.push(
              <h3 key={`h3-${i}`} className={dense ? "text-[11px] font-semibold text-neutral-700 dark:text-neutral-300 mt-1 mb-0.5 tracking-tight" : "text-[13.5px] font-semibold text-neutral-900 dark:text-neutral-100 mt-2.5 mb-1 tracking-tight"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h3>
            );
          } else {
            elements.push(
              <h4 key={`h4-${i}`} className={dense ? "text-[10.5px] font-medium text-neutral-700 dark:text-neutral-300 mt-1 mb-0.5 tracking-tight" : "text-[13px] font-semibold text-neutral-800 dark:text-neutral-200 mt-2 mb-0.5 tracking-tight"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h4>
            );
          }
          i++;
          continue;
        }
      }

      // 8. Lists (Unordered, Ordered, and Task Lists)
      const isListItem = /^(-|\*|\d+\.)\s+/.test(trimmed);
      if (isListItem) {
        const listItems: { text: string; isTask?: boolean; isChecked?: boolean }[] = [];
        const isOrdered = /^\d+\.\s+/.test(trimmed);

        while (i < rawLines.length && /^(-|\*|\d+\.)\s+/.test(rawLines[i].trim())) {
          let itemText = rawLines[i].trim().replace(/^(-|\*|\d+\.)\s+/, '');
          let isTask = false;
          let isChecked = false;

          const taskMatch = itemText.match(/^\[([ xX])\]\s+(.*)$/);
          if (taskMatch) {
            isTask = true;
            isChecked = taskMatch[1].toLowerCase() === 'x';
            itemText = taskMatch[2];
          }

          listItems.push({ text: itemText, isTask, isChecked });
          i++;
        }

        if (isOrdered) {
          elements.push(
            <ol key={`ol-${i}`} className={dense ? "my-1 space-y-0.5 list-decimal pl-4 text-[11px] leading-snug text-neutral-600 dark:text-neutral-400" : "my-2 space-y-1.5 list-decimal pl-5 text-[13.5px] leading-relaxed text-neutral-800 dark:text-neutral-200"}>
              {listItems.map((item, idx) => (
                <li key={idx} dangerouslySetInnerHTML={{ __html: formatInline(item.text, triggerOpenBrowser, dense) }} />
              ))}
            </ol>
          );
        } else {
          elements.push(
            <ul key={`ul-${i}`} className={dense ? "my-1 space-y-0.5 list-disc pl-4 text-[11px] leading-snug text-neutral-600 dark:text-neutral-400" : "my-2 space-y-1.5 list-disc pl-5 text-[13.5px] leading-relaxed text-neutral-800 dark:text-neutral-200"}>
              {listItems.map((item, idx) => (
                <li key={idx} className={item.isTask ? (dense ? 'list-none -ml-3 flex items-start gap-1.5 my-0.5' : 'list-none -ml-4 flex items-start gap-2.5 my-1') : ''}>
                  {item.isTask && (
                    <input
                      type="checkbox"
                      checked={item.isChecked}
                      readOnly
                      className={dense ? "mt-0.5 rounded border-neutral-300 text-blue-600 focus:ring-blue-500 cursor-default" : "mt-1 rounded border-neutral-300 text-blue-600 focus:ring-blue-500 cursor-default"}
                    />
                  )}
                  <span className={item.isChecked ? 'line-through text-neutral-400 dark:text-neutral-500' : ''} dangerouslySetInnerHTML={{ __html: formatInline(item.text, triggerOpenBrowser, dense) }} />
                </li>
              ))}
            </ul>
          );
        }
        continue;
      }

      // 9. Empty lines (spacing separator)
      if (!trimmed) {
        i++;
        continue;
      }

      // 10. Paragraph (consecutive lines aggregated into one spacious paragraph)
      const paragraphLines: string[] = [];
      while (
        i < rawLines.length &&
        rawLines[i].trim() &&
        !rawLines[i].trim().startsWith('```') &&
        !rawLines[i].trim().startsWith('#') &&
        !rawLines[i].trim().startsWith('>') &&
        !rawLines[i].trim().startsWith('|') &&
        !/^(-|\*|\d+\.)\s+/.test(rawLines[i].trim()) &&
        !/^(\*\*\*|---|___)$/.test(rawLines[i].trim()) &&
        !rawLines[i].trim().match(/^!\[(.*?)\]\((.*?)\)$/)
      ) {
        paragraphLines.push(rawLines[i].trim());
        i++;
      }

      if (paragraphLines.length > 0) {
        elements.push(
          <p
            key={`p-${i}`}
            className={
              dense
                ? "text-[11px] leading-snug text-neutral-600 dark:text-neutral-400 my-1 select-text font-normal"
                : "text-[13.5px] leading-[1.6] text-neutral-800 dark:text-neutral-200 my-2 select-text font-normal"
            }
            dangerouslySetInnerHTML={{ __html: formatInline(paragraphLines.join(' '), triggerOpenBrowser, dense) }}
          />
        );
      }
    }

    return elements;
  };

  return <div className={`font-sans select-text break-words [overflow-wrap:anywhere] max-w-full min-w-0 overflow-x-hidden ${className}`}>{renderFormatted(content)}</div>;
};

// ─────────────────────────────────────────────────────────────
// INLINE FORMATTING (Bold, Italic, Code, Links, Keyboard keys)
// ─────────────────────────────────────────────────────────────
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

function safeHref(raw: string): string | null {
  const href = raw.trim();
  if (/^https?:\/\//i.test(href) || /^mailto:/i.test(href)) return href;
  return null;
}

function formatInline(text: string, onOpenBrowser?: (url: string) => void, dense?: boolean): string {
  if (!text) return '';

  const codeClass = dense
    ? 'px-1 py-0.2 rounded text-[0.875em] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-700 dark:text-neutral-300 border border-neutral-200 dark:border-neutral-700 font-normal'
    : 'px-1.5 py-0.5 rounded text-[0.875em] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-800 dark:text-neutral-200 border border-neutral-200/90 dark:border-neutral-700 font-normal';

  // Escape first: model output is untrusted and must never inject raw HTML.
  return escapeHtml(text)
    // Escaped HTML tags allow-listing (e.g. <kbd>, <mark>, <b>, <i>, <br>)
    .replace(/\*\*(.*?)\*\*/g, '<strong class="font-semibold text-neutral-900 dark:text-neutral-100">$1</strong>')
    .replace(/__(.*?)__/g, '<strong class="font-semibold text-neutral-900 dark:text-neutral-100">$1</strong>')
    .replace(/\*(.*?)\*/g, '<em class="italic">$1</em>')
    .replace(/_([^_]+)_/g, '<em class="italic">$1</em>')
    .replace(/~~(.*?)~~/g, '<del class="line-through text-neutral-400 dark:text-neutral-500">$1</del>')
    .replace(/`([^`]+)`/g, `<code class="${codeClass}">$1</code>`)
    .replace(/&lt;kbd&gt;(.*?)&lt;\/kbd&gt;/gi, '<kbd class="px-1.5 py-0.5 rounded text-[11px] font-mono bg-neutral-200/80 dark:bg-neutral-700 border border-neutral-300 dark:border-neutral-600 shadow-2xs font-semibold">$1</kbd>')
    // Links: only http(s)/mailto are rendered as anchors. javascript:, data:
    // and relative schemes are shown as plain text.
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, (_match, label, href) => {
      const safe = safeHref(href);
      if (!safe) return `<span class="text-neutral-700 dark:text-neutral-300">${label}</span>`;
      return `<a href="${safe}" target="_blank" rel="noreferrer noopener" class="text-blue-600 dark:text-blue-400 font-medium underline decoration-blue-300 hover:decoration-blue-600 transition-colors inline-flex items-center gap-1">${label}</a>`;
    });
}

// ─────────────────────────────────────────────────────────────
// RICH CODE BLOCK WITH "RUN IN TERMINAL" & "PREVIEW"
// ─────────────────────────────────────────────────────────────
function CodeBlock({
  language,
  code,
  dense = false,
  onRunTerminal,
  onOpenBrowser,
}: {
  language: string;
  code: string;
  dense?: boolean;
  onRunTerminal?: (cmd: string) => void;
  onOpenBrowser?: (url: string) => void;
}) {
  const [copied, setCopied] = useState(false);
  const langLower = language.toLowerCase();
  const isTerminalLang = ['bash', 'sh', 'shell', 'zsh', 'terminal', 'powershell', 'ps1', 'cmd'].includes(langLower);
  const isWebLang = ['html', 'svg', 'xml', 'htm'].includes(langLower);

  const handleCopy = () => {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleRun = () => {
    if (onRunTerminal) {
      onRunTerminal(code);
    }
  };

  const handlePreview = () => {
    if (onOpenBrowser) {
      const dataUri = `data:text/html;charset=utf-8,${encodeURIComponent(code)}`;
      onOpenBrowser(dataUri);
    }
  };

  return (
    <div className={`${dense ? 'my-1.5 rounded-xl' : 'my-3 rounded-2xl'} overflow-hidden border border-neutral-200/90 dark:border-neutral-800 bg-[#1e1e1e] text-neutral-100 shadow-2xs font-sans`}>
      <div className={`flex items-center justify-between ${dense ? 'px-3 py-1.5' : 'px-4 py-2'} bg-[#252526] border-b border-neutral-800 text-xs font-mono text-neutral-400`}>
        <div className="flex items-center gap-2">
          {isTerminalLang ? (
            <Terminal className="w-3.5 h-3.5 text-emerald-400" />
          ) : isWebLang ? (
            <Globe className="w-3.5 h-3.5 text-blue-400" />
          ) : (
            <Code2 className="w-3.5 h-3.5 text-blue-400" />
          )}
          <span className="font-semibold text-neutral-300">{language || 'text'}</span>
        </div>

        <div className="flex items-center gap-1.5">
          {isTerminalLang && (
            <button
              onClick={handleRun}
              className="flex items-center gap-1 py-1 px-2.5 rounded-lg bg-emerald-600/20 hover:bg-emerald-600/30 text-emerald-300 hover:text-emerald-200 font-sans text-[11px] font-medium transition-colors border border-emerald-500/30 cursor-pointer"
              title="Execute in ODON Terminal panel"
            >
              <Play className="w-3 h-3 fill-current" />
              <span>Run in Terminal</span>
            </button>
          )}

          {isWebLang && (
            <button
              onClick={handlePreview}
              className="flex items-center gap-1 py-1 px-2.5 rounded-lg bg-blue-600/20 hover:bg-blue-600/30 text-blue-300 hover:text-blue-200 font-sans text-[11px] font-medium transition-colors border border-blue-500/30 cursor-pointer"
              title="Preview in Browser panel"
            >
              <ExternalLink className="w-3 h-3" />
              <span>Preview in Browser</span>
            </button>
          )}

          <button
            onClick={handleCopy}
            className="flex items-center gap-1 py-1 px-2.5 rounded-lg bg-neutral-800 hover:bg-neutral-700 text-neutral-300 hover:text-white font-sans text-[11px] font-medium transition-colors cursor-pointer"
          >
            {copied ? <Check className="w-3 h-3 text-emerald-400" /> : <Copy className="w-3 h-3" />}
            <span>{copied ? 'Copied' : 'Copy'}</span>
          </button>
        </div>
      </div>
      <pre className={`${dense ? "p-2.5 text-[11px] leading-snug" : "p-3.5 text-[12.5px] leading-relaxed"} font-mono overflow-x-auto text-neutral-200 scrollbar-thin max-w-full break-words whitespace-pre-wrap`}>
        <code>{code}</code>
      </pre>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// GFM TABLES
// ─────────────────────────────────────────────────────────────
function TableBlock({ lines, dense = false, onOpenBrowser }: { lines: string[]; dense?: boolean; onOpenBrowser?: (url: string) => void }) {
  if (lines.length < 2) return null;

  const headerCells = lines[0]
    .split('|')
    .slice(1, -1)
    .map((c) => c.trim());

  // skip line 1 (the separator |---|---|)
  const bodyRows = lines.slice(2).map((row) =>
    row
      .split('|')
      .slice(1, -1)
      .map((c) => c.trim())
  );

  return (
    <div className={`${dense ? "my-1.5" : "my-3"} overflow-x-auto max-w-full min-w-0 rounded-xl border border-neutral-200 dark:border-neutral-700 shadow-2xs`}>
      <table className="w-full text-xs text-left border-collapse font-sans">
        <thead className="bg-neutral-100 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-semibold border-b border-neutral-200 dark:border-neutral-700">
          <tr>
            {headerCells.map((h, idx) => (
              <th key={idx} className={`${dense ? "px-2.5 py-1.5 text-[11px]" : "px-3.5 py-2 text-xs"} border-r last:border-r-0 border-neutral-200 dark:border-neutral-700`}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(h, onOpenBrowser) }} />
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-neutral-100 dark:divide-neutral-800 text-neutral-800 dark:text-neutral-200">
          {bodyRows.map((row, rIdx) => (
            <tr key={rIdx} className="even:bg-neutral-50/60 dark:even:bg-neutral-900/40 hover:bg-neutral-100/40 transition-colors">
              {row.map((cell, cIdx) => (
                <td key={cIdx} className={`${dense ? "px-2.5 py-1.5 text-[11px] leading-snug" : "px-3.5 py-1.5 text-xs"} border-r last:border-r-0 border-neutral-200/60 dark:border-neutral-700`}>
                  <span dangerouslySetInnerHTML={{ __html: formatInline(cell, onOpenBrowser) }} />
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// GFM ALERTS (> [!NOTE], > [!TIP], > [!IMPORTANT], > [!WARNING], > [!CAUTION])
// ─────────────────────────────────────────────────────────────
function AlertBlock({
  type,
  content,
  dense = false,
  onOpenBrowser,
}: {
  type: 'NOTE' | 'TIP' | 'IMPORTANT' | 'WARNING' | 'CAUTION';
  content: string;
  dense?: boolean;
  onOpenBrowser?: (url: string) => void;
}) {
  const configs = {
    NOTE: {
      border: 'border-blue-500',
      bg: 'bg-blue-50/80 dark:bg-blue-950/30',
      titleColor: 'text-blue-900 dark:text-blue-200',
      textColor: 'text-blue-950 dark:text-blue-100',
      icon: <Info className="w-4 h-4 text-blue-600" />,
      title: 'Note',
    },
    TIP: {
      border: 'border-emerald-500',
      bg: 'bg-emerald-50/80 dark:bg-emerald-950/30',
      titleColor: 'text-emerald-900 dark:text-emerald-200',
      textColor: 'text-emerald-950 dark:text-emerald-100',
      icon: <Sparkles className="w-4 h-4 text-emerald-600" />,
      title: 'Tip',
    },
    IMPORTANT: {
      border: 'border-blue-500',
      bg: 'bg-blue-50/80 dark:bg-blue-950/30',
      titleColor: 'text-blue-900 dark:text-blue-200',
      textColor: 'text-blue-950 dark:text-blue-100',
      icon: <AlertCircle className="w-4 h-4 text-blue-600" />,
      title: 'Important',
    },
    WARNING: {
      border: 'border-amber-500',
      bg: 'bg-amber-50/80 dark:bg-amber-950/30',
      titleColor: 'text-amber-900 dark:text-amber-200',
      textColor: 'text-amber-950 dark:text-amber-100',
      icon: <AlertTriangle className="w-4 h-4 text-amber-600" />,
      title: 'Warning',
    },
    CAUTION: {
      border: 'border-rose-500',
      bg: 'bg-rose-50/80 dark:bg-rose-950/30',
      titleColor: 'text-rose-900 dark:text-rose-200',
      textColor: 'text-rose-950 dark:text-rose-100',
      icon: <ShieldAlert className="w-4 h-4 text-rose-600" />,
      title: 'Caution',
    },
  };

  const conf = configs[type] || configs.NOTE;

  return (
    <div className={`${dense ? 'my-1.5 p-2 rounded-lg' : 'my-2.5 p-3 rounded-xl'} border-l-4 ${conf.border} ${conf.bg} border-y border-r border-neutral-200/60 dark:border-neutral-800 font-sans`}>
      <div className={`flex items-center gap-1.5 font-semibold text-xs mb-0.5 ${conf.titleColor}`}>
        {conf.icon}
        <span>{conf.title}</span>
      </div>
      <div
        className={`${dense ? 'text-[11px] leading-snug' : 'text-[13px] leading-relaxed'} ${conf.textColor}`}
        dangerouslySetInnerHTML={{ __html: formatInline(content, onOpenBrowser, dense) }}
      />
    </div>
  );
}

// ─────────────────────────────────────────────────────────────
// MARKDOWN IMAGE
// ─────────────────────────────────────────────────────────────
export function MarkdownImage({ alt, src }: { alt: string; src: string }) {
  const [downloading, setDownloading] = useState(false);
  const safeSrc =
    /^https?:\/\//i.test(src) ||
    /^data:image\/(png|jpe?g|gif|webp|svg\+xml);base64,/i.test(src) ||
    src.startsWith('/api/')
      ? src
      : '';
  const fullSrc = api.authorizeUrl(safeSrc);
  const fileName = alt || src.split('/').pop() || 'image.png';

  if (!safeSrc) {
    return (
      <span className="inline-flex items-center gap-1 text-[11px] text-neutral-500">
        <ImageIcon className="w-3.5 h-3.5" />
        {alt || 'Image'}
      </span>
    );
  }

  const handleDownload = async () => {
    setDownloading(true);
    try {
      const resp = await api.fetchApi(fullSrc);
      const blob = await resp.blob();
      const url = window.URL.createObjectURL(blob);
      const a = document.createElement('a');
      a.href = url;
      a.download = fileName.endsWith('.png') || fileName.endsWith('.jpg') ? fileName : `${fileName}.png`;
      document.body.appendChild(a);
      a.click();
      window.URL.revokeObjectURL(url);
      document.body.removeChild(a);
    } catch {
      window.open(fullSrc, '_blank');
    } finally {
      setTimeout(() => setDownloading(false), 1500);
    }
  };

  return (
    <div className="my-4 rounded-2xl border border-neutral-200/90 bg-white shadow-2xs overflow-hidden max-w-lg font-sans">
      <div className="relative aspect-video bg-neutral-950 flex items-center justify-center overflow-hidden">
        <img
          src={fullSrc}
          alt={alt}
          className="w-full h-full object-contain"
          onError={(e) => {
            (e.currentTarget as HTMLImageElement).style.display = 'none';
          }}
        />
      </div>
      <div className="px-3.5 py-2.5 bg-neutral-50 border-t border-neutral-200/80 flex items-center justify-between text-xs">
        <div className="flex items-center gap-1.5 truncate max-w-[260px]">
          <ImageIcon className="w-3.5 h-3.5 text-neutral-500 shrink-0" />
          <span className="font-medium text-neutral-800 truncate">{alt || 'Generated Image'}</span>
        </div>
        <div className="flex items-center gap-2">
          <button
            onClick={() => window.open(fullSrc, '_blank')}
            className="p-1 text-neutral-400 hover:text-neutral-700 rounded transition-colors cursor-pointer"
            title="Open in new tab"
          >
            <ExternalLink className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleDownload}
            disabled={downloading}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-[11px] font-medium transition-colors shadow-2xs cursor-pointer"
          >
            {downloading ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Download className="w-3.5 h-3.5" />}
            <span>{downloading ? 'Saved' : 'Save Image'}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
