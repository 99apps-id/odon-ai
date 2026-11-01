import React, { useState, useEffect, useRef } from 'react';
import {
  Terminal as TerminalIcon,
  Play,
  RotateCcw,
  Copy,
  Check,
  Trash2,
  FolderOpen,
  Sparkles,
  Command,
  ArrowRight,
  Loader2,
} from 'lucide-react';
import { api } from '../api';
import { TerminalInfo, TerminalExecResult } from '../types';

interface HistoryItem {
  id: string;
  command: string;
  stdout: string;
  stderr: string;
  exit_code: number;
  cwd: string;
  timestamp: string;
}

interface TerminalViewProps {
  onClose?: () => void;
}

export const TerminalView: React.FC<TerminalViewProps> = ({ onClose }) => {
  const [terminalInfo, setTerminalInfo] = useState<TerminalInfo | null>(null);
  const [currentCwd, setCurrentCwd] = useState<string>('');
  const [inputVal, setInputVal] = useState<string>('');
  const [history, setHistory] = useState<HistoryItem[]>([]);
  const [commandHistory, setCommandHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState<number>(-1);
  const [isRunning, setIsRunning] = useState<boolean>(false);
  const [copied, setCopied] = useState<boolean>(false);

  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // Load terminal info & cwd on mount
  useEffect(() => {
    let isMounted = true;
    api.getTerminalInfo().then((info) => {
      if (isMounted) {
        setTerminalInfo(info);
        setCurrentCwd(info.default_cwd);
      }
    }).catch(() => {
      if (isMounted) {
        setTerminalInfo({
          shell: 'PowerShell',
          os: 'windows',
          arch: 'x86_64',
          default_cwd: 'C:/Project/odon-ai',
          user: 'developer',
        });
        setCurrentCwd('C:/Project/odon-ai');
      }
    });

    // Initial banner
    setHistory([
      {
        id: 'init',
        command: '# Welcome to ODON Integrated Terminal',
        stdout: 'ODON Terminal Environment ready.\nType any PowerShell / CLI command and press Enter.\nUse Up/Down arrows to navigate command history.',
        stderr: '',
        exit_code: 0,
        cwd: 'C:/Project/odon-ai',
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
      },
    ]);

    return () => {
      isMounted = false;
    };
  }, []);

  // Auto scroll to bottom
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [history, isRunning]);

  const handleRunCommand = async (cmdToRun?: string) => {
    const rawCmd = (cmdToRun !== undefined ? cmdToRun : inputVal).trim();
    if (!rawCmd || isRunning) return;

    setInputVal('');
    setHistoryIndex(-1);
    setIsRunning(true);

    // Save to command history
    setCommandHistory((prev) => [rawCmd, ...prev.filter((c) => c !== rawCmd)]);

    const cwdBefore = currentCwd || terminalInfo?.default_cwd || '';
    const nowStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });

    try {
      const result: TerminalExecResult = await api.executeTerminal(rawCmd, cwdBefore);
      if (result.cwd) {
        setCurrentCwd(result.cwd);
      }

      setHistory((prev) => [
        ...prev,
        {
          id: `${Date.now()}-${Math.random()}`,
          command: rawCmd,
          stdout: result.stdout,
          stderr: result.stderr,
          exit_code: result.exit_code,
          cwd: result.cwd || cwdBefore,
          timestamp: nowStr,
        },
      ]);
    } catch (err: any) {
      setHistory((prev) => [
        ...prev,
        {
          id: `${Date.now()}-${Math.random()}`,
          command: rawCmd,
          stdout: '',
          stderr: err.message || 'Execution failed',
          exit_code: 1,
          cwd: cwdBefore,
          timestamp: nowStr,
        },
      ]);
    } finally {
      setIsRunning(false);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      e.preventDefault();
      handleRunCommand();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (commandHistory.length === 0) return;
      const nextIndex = historyIndex + 1;
      if (nextIndex < commandHistory.length) {
        setHistoryIndex(nextIndex);
        setInputVal(commandHistory[nextIndex]);
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      const prevIndex = historyIndex - 1;
      if (prevIndex >= 0) {
        setHistoryIndex(prevIndex);
        setInputVal(commandHistory[prevIndex]);
      } else {
        setHistoryIndex(-1);
        setInputVal('');
      }
    } else if (e.ctrlKey && e.key === 'c') {
      e.preventDefault();
      setInputVal('');
    } else if (e.ctrlKey && e.key === 'l') {
      e.preventDefault();
      handleClear();
    }
  };

  const handleClear = () => {
    setHistory([]);
    setTimeout(() => inputRef.current?.focus(), 50);
  };

  const handleCopyAll = () => {
    const text = history
      .map((h) => `PS ${h.cwd}> ${h.command}\n${h.stdout}${h.stderr ? `\nSTDERR:\n${h.stderr}` : ''}`)
      .join('\n\n');
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const quickChips = [
    { label: 'git status', cmd: 'git status' },
    { label: 'cargo check', cmd: 'cargo check --workspace' },
    { label: 'pnpm build', cmd: 'pnpm --filter desktop build' },
    { label: 'Get-ChildItem', cmd: 'Get-ChildItem' },
    { label: 'node -v', cmd: 'node -v' },
  ];

  return (
    <div className="flex flex-col h-full bg-[#121316] text-neutral-200 font-mono text-xs select-text">
      {/* Terminal Top Bar */}
      <div className="flex items-center justify-between px-3.5 py-2.5 bg-[#18191d] border-b border-neutral-800 shrink-0">
        <div className="flex items-center gap-2 overflow-hidden">
          <div className="flex items-center gap-1.5 px-2 py-0.5 rounded bg-neutral-800 text-neutral-300 font-sans font-medium text-[11px]">
            <TerminalIcon className="w-3.5 h-3.5 text-blue-400" />
            <span>{terminalInfo?.shell || 'PowerShell'}</span>
          </div>

          <div className="flex items-center gap-1 px-1.5 py-0.5 text-[11px] text-neutral-400 font-mono truncate max-w-[200px]" title={currentCwd}>
            <FolderOpen className="w-3 h-3 text-neutral-500 shrink-0" />
            <span className="truncate">{currentCwd || '...'}</span>
          </div>
        </div>

        <div className="flex items-center gap-1">
          <button
            onClick={handleCopyAll}
            className="p-1 rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
            title="Copy all output"
          >
            {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
          </button>
          <button
            onClick={handleClear}
            className="p-1 rounded text-neutral-400 hover:text-neutral-200 hover:bg-neutral-800 transition-colors"
            title="Clear terminal (Ctrl+L)"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* Quick Command Chips */}
      <div className="flex items-center gap-1.5 px-3 py-1.5 bg-[#151619] border-b border-neutral-800/60 overflow-x-auto no-scrollbar shrink-0">
        <span className="text-[10px] text-neutral-500 font-sans uppercase tracking-wider font-semibold mr-1">Quick:</span>
        {quickChips.map((chip) => (
          <button
            key={chip.label}
            onClick={() => handleRunCommand(chip.cmd)}
            disabled={isRunning}
            className="px-2 py-0.5 rounded bg-neutral-800/80 hover:bg-neutral-700/80 text-neutral-300 hover:text-white text-[11px] whitespace-nowrap transition-colors flex items-center gap-1 border border-neutral-700/40"
          >
            <span>{chip.label}</span>
          </button>
        ))}
      </div>

      {/* Scrollable Output Stream */}
      <div
        className="flex-1 p-3.5 overflow-y-auto font-mono text-[11.5px] leading-relaxed space-y-3 cursor-text selection:bg-blue-600/40"
        onClick={() => inputRef.current?.focus()}
      >
        {history.map((item) => (
          <div key={item.id} className="space-y-1">
            {/* Command Header line */}
            <div className="flex items-center gap-1.5 text-neutral-400">
              <span className="text-blue-400 font-semibold">PS</span>
              <span className="text-neutral-500 truncate max-w-[220px]">{item.cwd}&gt;</span>
              <span className="text-neutral-100 font-medium">{item.command}</span>
              <span className="text-[10px] text-neutral-600 ml-auto font-sans">{item.timestamp}</span>
            </div>

            {/* STDOUT */}
            {item.stdout && (
              <pre className="text-neutral-300 whitespace-pre-wrap font-mono pl-4 border-l border-neutral-700/50">
                {item.stdout}
              </pre>
            )}

            {/* STDERR */}
            {item.stderr && (
              <pre className="text-rose-400 whitespace-pre-wrap font-mono pl-4 border-l-2 border-rose-500/60 bg-rose-950/20 py-1 rounded-r">
                {item.stderr}
              </pre>
            )}

            {/* Exit status badge if not zero */}
            {item.exit_code !== 0 && (
              <div className="text-[10px] text-rose-400 pl-4 font-sans">
                Process exited with status code {item.exit_code}
              </div>
            )}
          </div>
        ))}

        {/* Currently Running Command */}
        {isRunning && (
          <div className="flex items-center gap-2 text-blue-400 pl-4 py-1">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            <span className="text-neutral-400 italic text-[11px]">Executing command...</span>
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Prompt Input Line */}
      <div className="flex items-center gap-2 px-3.5 py-2.5 bg-[#18191d] border-t border-neutral-800 shrink-0">
        <div className="flex items-center gap-1 text-blue-400 font-semibold select-none">
          <span>PS</span>
          <span className="text-neutral-500">&gt;</span>
        </div>
        <input
          ref={inputRef}
          type="text"
          value={inputVal}
          onChange={(e) => setInputVal(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={isRunning}
          placeholder={isRunning ? 'Command running...' : 'Type PowerShell command (e.g. dir, git status)...'}
          className="flex-1 bg-transparent text-neutral-100 font-mono text-[11.5px] outline-none placeholder:text-neutral-600 disabled:opacity-50"
          autoFocus
        />
        <button
          onClick={() => handleRunCommand()}
          disabled={!inputVal.trim() || isRunning}
          className="p-1 rounded bg-blue-600 hover:bg-blue-500 disabled:bg-neutral-800 text-white disabled:text-neutral-600 transition-colors"
          title="Run command (Enter)"
        >
          <Play className="w-3 h-3 fill-current" />
        </button>
      </div>
    </div>
  );
};
