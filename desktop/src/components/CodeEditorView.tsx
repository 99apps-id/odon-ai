import React, { useState } from 'react';
import { Copy, Check, FileCode, Play } from 'lucide-react';

interface CodeFile {
  name: string;
  path: string;
  language: string;
  content: string;
}


interface CodeEditorViewProps {
  onRunPreview?: () => void;
  onOpenTerminal?: () => void;
  files?: CodeFile[];
}

export function CodeEditorView({ onRunPreview, onOpenTerminal, files }: CodeEditorViewProps) {
  const fileList = files && files.length > 0 ? files : [];
  const [activeFileIndex, setActiveFileIndex] = useState(0);
  const [copied, setCopied] = useState(false);
  const currentFile = fileList[activeFileIndex] || fileList[0];

  const handleCopy = () => {
    if (!currentFile) return;
    navigator.clipboard.writeText(currentFile.content);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const lines = currentFile ? currentFile.content.split('\n') : [];

  if (!currentFile) {
    return (
      <div className="h-full flex flex-col items-center justify-center gap-2 bg-slate-50/70 text-slate-500 rounded-xl border border-slate-200 p-8 text-center font-sans">
        <FileCode className="w-8 h-8 text-slate-300" />
        <div className="text-sm font-semibold text-slate-700">No generated code yet</div>
        <p className="text-xs max-w-sm leading-relaxed">
          Ask the agent to build a project or module, or open a workspace file from the File Explorer.
        </p>
      </div>
    );
  }

  return (
    <div className="h-full flex flex-col bg-slate-50/70 text-slate-800 rounded-xl overflow-hidden border border-slate-200 font-mono text-[13px] select-text">
      {/* File Tabs & Actions */}
      <div className="flex items-center justify-between bg-white px-2 border-b border-slate-200 shrink-0">
        <div className="flex items-center overflow-x-auto scrollbar-none">
          {fileList.map((file, idx) => {
            const isActive = idx === activeFileIndex;
            return (
              <button
                key={file.path}
                onClick={() => setActiveFileIndex(idx)}
                className={`flex items-center gap-1.5 px-3 py-2 text-xs border-b-2 transition-colors whitespace-nowrap cursor-pointer ${
                  isActive
                    ? 'border-blue-600 bg-blue-50/50 text-blue-700 font-semibold'
                    : 'border-transparent text-slate-500 hover:text-slate-800 hover:bg-slate-50'
                }`}
              >
                <FileCode className={`w-3.5 h-3.5 ${isActive ? 'text-blue-600' : 'text-slate-400'}`} />
                {file.name}
              </button>
            );
          })}
        </div>

        <div className="flex items-center gap-1.5 py-1 pl-2 shrink-0">
          {onOpenTerminal && (
            <button
              onClick={onOpenTerminal}
              className="flex items-center gap-1 px-2.5 py-1 rounded bg-slate-100 hover:bg-slate-200 text-slate-700 text-xs font-sans font-medium transition-colors cursor-pointer"
              title="Open Terminal"
            >
              Terminal
            </button>
          )}

          {onRunPreview && (
            <button
              onClick={onRunPreview}
              className="flex items-center gap-1.5 px-2.5 py-1 rounded bg-blue-50 hover:bg-blue-100 text-blue-700 text-xs font-sans font-medium transition-colors border border-blue-200 cursor-pointer"
              title="Preview in Browser"
            >
              <Play className="w-3 h-3 fill-current text-blue-600" />
              Preview
            </button>
          )}

          <button
            onClick={handleCopy}
            className="flex items-center gap-1 px-2.5 py-1 rounded hover:bg-slate-100 text-slate-600 text-xs font-sans transition-colors"
            title="Copy code"
          >
            {copied ? (
              <>
                <Check className="w-3.5 h-3.5 text-emerald-600" />
                <span className="text-emerald-600 text-xs font-medium">Copied</span>
              </>
            ) : (
              <>
                <Copy className="w-3.5 h-3.5" />
                <span>Copy</span>
              </>
            )}
          </button>
        </div>
      </div>

      {/* Code Area */}
      <div className="flex-1 overflow-auto bg-slate-50/50 p-3 leading-relaxed">
        <table className="w-full border-collapse">
          <tbody>
            {lines.map((line, i) => (
              <tr key={i} className="hover:bg-blue-50/40">
                <td className="w-9 text-right pr-4 select-none text-slate-400 text-xs align-top">
                  {i + 1}
                </td>
                <td className="whitespace-pre overflow-x-visible align-top font-mono">
                  {renderSyntaxLine(line)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* Editor Footer */}
      <div className="h-6 bg-white border-t border-slate-200 px-3 flex items-center justify-between text-[11px] text-slate-500 shrink-0 font-sans">
        <span className="truncate">{currentFile.path}</span>
        <div className="flex items-center gap-3">
          <span className="uppercase">{currentFile.language}</span>
          <span>UTF-8</span>
        </div>
      </div>
    </div>
  );
}

function renderSyntaxLine(line: string) {
  if (line.trim().startsWith('//')) {
    return <span className="text-emerald-600 italic">{line}</span>;
  }

  const tokens = line.split(/(\b(?:import|from|export|default|function|const|let|return|interface|type)\b|'[^']*'|"[^"]*"|`[^`]*`)/g);

  return (
    <span>
      {tokens.map((token, idx) => {
        if (/^(import|from|export|default|function|const|let|return|interface|type)$/.test(token)) {
          return <span key={idx} className="text-blue-700 font-semibold">{token}</span>;
        }
        if (/^['"`]/.test(token)) {
          return <span key={idx} className="text-amber-700">{token}</span>;
        }
        if (/^[A-Z][a-zA-Z0-9]+/.test(token)) {
          return <span key={idx} className="text-teal-700 font-medium">{token}</span>;
        }
        return <span key={idx} className="text-slate-700">{token}</span>;
      })}
    </span>
  );
}
