import React, { useMemo, useState } from 'react';
import { api } from '../api';
import { DiffView, looksLikeUnifiedDiff } from './DiffView';
import { TOKEN_CLASS, isDiffLanguage, tokenize } from '../lib/highlight';
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
  Lightbulb,
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
  preserveLineBreaks?: boolean;
  onRunTerminal?: (cmd: string) => void;
  onOpenBrowser?: (url: string) => void;
}

type ChartSeries = { name?: string; values: number[] };
type ChartSpec = {
  type?: 'bar' | 'line';
  title?: string;
  labels?: string[];
  values?: number[];
  series?: ChartSeries[];
};

const CHART_COLORS = ['#65766b', '#9a8064', '#7d8175', '#b08d74', '#667b79', '#a29a80'];

const ChartBlock: React.FC<{ spec: ChartSpec; dense?: boolean }> = ({ spec, dense = false }) => {
  const series =
    spec.series && spec.series.length > 0
      ? spec.series
      : [{ name: spec.title || 'Value', values: spec.values || [] }];
  const labels = spec.labels || [];
  const allValues = series.flatMap((s) => s.values);
  const maxValue = Math.max(1, ...allValues);
  const width = 640;
  const height = dense ? 190 : 260;
  const padLeft = 42;
  const padRight = 16;
  const padTop = 20;
  const padBottom = 32;
  const plotW = width - padLeft - padRight;
  const plotH = height - padTop - padBottom;
  const count = Math.max(1, labels.length || Math.max(...series.map((s) => s.values.length)));
  const groupW = plotW / count;
  const barW = Math.max(2, (groupW * 0.7) / Math.max(1, series.length));
  const y = (value: number) => padTop + plotH - (value / maxValue) * plotH;

  return (
    <div className="my-3 p-3 rounded-xl border border-neutral-200 dark:border-neutral-700 bg-white dark:bg-neutral-900">
      {spec.title && (
        <div className="text-[12px] font-semibold text-neutral-800 dark:text-neutral-200 mb-2">
          {spec.title}
        </div>
      )}
      <svg viewBox={`0 0 ${width} ${height}`} className="w-full h-auto" role="img">
        {[0, 0.25, 0.5, 0.75, 1].map((tick) => {
          const tickY = padTop + plotH - tick * plotH;
          return (
            <g key={tick}>
              <line
                x1={padLeft}
                y1={tickY}
                x2={width - padRight}
                y2={tickY}
                stroke="#e5e7eb"
                strokeWidth="1"
              />
              <text x={padLeft - 6} y={tickY + 3} textAnchor="end" fontSize="9" fill="#9ca3af">
                {Math.round(maxValue * tick)}
              </text>
            </g>
          );
        })}
        {labels.map((label, index) => (
          <text
            key={`${label}-${index}`}
            x={padLeft + groupW * index + groupW / 2}
            y={height - 10}
            textAnchor="middle"
            fontSize="9"
            fill="#9ca3af"
          >
            {label.length > 12 ? `${label.slice(0, 11)}…` : label}
          </text>
        ))}
        {spec.type === 'line'
          ? series.map((s, seriesIndex) => {
              const points = s.values
                .map((value, index) => {
                  const x = padLeft + groupW * index + groupW / 2;
                  return `${x},${y(value)}`;
                })
                .join(' ');
              return (
                <g key={s.name || seriesIndex}>
                  <polyline
                    points={points}
                    fill="none"
                    stroke={CHART_COLORS[seriesIndex % CHART_COLORS.length]}
                    strokeWidth="2.5"
                    strokeLinejoin="round"
                  />
                  {s.values.map((value, index) => (
                    <circle
                      key={index}
                      cx={padLeft + groupW * index + groupW / 2}
                      cy={y(value)}
                      r="3"
                      fill={CHART_COLORS[seriesIndex % CHART_COLORS.length]}
                    />
                  ))}
                </g>
              );
            })
          : series.map((s, seriesIndex) =>
              s.values.map((value, index) => {
                const x =
                  padLeft +
                  groupW * index +
                  groupW / 2 -
                  (series.length * barW) / 2 +
                  seriesIndex * barW;
                const barH = Math.max(0, padTop + plotH - y(value));
                return (
                  <rect
                    key={`${seriesIndex}-${index}`}
                    x={x}
                    y={y(value)}
                    width={Math.max(1, barW - 2)}
                    height={barH}
                    rx="2"
                    fill={CHART_COLORS[seriesIndex % CHART_COLORS.length]}
                  />
                );
              }),
            )}
      </svg>
      {series.length > 1 && (
        <div className="flex flex-wrap items-center gap-3 mt-1">
          {series.map((s, index) => (
            <span key={s.name || index} className="flex items-center gap-1 text-[10px] text-neutral-500">
              <span
                className="inline-block w-2 h-2 rounded-full"
                style={{ backgroundColor: CHART_COLORS[index % CHART_COLORS.length] }}
              />
              {s.name || `Series ${index + 1}`}
            </span>
          ))}
        </div>
      )}
    </div>
  );
};

