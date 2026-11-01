import React, { useState } from 'react';
import { Check, Copy } from 'lucide-react';

interface MarkdownViewProps {
  content: string;
  className?: string;
}

export const MarkdownView: React.FC<MarkdownViewProps> = ({ content, className = '' }) => {
  // Simple, robust markdown block parser
  const renderFormatted = (text: string) => {
    const lines = text.split('\n');
    const elements: React.ReactNode[] = [];
    let inCodeBlock = false;
    let codeLanguage = '';
    let codeBuffer: string[] = [];
    let listBuffer: string[] = [];

    const flushList = (key: string) => {
      if (listBuffer.length > 0) {
        elements.push(
          <ul key={key} className="my-2 space-y-1 list-disc pl-5 text-[14px] text-neutral-800 dark:text-neutral-200">
            {listBuffer.map((item, idx) => (
              <li key={idx} dangerouslySetInnerHTML={{ __html: formatInline(item) }} />
            ))}
          </ul>
        );
        listBuffer = [];
      }
    };

    lines.forEach((line, index) => {
      // Code blocks
      if (line.trim().startsWith('```')) {
        if (!inCodeBlock) {
          flushList(`list-${index}`);
          inCodeBlock = true;
          codeLanguage = line.trim().replace(/^```/, '') || 'text';
          codeBuffer = [];
        } else {
          inCodeBlock = false;
          const codeContent = codeBuffer.join('\n');
          elements.push(
            <CodeBlock key={`code-${index}`} language={codeLanguage} code={codeContent} />
          );
          codeBuffer = [];
        }
        return;
      }

      if (inCodeBlock) {
        codeBuffer.push(line);
        return;
      }

      // Lists
      if (line.trim().startsWith('- ') || line.trim().startsWith('* ')) {
        listBuffer.push(line.trim().replace(/^[-*]\s+/, ''));
        return;
      } else {
        flushList(`list-before-${index}`);
      }

      // Empty line
      if (!line.trim()) {
        elements.push(<div key={`empty-${index}`} className="h-2" />);
        return;
      }

      // Headings
      if (line.startsWith('### ')) {
        elements.push(
          <h3 key={`h3-${index}`} className="text-[16px] font-semibold text-neutral-900 dark:text-neutral-100 mt-4 mb-2 tracking-tight">
            {line.replace('### ', '')}
          </h3>
        );
        return;
      }
      if (line.startsWith('## ')) {
        elements.push(
          <h2 key={`h2-${index}`} className="text-[18px] font-semibold text-neutral-900 dark:text-neutral-100 mt-5 mb-2 tracking-tight">
            {line.replace('## ', '')}
          </h2>
        );
        return;
      }
      if (line.startsWith('# ')) {
        elements.push(
          <h1 key={`h1-${index}`} className="text-[20px] font-bold text-neutral-900 dark:text-neutral-100 mt-6 mb-3 tracking-tight">
            {line.replace('# ', '')}
          </h1>
        );
        return;
      }

      // Blockquotes
      if (line.startsWith('> ')) {
        elements.push(
          <blockquote key={`quote-${index}`} className="border-l-2 border-neutral-300 dark:border-neutral-700 pl-3 my-2 text-neutral-600 dark:text-neutral-400 italic text-[13.5px]">
            {line.replace('> ', '')}
          </blockquote>
        );
        return;
      }

      // Paragraph
      elements.push(
        <p
          key={`p-${index}`}
          className="text-[14px] leading-relaxed text-neutral-800 dark:text-neutral-200 my-1"
          dangerouslySetInnerHTML={{ __html: formatInline(line) }}
        />
      );
    });

    flushList('list-end');

    // If code block was not closed
    if (inCodeBlock && codeBuffer.length > 0) {
      elements.push(
        <CodeBlock key="code-unclosed" language={codeLanguage} code={codeBuffer.join('\n')} />
      );
    }

    return elements;
  };

  return <div className={`space-y-1 font-sans ${className}`}>{renderFormatted(content)}</div>;
};

function formatInline(text: string): string {
  return text
    .replace(/\*\*(.*?)\*\*/g, '<strong class="font-semibold text-neutral-900 dark:text-neutral-100">$1</strong>')
    .replace(/\*(.*?)\*/g, '<em class="italic">$1</em>')
    .replace(/`([^`]+)`/g, '<code class="px-1.5 py-0.5 rounded text-[12.5px] font-mono bg-neutral-100 dark:bg-neutral-800 text-neutral-800 dark:text-neutral-200 border border-neutral-200 dark:border-neutral-700">$1</code>')
    .replace(/\[([^\]]+)\]\(([^)]+)\)/g, '<a href="$2" target="_blank" rel="noreferrer" class="text-neutral-900 dark:text-neutral-100 underline decoration-neutral-400 underline-offset-2 hover:decoration-neutral-900">$1</a>');
}

function CodeBlock({ language, code }: { language: string; code: string }) {
  const [copied, setCopied] = useState(false);

  const handleCopy = () => {
    navigator.clipboard.writeText(code);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  return (
    <div className="my-3 rounded-xl overflow-hidden border border-neutral-200 dark:border-neutral-800 bg-[#1e1e1e] text-neutral-100 shadow-xs">
      <div className="flex items-center justify-between px-3.5 py-1.5 bg-[#252526] border-b border-neutral-800 text-[11px] font-mono text-neutral-400">
        <span>{language}</span>
        <button
          onClick={handleCopy}
          className="flex items-center gap-1.5 py-1 px-2 rounded hover:bg-neutral-800 text-neutral-300 hover:text-white transition-colors"
        >
          {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
          <span>{copied ? 'Copied' : 'Copy code'}</span>
        </button>
      </div>
      <pre className="p-4 text-[13px] font-mono leading-relaxed overflow-x-auto text-neutral-200">
        <code>{code}</code>
      </pre>
    </div>
  );
}

