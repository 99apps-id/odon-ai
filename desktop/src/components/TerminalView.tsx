import React, { useState, useEffect, useRef } from 'react';
import {
  Terminal as TerminalIcon,
  Play,
  RotateCcw,
  Copy,
  Check,
  Trash2,
  FolderOpen,
  Plus,
  X,
  Command,
  ArrowRight,
  Loader2,
  ChevronDown,
  Square,
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

interface TerminalTab {
  id: string;
  title: string;
  shell: string;
  cwd: string;
  history: HistoryItem[];
  commandHistory: string[];
  historyIndex: number;
  isRunning: boolean;
}

interface TerminalViewProps {
  onClose?: () => void;
}

const DEFAULT_CWD = 'C:/Project/odon-ai';

export const TerminalView: React.FC<TerminalViewProps> = ({ onClose }) => {
  const [terminalInfo, setTerminalInfo] = useState<TerminalInfo | null>(null);
  const [daemonOnline, setDaemonOnline] = useState<boolean>(false);
  const [copied, setCopied] = useState<boolean>(false);

  // Multi-Tab Terminal State
  const [tabs, setTabs] = useState<TerminalTab[]>([
    {
      id: 'tab-1',
      title: 'PowerShell #1',
      shell: 'PowerShell',
      cwd: DEFAULT_CWD,
      history: [
        {
          id: 'init-1',
          command: '# ODON Native PTY Terminal Session 1',
          stdout: 'ODON Interactive Multi-Tab Terminal ready.\nType any PowerShell / CLI command and press Enter.\nUse Up/Down arrows to navigate command history.',
          stderr: '',
          exit_code: 0,
          cwd: DEFAULT_CWD,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
        },
      ],
      commandHistory: [],
      historyIndex: -1,
      isRunning: false,
    },
  ]);
  const [activeTabId, setActiveTabId] = useState<string>('tab-1');
  const [inputVal, setInputVal] = useState<string>('');

  const activeTab = tabs.find((t) => t.id === activeTabId) || tabs[0];

  const bottomRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const checkConnection = () => {
    api.getTerminalInfo().then((info) => {
      setTerminalInfo(info);
      setDaemonOnline(true);
    }).catch(() => {
      setDaemonOnline(false);
      setTerminalInfo({
        shell: 'PowerShell',
        os: 'windows',
        arch: 'x86_64',
        default_cwd: DEFAULT_CWD,
        user: 'developer',
      });
    });
  };

  useEffect(() => {
    checkConnection();
  }, []);

  // Auto scroll to bottom
  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [activeTab?.history, activeTab?.isRunning]);

  const handleCreateTab = () => {
    const newId = `tab-${Date.now()}`;
    const newNumber = tabs.length + 1;
    const newTab: TerminalTab = {
      id: newId,
      title: `Terminal #${newNumber}`,
      shell: terminalInfo?.shell || 'PowerShell',
      cwd: activeTab?.cwd || DEFAULT_CWD,
      history: [
        {
          id: `init-${newId}`,
          command: `# ODON Native PTY Terminal Session ${newNumber}`,
          stdout: 'New interactive session initialized.',
          stderr: '',
          exit_code: 0,
          cwd: activeTab?.cwd || DEFAULT_CWD,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' }),
        },
      ],
      commandHistory: [],
      historyIndex: -1,
      isRunning: false,
    };
    setTabs((prev) => [...prev, newTab]);
    setActiveTabId(newId);
    setTimeout(() => inputRef.current?.focus(), 50);
  };

  const handleCloseTab = (tabId: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (tabs.length === 1) return;
    const nextTabs = tabs.filter((t) => t.id !== tabId);
    setTabs(nextTabs);
    if (activeTabId === tabId) {
      setActiveTabId(nextTabs[0].id);
    }
  };

  const handleClear = () => {
    setTabs((prev) =>
      prev.map((t) => (t.id === activeTabId ? { ...t, history: [] } : t))
    );
  };

  const handleCopyHistory = () => {
    if (!activeTab) return;
    const text = activeTab.history
      .map((item) => `$ ${item.command}\n${item.stdout}${item.stderr ? '\n' + item.stderr : ''}`)
      .join('\n\n');
    navigator.clipboard.writeText(text);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const handleRunCommand = async (cmdToRun?: string) => {
    const rawCmd = (cmdToRun !== undefined ? cmdToRun : inputVal).trim();
    if (!rawCmd || activeTab?.isRunning) return;

    setInputVal('');

    if (rawCmd.toLowerCase() === 'cls' || rawCmd.toLowerCase() === 'clear') {
      handleClear();
      return;
    }

    const nowStr = new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' });
    const cwdBefore = activeTab.cwd;

    // Set running state on active tab
    setTabs((prev) =>
      prev.map((t) =>
        t.id === activeTabId
          ? {
              ...t,
              isRunning: true,
              commandHistory: [rawCmd, ...t.commandHistory.filter((c) => c !== rawCmd)],
              historyIndex: -1,
            }
          : t
      )
    );

    try {
      const result: TerminalExecResult = await api.executeTerminal(rawCmd, cwdBefore);
      setDaemonOnline(true);
      const updatedCwd = result.cwd || cwdBefore;

      setTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId
            ? {
                ...t,
                cwd: updatedCwd,
                isRunning: false,
                history: [
                  ...t.history,
                  {
                    id: `${Date.now()}-${Math.random()}`,
                    command: rawCmd,
                    stdout: result.stdout,
                    stderr: result.stderr,
                    exit_code: result.exit_code,
                    cwd: updatedCwd,
                    timestamp: nowStr,
                  },
                ],
              }
            : t
        )
      );
    } catch (err: any) {
      setDaemonOnline(false);
      const isConnectionError = err.message?.includes('Failed to fetch') || err.message?.includes('NetworkError');
      const errorMsg = isConnectionError
        ? `Failed to communicate with ODON daemon (http://127.0.0.1:4096). Please ensure 'odond' is running.\nDetails: ${err.message}`
        : (err.message || 'Execution failed');

      setTabs((prev) =>
        prev.map((t) =>
          t.id === activeTabId
            ? {
                ...t,
                isRunning: false,
                history: [
                  ...t.history,
                  {
                    id: `${Date.now()}-${Math.random()}`,
                    command: rawCmd,
                    stdout: '',
                    stderr: errorMsg,
                    exit_code: 1,
                    cwd: cwdBefore,
                    timestamp: nowStr,
                  },
                ],
              }
            : t
        )
      );
    } finally {
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (!activeTab) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      handleRunCommand();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      const historyList = activeTab.commandHistory;
      if (historyList.length === 0) return;
      const nextIndex = Math.min(activeTab.historyIndex + 1, historyList.length - 1);
      setTabs((prev) =>
        prev.map((t) => (t.id === activeTabId ? { ...t, historyIndex: nextIndex } : t))
      );
      setInputVal(historyList[nextIndex]);
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      const nextIndex = activeTab.historyIndex - 1;
      if (nextIndex < 0) {
        setTabs((prev) =>
          prev.map((t) => (t.id === activeTabId ? { ...t, historyIndex: -1 } : t))
        );
        setInputVal('');
      } else {
        setTabs((prev) =>
          prev.map((t) => (t.id === activeTabId ? { ...t, historyIndex: nextIndex } : t))
        );
        setInputVal(activeTab.commandHistory[nextIndex]);
      }
    } else if (e.ctrlKey && e.key === 'c') {
      // Interrupt signal
      if (activeTab.isRunning) {
        setTabs((prev) =>
          prev.map((t) => (t.id === activeTabId ? { ...t, isRunning: false } : t))
        );
      }
    }
  };

  return (
    <div className="flex flex-col h-full bg-[#0d1117] text-[#c9d1d9] font-mono text-xs select-text">
      {/* Tab bar header */}
      <div className="flex items-center justify-between px-3 py-2 bg-[#161b22] border-b border-[#30363d] shrink-0">
        <div className="flex items-center gap-1.5 overflow-x-auto max-w-full">
          {tabs.map((tab) => {
            const isActive = tab.id === activeTabId;
            return (
              <button
                key={tab.id}
                type="button"
                onClick={() => {
                  setActiveTabId(tab.id);
                  setTimeout(() => inputRef.current?.focus(), 50);
                }}
                className={`group flex items-center gap-2 px-3 py-1.5 rounded-lg text-xs transition-all font-medium border ${
                  isActive
                    ? 'bg-[#0d1117] text-white border-[#388bfd] shadow-sm'
                    : 'bg-[#21262d] text-[#8b949e] border-transparent hover:text-[#c9d1d9] hover:bg-[#30363d]'
                }`}
              >
                <TerminalIcon className={`w-3.5 h-3.5 ${isActive ? 'text-[#58a6ff]' : 'text-[#8b949e]'}`} />
                <span>{tab.title}</span>
                {tabs.length > 1 && (
                  <span
                    onClick={(e) => handleCloseTab(tab.id, e)}
                    className="opacity-0 group-hover:opacity-100 hover:text-rose-400 rounded p-0.5 ml-1 transition-opacity"
                  >
                    <X className="w-3 h-3" />
                  </span>
                )}
              </button>
            );
          })}
          <button
            type="button"
            onClick={handleCreateTab}
            title="New Terminal Tab"
            className="flex items-center justify-center w-7 h-7 rounded-lg bg-[#21262d] text-[#8b949e] hover:text-white hover:bg-[#30363d] transition-all"
          >
            <Plus className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* Toolbar actions */}
        <div className="flex items-center gap-1.5 shrink-0 ml-2">
          <span className="text-[10px] text-[#8b949e] bg-[#21262d] px-2 py-0.5 rounded border border-[#30363d]">
            {activeTab?.shell || 'PowerShell'}
          </span>
          <button
            type="button"
            onClick={handleCopyHistory}
            title="Copy Output"
            className="p-1.5 rounded-md hover:bg-[#30363d] text-[#8b949e] hover:text-white transition-colors"
          >
            {copied ? <Check className="w-3.5 h-3.5 text-emerald-400" /> : <Copy className="w-3.5 h-3.5" />}
          </button>
          <button
            type="button"
            onClick={handleClear}
            title="Clear Buffer"
            className="p-1.5 rounded-md hover:bg-[#30363d] text-[#8b949e] hover:text-white transition-colors"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
          {onClose && (
            <button
              type="button"
              onClick={onClose}
              title="Close Terminal"
              className="p-1.5 rounded-md hover:bg-[#30363d] text-[#8b949e] hover:text-white transition-colors ml-1"
            >
              <X className="w-3.5 h-3.5" />
            </button>
          )}
        </div>
      </div>

      {/* Quick Execution Presets Bar */}
      <div className="flex items-center gap-2 px-3 py-1.5 bg-[#090d13] border-b border-[#21262d] overflow-x-auto text-[11px] shrink-0">
        <span className="text-[#8b949e] uppercase tracking-wider text-[10px]">Quick:</span>
        <button
          onClick={() => handleRunCommand('cargo test -p odon-tools')}
          className="px-2 py-0.5 rounded bg-[#161b22] hover:bg-[#30363d] text-[#58a6ff] border border-[#30363d] transition-colors"
        >
          cargo test
        </button>
        <button
          onClick={() => handleRunCommand('git status -s')}
          className="px-2 py-0.5 rounded bg-[#161b22] hover:bg-[#30363d] text-[#7ee787] border border-[#30363d] transition-colors"
        >
          git status
        </button>
        <button
          onClick={() => handleRunCommand('git log -5 --oneline')}
          className="px-2 py-0.5 rounded bg-[#161b22] hover:bg-[#30363d] text-[#d2a8ff] border border-[#30363d] transition-colors"
        >
          git log
        </button>
        <button
          onClick={() => handleRunCommand('ls')}
          className="px-2 py-0.5 rounded bg-[#161b22] hover:bg-[#30363d] text-[#ffa657] border border-[#30363d] transition-colors"
        >
          ls
        </button>
      </div>

      {/* Terminal Output Log Area */}
      <div className="flex-1 overflow-y-auto p-4 space-y-4">
        {activeTab?.history.map((item) => (
          <div key={item.id} className="space-y-1">
            <div className="flex items-center gap-2 text-[#8b949e]">
              <span className="text-[#58a6ff] font-semibold">❯</span>
              <span className="text-[#e6edf3] font-semibold">{item.command}</span>
              <span className="text-[10px] ml-auto opacity-60">{item.timestamp}</span>
            </div>
            {item.stdout && (
              <pre className="text-[#c9d1d9] whitespace-pre-wrap break-all pl-4 border-l border-[#30363d]/60 leading-relaxed">
                {item.stdout}
              </pre>
            )}
            {item.stderr && (
              <pre className="text-[#ff7b72] whitespace-pre-wrap break-all pl-4 border-l border-[#ff7b72]/40 leading-relaxed">
                {item.stderr}
              </pre>
            )}
          </div>
        ))}

        {activeTab?.isRunning && (
          <div className="flex items-center gap-2 text-[#58a6ff] py-1">
            <Loader2 className="w-3.5 h-3.5 animate-spin" />
            <span className="text-xs">Executing command...</span>
          </div>
        )}

        <div ref={bottomRef} />
      </div>

      {/* Terminal Interactive Prompt Bar */}
      <div className="flex items-center gap-2 p-2.5 bg-[#161b22] border-t border-[#30363d] shrink-0">
        <div className="flex items-center gap-1 text-[#58a6ff] shrink-0 font-semibold text-xs">
          <span>{activeTab?.cwd ? activeTab.cwd.split('/').pop() || activeTab.cwd : 'odon'}</span>
          <span>❯</span>
        </div>
        <input
          ref={inputRef}
          type="text"
          value={inputVal}
          onChange={(e) => setInputVal(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={activeTab?.isRunning}
          placeholder={activeTab?.isRunning ? 'Running process...' : 'Type CLI / PowerShell command...'}
          className="flex-1 bg-transparent border-none text-[#e6edf3] focus:outline-none text-xs font-mono placeholder-[#6e7681]"
          autoFocus
        />
        <button
          type="button"
          onClick={() => handleRunCommand()}
          disabled={!inputVal.trim() || activeTab?.isRunning}
          className="p-1.5 rounded-md bg-[#238636] hover:bg-[#2ea043] disabled:opacity-40 disabled:hover:bg-[#238636] text-white transition-colors"
        >
          <Play className="w-3.5 h-3.5" />
        </button>
      </div>
    </div>
  );
};