export const MarkdownView: React.FC<MarkdownViewProps> = ({
  content,
  className = '',
  dense = false,
  preserveLineBreaks = false,
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
        if (lang.toLowerCase() === 'chart') {
          try {
            const spec = JSON.parse(codeString) as ChartSpec;
            elements.push(<ChartBlock key={`chart-${i}`} spec={spec} dense={dense} />);
            continue;
          } catch {
            // Invalid chart JSON falls through to a normal code block below.
          }
        }
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
      // Every pipe-leading line must be consumed here. A partial table (header
      // row still streaming in, or a row without a `---` separator) used to
      // match no branch and leave the cursor in place, which hung the renderer.
      if (trimmed.startsWith('|')) {
        const tableLines: string[] = [];
        while (i < rawLines.length && rawLines[i].trim().startsWith('|')) {
          tableLines.push(rawLines[i].trim());
          i++;
        }
        const looksLikeTable = tableLines.length >= 2 && tableLines[1].includes('---');
        if (looksLikeTable) {
          elements.push(<TableBlock key={`table-${i}`} lines={tableLines} dense={dense} onOpenBrowser={triggerOpenBrowser} />);
        } else {
          elements.push(
            <p
              key={`pipes-${i}`}
              className={
                dense
                  ? "text-[12.5px] leading-relaxed text-neutral-600 dark:text-neutral-400 my-1 select-text font-normal"
                  : "text-[15px] leading-[1.7] text-neutral-800 dark:text-neutral-200 my-2 select-text font-normal"
              }
              dangerouslySetInnerHTML={{ __html: formatInline(tableLines.join(' '), triggerOpenBrowser, dense) }}
            />
          );
        }
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
                ? "border-l-2 border-neutral-300 dark:border-neutral-600 pl-2.5 py-0.5 my-1 bg-neutral-100/40 dark:bg-neutral-800/30 rounded-r-lg italic text-[12px] text-neutral-600 dark:text-neutral-400 leading-relaxed"
                : "border-l-3 border-neutral-300 dark:border-neutral-600 pl-3.5 py-1 my-2 bg-neutral-50/70 dark:bg-neutral-800/40 rounded-r-xl italic text-[14.5px] text-neutral-700 dark:text-neutral-300 leading-relaxed"
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
              <h1 key={`h1-${i}`} className={dense ? "text-[13px] font-semibold text-neutral-800 dark:text-neutral-200 mt-2 mb-1 tracking-tight" : "text-[17px] font-semibold text-neutral-900 dark:text-neutral-100 mt-3.5 mb-1.5 tracking-tight border-b border-neutral-100 pb-1"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h1>
            );
          } else if (level === 2) {
            elements.push(
              <h2 key={`h2-${i}`} className={dense ? "text-[12.5px] font-semibold text-neutral-800 dark:text-neutral-200 mt-1.5 mb-0.5 tracking-tight" : "text-[16px] font-semibold text-neutral-900 dark:text-neutral-100 mt-3 mb-1 tracking-tight"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h2>
            );
          } else if (level === 3) {
            elements.push(
              <h3 key={`h3-${i}`} className={dense ? "text-[12px] font-semibold text-neutral-700 dark:text-neutral-300 mt-1 mb-0.5 tracking-tight" : "text-[15px] font-semibold text-neutral-900 dark:text-neutral-100 mt-2.5 mb-1 tracking-tight"}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(textContent, triggerOpenBrowser, dense) }} />
              </h3>
            );
          } else {
            elements.push(
              <h4 key={`h4-${i}`} className={dense ? "text-[11.5px] font-medium text-neutral-700 dark:text-neutral-300 mt-1 mb-0.5 tracking-tight" : "text-[14.5px] font-semibold text-neutral-800 dark:text-neutral-200 mt-2 mb-0.5 tracking-tight"}>
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
            <ol key={`ol-${i}`} className={dense ? "my-1 space-y-0.5 list-decimal pl-4 text-[12.5px] leading-relaxed text-neutral-600 dark:text-neutral-400" : "my-2 space-y-1.5 list-decimal pl-5 text-[15px] leading-relaxed text-neutral-800 dark:text-neutral-200"}>
              {listItems.map((item, idx) => (
                <li key={idx} dangerouslySetInnerHTML={{ __html: formatInline(item.text, triggerOpenBrowser, dense) }} />
              ))}
            </ol>
          );
        } else {
          elements.push(
            <ul key={`ul-${i}`} className={dense ? "my-1 space-y-0.5 list-disc pl-4 text-[12.5px] leading-relaxed text-neutral-600 dark:text-neutral-400" : "my-2 space-y-1.5 list-disc pl-5 text-[15px] leading-relaxed text-neutral-800 dark:text-neutral-200"}>
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
                ? "text-[12.5px] leading-relaxed text-neutral-600 dark:text-neutral-400 my-1 select-text font-normal"
                : "text-[15px] leading-[1.7] text-neutral-800 dark:text-neutral-200 my-2 select-text font-normal"
            }
            dangerouslySetInnerHTML={{ __html: formatInline(paragraphLines.join(preserveLineBreaks ? '\n' : ' '), triggerOpenBrowser, dense) }}
          />
        );
      } else {
        // Safety net: a line that no branch claimed must still advance the
        // cursor, otherwise the parser can never terminate (renderer hang).
        elements.push(
          <p
            key={`raw-${i}`}
            className={
              dense
                ? "text-[12.5px] leading-relaxed text-neutral-600 dark:text-neutral-400 my-1 select-text font-normal"
                : "text-[15px] leading-[1.7] text-neutral-800 dark:text-neutral-200 my-2 select-text font-normal"
            }
          >
            {line}
          </p>
        );
        i++;
      }
    }

    return elements;
  };

  return <div className={`font-sans select-text break-words [overflow-wrap:anywhere] max-w-full min-w-0 overflow-x-hidden ${preserveLineBreaks ? 'reasoning-markdown preserve-line-breaks' : ''} ${className}`}>{renderFormatted(content)}</div>;
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
    ? 'px-1 py-0.2 rounded text-[0.875em] font-mono bg-neutral-200 dark:bg-neutral-800 text-neutral-800 dark:text-neutral-300 border border-neutral-300 dark:border-neutral-700 font-normal'
    : 'px-1.5 py-0.5 rounded text-[0.875em] font-mono bg-neutral-200 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-200 border border-neutral-300 dark:border-neutral-700 font-normal';

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

  const diffMode = isDiffLanguage(langLower) || looksLikeUnifiedDiff(code);
  const tokens = useMemo(
    () => (diffMode ? [] : tokenize(code, langLower)),
    [code, langLower, diffMode],
  );
  const gutterWidth = String(tokens.length).length;

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
    } else {
      // The Browser panel rejects the data: scheme, but it renders raw HTML
      // in a sandboxed srcdoc iframe. Send the document, not a URL.
      window.dispatchEvent(new CustomEvent('odon:open-browser', { detail: { html: code } }));
    }
  };

  if (diffMode) {
    return <DiffView diff={code} dense={dense} label={langLower === 'diff' || langLower === 'patch' ? undefined : (language || 'diff')} />;
  }

  return (
    <div className={`${dense ? 'my-1.5 rounded-xl' : 'my-3 rounded-2xl'} overflow-hidden border border-[#cbd8c9] bg-white shadow-2xs font-sans`}>
      <div className={`flex items-center justify-between ${dense ? 'px-3 py-1.5' : 'px-4 py-2'} bg-[#eaf1e7] border-b border-[#cbd8c9] text-xs font-mono text-[#4d5a4b]`}>
        <div className="flex items-center gap-2">
          {isTerminalLang ? (
            <Terminal className="w-3.5 h-3.5 text-[#4a6142]" />
          ) : isWebLang ? (
            <Globe className="w-3.5 h-3.5 text-[#4a6142]" />
          ) : (
            <Code2 className="w-3.5 h-3.5 text-[#4a6142]" />
          )}
          <span className="font-semibold text-[#3c4a3a]">{language || 'text'}</span>
        </div>

        <div className="flex items-center gap-1.5">
          {isTerminalLang && (
            <button
              onClick={handleRun}
              className="flex items-center gap-1 py-1 px-2.5 rounded-lg bg-[#dbe7d6] hover:bg-[#d0dfca] text-[#3c4a3a] font-sans text-[11px] font-medium transition-colors border border-[#bccdb6] cursor-pointer"
              title="Execute in ODON Terminal panel"
            >
              <Play className="w-3 h-3 fill-current" />
              <span>Run in Terminal</span>
            </button>
          )}

          {isWebLang && (
            <button
              onClick={handlePreview}
              className="flex items-center gap-1 py-1 px-2.5 rounded-lg bg-[#dbe7d6] hover:bg-[#d0dfca] text-[#3c4a3a] font-sans text-[11px] font-medium transition-colors border border-[#bccdb6] cursor-pointer"
              title="Preview in Browser panel"
            >
              <ExternalLink className="w-3 h-3" />
              <span>Preview in Browser</span>
            </button>
          )}

          <button
            onClick={handleCopy}
            className="flex items-center gap-1 py-1 px-2.5 rounded-lg bg-[#e2e8dd] hover:bg-[#d7dfd0] text-[#3c4a3a] font-sans text-[11px] font-medium transition-colors cursor-pointer"
          >
            {copied ? <Check className="w-3 h-3 text-[#2f6b3a]" /> : <Copy className="w-3 h-3" />}
            <span>{copied ? 'Copied' : 'Copy'}</span>
          </button>
        </div>
      </div>
      <div className="bg-white">
        <pre className={`${dense ? 'p-2.5 text-[12px] leading-relaxed' : 'p-3.5 text-[13px] leading-relaxed'} font-mono overflow-x-auto scrollbar-thin max-w-full whitespace-pre-wrap break-words`}>
          <code>
            {tokens.map((line, i) => (
              <div key={i} className="flex gap-3">
                <span
                  className="select-none shrink-0 text-right text-[#c4bfb2]"
                  style={{ width: `${Math.max(gutterWidth, 2)}ch` }}
                >
                  {i + 1}
                </span>
                <span className="min-w-0 flex-1">
                  {line.length === 0 ? (
                    <span>{'\u00a0'}</span>
                  ) : (
                    line.map((tok, j) => (
                      <span key={j} className={TOKEN_CLASS[tok.kind]}>
                        {tok.text}
                      </span>
                    ))
                  )}
                </span>
              </div>
            ))}
          </code>
        </pre>
      </div>
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
      <table className="w-full text-[13.5px] text-left border-collapse font-sans">
        <thead className="bg-neutral-100 dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-semibold border-b border-neutral-200 dark:border-neutral-700">
          <tr>
            {headerCells.map((h, idx) => (
              <th key={idx} className={`${dense ? "px-2.5 py-1.5 text-[12px]" : "px-3.5 py-2 text-[13px]"} border-r last:border-r-0 border-neutral-200 dark:border-neutral-700`}>
                <span dangerouslySetInnerHTML={{ __html: formatInline(h, onOpenBrowser) }} />
              </th>
            ))}
          </tr>
        </thead>
        <tbody className="divide-y divide-neutral-100 dark:divide-neutral-800 text-neutral-800 dark:text-neutral-200">
          {bodyRows.map((row, rIdx) => (
            <tr key={rIdx} className="even:bg-neutral-50/60 dark:even:bg-neutral-900/40 hover:bg-neutral-100/40 transition-colors">
              {row.map((cell, cIdx) => (
                <td key={cIdx} className={`${dense ? "px-2.5 py-1.5 text-[12px] leading-relaxed" : "px-3.5 py-1.5 text-[13px]"} border-r last:border-r-0 border-neutral-200/60 dark:border-neutral-700`}>
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
      icon: <Lightbulb className="w-4 h-4 text-emerald-600" />,
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
      border: 'border-clay-500',
      bg: 'bg-clay-50/80 dark:bg-clay-950/30',
      titleColor: 'text-clay-900 dark:text-clay-200',
      textColor: 'text-clay-950 dark:text-clay-100',
      icon: <ShieldAlert className="w-4 h-4 text-clay-600" />,
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
        className={`${dense ? 'text-[12px] leading-relaxed' : 'text-[14px] leading-relaxed'} ${conf.textColor}`}
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
          referrerPolicy="no-referrer"
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
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-[11px] font-medium transition-colors shadow-2xs cursor-pointer"
          >
            {downloading ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Download className="w-3.5 h-3.5" />}
            <span>{downloading ? 'Saved' : 'Save Image'}</span>
          </button>
        </div>
      </div>
    </div>
  );
}
