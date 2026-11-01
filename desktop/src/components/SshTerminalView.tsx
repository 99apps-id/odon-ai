import React, { useState, useEffect, useRef } from 'react';
import {
  Server,
  Terminal as TerminalIcon,
  Play,
  RotateCcw,
  Copy,
  Check,
  Trash2,
  Plus,
  Wifi,
  WifiOff,
  Shield,
  Key,
  ExternalLink,
  ChevronDown,
  X,
  Loader2,
  FolderOpen,
} from 'lucide-react';
import { api, SshProfile } from '../api';

interface SshHistoryItem {
  id: string;
  command: string;
  stdout: string;
  stderr: string;
  exit_code: number;
  timestamp: string;
}

export const SshTerminalView: React.FC = () => {
  const [profiles, setProfiles] = useState<SshProfile[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<SshProfile | null>(null);
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [isConnecting, setIsConnecting] = useState<boolean>(false);

  // Command & Output state
  const [commandInput, setCommandInput] = useState<string>('');
  const [isRunning, setIsRunning] = useState<boolean>(false);
  const [history, setHistory] = useState<SshHistoryItem[]>([]);
  const [commandHistory, setCommandHistory] = useState<string[]>([]);
  const [historyIndex, setHistoryIndex] = useState<number>(-1);
  const [copiedId, setCopiedId] = useState<string | null>(null);

  // Profile modal state
  const [isModalOpen, setIsModalOpen] = useState<boolean>(false);
  const [newAlias, setNewAlias] = useState<string>('');
  const [newHost, setNewHost] = useState<string>('');
  const [newPort, setNewPort] = useState<number>(22);
  const [newUser, setNewUser] = useState<string>('root');
  const [newKeyPath, setNewKeyPath] = useState<string>('');
  const [newDesc, setNewDesc] = useState<string>('');

  const scrollRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const loadProfiles = async () => {
    try {
      const res = await api.getSshProfiles();
      setProfiles(res.profiles || []);
      if (res.profiles.length > 0 && !selectedProfile) {
        setSelectedProfile(res.profiles[0]);
      }
    } catch (err) {
      console.error('Failed to load SSH profiles:', err);
    }
  };

  useEffect(() => {
    loadProfiles();
  }, []);

  useEffect(() => {
    scrollRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [history]);

  const handleConnect = async () => {
    if (!selectedProfile) return;
    setIsConnecting(true);
    // Test SSH connectivity with a quick uname -a or hostname
    try {
      const res = await api.execSsh({
        profile_id: selectedProfile.id,
        command: 'uname -a 2>/dev/null || ver',
      });
      if (res.success || res.exit_code === 0) {
        setIsConnected(true);
        setHistory([
          {
            id: `init-${Date.now()}`,
            command: `# Connected to ${selectedProfile.alias} (${selectedProfile.host}:${selectedProfile.port})`,
            stdout: `Remote Host Banner:\n${res.stdout || 'Linux remote environment ready.'}\nUser: ${selectedProfile.user}\nAuth: ${selectedProfile.auth_type}`,
            stderr: '',
            exit_code: 0,
            timestamp: new Date().toLocaleTimeString(),
          },
        ]);
      } else {
        setIsConnected(false);
        setHistory([
          {
            id: `err-${Date.now()}`,
            command: `# Connection to ${selectedProfile.alias} failed`,
            stdout: '',
            stderr: res.stderr || 'Host unreachable or SSH authentication failed.',
            exit_code: res.exit_code,
            timestamp: new Date().toLocaleTimeString(),
          },
        ]);
      }
    } catch (err) {
      setIsConnected(false);
      console.error('SSH connection failed:', err);
    } finally {
      setIsConnecting(false);
    }
  };

  const handleDisconnect = () => {
    setIsConnected(false);
    setHistory((prev) => [
      ...prev,
      {
        id: `disc-${Date.now()}`,
        command: `# Disconnected from ${selectedProfile?.alias}`,
        stdout: 'Session ended.',
        stderr: '',
        exit_code: 0,
        timestamp: new Date().toLocaleTimeString(),
      },
    ]);
  };

  const handleExecute = async () => {
    const cmd = commandInput.trim();
    if (!cmd || isRunning) return;

    if (cmd === 'clear' || cmd === 'cls') {
      setHistory([]);
      setCommandInput('');
      return;
    }

    setCommandHistory((prev) => [cmd, ...prev]);
    setHistoryIndex(-1);
    setCommandInput('');
    setIsRunning(true);

    try {
      const res = await api.execSsh({
        profile_id: selectedProfile?.id,
        command: cmd,
      });

      const item: SshHistoryItem = {
        id: `exec-${Date.now()}`,
        command: cmd,
        stdout: res.stdout,
        stderr: res.stderr,
        exit_code: res.exit_code,
        timestamp: new Date().toLocaleTimeString(),
      };

      setHistory((prev) => [...prev, item]);
    } catch (err) {
      setHistory((prev) => [
        ...prev,
        {
          id: `err-${Date.now()}`,
          command: cmd,
          stdout: '',
          stderr: String(err),
          exit_code: 1,
          timestamp: new Date().toLocaleTimeString(),
        },
      ]);
    } finally {
      setIsRunning(false);
      setTimeout(() => inputRef.current?.focus(), 50);
    }
  };

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (e.key === 'Enter') {
      handleExecute();
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (commandHistory.length > 0 && historyIndex < commandHistory.length - 1) {
        const nextIndex = historyIndex + 1;
        setHistoryIndex(nextIndex);
        setCommandInput(commandHistory[nextIndex]);
      }
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (historyIndex > 0) {
        const nextIndex = historyIndex - 1;
        setHistoryIndex(nextIndex);
        setCommandInput(commandHistory[nextIndex]);
      } else if (historyIndex === 0) {
        setHistoryIndex(-1);
        setCommandInput('');
      }
    }
  };

  const handleSaveNewProfile = async () => {
    if (!newAlias.trim() || !newHost.trim()) return;
    const profile: SshProfile = {
      id: newAlias.trim().toLowerCase().replace(/\s+/g, '-'),
      alias: newAlias.trim(),
      host: newHost.trim(),
      port: Number(newPort) || 22,
      user: newUser.trim() || 'root',
      key_path: newKeyPath.trim() || undefined,
      auth_type: newKeyPath.trim() ? 'key' : 'password',
      description: newDesc.trim() || undefined,
      created_at: new Date().toISOString(),
    };
    try {
      const res = await api.saveSshProfile(profile);
      setProfiles(res.profiles || []);
      setSelectedProfile(profile);
      setIsModalOpen(false);
      setNewAlias('');
      setNewHost('');
      setNewKeyPath('');
      setNewDesc('');
    } catch (err) {
      console.error('Failed to save profile:', err);
    }
  };

  const handleDeleteProfile = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (!confirm('Are you sure you want to delete this SSH profile?')) return;
    try {
      const res = await api.deleteSshProfile(id);
      setProfiles(res.profiles || []);
      if (selectedProfile?.id === id) {
        setSelectedProfile(res.profiles?.[0] || null);
        setIsConnected(false);
      }
    } catch (err) {
      console.error('Failed to delete profile:', err);
    }
  };

  const handleCopyOutput = (text: string, id: string) => {
    navigator.clipboard.writeText(text);
    setCopiedId(id);
    setTimeout(() => setCopiedId(null), 1500);
  };

  return (
    <div className="flex-1 flex flex-col overflow-hidden rounded-2xl border border-neutral-200/90 bg-white shadow-2xs font-sans">
      {/* ───────────────────────────────────────────────────────────
          SSH HEADER & PROFILE SELECTOR
      ─────────────────────────────────────────────────────────── */}
      <div className="p-3 border-b border-neutral-200 bg-neutral-50/70 flex items-center justify-between shrink-0">
        <div className="flex items-center gap-3">
          <div className="flex items-center gap-2">
            <Server className="w-4 h-4 text-purple-600" />
            <span className="font-bold text-xs text-neutral-800">Remote SSH Terminal</span>
          </div>

          {/* Profile Dropdown */}
          <div className="flex items-center gap-1.5 bg-white border border-neutral-200 rounded-xl px-2.5 py-1 text-xs">
            <span className="text-[11px] text-neutral-400">Host:</span>
            <select
              value={selectedProfile?.id || ''}
              onChange={(e) => {
                const p = profiles.find((prof) => prof.id === e.target.value);
                if (p) {
                  setSelectedProfile(p);
                  setIsConnected(false);
                }
              }}
              className="bg-transparent outline-none font-semibold text-neutral-800 cursor-pointer"
            >
              {profiles.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.alias} ({p.user}@{p.host}:{p.port})
                </option>
              ))}
            </select>
          </div>

          {/* Status Indicator */}
          <div className="flex items-center gap-1.5 px-2 py-0.5 rounded-full text-[11px] border bg-white">
            <span
              className={`w-2 h-2 rounded-full ${
                isConnected ? 'bg-emerald-500 animate-pulse' : 'bg-neutral-300'
              }`}
            />
            <span className={isConnected ? 'text-emerald-700 font-medium' : 'text-neutral-500'}>
              {isConnecting ? 'Connecting...' : isConnected ? 'Online' : 'Disconnected'}
            </span>
          </div>
        </div>

        {/* Action Controls */}
        <div className="flex items-center gap-2">
          {!isConnected ? (
            <button
              onClick={handleConnect}
              disabled={isConnecting || !selectedProfile}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-white bg-purple-600 hover:bg-purple-700 disabled:opacity-50 cursor-pointer shadow-xs transition-colors"
            >
              {isConnecting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Wifi className="w-3.5 h-3.5" />}
              <span>Connect</span>
            </button>
          ) : (
            <button
              onClick={handleDisconnect}
              className="flex items-center gap-1.5 px-3 py-1.5 rounded-xl text-xs font-semibold text-red-600 bg-red-50 hover:bg-red-100 border border-red-200 cursor-pointer transition-colors"
            >
              <WifiOff className="w-3.5 h-3.5" />
              <span>Disconnect</span>
            </button>
          )}

          <button
            onClick={() => setIsModalOpen(true)}
            className="flex items-center gap-1 px-2.5 py-1.5 rounded-xl text-xs font-medium text-neutral-600 hover:bg-neutral-200/60 border border-neutral-200 cursor-pointer"
            title="Add SSH Host"
          >
            <Plus className="w-3.5 h-3.5" />
            <span>Add Host</span>
          </button>

          <button
            onClick={() => setHistory([])}
            className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-200/60 rounded-xl cursor-pointer"
            title="Clear Terminal"
          >
            <Trash2 className="w-3.5 h-3.5" />
          </button>
        </div>
      </div>

      {/* ───────────────────────────────────────────────────────────
          TERMINAL SCROLL OUTPUT AREA
      ─────────────────────────────────────────────────────────── */}
      <div className="flex-1 overflow-y-auto p-4 bg-[#0d1117] font-mono text-xs text-[#c9d1d9] space-y-4 select-text">
        {history.length === 0 ? (
          <div className="p-8 text-center text-neutral-500">
            {isConnected ? (
              <span>Remote session ready. Type any Linux command below and press Enter.</span>
            ) : (
              <span>Click Connect above to establish an SSH session with {selectedProfile?.alias || 'remote host'}.</span>
            )}
          </div>
        ) : (
          history.map((item) => (
            <div key={item.id} className="space-y-1">
              <div className="flex items-center justify-between text-neutral-400 text-[11px] border-b border-neutral-800 pb-1">
                <div className="flex items-center gap-1.5 text-purple-400">
                  <span className="text-neutral-500">$</span>
                  <span className="text-white font-semibold">{item.command}</span>
                </div>
                <div className="flex items-center gap-2">
                  <span>{item.timestamp}</span>
                  <button
                    onClick={() => handleCopyOutput(item.stdout || item.stderr, item.id)}
                    className="hover:text-white p-0.5 cursor-pointer"
                    title="Copy output"
                  >
                    {copiedId === item.id ? (
                      <Check className="w-3 h-3 text-emerald-400" />
                    ) : (
                      <Copy className="w-3 h-3" />
                    )}
                  </button>
                </div>
              </div>

              {item.stdout && (
                <pre className="whitespace-pre-wrap break-all text-neutral-200 font-mono text-[11.5px] leading-relaxed">
                  {item.stdout}
                </pre>
              )}

              {item.stderr && (
                <pre className="whitespace-pre-wrap break-all text-rose-400 font-mono text-[11.5px] leading-relaxed">
                  {item.stderr}
                </pre>
              )}
            </div>
          ))
        )}
        <div ref={scrollRef} />
      </div>

      {/* ───────────────────────────────────────────────────────────
          COMMAND INPUT BAR
      ─────────────────────────────────────────────────────────── */}
      <div className="p-2.5 border-t border-neutral-200 bg-white flex items-center gap-2 shrink-0">
        <div className="flex items-center gap-1 font-mono text-xs text-purple-600 font-bold px-1">
          <span>{selectedProfile?.alias || 'ssh'}&gt;</span>
        </div>

        <input
          ref={inputRef}
          type="text"
          value={commandInput}
          onChange={(e) => setCommandInput(e.target.value)}
          onKeyDown={handleKeyDown}
          disabled={!isConnected || isRunning}
          placeholder={isConnected ? 'Type command (e.g. ls -la, htop, docker ps)...' : 'Connect to execute commands'}
          className="flex-1 text-xs font-mono text-neutral-800 outline-none bg-transparent placeholder-neutral-400"
          autoFocus
        />

        <button
          onClick={handleExecute}
          disabled={!isConnected || !commandInput.trim() || isRunning}
          className="flex items-center gap-1 px-3 py-1.5 rounded-xl bg-purple-600 hover:bg-purple-700 disabled:opacity-40 text-white text-xs font-medium cursor-pointer shadow-xs transition-colors"
        >
          {isRunning ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Play className="w-3.5 h-3.5 fill-current" />}
          <span>Send</span>
        </button>
      </div>

      {/* ───────────────────────────────────────────────────────────
          ADD SSH PROFILE MODAL
      ─────────────────────────────────────────────────────────── */}
      {isModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/40 backdrop-blur-2xs">
          <div className="w-full max-w-md bg-white rounded-2xl shadow-xl border border-neutral-200 p-5 space-y-4">
            <div className="flex items-center justify-between font-bold text-sm text-neutral-900">
              <div className="flex items-center gap-2">
                <Server className="w-4 h-4 text-purple-600" />
                <span>Add Remote SSH Host</span>
              </div>
              <button onClick={() => setIsModalOpen(false)} className="text-neutral-400 hover:text-neutral-700">
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="space-y-3 text-xs">
              <div>
                <label className="text-neutral-500 font-mono mb-1 block">Profile Name / Alias *</label>
                <input
                  type="text"
                  value={newAlias}
                  onChange={(e) => setNewAlias(e.target.value)}
                  placeholder="e.g. prod-server, vps-backup"
                  className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-purple-500"
                />
              </div>

              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2">
                  <label className="text-neutral-500 font-mono mb-1 block">Host IP / Domain *</label>
                  <input
                    type="text"
                    value={newHost}
                    onChange={(e) => setNewHost(e.target.value)}
                    placeholder="103.126.116.111 or example.com"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-purple-500"
                  />
                </div>
                <div>
                  <label className="text-neutral-500 font-mono mb-1 block">Port</label>
                  <input
                    type="number"
                    value={newPort}
                    onChange={(e) => setNewPort(Number(e.target.value))}
                    placeholder="22"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-purple-500"
                  />
                </div>
              </div>

              <div className="grid grid-cols-2 gap-2">
                <div>
                  <label className="text-neutral-500 font-mono mb-1 block">User</label>
                  <input
                    type="text"
                    value={newUser}
                    onChange={(e) => setNewUser(e.target.value)}
                    placeholder="root or ubuntu"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-purple-500"
                  />
                </div>
                <div>
                  <label className="text-neutral-500 font-mono mb-1 block">Key Path (optional)</label>
                  <input
                    type="text"
                    value={newKeyPath}
                    onChange={(e) => setNewKeyPath(e.target.value)}
                    placeholder="~/.ssh/id_rsa or ~/.ssh/key.pem"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-purple-500"
                  />
                </div>
              </div>

              <div>
                <label className="text-neutral-500 font-mono mb-1 block">Description</label>
                <input
                  type="text"
                  value={newDesc}
                  onChange={(e) => setNewDesc(e.target.value)}
                  placeholder="e.g. Production web application host"
                  className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 outline-none focus:border-purple-500"
                />
              </div>
            </div>

            <div className="flex justify-end gap-2 pt-2 border-t border-neutral-100">
              <button
                onClick={() => setIsModalOpen(false)}
                className="px-3.5 py-1.5 rounded-lg text-xs font-medium text-neutral-600 hover:bg-neutral-100 cursor-pointer"
              >
                Cancel
              </button>
              <button
                onClick={handleSaveNewProfile}
                disabled={!newAlias.trim() || !newHost.trim()}
                className="px-3.5 py-1.5 rounded-lg text-xs font-medium text-white bg-purple-600 hover:bg-purple-700 disabled:opacity-40 cursor-pointer shadow-xs"
              >
                Save Profile
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
};
