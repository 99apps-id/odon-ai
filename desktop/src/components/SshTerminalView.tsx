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
  UploadCloud,
  FileCheck,
  FileKey,
  FileText,
  Clipboard,
} from 'lucide-react';
import { api, SshProfile } from '../api';
import { NativeTerminal } from './NativeTerminal';

export const SshTerminalView: React.FC = () => {
  const [profiles, setProfiles] = useState<SshProfile[]>([]);
  const [selectedProfile, setSelectedProfile] = useState<SshProfile | null>(null);
  const [isConnected, setIsConnected] = useState<boolean>(false);
  const [isConnecting, setIsConnecting] = useState<boolean>(false);

  // Profile modal state
  const [isModalOpen, setIsModalOpen] = useState<boolean>(false);
  const [newAlias, setNewAlias] = useState<string>('');
  const [newHost, setNewHost] = useState<string>('');
  const [newPort, setNewPort] = useState<number>(22);
  const [newUser, setNewUser] = useState<string>('root');
  const [newKeyPath, setNewKeyPath] = useState<string>('');
  const [newDesc, setNewDesc] = useState<string>('');

  // Certificate & Key import / paste state
  const [keyInputTab, setKeyInputTab] = useState<'file' | 'paste' | 'path'>('file');
  const [importedCertName, setImportedCertName] = useState<string | null>(null);
  const [pastedKey, setPastedKey] = useState<string>('');
  const [isImportingCert, setIsImportingCert] = useState<boolean>(false);
  const [isPastingKey, setIsPastingKey] = useState<boolean>(false);
  const [pasteSuccessMessage, setPasteSuccessMessage] = useState<string | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

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

  const handleConnect = () => {
    if (!selectedProfile) return;
    setIsConnecting(true);
    setTimeout(() => {
      setIsConnected(true);
      setIsConnecting(false);
    }, 400);
  };

  const handleDisconnect = () => {
    setIsConnected(false);
  };

  const handleCertFileChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;

    setIsImportingCert(true);
    try {
      const reader = new FileReader();
      reader.onload = async () => {
        const content = typeof reader.result === 'string' ? reader.result : '';
        try {
          const res = await api.importSshCertificate(file.name, content);
          if (res.success && res.saved_path) {
            setNewKeyPath(res.saved_path);
            setImportedCertName(file.name);
          }
        } catch (err) {
          console.error('Failed to import certificate to backend:', err);
          // Fallback: set key path to file name
          setNewKeyPath(file.name);
          setImportedCertName(file.name);
        } finally {
          setIsImportingCert(false);
        }
      };
      reader.readAsText(file);
    } catch (err) {
      console.error('Failed to read file:', err);
      setIsImportingCert(false);
    }
  };

  const handleApplyPastedKey = async () => {
    if (!pastedKey.trim()) return;
    setIsPastingKey(true);
    setPasteSuccessMessage(null);
    try {
      const cleanAlias = (newAlias.trim().toLowerCase().replace(/[^a-z0-9_-]/g, '_') || 'custom-key');
      const filename = `${cleanAlias}.pem`;
      const res = await api.importSshCertificate(filename, pastedKey.trim());
      if (res.success && res.saved_path) {
        setNewKeyPath(res.saved_path);
        setImportedCertName(filename);
        setPasteSuccessMessage(`Key saved to ${filename}`);
      }
    } catch (err) {
      console.error('Failed to save pasted key:', err);
      setPasteSuccessMessage('Failed to save key to backend.');
    } finally {
      setIsPastingKey(false);
    }
  };

  const handleSaveNewProfile = async () => {
    if (!newAlias.trim() || !newHost.trim()) return;

    let finalKeyPath = newKeyPath.trim();
    let finalCertName = importedCertName;

    // If user pasted key and didn't click save button yet, auto-save key first
    if (keyInputTab === 'paste' && pastedKey.trim() && !finalKeyPath) {
      try {
        const cleanAlias = (newAlias.trim().toLowerCase().replace(/[^a-z0-9_-]/g, '_') || 'custom-key');
        const filename = `${cleanAlias}.pem`;
        const res = await api.importSshCertificate(filename, pastedKey.trim());
        if (res.success && res.saved_path) {
          finalKeyPath = res.saved_path;
          finalCertName = filename;
        }
      } catch (err) {
        console.error('Auto import pasted key failed:', err);
      }
    }

    const generatedId =
      typeof crypto !== 'undefined' && typeof crypto.randomUUID === 'function'
        ? `ssh-${crypto.randomUUID()}`
        : `ssh-${Date.now()}-${Math.random().toString(36).slice(2, 8)}`;
    const newProfile: SshProfile = {
      id: generatedId,
      alias: newAlias.trim(),
      host: newHost.trim(),
      port: newPort || 22,
      user: newUser.trim() || 'root',
      key_path: finalKeyPath || undefined,
      certificate_filename: finalCertName || undefined,
      auth_type: finalKeyPath || finalCertName ? 'certificate' : 'password',
      description: newDesc.trim() || undefined,
      created_at: new Date().toISOString(),
    };

    try {
      const res = await api.saveSshProfile(newProfile);
      setProfiles(res.profiles || []);
      setSelectedProfile(newProfile);
      setIsModalOpen(false);
      resetModalForm();
    } catch (err) {
      console.error('Failed to save profile:', err);
    }
  };

  const handleDeleteProfile = async (id: string, e: React.MouseEvent) => {
    e.stopPropagation();
    if (!confirm('Are you sure you want to delete this SSH host profile?')) return;
    try {
      const res = await api.deleteSshProfile(id);
      setProfiles(res.profiles || []);
      if (selectedProfile?.id === id) {
        setSelectedProfile(res.profiles[0] || null);
        setIsConnected(false);
      }
    } catch (err) {
      console.error('Failed to delete profile:', err);
    }
  };

  const resetModalForm = () => {
    setNewAlias('');
    setNewHost('');
    setNewPort(22);
    setNewUser('root');
    setNewKeyPath('');
    setNewDesc('');
    setImportedCertName(null);
    setPastedKey('');
    setPasteSuccessMessage(null);
    setKeyInputTab('file');
  };

  return (
    <div className="flex-1 flex overflow-hidden rounded-2xl border border-neutral-200/90 bg-white shadow-2xs">
      {/* ───────────────────────────────────────────────────────────
          LEFT: HOST / SERVERS SIDEBAR
      ─────────────────────────────────────────────────────────── */}
      <div className="w-72 shrink-0 border-r border-neutral-200 flex flex-col bg-neutral-50/50">
        <div className="p-3 border-b border-neutral-200 flex items-center justify-between">
          <div className="flex items-center gap-1.5 font-bold text-xs text-neutral-800">
            <Server className="w-4 h-4 text-blue-600" />
            <span>Remote Hosts</span>
          </div>

          <button
            onClick={() => {
              resetModalForm();
              setIsModalOpen(true);
            }}
            className="flex items-center gap-1 px-2.5 py-1 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-[11px] font-semibold transition-colors shadow-2xs cursor-pointer"
          >
            <Plus className="w-3 h-3" />
            <span>Add Host</span>
          </button>
        </div>

        {/* Profiles List */}
        <div className="flex-1 overflow-y-auto p-2 space-y-1">
          {profiles.length === 0 ? (
            <div className="p-6 text-center text-xs text-neutral-400">
              No SSH hosts configured. Click Add Host above.
            </div>
          ) : (
            profiles.map((p) => {
              const isSelected = selectedProfile?.id === p.id;
              return (
                <div
                  key={p.id}
                  onClick={() => {
                    if (selectedProfile?.id !== p.id) {
                      setSelectedProfile(p);
                      setIsConnected(false);
                    }
                  }}
                  className={`group p-2.5 rounded-xl border text-xs cursor-pointer transition-all flex items-start justify-between ${
                    isSelected
                      ? 'bg-blue-50/70 border-blue-200 shadow-2xs text-blue-950'
                      : 'bg-white border-neutral-200/70 hover:border-neutral-300 text-neutral-800'
                  }`}
                >
                  <div className="space-y-1 overflow-hidden">
                    <div className="flex items-center gap-1.5">
                      <span className={`w-2 h-2 rounded-full ${isSelected && isConnected ? 'bg-emerald-500 animate-pulse' : 'bg-neutral-300'}`} />
                      <span className="font-bold truncate">{p.alias}</span>
                    </div>

                    <div className="text-[11px] font-mono text-neutral-500 truncate">
                      {p.user}@{p.host}:{p.port}
                    </div>

                    {p.key_path && (
                      <div className="flex items-center gap-1 text-[10px] text-blue-600 font-mono">
                        <Key className="w-2.5 h-2.5" />
                        <span className="truncate">{p.certificate_filename || p.key_path.split('/').pop() || p.key_path.split('\\').pop()}</span>
                      </div>
                    )}
                  </div>

                  <button
                    onClick={(e) => handleDeleteProfile(p.id, e)}
                    className="opacity-0 group-hover:opacity-100 p-1 text-neutral-400 hover:text-clay-600 rounded transition-opacity"
                    title="Delete profile"
                  >
                    <Trash2 className="w-3.5 h-3.5" />
                  </button>
                </div>
              );
            })
          )}
        </div>
      </div>

      {/* ───────────────────────────────────────────────────────────
          RIGHT: NATIVE FULL COMMAND TERMINAL PANE
      ─────────────────────────────────────────────────────────── */}
      <div className="flex-1 flex flex-col overflow-hidden bg-neutral-900">
        {/* Terminal Header */}
        <div className="px-4 py-2 bg-neutral-800 border-b border-neutral-700 flex items-center justify-between shrink-0">
          <div className="flex items-center gap-2">
            <TerminalIcon className="w-4 h-4 text-blue-400" />
            <span className="font-bold text-xs text-white">
              {selectedProfile ? `${selectedProfile.user}@${selectedProfile.host}:${selectedProfile.port}` : 'SSH Remote Terminal'}
            </span>
            {selectedProfile?.alias && (
              <span className="px-2 py-0.5 rounded-full bg-blue-900/60 text-blue-300 font-mono text-[10.5px]">
                {selectedProfile.alias}
              </span>
            )}
            <span className="text-[10px] text-neutral-400 font-mono hidden md:inline">• Full Command Interactive Terminal</span>
          </div>

          <div className="flex items-center gap-2">
            {isConnected ? (
              <button
                onClick={handleDisconnect}
                className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-clay-600/20 hover:bg-clay-600/30 text-clay-300 border border-clay-500/30 text-xs font-semibold cursor-pointer transition-colors"
              >
                <WifiOff className="w-3.5 h-3.5" />
                <span>Disconnect</span>
              </button>
            ) : (
              <button
                onClick={handleConnect}
                disabled={!selectedProfile || isConnecting}
                className="flex items-center gap-1.5 px-3 py-1 rounded-lg bg-emerald-600 hover:bg-emerald-700 disabled:opacity-40 text-white text-xs font-semibold cursor-pointer shadow-2xs transition-colors"
              >
                {isConnecting ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Wifi className="w-3.5 h-3.5" />}
                <span>{isConnecting ? 'Connecting...' : 'Connect Host'}</span>
              </button>
            )}
          </div>
        </div>

        {/* Real Interactive Terminal or Connection Prompt */}
        <div className="flex-1 relative overflow-hidden">
          {isConnected && selectedProfile ? (
            <NativeTerminal
              key={selectedProfile.id}
              sshProfileId={selectedProfile.id}
              title={`SSH: ${selectedProfile.user}@${selectedProfile.host}`}
              className="w-full h-full rounded-none border-none"
            />
          ) : (
            <div className="w-full h-full flex flex-col items-center justify-center p-8 text-center text-neutral-400 space-y-4">
              <div className="w-14 h-14 rounded-2xl bg-neutral-800 flex items-center justify-center text-blue-400 border border-neutral-700 shadow-inner">
                <Server className="w-7 h-7" />
              </div>
              <div className="space-y-1 max-w-sm">
                <h3 className="text-sm font-bold text-neutral-200">
                  {selectedProfile ? `Ready to connect to ${selectedProfile.alias}` : 'No Host Selected'}
                </h3>
                <p className="text-xs text-neutral-400 leading-relaxed">
                  {selectedProfile
                    ? `Click "Connect Host" in the top-right corner to open a real interactive terminal with full command access to ${selectedProfile.host}.`
                    : 'Select a host in the left sidebar or add a new one with the Add Host button.'}
                </p>
              </div>

              {selectedProfile && (
                <button
                  onClick={handleConnect}
                  disabled={isConnecting}
                  className="flex items-center gap-2 px-4 py-2 rounded-xl bg-blue-600 hover:bg-blue-500 text-white text-xs font-semibold shadow-xs cursor-pointer transition-colors"
                >
                  {isConnecting ? <Loader2 className="w-4 h-4 animate-spin" /> : <Play className="w-4 h-4 fill-current" />}
                  <span>Launch Interactive SSH Terminal</span>
                </button>
              )}
            </div>
          )}
        </div>
      </div>

      {/* ───────────────────────────────────────────────────────────
          ADD SSH PROFILE MODAL WITH CERTIFICATE / KEY IMPORT
      ─────────────────────────────────────────────────────────── */}
      {isModalOpen && (
        <div className="fixed inset-0 z-50 flex items-center justify-center p-4 bg-neutral-200/70 backdrop-blur-2xs">
          <div className="w-full max-w-md bg-white rounded-2xl shadow-xl border border-neutral-200 p-5 space-y-4">
            <div className="flex items-center justify-between font-bold text-sm text-neutral-900">
              <div className="flex items-center gap-2">
                <Server className="w-4 h-4 text-blue-600" />
                <span>Add Remote SSH Host</span>
              </div>
              <button onClick={() => setIsModalOpen(false)} className="text-neutral-400 hover:text-neutral-700 cursor-pointer">
                <X className="w-4 h-4" />
              </button>
            </div>

            <div className="space-y-3 text-xs">
              <div>
                <label className="text-neutral-600 font-semibold mb-1 block">Profile Name / Alias *</label>
                <input
                  type="text"
                  value={newAlias}
                  onChange={(e) => setNewAlias(e.target.value)}
                  placeholder="e.g. prod-server, cloud-vps"
                  className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-blue-500"
                />
              </div>

              <div className="grid grid-cols-3 gap-2">
                <div className="col-span-2">
                  <label className="text-neutral-600 font-semibold mb-1 block">Host IP / Domain *</label>
                  <input
                    type="text"
                    value={newHost}
                    onChange={(e) => setNewHost(e.target.value)}
                    placeholder="192.168.1.100 or server.example.com"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-blue-500"
                  />
                </div>
                <div>
                  <label className="text-neutral-600 font-semibold mb-1 block">Port</label>
                  <input
                    type="number"
                    value={newPort}
                    onChange={(e) => setNewPort(Number(e.target.value))}
                    placeholder="22"
                    className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-blue-500"
                  />
                </div>
              </div>

              <div>
                <label className="text-neutral-600 font-semibold mb-1 block">User *</label>
                <input
                  type="text"
                  value={newUser}
                  onChange={(e) => setNewUser(e.target.value)}
                  placeholder="root or ubuntu"
                  className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 font-mono outline-none focus:border-blue-500"
                />
              </div>

              {/* ───────────────────────────────────────────
                  CERTIFICATE / PRIVATE KEY IMPORT & PASTE SECTION
              ─────────────────────────────────────────── */}
              <div className="p-3 rounded-xl bg-blue-50/40 border border-blue-200/80 space-y-2.5">
                <div className="flex items-center justify-between">
                  <div className="flex items-center gap-1.5 font-semibold text-neutral-800">
                    <FileKey className="w-4 h-4 text-blue-600" />
                    <span>SSH Key & Certificate</span>
                  </div>
                  <span className="text-[10px] text-blue-700 bg-blue-100/70 font-mono px-2 py-0.5 rounded-md">
                    .pem • .key • id_rsa • raw
                  </span>
                </div>

                {/* Mode Switcher Tabs */}
                <div className="flex p-0.5 rounded-lg bg-neutral-200/70 text-[11px] font-medium text-neutral-600">
                  <button
                    type="button"
                    onClick={() => setKeyInputTab('file')}
                    className={`flex-1 py-1 rounded-md text-center transition-all cursor-pointer ${
                      keyInputTab === 'file'
                        ? 'bg-blue-600 text-white font-semibold shadow-2xs'
                        : 'hover:text-neutral-900'
                    }`}
                  >
                    Impor File Key
                  </button>
                  <button
                    type="button"
                    onClick={() => setKeyInputTab('paste')}
                    className={`flex-1 py-1 rounded-md text-center transition-all cursor-pointer ${
                      keyInputTab === 'paste'
                        ? 'bg-blue-600 text-white font-semibold shadow-2xs'
                        : 'hover:text-neutral-900'
                    }`}
                  >
                    Paste Key Text
                  </button>
                  <button
                    type="button"
                    onClick={() => setKeyInputTab('path')}
                    className={`flex-1 py-1 rounded-md text-center transition-all cursor-pointer ${
                      keyInputTab === 'path'
                        ? 'bg-blue-600 text-white font-semibold shadow-2xs'
                        : 'hover:text-neutral-900'
                    }`}
                  >
                    Path Lokal
                  </button>
                </div>

                {/* TAB 1: FILE UPLOAD */}
                {keyInputTab === 'file' && (
                  <div className="space-y-2">
                    <p className="text-[11px] text-neutral-600 leading-snug">
                      Select a certificate file (<code className="font-mono text-blue-700 bg-blue-100/60 px-1 py-0.5 rounded">.pem</code>, <code className="font-mono text-blue-700 bg-blue-100/60 px-1 py-0.5 rounded">.key</code>, <code className="font-mono text-blue-700 bg-blue-100/60 px-1 py-0.5 rounded">id_rsa</code>) from your computer:
                    </p>

                    <input
                      type="file"
                      ref={fileInputRef}
                      onChange={handleCertFileChange}
                      accept=".pem,.key,.crt,.cer,.pub,id_rsa,id_ed25519,*"
                      className="hidden"
                    />

                    <div className="flex items-center gap-2">
                      <button
                        type="button"
                        onClick={() => fileInputRef.current?.click()}
                        disabled={isImportingCert}
                        className="flex-1 flex items-center justify-center gap-1.5 py-2 px-3 rounded-lg border border-blue-300 bg-white hover:bg-blue-50/80 text-blue-700 font-semibold transition-colors cursor-pointer shadow-2xs"
                      >
                        {isImportingCert ? (
                          <Loader2 className="w-3.5 h-3.5 animate-spin" />
                        ) : (
                          <UploadCloud className="w-3.5 h-3.5 text-blue-600" />
                        )}
                        <span>{isImportingCert ? 'Importing file...' : 'Select Certificate / Key File'}</span>
                      </button>
                    </div>

                    {importedCertName && (
                      <div className="flex items-center justify-between text-[11px] text-emerald-700 bg-emerald-50 px-2.5 py-1.5 rounded-lg border border-emerald-200">
                        <div className="flex items-center gap-1.5 truncate">
                          <FileCheck className="w-3.5 h-3.5 shrink-0" />
                          <span className="truncate font-mono">File siap: {importedCertName}</span>
                        </div>
                        <button
                          type="button"
                          onClick={() => {
                            setImportedCertName(null);
                            setNewKeyPath('');
                          }}
                          className="text-neutral-400 hover:text-clay-600 p-0.5 cursor-pointer"
                          title="Delete file"
                        >
                          <X className="w-3 h-3" />
                        </button>
                      </div>
                    )}
                  </div>
                )}

                {/* TAB 2: PASTE KEY TEXT */}
                {keyInputTab === 'paste' && (
                  <div className="space-y-2">
                    <p className="text-[11px] text-neutral-600 leading-snug">
                      Paste your RSA/Ed25519 private key or public key here:
                    </p>

                    <textarea
                      rows={4}
                      value={pastedKey}
                      onChange={(e) => {
                        setPastedKey(e.target.value);
                        setPasteSuccessMessage(null);
                      }}
                      placeholder={`-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAACFwAAAAdzc2gtcn\n...\n-----END OPENSSH PRIVATE KEY-----`}
                      className="w-full p-2.5 text-[11px] font-mono leading-tight rounded-lg border border-neutral-300 bg-white outline-none focus:border-blue-500 placeholder:text-neutral-300 resize-y"
                    />

                    <div className="flex items-center justify-between gap-2">
                      <span className="text-[10px] text-neutral-500 font-mono">
                        {pastedKey.length > 0 ? `${pastedKey.length} characters • ${pastedKey.split('\n').length} lines` : 'No key pasted yet'}
                      </span>

                      <button
                        type="button"
                        onClick={handleApplyPastedKey}
                        disabled={!pastedKey.trim() || isPastingKey}
                        className="flex items-center gap-1 px-3 py-1 rounded-lg bg-blue-600 hover:bg-blue-500 disabled:opacity-40 text-white text-[11px] font-semibold cursor-pointer shadow-2xs transition-colors"
                      >
                        {isPastingKey ? <Loader2 className="w-3 h-3 animate-spin" /> : <Check className="w-3 h-3" />}
                        <span>Use This Key</span>
                      </button>
                    </div>

                    {pasteSuccessMessage && (
                      <div className="flex items-center gap-1.5 text-[11px] text-emerald-700 bg-emerald-50 px-2.5 py-1.5 rounded-lg border border-emerald-200">
                        <FileCheck className="w-3.5 h-3.5 shrink-0" />
                        <span className="truncate font-mono">{pasteSuccessMessage}</span>
                      </div>
                    )}
                  </div>
                )}

                {/* TAB 3: LOCAL PATH */}
                {keyInputTab === 'path' && (
                  <div className="space-y-1.5">
                    <p className="text-[11px] text-neutral-600 leading-snug">
                      Enter the absolute path to an existing key on your computer:
                    </p>
                    <input
                      type="text"
                      value={newKeyPath}
                      onChange={(e) => setNewKeyPath(e.target.value)}
                      placeholder="~/.ssh/id_rsa or C:\Users\...\id_ed25519"
                      className="w-full px-2.5 py-1.5 text-[11.5px] rounded-lg border border-neutral-300 bg-white font-mono outline-none focus:border-blue-500"
                    />
                  </div>
                )}

                {/* Display resolved path if any */}
                {newKeyPath && keyInputTab !== 'path' && (
                  <div className="pt-1 border-t border-blue-200/50">
                    <div className="flex items-center justify-between text-[10px] text-neutral-500 font-mono">
                      <span className="truncate">Key path: {newKeyPath}</span>
                    </div>
                  </div>
                )}
              </div>

              <div>
                <label className="text-neutral-600 font-semibold mb-1 block">Description (optional)</label>
                <input
                  type="text"
                  value={newDesc}
                  onChange={(e) => setNewDesc(e.target.value)}
                  placeholder="e.g. Production Ubuntu VPS host"
                  className="w-full px-3 py-1.5 rounded-lg border border-neutral-300 outline-none focus:border-blue-500"
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
                className="px-3.5 py-1.5 rounded-lg text-xs font-semibold text-white bg-blue-600 hover:bg-blue-500 disabled:opacity-40 cursor-pointer shadow-xs transition-colors"
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
