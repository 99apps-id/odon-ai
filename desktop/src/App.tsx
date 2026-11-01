import React, { useState, useEffect, useRef } from 'react';
import {
  MessageSquare,
  Code2,
  Image as ImageIcon,
  FileText,
  Folder,
  Bot,
  Share2,
  Settings,
  Plus,
  Search,
  Bell,
  PanelRightClose,
  PanelRightOpen,
  ChevronDown,
  Paperclip,
  Globe,
  ArrowUp,
  ExternalLink,
  Radio,
  Activity,
  ChevronRight,
  Video,
  Minus,
  Square,
  X,
  Sparkles,
  Layers,
  ArrowLeft,
  CheckCircle2,
  Check,
  Terminal,
  Loader2,
} from 'lucide-react';
import { CatLogo } from './components/CatLogo';
import { CodeEditorView } from './components/CodeEditorView';
import { BrowserPreviewView } from './components/BrowserPreviewView';
import { TerminalView } from './components/TerminalView';
import { SettingsModal } from './components/SettingsModal';
import { api } from './api';
import { Session, Message, Artifact, SystemStatus, ProviderItem, ModelItem, FALLBACK_PROVIDERS } from './types';

interface ChatMessage {
  id: string;
  role: 'user' | 'assistant';
  content: string;
  timestamp: string;
  showAppCard?: boolean;
  thought?: string;
  toolCalls?: string[];
}

// Model lists are now dynamically populated from the active provider chosen in Settings.

const REGISTERED_TOOLS = [
  { name: 'fs_read', category: 'File System', tier: 'Tier 0', desc: 'Read file contents' },
  { name: 'fs_write', category: 'File System', tier: 'Tier 1', desc: 'Write or overwrite file' },
  { name: 'fs_edit', category: 'File System', tier: 'Tier 1', desc: 'Find and replace file text' },
  { name: 'fs_search', category: 'File System', tier: 'Tier 0', desc: 'Search workspace files' },
  { name: 'fs_delete', category: 'File System', tier: 'Tier 1', desc: 'Delete workspace file' },
  { name: 'git_status', category: 'Git', tier: 'Tier 0', desc: 'Working tree status' },
  { name: 'git_diff', category: 'Git', tier: 'Tier 0', desc: 'Inspect uncommitted diffs' },
  { name: 'git_log', category: 'Git', tier: 'Tier 0', desc: 'View commit history' },
  { name: 'git_branch', category: 'Git', tier: 'Tier 0', desc: 'List or create branches' },
  { name: 'web_search', category: 'Web', tier: 'Tier 0', desc: 'DuckDuckGo web search' },
  { name: 'web_fetch', category: 'Web', tier: 'Tier 0', desc: 'Fetch webpage content' },
  { name: 'system_info', category: 'System', tier: 'Tier 0', desc: 'OS, CPU, memory details' },
  { name: 'calc_eval', category: 'Utility', tier: 'Tier 0', desc: 'Safe math evaluator' },
  { name: 'shell_exec', category: 'Execution', tier: 'Tier 2', desc: 'Run PowerShell / sh command' },
  { name: 'process_spawn', category: 'Execution', tier: 'Tier 2', desc: 'Background daemon process' },
  { name: 'memory_store', category: 'Memory', tier: 'Tier 0', desc: 'Save long-term project note' },
  { name: 'memory_recall', category: 'Memory', tier: 'Tier 0', desc: 'Recall persistent memories' },
  { name: 'artifact_create', category: 'Artifact', tier: 'Tier 0', desc: 'Synthesize code/doc/image' },
];

export default function App() {
  // Navigation & View States
  const [activeNav, setActiveNav] = useState('chat');
  const [isRightPanelOpen, setIsRightPanelOpen] = useState(true);
  const [rightPanelView, setRightPanelView] = useState<'artifacts' | 'editor' | 'browser' | 'terminal'>('artifacts');
  // Provider & Model States (Synchronized with Settings)
  const [providers, setProviders] = useState<ProviderItem[]>(FALLBACK_PROVIDERS);
  const [activeProviderId, setActiveProviderId] = useState<string>(() => {
    if (typeof window !== 'undefined') {
      return localStorage.getItem('odon_active_provider') || 'openai';
    }
    return 'openai';
  });
  const [selectedModel, setSelectedModel] = useState<string>(() => {
    if (typeof window !== 'undefined') {
      return localStorage.getItem('odon_selected_model') || 'gpt-4o';
    }
    return 'gpt-4o';
  });
  const [inputMessage, setInputMessage] = useState('');
  const [settingsOpen, setSettingsOpen] = useState(false);

  // Menus & Modals
  const [isModelMenuOpen, setIsModelMenuOpen] = useState(false);
  const [isToolsMenuOpen, setIsToolsMenuOpen] = useState(false);
  const [isSearchOpen, setIsSearchOpen] = useState(false);
  const [searchQuery, setSearchQuery] = useState('');
  const [isBellOpen, setIsBellOpen] = useState(false);
  const [showAllRecent, setShowAllRecent] = useState(false);
  const [relayModalOpen, setRelayModalOpen] = useState<'telegram' | 'whatsapp' | null>(null);
  const fileInputRef = useRef<HTMLInputElement>(null);

  // Active Provider & Model resolution
  const activeProvider = providers.find((p) => p.id === activeProviderId) || providers[0] || FALLBACK_PROVIDERS[0];
  const activeModels = activeProvider?.models || [];
  const currentModelObj = activeModels.find((m) => m.id === selectedModel || `${activeProvider?.id}::${m.id}` === selectedModel || m.name === selectedModel) || activeModels[0];
  const displayModelName = currentModelObj ? currentModelObj.name : selectedModel;

  const handleSelectProvider = (providerId: string) => {
    setActiveProviderId(providerId);
    if (typeof window !== 'undefined') {
      localStorage.setItem('odon_active_provider', providerId);
    }
    const prov = providers.find((p) => p.id === providerId);
    if (prov && prov.models.length > 0) {
      const rec = prov.models.find((m) => m.recommended) || prov.models[0];
      setSelectedModel(rec.id);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_selected_model', rec.id);
      }
    }
  };

  const handleSelectModel = (modelId: string) => {
    if (modelId.includes('::')) {
      const [pId, mId] = modelId.split('::');
      setActiveProviderId(pId);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_active_provider', pId);
      }
      setSelectedModel(mId);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_selected_model', mId);
      }
    } else {
      setSelectedModel(modelId);
      if (typeof window !== 'undefined') {
        localStorage.setItem('odon_selected_model', modelId);
      }
    }
    setIsModelMenuOpen(false);
  };

  // Resizable Right Panel State
  const [rightPanelWidth, setRightPanelWidth] = useState<number>(() => {
    if (typeof window !== 'undefined') {
      const saved = localStorage.getItem('odon_right_panel_width');
      if (saved) {
        const parsed = parseInt(saved, 10);
        if (!isNaN(parsed) && parsed >= 280 && parsed <= 900) return parsed;
      }
    }
    return 340;
  });
  const [isResizingRight, setIsResizingRight] = useState(false);

  useEffect(() => {
    const handleMouseMove = (e: MouseEvent) => {
      if (!isResizingRight) return;
      const newWidth = window.innerWidth - e.clientX - 16;
      if (newWidth >= 280 && newWidth <= Math.min(850, window.innerWidth * 0.65)) {
        setRightPanelWidth(newWidth);
      }
    };

    const handleMouseUp = () => {
      if (isResizingRight) {
        setIsResizingRight(false);
        localStorage.setItem('odon_right_panel_width', rightPanelWidth.toString());
        document.body.style.cursor = '';
        document.body.style.userSelect = '';
      }
    };

    if (isResizingRight) {
      document.body.style.cursor = 'col-resize';
      document.body.style.userSelect = 'none';
      window.addEventListener('mousemove', handleMouseMove);
      window.addEventListener('mouseup', handleMouseUp);
    }

    return () => {
      window.removeEventListener('mousemove', handleMouseMove);
      window.removeEventListener('mouseup', handleMouseUp);
    };
  }, [isResizingRight, rightPanelWidth]);

  // Backend & Data States
  const [daemonOnline, setDaemonOnline] = useState(false);
  const [systemStatus, setSystemStatus] = useState<SystemStatus | null>(null);
  const [sessions, setSessions] = useState<Session[]>([]);
  const [currentSessionId, setCurrentSessionId] = useState<string | null>(null);
  const [artifacts, setArtifacts] = useState<Artifact[]>([]);

  // Chat & Stream State
  const [inChatMode, setInChatMode] = useState(false);
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [isGenerating, setIsGenerating] = useState(false);
  const [streamingText, setStreamingText] = useState('');
  const [streamingThought, setStreamingThought] = useState('');
  const [streamingTool, setStreamingTool] = useState<string | null>(null);

  const chatEndRef = useRef<HTMLDivElement>(null);
  const currentSessionIdRef = useRef<string | null>(null);

  useEffect(() => {
    currentSessionIdRef.current = currentSessionId;
  }, [currentSessionId]);

  // Check daemon status and load sessions
  const checkDaemonStatus = async () => {
    try {
      const status = await api.getStatus();
      setDaemonOnline(true);
      setSystemStatus(status);
    } catch {
      setDaemonOnline(false);
    }
  };

  const loadSessions = async () => {
    try {
      const list = await api.listSessions();
      setSessions(list);
    } catch {
      // Local fallback
    }
  };

  const loadArtifacts = async (sessionId: string) => {
    try {
      const list = await api.listArtifacts(sessionId);
      if (list && list.length > 0) {
        setArtifacts(list);
      }
    } catch { }
  };

  // Initial setup & SSE subscription
  useEffect(() => {
    checkDaemonStatus();
    loadSessions();
    api.getModels().then((data) => {
      if (data?.providers?.length) {
        setProviders(data.providers);
        const storedProvider = localStorage.getItem('odon_active_provider');
        if (!storedProvider || storedProvider === 'openai') {
          const readyProv = data.providers.find((p: any) => p.connected || p.configured);
          if (readyProv) {
            setActiveProviderId(readyProv.id);
            localStorage.setItem('odon_active_provider', readyProv.id);
            const rec = readyProv.models.find((m: any) => m.recommended) || readyProv.models[0];
            if (rec) {
              setSelectedModel(rec.id);
              localStorage.setItem('odon_selected_model', rec.id);
            }
          }
        }
      }
    }).catch(() => {
      // Use fallback
    });

    const statusInterval = setInterval(checkDaemonStatus, 5000);

    const unsubscribe = api.subscribeEvents((event) => {
      if (event.type === 'text_delta') {
        setStreamingText((prev) => prev + event.payload.delta);
      } else if (event.type === 'thought_delta') {
        setStreamingThought((prev) => prev + event.payload.delta);
      } else if (event.type === 'tool_call_started') {
        setStreamingTool(event.payload.tool_name);
      } else if (event.type === 'tool_call_finished') {
        setStreamingTool(null);
      } else if (event.type === 'artifact_created') {
        setArtifacts((prev) => [event.payload.artifact, ...prev.filter((a) => a.id !== event.payload.artifact.id)]);
      } else if (event.type === 'state_changed') {
        if (event.payload.state === 'completed') {
          setIsGenerating(false);
          setStreamingTool(null);
          if (currentSessionId) {
            loadArtifacts(currentSessionId);
            loadSessions();
          }
        }
      }
    });

    return () => {
      clearInterval(statusInterval);
      unsubscribe();
    };
  }, [currentSessionId]);

  // Auto-scroll chat
  useEffect(() => {
    chatEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages, streamingText, streamingThought, isGenerating]);

  // Start a new chat (return to hero)
  const handleNewChat = () => {
    setInChatMode(false);
    setCurrentSessionId(null);
    setMessages([]);
    setInputMessage('');
    setStreamingText('');
    setStreamingThought('');
    setStreamingTool(null);
  };

  // Select an existing session from recent history
  const loadSessionChat = async (id: string, title: string) => {
    setCurrentSessionId(id);
    setInChatMode(true);
    setStreamingText('');
    setStreamingThought('');
    try {
      const data = await api.getSession(id);
      if (data.messages && data.messages.length > 0) {
        setMessages(
          data.messages.map((m) => ({
            id: m.id,
            role: m.role as 'user' | 'assistant',
            content: m.content.text,
            timestamp: new Date(m.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
            showAppCard: m.content.artifact_ids?.length > 0 || m.content.text.includes('```'),
            thought: m.content.thought,
            toolCalls: m.content.tool_calls?.map((t) => t.tool_name),
          }))
        );
      } else {
        setMessages([
          {
            id: 'init-ast',
            role: 'assistant',
            content: `Session "${title}" loaded. How can I help you continue this work?`,
            timestamp: 'Just now',
          },
        ]);
      }
      if (data.artifacts && data.artifacts.length > 0) {
        setArtifacts(data.artifacts);
      }
    } catch {
      setMessages([
        {
          id: 'fb-user',
          role: 'user',
          content: title,
          timestamp: 'Just now',
        },
        {
          id: 'fb-ast',
          role: 'assistant',
          content: `Here is the progress and artifacts generated for "${title}". All components are compiled and verified.`,
          timestamp: 'Just now',
          showAppCard: true,
        },
      ]);
    }
  };

  // Submit prompt from input box or action cards
  const handleSendMessage = async (customText?: string, mode: string = 'general') => {
    const textToSend = (customText || inputMessage).trim();
    if (!textToSend || isGenerating) return;

    const userMsg: ChatMessage = {
      id: `usr_${Date.now()}`,
      role: 'user',
      content: textToSend,
      timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
    };

    setMessages((prev) => [...prev, userMsg]);
    setInputMessage('');
    setInChatMode(true);
    setIsGenerating(true);
    setStreamingText('');
    setStreamingThought('');
    setStreamingTool(null);

    let sid = currentSessionId;

    if (daemonOnline) {
      try {
        if (!sid) {
          const title = textToSend.slice(0, 32) + (textToSend.length > 32 ? '...' : '');
          const newSession = await api.createSession(title, mode, selectedModel);
          sid = newSession.id;
          setCurrentSessionId(sid);
          setSessions((prev) => [newSession, ...prev]);
        }

        const modelPayload = selectedModel.includes('::') ? selectedModel : `${activeProviderId}::${selectedModel}`;
        const resMsg = await api.sendMessage(sid, textToSend, modelPayload);

        const assistantMsg: ChatMessage = {
          id: resMsg.id,
          role: 'assistant',
          content: resMsg.content.text || streamingText,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
          showAppCard: resMsg.content.artifact_ids?.length > 0 || resMsg.content.text.includes('```'),
          thought: resMsg.content.thought,
          toolCalls: resMsg.content.tool_calls?.map((t) => t.tool_name),
        };

        setMessages((prev) => [...prev, assistantMsg]);
        loadArtifacts(sid);
        loadSessions();
      } catch (err: any) {
        console.error('Daemon message send error:', err);
        const errorMsg: ChatMessage = {
          id: `ast_err_${Date.now()}`,
          role: 'assistant',
          content: `⚠️ **Gagal menghubungi model AI**: ${err.message || 'Terjadi kesalahan saat memproses jawaban.'}

Periksa konfigurasi API key atau koneksi akun provider pada menu **Settings**.`,
          timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
        };
        setMessages((prev) => [...prev, errorMsg]);
      } finally {
        setIsGenerating(false);
        setStreamingText('');
        setStreamingThought('');
        setStreamingTool(null);
      }
    } else {
      const offlineMsg: ChatMessage = {
        id: `ast_off_${Date.now()}`,
        role: 'assistant',
        content: `⚠️ **Daemon ODON (odond) sedang offline**.

Pastikan service daemon aktif di \`http://127.0.0.1:4096\` untuk mendapatkan jawaban langsung dari AI model.`,
        timestamp: new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
      };
      setMessages((prev) => [...prev, offlineMsg]);
      setIsGenerating(false);
    }
  };

  // Trigger prompt from hero action cards
  const handleActionCard = (title: string, promptText: string, mode: string) => {
    setActiveNav(mode);
    handleSendMessage(promptText, mode);
  };

  // File attachment handler
  const handleFileAttach = (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0];
    if (!file) return;
    const reader = new FileReader();
    reader.onload = () => {
      const text = typeof reader.result === 'string' ? reader.result : '';
      const snippet = text.length > 2000 ? text.slice(0, 2000) + '\n... (truncated)' : text;
      setInputMessage((prev) => `${prev ? prev + '\n' : ''}[Attached file: ${file.name}]\n\`\`\`\n${snippet}\n\`\`\`\n`);
    };
    reader.readAsText(file);
    e.target.value = '';
  };

  // Dynamic input placeholder based on active navigation tab
  const getPlaceholder = () => {
    switch (activeNav) {
      case 'code':
        return 'Describe code to generate, refactor, or test...';
      case 'create':
        return 'Describe an image or visual creative asset to generate...';
      case 'documents':
        return 'Draft a PRD, technical specification, or document...';
      case 'files':
        return 'Ask ODON to search, inspect, or edit workspace files...';
      case 'agents':
        return 'Define an autonomous multi-step agent mission...';
      case 'automations':
        return 'Configure a background automation or scheduled workflow...';
      case 'terminal':
        return 'Ask ODON to run terminal commands or script in workspace...';
      default:
        return 'Ask ODON anything...';
    }
  };

  // Window control actions
  const handleMinimize = () => {
    setIsRightPanelOpen(false);
  };

  const handleMaximize = () => {
    if (!document.fullscreenElement) {
      document.documentElement.requestFullscreen().catch(() => { });
    } else {
      document.exitFullscreen().catch(() => { });
    }
  };

  const handleClose = () => {
    handleNewChat();
  };

  // Extract generated code files for CodeEditorView
  const latestCodeArtifact = artifacts.find((a) => a.kind === 'code');
  const codeFiles = latestCodeArtifact && latestCodeArtifact.payload && 'data' in latestCodeArtifact.payload && 'files' in (latestCodeArtifact.payload as any).data
    ? ((latestCodeArtifact.payload as any).data.files as any[]).map((f) => ({
      name: f.path.split('/').pop() || f.path,
      path: f.path,
      language: (latestCodeArtifact.payload as any).data.language || 'typescript',
      content: f.content,
    }))
    : undefined;

  // Session list to render
  const recentItems = sessions.length > 0
    ? (showAllRecent ? sessions : sessions.slice(0, 5)).map((s) => ({
      id: s.id,
      title: s.title,
      time: new Date(s.updated_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' }),
    }))
    : [
      { id: 'mock-1', title: 'Rencana arsitektur ODON', time: '2h ago' },
      { id: 'mock-2', title: 'Refactor modul agent (Rust)', time: '4h ago' },
      { id: 'mock-3', title: 'Buat gambar ilustrasi produk', time: 'Yesterday' },
      { id: 'mock-4', title: 'Konversi PDF ke DOCX', time: 'Yesterday' },
    ];

  return (
    <div className="flex flex-col h-screen w-screen overflow-hidden bg-[#f4f5f8] text-neutral-800 font-sans select-none">
      {/* Hidden File Picker */}
      <input
        type="file"
        ref={fileInputRef}
        onChange={handleFileAttach}
        className="hidden"
      />

      {/* ─────────────────────────────────────────────────────────────
          1. WINDOW TITLE BAR (DESKTOP CHROME)
      ───────────────────────────────────────────────────────────── */}
      <header className="h-8 shrink-0 flex items-center justify-between px-3 text-xs text-neutral-600 bg-[#f4f5f8]">
        {/* Left: App Logo & Name + Backend Status */}
        <div className="flex items-center gap-2">
          <div
            onClick={handleNewChat}
            className="flex items-center gap-1.5 font-medium text-neutral-800 tracking-tight cursor-pointer hover:opacity-80 transition-opacity"
            title="Return to Home"
          >
            <CatLogo className="w-4 h-4 text-neutral-800" strokeWidth={2.4} />
            <span className="font-semibold text-[13px] tracking-wide">ODON</span>
          </div>

          <div
            onClick={() => setSettingsOpen(true)}
            className="flex items-center gap-1.5 px-2 py-0.5 rounded-full bg-white/90 border border-neutral-200/80 text-[10px] text-neutral-600 shadow-2xs cursor-pointer hover:bg-neutral-50 transition-colors"
            title="Click to configure Daemon connection"
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${daemonOnline ? 'bg-emerald-500 animate-pulse' : 'bg-amber-400'
                }`}
            />
            <span>{daemonOnline ? 'Daemon Online (127.0.0.1:4096)' : 'Local Standby'}</span>
          </div>
        </div>

        {/* Center: Draggable Spacer */}
        <div className="flex-1 h-full mx-4" />

        {/* Right: Window Controls */}
        <div className="flex items-center gap-1">
          <button
            onClick={handleMinimize}
            className="w-7 h-6 flex items-center justify-center rounded hover:bg-neutral-200/70 text-neutral-600 transition-colors cursor-pointer"
            title="Collapse Panel"
          >
            <Minus className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleMaximize}
            className="w-7 h-6 flex items-center justify-center rounded hover:bg-neutral-200/70 text-neutral-600 transition-colors cursor-pointer"
            title="Toggle Fullscreen"
          >
            <Square className="w-3 h-3" />
          </button>
          <button
            onClick={handleClose}
            className="w-7 h-6 flex items-center justify-center rounded hover:bg-red-500 hover:text-white text-neutral-600 transition-colors cursor-pointer"
            title="Reset to Home Screen"
          >
            <X className="w-3.5 h-3.5" />
          </button>
        </div>
      </header>

      {/* ─────────────────────────────────────────────────────────────
          2. MAIN 3-COLUMN WORKSPACE (ROUNDED WHITE CARDS)
      ───────────────────────────────────────────────────────────── */}
      <div className="flex-1 flex overflow-hidden p-2.5 pt-0 gap-2.5">

        {/* ───────────────────────────────────────────────────────────
            LEFT SIDEBAR CARD
        ─────────────────────────────────────────────────────────── */}
        <aside className="w-[250px] shrink-0 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col justify-between p-3.5 overflow-hidden">
          {/* Top Section */}
          <div className="flex flex-col overflow-y-auto">
            {/* Header: Cat Logo + ODON + Subtitle */}
            <div className="flex items-center gap-2.5 px-1 py-1">
              <CatLogo className="w-8 h-8 text-neutral-900 shrink-0" strokeWidth={2.2} />
              <div className="leading-tight">
                <div className="font-bold text-base text-neutral-900 tracking-tight">ODON</div>
                <div className="text-[10px] text-neutral-500 font-normal leading-tight">
                  Open Digital Orchestration Network
                </div>
              </div>
            </div>

            {/* + New Chat Button */}
            <button
              onClick={handleNewChat}
              className="mt-3.5 w-full flex items-center justify-between px-3 py-2 rounded-xl border border-neutral-200 hover:border-neutral-300 hover:bg-neutral-50 transition-all text-xs font-medium text-neutral-800 shadow-2xs group"
            >
              <div className="flex items-center gap-1.5 font-semibold">
                <Plus className="w-3.5 h-3.5 text-neutral-700 group-hover:scale-110 transition-transform" />
                <span>New Chat</span>
              </div>
              <span className="text-[11px] font-mono text-neutral-400 bg-neutral-100 px-1.5 py-0.5 rounded">
                Ctrl N
              </span>
            </button>

            {/* Navigation Menu */}
            <nav className="mt-3.5 space-y-0.5">
              <button
                onClick={() => { setActiveNav('chat'); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'chat'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold shadow-2xs'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <MessageSquare className={`w-4 h-4 ${activeNav === 'chat' ? 'text-blue-600' : 'text-neutral-600'}`} />
                <span>AI Chat</span>
              </button>

              <button
                onClick={() => { setActiveNav('code'); setRightPanelView('editor'); setIsRightPanelOpen(true); }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'code'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Code2 className="w-4 h-4 text-neutral-600" />
                <span>Code</span>
              </button>

              <button
                onClick={() => setActiveNav('create')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'create'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <ImageIcon className="w-4 h-4 text-neutral-600" />
                <span>Create</span>
              </button>

              <button
                onClick={() => setActiveNav('documents')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'documents'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <FileText className="w-4 h-4 text-neutral-600" />
                <span>Documents</span>
              </button>

              <button
                onClick={() => setActiveNav('files')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'files'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Folder className="w-4 h-4 text-neutral-600" />
                <span>Files</span>
              </button>

              <button
                onClick={() => setActiveNav('agents')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'agents'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Bot className="w-4 h-4 text-neutral-600" />
                <span>Agents</span>
              </button>

              <button
                onClick={() => setActiveNav('automations')}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'automations'
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Share2 className="w-4 h-4 text-neutral-600" />
                <span>Automations</span>
              </button>

              <button
                onClick={() => {
                  setActiveNav('terminal');
                  setRightPanelView('terminal');
                  setIsRightPanelOpen(true);
                }}
                className={`w-full flex items-center gap-2.5 px-3 py-2 rounded-xl text-xs font-medium transition-all ${activeNav === 'terminal' || (rightPanelView === 'terminal' && isRightPanelOpen)
                    ? 'bg-[#e9f2ff] text-blue-600 font-semibold'
                    : 'text-neutral-700 hover:bg-neutral-100/70'
                  }`}
              >
                <Terminal className="w-4 h-4 text-neutral-600" />
                <span>Terminal</span>
              </button>
            </nav>

            {/* Recent Section (Bound to SQLite Sessions) */}
            <div className="mt-5 pt-3 border-t border-neutral-100">
              <div className="text-[11px] font-medium text-neutral-400 px-1 mb-2">Recent</div>
              <div className="space-y-1">
                {recentItems.map((item) => (
                  <button
                    key={item.id}
                    onClick={() => loadSessionChat(item.id, item.title)}
                    className={`w-full flex items-center justify-between px-2 py-1.5 rounded-lg text-xs transition-colors group text-left ${currentSessionId === item.id
                        ? 'bg-neutral-100 font-medium text-neutral-900'
                        : 'text-neutral-700 hover:bg-neutral-100/80'
                      }`}
                  >
                    <div className="flex items-center gap-2 truncate">
                      <MessageSquare className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-600 shrink-0" />
                      <span className="truncate">{item.title}</span>
                    </div>
                    <span className="text-[10px] text-neutral-400 shrink-0 ml-1">{item.time}</span>
                  </button>
                ))}
              </div>
              <div className="flex items-center justify-between px-1 mt-1 text-[11px] text-neutral-400">
                <button
                  onClick={() => setShowAllRecent(!showAllRecent)}
                  className="hover:text-neutral-700 py-0.5 transition-colors cursor-pointer"
                >
                  {showAllRecent ? 'Show less' : 'Show more...'}
                </button>
                <button
                  onClick={loadSessions}
                  className="hover:text-neutral-700 py-0.5 transition-colors cursor-pointer"
                  title="Reload from SQLite"
                >
                  Refresh
                </button>
              </div>
            </div>
          </div>

          {/* User Profile at Bottom */}
          <div className="pt-3 border-t border-neutral-100 flex items-center justify-between">
            <div className="flex items-center gap-2.5">
              <div className="w-7 h-7 rounded-full bg-blue-600 text-white flex items-center justify-center font-semibold text-xs tracking-tight shadow-2xs">
                OD
              </div>
              <div className="leading-tight">
                <div className="font-semibold text-xs text-neutral-900">Odon User</div>
                <div className="text-[10px] text-neutral-400">Pro • Local</div>
              </div>
            </div>
            <button
              onClick={() => setSettingsOpen(true)}
              className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-lg transition-colors cursor-pointer"
              title="Settings & Preferences"
            >
              <Settings className="w-4 h-4" />
            </button>
          </div>
        </aside>

        {/* ───────────────────────────────────────────────────────────
            CENTER MAIN CHAT / HERO CARD
        ─────────────────────────────────────────────────────────── */}
        <main className="flex-1 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col justify-between overflow-hidden relative">

          {/* Top Bar inside Center Card */}
          <div className="h-12 border-b border-neutral-100 px-4 flex items-center justify-between shrink-0">
            {/* Model Selector Pill */}
            <div className="flex items-center gap-2">
              <div
                onClick={() => setIsModelMenuOpen(!isModelMenuOpen)}
                className="flex items-center gap-1.5 px-3 py-1.5 rounded-full border border-neutral-200/90 text-xs font-medium text-neutral-800 hover:bg-neutral-50 transition-colors shadow-2xs cursor-pointer"
                title={`Active Provider: ${activeProvider?.name} (Click to change)`}
              >
                <Sparkles className="w-3.5 h-3.5 text-blue-600" />
                <span className="text-[11px] font-semibold text-neutral-500">{activeProvider?.name.split(' (')[0]}:</span>
                <span className="font-semibold text-neutral-900 truncate max-w-[150px]">{displayModelName}</span>
                <ChevronDown className="w-3 h-3 text-neutral-400 ml-0.5" />
              </div>

              {inChatMode && (
                <button
                  onClick={handleNewChat}
                  className="text-xs text-blue-600 hover:text-blue-800 font-medium px-2 py-1 rounded-md hover:bg-blue-50 transition-colors cursor-pointer"
                >
                  ← Back to Home
                </button>
              )}
            </div>

            {/* Action Buttons on Right */}
            <div className="flex items-center gap-1">
              <button
                onClick={() => setIsSearchOpen(true)}
                className="p-2 text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-lg transition-colors cursor-pointer"
                title="Search Sessions & Artifacts"
              >
                <Search className="w-4 h-4" />
              </button>
              <button
                onClick={() => setIsBellOpen(true)}
                className="p-2 text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-lg transition-colors cursor-pointer"
                title="System Notifications & Activity"
              >
                <Bell className="w-4 h-4" />
              </button>
              <button
                onClick={() => setIsRightPanelOpen(!isRightPanelOpen)}
                className={`p-2 rounded-lg transition-colors cursor-pointer ${isRightPanelOpen
                    ? 'text-neutral-700 hover:bg-neutral-100'
                    : 'text-neutral-400 hover:text-neutral-800 hover:bg-neutral-100'
                  }`}
                title={isRightPanelOpen ? 'Collapse panel' : 'Open panel'}
              >
                {isRightPanelOpen ? <PanelRightClose className="w-4 h-4" /> : <PanelRightOpen className="w-4 h-4" />}
              </button>
            </div>
          </div>

          {/* Center Content Area */}
          <div className="flex-1 overflow-y-auto px-6 py-6 flex flex-col justify-start">

            {/* VIEW A: WELCOME HERO (EXACT MATCH TO DESIGN) */}
            {!inChatMode && (
              <div className="w-full max-w-2xl flex flex-col items-center my-auto mx-auto">
                {/* Cute Cat Logo Illustration */}
                <div className="mb-3 text-neutral-900 hover:scale-105 transition-transform">
                  <CatLogo className="w-20 h-20" strokeWidth={2} />
                </div>

                {/* Main Heading */}
                <h1 className="text-2xl font-bold text-neutral-900 tracking-tight text-center">
                  What can ODON do for you?
                </h1>

                {/* Subheading */}
                <p className="text-xs text-neutral-500 text-center mt-1.5 mb-8">
                  Your universal AI assistant for coding, creating, documents, and more.
                </p>

                {/* 6 Feature Action Cards (3x2 Grid) */}
                <div className="grid grid-cols-3 gap-3.5 w-full">
                  {/* Card 1: Build an app */}
                  <button
                    onClick={() => handleActionCard('Build an app', 'Build a personal notes web application with clean modular React 19 and SQLite storage.', 'code')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Code2 className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Build an app</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Create, refactor, and debug code
                    </div>
                  </button>

                  {/* Card 2: Create an image */}
                  <button
                    onClick={() => handleActionCard('Create an image', 'Generate a modern minimalist product illustration for ODON desktop UI.', 'create')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <ImageIcon className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Create an image</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Generate or edit images
                    </div>
                  </button>

                  {/* Card 3: Make a document */}
                  <button
                    onClick={() => handleActionCard('Make a document', 'Draft a comprehensive product requirements document (PRD) for local AI orchestration.', 'documents')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <FileText className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Make a document</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Write, convert, or analyze files
                    </div>
                  </button>

                  {/* Card 4: Run an agent */}
                  <button
                    onClick={() => handleActionCard('Run an agent', 'Run autonomous security audit agent on the workspace tools and policy engine.', 'agents')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Bot className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Run an agent</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Let AI work on complex tasks
                    </div>
                  </button>

                  {/* Card 5: Automate a workflow */}
                  <button
                    onClick={() => handleActionCard('Automate a workflow', 'Set up automated background scheduler for Telegram and WhatsApp relay sync.', 'automations')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Share2 className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Automate a workflow</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Schedule and orchestrate tasks
                    </div>
                  </button>

                  {/* Card 6: Create a video */}
                  <button
                    onClick={() => handleActionCard('Create a video', 'Generate a product showcase walkthrough storyboard and video script.', 'create')}
                    className="p-3.5 bg-white rounded-2xl border border-neutral-200/90 hover:border-neutral-300 hover:shadow-xs transition-all text-left group cursor-pointer"
                  >
                    <div className="text-neutral-800 mb-2">
                      <Video className="w-5 h-5 text-neutral-800 group-hover:scale-110 transition-transform" />
                    </div>
                    <div className="font-semibold text-xs text-neutral-900">Create a video</div>
                    <div className="text-[11px] text-neutral-500 mt-0.5 leading-snug">
                      Generate video from ideas
                    </div>
                  </button>
                </div>
              </div>
            )}

            {/* VIEW B: CONVERSATION STREAM */}
            {inChatMode && (
              <div className="w-full max-w-2xl flex flex-col space-y-6 mx-auto">
                {messages.map((msg) => (
                  <div key={msg.id} className="flex flex-col space-y-2">
                    <div className="flex items-center gap-2">
                      <div className={`w-5 h-5 rounded-full flex items-center justify-center text-[10px] font-bold ${msg.role === 'user'
                          ? 'bg-blue-600 text-white'
                          : 'bg-neutral-900 text-white'
                        }`}>
                        {msg.role === 'user' ? 'U' : <CatLogo className="w-3.5 h-3.5 text-white" />}
                      </div>
                      <span className="text-xs font-semibold text-neutral-900">
                        {msg.role === 'user' ? 'You' : 'ODON'}
                      </span>
                      <span className="text-[10px] text-neutral-400">{msg.timestamp}</span>
                    </div>

                    {msg.thought && (
                      <div className="ml-7 p-2 rounded-lg bg-neutral-50 border border-neutral-200/60 text-[11px] text-neutral-500 italic">
                        {msg.thought}
                      </div>
                    )}

                    <div className="text-xs text-neutral-800 leading-relaxed pl-7 whitespace-pre-wrap">
                      {msg.content}
                    </div>

                    {/* Interactive App Card */}
                    {msg.showAppCard && (
                      <div className="ml-7 mt-2 p-3.5 rounded-xl border border-neutral-200/90 bg-neutral-50/60 shadow-2xs max-w-lg">
                        <div className="flex items-center justify-between mb-2">
                          <div className="flex items-center gap-2">
                            <div className="p-1.5 rounded-lg bg-blue-50 text-blue-600 border border-blue-100">
                              <Code2 className="w-4 h-4" />
                            </div>
                            <div>
                              <div className="font-semibold text-xs text-neutral-900">personal-notes</div>
                              <div className="text-[10px] text-neutral-500">React 19 + TypeScript + Local Storage</div>
                            </div>
                          </div>
                          <span className="inline-flex items-center gap-1 text-[10px] font-medium text-emerald-700 bg-emerald-50 px-2 py-0.5 rounded-full border border-emerald-200">
                            <CheckCircle2 className="w-3 h-3 text-emerald-600" />
                            Verified
                          </span>
                        </div>

                        <div className="flex items-center gap-2 mt-3">
                          <button
                            onClick={() => {
                              setRightPanelView('browser');
                              setIsRightPanelOpen(true);
                            }}
                            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-neutral-900 text-white hover:bg-neutral-800 text-xs font-medium transition-colors shadow-2xs cursor-pointer"
                          >
                            <ExternalLink className="w-3 h-3" />
                            Preview in Browser
                          </button>
                          <button
                            onClick={() => {
                              setRightPanelView('editor');
                              setIsRightPanelOpen(true);
                            }}
                            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 bg-white hover:bg-neutral-50 text-neutral-700 text-xs font-medium transition-colors cursor-pointer"
                          >
                            <Code2 className="w-3 h-3 text-neutral-500" />
                            Inspect Source
                          </button>
                          <button
                            onClick={() => {
                              setRightPanelView('terminal');
                              setIsRightPanelOpen(true);
                            }}
                            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 bg-white hover:bg-neutral-50 text-neutral-700 text-xs font-medium transition-colors cursor-pointer"
                          >
                            <Terminal className="w-3 h-3 text-neutral-500" />
                            Terminal
                          </button>
                        </div>
                      </div>
                    )}
                  </div>
                ))}

                {/* Active Generating / Streaming Turn */}
                {isGenerating && (
                  <div className="flex flex-col space-y-2">
                    <div className="flex items-center gap-2">
                      <div className="w-5 h-5 rounded-full bg-neutral-900 text-white flex items-center justify-center">
                        <CatLogo className="w-3.5 h-3.5 text-white" />
                      </div>
                      <span className="text-xs font-semibold text-neutral-900">ODON</span>
                      <span className="text-[10px] text-neutral-400 flex items-center gap-1">
                        <Loader2 className="w-3 h-3 animate-spin text-blue-600" />
                        Thinking...
                      </span>
                    </div>

                    {streamingTool && (
                      <div className="ml-7 inline-flex items-center gap-1.5 px-2.5 py-1 rounded-md bg-blue-50 text-blue-700 border border-blue-200/60 text-[11px] font-medium animate-pulse">
                        <Terminal className="w-3 h-3" />
                        <span>Running tool: {streamingTool}...</span>
                      </div>
                    )}

                    {streamingThought && (
                      <div className="ml-7 p-2 rounded-lg bg-neutral-50 border border-neutral-200/60 text-[11px] text-neutral-500 italic">
                        {streamingThought}
                      </div>
                    )}

                    <div className="text-xs text-neutral-800 leading-relaxed pl-7 whitespace-pre-wrap">
                      {streamingText || 'Orchestrating tools and synthesizing solution...'}
                    </div>
                  </div>
                )}

                <div ref={chatEndRef} />
              </div>
            )}
          </div>

          {/* Bottom Prompt Input Box */}
          <div className="p-4 pt-0 shrink-0">
            <div className="w-full max-w-2xl mx-auto rounded-3xl border border-neutral-200/90 bg-white shadow-2xs p-3 focus-within:border-neutral-300 focus-within:shadow-xs transition-all">
              {/* Input Area */}
              <input
                type="text"
                value={inputMessage}
                onChange={(e) => setInputMessage(e.target.value)}
                onKeyDown={(e) => e.key === 'Enter' && handleSendMessage()}
                placeholder={getPlaceholder()}
                disabled={isGenerating}
                className="w-full text-xs text-neutral-800 placeholder-neutral-400 outline-none px-1 py-1 bg-transparent"
              />

              {/* Bottom Row Tools inside Input Box */}
              <div className="flex items-center justify-between mt-2 pt-2 border-t border-neutral-100">
                {/* Left Tools */}
                <div className="flex items-center gap-1">
                  <button
                    onClick={() => fileInputRef.current?.click()}
                    className="p-1.5 text-neutral-500 hover:text-neutral-800 hover:bg-neutral-100 rounded-full transition-colors cursor-pointer"
                    title="Attach file to prompt"
                  >
                    <Plus className="w-4 h-4" />
                  </button>

                  {/* Tools Dropdown */}
                  <div className="relative">
                    <button
                      onClick={() => setIsToolsMenuOpen(!isToolsMenuOpen)}
                      className="flex items-center gap-1 px-2.5 py-1 text-xs text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-full transition-colors ml-1 border border-neutral-200/60 cursor-pointer"
                    >
                      <Globe className="w-3.5 h-3.5 text-neutral-500" />
                      <span>Tools ({REGISTERED_TOOLS.length})</span>
                      <ChevronDown className="w-3 h-3 text-neutral-400" />
                    </button>
                  </div>
                </div>

                {/* Right Tools */}
                <div className="flex items-center gap-2">
                  {/* Model Selector Pill */}
                  <div className="relative">
                    <button
                      onClick={() => setIsModelMenuOpen(!isModelMenuOpen)}
                      className="flex items-center gap-1.5 px-2.5 py-1 text-xs text-neutral-600 hover:text-neutral-900 hover:bg-neutral-100 rounded-full transition-colors border border-neutral-200/60 cursor-pointer"
                      title={`Active Provider: ${activeProvider?.name}`}
                    >
                      <Sparkles className="w-3 h-3 text-blue-500" />
                      <span className="text-[10px] text-neutral-400 font-normal">{activeProvider?.name.split(' (')[0]}:</span>
                      <span className="truncate max-w-[130px] font-medium">{displayModelName}</span>
                      <ChevronDown className="w-3 h-3 text-neutral-400" />
                    </button>
                  </div>

                  {/* Send Button */}
                  <button
                    onClick={() => handleSendMessage()}
                    disabled={isGenerating || !inputMessage.trim()}
                    className="w-7 h-7 rounded-full bg-neutral-900 hover:bg-neutral-800 disabled:opacity-40 text-white flex items-center justify-center transition-transform active:scale-95 shadow-2xs cursor-pointer"
                  >
                    <ArrowUp className="w-3.5 h-3.5 stroke-[2.5]" />
                  </button>
                </div>
              </div>
            </div>
          </div>
        </main>

        {/* ───────────────────────────────────────────────────────────
            RIGHT PANEL: ARTIFACTS / RELAYS / CODE / BROWSER / TERMINAL
        ─────────────────────────────────────────────────────────── */}
        {isRightPanelOpen && (
          <>
            {/* Draggable resize handle */}
            <div
              onMouseDown={(e) => {
                e.preventDefault();
                setIsResizingRight(true);
              }}
              className={`w-2 -mr-1 z-20 cursor-col-resize flex items-center justify-center group shrink-0 select-none transition-colors ${isResizingRight ? 'bg-blue-500/20' : 'hover:bg-neutral-300/60'
                }`}
              title="Drag to resize panel"
            >
              <div
                className={`w-0.5 h-8 rounded-full transition-colors ${isResizingRight ? 'bg-blue-600' : 'bg-neutral-300 group-hover:bg-neutral-500'
                  }`}
              />
            </div>

            <aside
              style={{ width: `${rightPanelWidth}px` }}
              className="shrink-0 bg-white rounded-2xl border border-neutral-200/90 shadow-2xs flex flex-col p-3.5 overflow-hidden relative"
            >

              {/* Top View Selector Tabs (Artifacts / Code / Browser / Terminal) */}
              <div className="flex items-center gap-1 p-1 bg-neutral-100/80 rounded-xl mb-3 text-[11px] font-medium text-neutral-600 shrink-0 select-none">
                <button
                  onClick={() => setRightPanelView('artifacts')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer ${rightPanelView === 'artifacts'
                      ? 'bg-white text-neutral-900 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900'
                    }`}
                >
                  Artifacts
                </button>
                <button
                  onClick={() => setRightPanelView('editor')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer ${rightPanelView === 'editor'
                      ? 'bg-white text-neutral-900 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900'
                    }`}
                >
                  Code
                </button>
                <button
                  onClick={() => setRightPanelView('browser')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer ${rightPanelView === 'browser'
                      ? 'bg-white text-neutral-900 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900'
                    }`}
                >
                  Browser
                </button>
                <button
                  onClick={() => setRightPanelView('terminal')}
                  className={`flex-1 py-1 px-1.5 rounded-lg text-center transition-all cursor-pointer flex items-center justify-center gap-1 ${rightPanelView === 'terminal'
                      ? 'bg-white text-blue-600 font-semibold shadow-2xs'
                      : 'hover:text-neutral-900 text-neutral-600'
                    }`}
                >
                  <Terminal className="w-3 h-3 text-blue-600" />
                  <span>Terminal</span>
                </button>
              </div>

              {/* Right Panel Sub-View: Code Editor */}
              {rightPanelView === 'editor' && (
                <div className="h-full flex flex-col">
                  <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 mb-2.5 shrink-0">
                    <div className="flex items-center gap-1.5">
                      <button
                        onClick={() => setRightPanelView('artifacts')}
                        className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors cursor-pointer"
                        title="Back to Artifacts"
                      >
                        <ArrowLeft className="w-3.5 h-3.5" />
                      </button>
                      <span className="font-bold text-xs text-neutral-900">Code Editor</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setRightPanelView('terminal')}
                        className="text-[11px] text-neutral-600 hover:text-neutral-900 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <Terminal className="w-3 h-3 text-neutral-500" />
                        <span>Terminal</span>
                      </button>
                      <button
                        onClick={() => setRightPanelView('browser')}
                        className="text-[11px] text-blue-600 hover:text-blue-800 font-medium cursor-pointer"
                      >
                        Browser →
                      </button>
                    </div>
                  </div>
                  <div className="flex-1 overflow-hidden">
                    <CodeEditorView
                      files={codeFiles}
                      onRunPreview={() => setRightPanelView('browser')}
                      onOpenTerminal={() => setRightPanelView('terminal')}
                    />
                  </div>
                </div>
              )}

              {/* Right Panel Sub-View: Browser Preview */}
              {rightPanelView === 'browser' && (
                <div className="h-full flex flex-col">
                  <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 mb-2.5 shrink-0">
                    <div className="flex items-center gap-1.5">
                      <button
                        onClick={() => setRightPanelView('artifacts')}
                        className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors cursor-pointer"
                        title="Back to Artifacts"
                      >
                        <ArrowLeft className="w-3.5 h-3.5" />
                      </button>
                      <span className="font-bold text-xs text-neutral-900">Browser Preview</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setRightPanelView('terminal')}
                        className="text-[11px] text-neutral-600 hover:text-neutral-900 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <Terminal className="w-3 h-3 text-neutral-500" />
                        <span>Terminal</span>
                      </button>
                      <button
                        onClick={() => setRightPanelView('editor')}
                        className="text-[11px] text-blue-600 hover:text-blue-800 font-medium cursor-pointer"
                      >
                        View Code →
                      </button>
                    </div>
                  </div>
                  <div className="flex-1 overflow-hidden">
                    <BrowserPreviewView />
                  </div>
                </div>
              )}

              {/* Right Panel Sub-View: Integrated Terminal */}
              {rightPanelView === 'terminal' && (
                <div className="h-full flex flex-col">
                  <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 mb-2.5 shrink-0">
                    <div className="flex items-center gap-1.5">
                      <button
                        onClick={() => setRightPanelView('artifacts')}
                        className="p-1 hover:bg-neutral-100 rounded-md text-neutral-500 hover:text-neutral-900 transition-colors cursor-pointer"
                        title="Back to Artifacts"
                      >
                        <ArrowLeft className="w-3.5 h-3.5" />
                      </button>
                      <span className="font-bold text-xs text-neutral-900">Integrated Terminal</span>
                    </div>
                    <div className="flex items-center gap-2">
                      <button
                        onClick={() => setRightPanelView('editor')}
                        className="text-[11px] text-neutral-600 hover:text-neutral-900 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <Code2 className="w-3 h-3 text-neutral-500" />
                        <span>Code</span>
                      </button>
                      <button
                        onClick={() => setRightPanelView('browser')}
                        className="text-[11px] text-blue-600 hover:text-blue-800 font-medium cursor-pointer flex items-center gap-1"
                      >
                        <ExternalLink className="w-3 h-3 text-blue-600" />
                        <span>Browser</span>
                      </button>
                    </div>
                  </div>
                  <div className="flex-1 overflow-hidden">
                    <TerminalView />
                  </div>
                </div>
              )}

              {/* Right Panel Default View: Artifacts & Relays */}
              {rightPanelView === 'artifacts' && (
                <div className="flex-1 flex flex-col justify-between overflow-y-auto">
                  <div className="space-y-4">
                    {/* 1. Artifacts Section */}
                    <div>
                      <div className="flex items-center gap-2 mb-0.5">
                        <Layers className="w-4 h-4 text-neutral-900" />
                        <h2 className="font-bold text-sm text-neutral-900">Artifacts</h2>
                      </div>
                      <p className="text-[11px] text-neutral-500 mb-3">
                        View and manage results from your tasks.
                      </p>

                      {/* Artifacts List (Bound to real or sample artifacts) */}
                      <div className="space-y-2">
                        {artifacts.length > 0 ? (
                          artifacts.map((art) => (
                            <div
                              key={art.id}
                              onClick={() => {
                                if (art.kind === 'code' || art.kind === 'patch') {
                                  setRightPanelView('editor');
                                } else {
                                  setRightPanelView('browser');
                                }
                              }}
                              className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group cursor-pointer"
                            >
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                                  {art.kind === 'code' ? (
                                    <Code2 className="w-4 h-4" />
                                  ) : art.kind === 'image' ? (
                                    <ImageIcon className="w-4 h-4" />
                                  ) : (
                                    <FileText className="w-4 h-4" />
                                  )}
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    {art.title}
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5 capitalize">
                                    {art.kind} • {new Date(art.created_at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' })}
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          ))
                        ) : (
                          <>
                            {/* Sample Item 1: odon-architecture.png */}
                            <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-blue-50 text-blue-600">
                                  <ImageIcon className="w-4 h-4" />
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    odon-architecture.png
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5">
                                    Image • 2.4 MB • 2h ago
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>

                            {/* Sample Item 2: agent.rs */}
                            <div
                              onClick={() => setRightPanelView('editor')}
                              className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group cursor-pointer"
                            >
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-neutral-100 text-neutral-700">
                                  <Code2 className="w-4 h-4" />
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    agent.rs
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5">
                                    Code • 12 KB • 4h ago
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>

                            {/* Sample Item 3: rencana-produk.pdf */}
                            <div className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors group">
                              <div className="flex items-center gap-2.5">
                                <div className="p-2 rounded-xl bg-red-50 text-red-500">
                                  <FileText className="w-4 h-4" />
                                </div>
                                <div>
                                  <div className="text-xs font-medium text-neutral-900 leading-tight">
                                    rencana-produk.pdf
                                  </div>
                                  <div className="text-[10px] text-neutral-400 mt-0.5">
                                    PDF • 1.1 MB • Yesterday
                                  </div>
                                </div>
                              </div>
                              <button className="text-neutral-400 hover:text-neutral-700 p-1">
                                <ExternalLink className="w-3.5 h-3.5" />
                              </button>
                            </div>
                          </>
                        )}
                      </div>

                      {/* View all artifacts link */}
                      <button
                        onClick={() => setRightPanelView('browser')}
                        className="text-xs font-medium text-neutral-700 hover:text-neutral-900 flex items-center gap-1 mt-3 px-1 cursor-pointer"
                      >
                        <span>View all artifacts</span>
                        <span>→</span>
                      </button>
                    </div>

                    {/* 1.5 Host Terminal Quick Launch Card */}
                    <div
                      onClick={() => setRightPanelView('terminal')}
                      className="flex items-center justify-between p-2.5 rounded-xl border border-neutral-200/90 bg-neutral-50/70 hover:bg-neutral-100/70 transition-all cursor-pointer group shadow-2xs"
                    >
                      <div className="flex items-center gap-2.5">
                        <div className="p-2 rounded-xl bg-neutral-900 text-white group-hover:scale-105 transition-transform">
                          <Terminal className="w-4 h-4" />
                        </div>
                        <div>
                          <div className="text-xs font-semibold text-neutral-900 leading-tight flex items-center gap-1.5">
                            <span>Host Terminal</span>
                            <span className="text-[9px] px-1.5 py-0.2 bg-emerald-50 text-emerald-600 border border-emerald-200 rounded font-normal">Active</span>
                          </div>
                          <div className="text-[10px] text-neutral-500 mt-0.5">
                            PowerShell / sh • Sandbox CLI • 18 Tools
                          </div>
                        </div>
                      </div>
                      <ChevronRight className="w-4 h-4 text-neutral-400 group-hover:text-neutral-700 transition-colors" />
                    </div>

                    {/* Divider */}
                    <div className="border-t border-neutral-100 my-3" />

                    {/* 2. Relays Section */}
                    <div>
                      <div className="flex items-center gap-2 mb-0.5">
                        <Radio className="w-4 h-4 text-neutral-900" />
                        <h2 className="font-bold text-sm text-neutral-900">Relays</h2>
                      </div>
                      <p className="text-[11px] text-neutral-500 mb-3">
                        Connect and control ODON from your chat apps.
                      </p>

                      <div className="space-y-2">
                        {/* Telegram */}
                        <div
                          onClick={() => setRelayModalOpen('telegram')}
                          className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors cursor-pointer group"
                          title="Configure Telegram Bot Relay"
                        >
                          <div className="flex items-center gap-2.5">
                            <div className="w-7 h-7 rounded-full bg-[#0088cc] text-white flex items-center justify-center">
                              <svg viewBox="0 0 24 24" fill="currentColor" className="w-3.5 h-3.5">
                                <path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm4.64 6.8c-.15 1.58-.8 5.42-1.13 7.19-.14.75-.42 1-.68 1.03-.58.05-1.02-.38-1.58-.75-.88-.58-1.38-.94-2.23-1.5-.99-.65-.35-1.01.22-1.59.15-.15 2.71-2.48 2.76-2.69a.2.2 0 00-.05-.18c-.06-.05-.14-.03-.21-.02-.09.02-1.49.95-4.22 2.79-.4.27-.76.41-1.08.4-.36-.01-1.04-.2-1.55-.37-.63-.2-1.12-.31-1.08-.66.02-.18.27-.36.75-.55 2.92-1.27 4.86-2.11 5.83-2.52 2.77-1.16 3.35-1.36 3.73-1.37.08 0 .27.02.39.12.1.08.13.19.14.27-.01.06.01.24 0 .38z" />
                              </svg>
                            </div>
                            <span className="text-xs font-semibold text-neutral-900">Telegram</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <div className="flex items-center gap-1.5 text-[11px] text-neutral-600">
                              <span
                                className={`w-1.5 h-1.5 rounded-full ${systemStatus?.relays.telegram ? 'bg-emerald-500 animate-pulse' : 'bg-neutral-300'
                                  }`}
                              />
                              <span>{systemStatus?.relays.telegram ? 'Connected' : 'Standby'}</span>
                            </div>
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-700 transition-colors" />
                          </div>
                        </div>

                        {/* WhatsApp */}
                        <div
                          onClick={() => setRelayModalOpen('whatsapp')}
                          className="flex items-center justify-between p-2 rounded-xl hover:bg-neutral-50 transition-colors cursor-pointer group"
                          title="Configure WhatsApp Bridge Relay"
                        >
                          <div className="flex items-center gap-2.5">
                            <div className="w-7 h-7 rounded-full bg-[#25d366] text-white flex items-center justify-center">
                              <svg viewBox="0 0 24 24" fill="currentColor" className="w-3.5 h-3.5">
                                <path d="M12.04 2c-5.46 0-9.91 4.45-9.91 9.91 0 1.75.46 3.45 1.32 4.95L2.05 22l5.25-1.38c1.45.79 3.08 1.21 4.74 1.21 5.46 0 9.91-4.45 9.91-9.91 0-2.65-1.03-5.14-2.9-7.01A9.816 9.816 0 0012.04 2zm0 18.15c-1.49 0-2.94-.4-4.22-1.16l-.3-.18-3.13.82.83-3.05-.2-.31a8.17 8.17 0 01-1.25-4.36c0-4.52 3.68-8.2 8.2-8.2 2.19 0 4.25.85 5.8 2.4 1.55 1.55 2.4 3.61 2.4 5.8 0 4.52-3.68 8.2-8.13 8.2zm4.49-6.14c-.25-.12-1.47-.72-1.7-.81-.23-.08-.39-.12-.56.12-.17.25-.64.81-.79.97-.14.17-.29.19-.54.06-.25-.12-1.05-.39-2-1.23-.74-.66-1.23-1.47-1.38-1.72-.14-.25-.02-.38.11-.5.11-.11.25-.29.37-.43.12-.14.17-.25.25-.41.08-.17.04-.31-.02-.43s-.56-1.34-.76-1.84c-.2-.48-.41-.42-.56-.43h-.48c-.17 0-.43.06-.66.31-.22.25-.86.84-.86 2.05s.88 2.38 1 2.55c.12.17 1.73 2.65 4.2 3.71.59.25 1.05.41 1.41.52.59.19 1.13.16 1.56.1.48-.07 1.47-.6 1.68-1.18.21-.58.21-1.07.14-1.18-.06-.11-.23-.17-.48-.3z" />
                              </svg>
                            </div>
                            <span className="text-xs font-semibold text-neutral-900">WhatsApp</span>
                          </div>
                          <div className="flex items-center gap-2">
                            <div className="flex items-center gap-1.5 text-[11px] text-neutral-600">
                              <span
                                className={`w-1.5 h-1.5 rounded-full ${systemStatus?.relays.whatsapp ? 'bg-emerald-500 animate-pulse' : 'bg-neutral-300'
                                  }`}
                              />
                              <span>{systemStatus?.relays.whatsapp ? 'Connected' : 'Standby'}</span>
                            </div>
                            <ChevronRight className="w-3.5 h-3.5 text-neutral-400 group-hover:text-neutral-700 transition-colors" />
                          </div>
                        </div>
                      </div>
                    </div>
                  </div>

                  {/* 3. Agent Activity Section at Bottom */}
                  <div className="pt-3 border-t border-neutral-100">
                    <div className="flex items-center gap-2 mb-1">
                      <Activity className="w-4 h-4 text-neutral-900" />
                      <h3 className="font-bold text-xs text-neutral-900">Agent Activity</h3>
                    </div>
                    <div className="flex items-center gap-1.5 text-xs text-neutral-500 mt-1">
                      <span
                        className={`w-1.5 h-1.5 rounded-full ${isGenerating ? 'bg-blue-600 animate-ping' : 'bg-emerald-500'
                          }`}
                      />
                      <span>
                        {isGenerating
                          ? streamingTool
                            ? `Executing tool: ${streamingTool}...`
                            : 'Synthesizing response...'
                          : 'Idle • Ready for new tasks'}
                      </span>
                    </div>
                  </div>
                </div>
              )}
            </aside>
          </>
        )}
      </div>

      {/* Settings Modal */}
      <SettingsModal
        isOpen={settingsOpen}
        onClose={() => setSettingsOpen(false)}
        isDark={false}
        onToggleTheme={() => { }}
        activeProviderId={activeProviderId}
        onSelectProvider={handleSelectProvider}
        currentModel={selectedModel}
        onSelectModel={handleSelectModel}
      />

      {/* ─────────────────────────────────────────────────────────────
          FLOATING MODEL SELECTOR POPOVER (FOLLOWS SETTINGS PROVIDER)
      ───────────────────────────────────────────────────────────── */}
      {isModelMenuOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/20 backdrop-blur-2xs"
          onClick={() => setIsModelMenuOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[430px] max-h-[85vh] flex flex-col bg-white rounded-2xl border border-neutral-200 shadow-xl p-3.5 animate-in fade-in zoom-in-95 duration-100"
          >
            {/* Popover Header */}
            <div className="flex items-center justify-between px-1 pb-2.5 border-b border-neutral-100 mb-2">
              <div>
                <div className="font-bold text-xs text-neutral-900">Select AI Model</div>
                <div className="text-[11px] text-neutral-500 flex items-center gap-1.5 mt-0.5">
                  <span>Provider:</span>
                  <span className="font-semibold text-blue-600">{activeProvider?.name}</span>
                </div>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={() => {
                    setIsModelMenuOpen(false);
                    setSettingsOpen(true);
                  }}
                  className="text-[11px] text-blue-600 hover:text-blue-800 font-medium underline cursor-pointer"
                  title="Configure API keys and providers in Settings"
                >
                  Configure in Settings →
                </button>
                <button
                  onClick={() => setIsModelMenuOpen(false)}
                  className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
                >
                  <X className="w-3.5 h-3.5" />
                </button>
              </div>
            </div>

            {/* Quick Provider Switcher Strip */}
            <div className="flex items-center gap-1 overflow-x-auto pb-2 mb-2 shrink-0 border-b border-neutral-100">
              {providers.map((p) => {
                const isActive = p.id === activeProviderId;
                return (
                  <button
                    key={p.id}
                    onClick={() => handleSelectProvider(p.id)}
                    className={`px-2.5 py-1 rounded-lg text-[11px] font-medium transition-all shrink-0 cursor-pointer flex items-center gap-1 ${
                      isActive
                        ? 'bg-neutral-900 text-white shadow-2xs'
                        : 'bg-neutral-100 hover:bg-neutral-200 text-neutral-600'
                    }`}
                  >
                    <span>{p.name.split(' (')[0]}</span>
                    {isActive && <Check className="w-2.5 h-2.5 text-white" />}
                  </button>
                );
              })}
            </div>

            {/* Models List belonging to Active Provider */}
            <div className="space-y-1.5 overflow-y-auto max-h-[360px] pr-1">
              <div className="text-[10px] uppercase font-bold text-neutral-400 tracking-wider px-1 mb-1">
                {activeProvider?.name} Models ({activeModels.length})
              </div>

              {activeModels.map((m) => {
                const isSelected = selectedModel === m.id || selectedModel === m.name || `${activeProvider.id}::${m.id}` === selectedModel;
                return (
                  <button
                    key={m.id}
                    onClick={() => handleSelectModel(m.id)}
                    className={`w-full text-left p-2.5 rounded-xl transition-all cursor-pointer flex items-start justify-between border ${
                      isSelected
                        ? 'bg-blue-50/80 border-blue-200 text-blue-900 shadow-2xs'
                        : 'border-transparent hover:border-neutral-200 hover:bg-neutral-50 text-neutral-800'
                    }`}
                  >
                    <div>
                      <div className="flex items-center gap-2 font-semibold text-xs">
                        <span>{m.name}</span>
                        {m.recommended && (
                          <span className="text-[9px] px-1.5 py-0.5 rounded-full bg-emerald-100 text-emerald-700 font-medium">
                            Recommended
                          </span>
                        )}
                      </div>
                      <div className="text-[11px] text-neutral-500 mt-0.5 flex items-center gap-2">
                        <span className="font-mono text-[10px] text-neutral-400">id: {m.id}</span>
                        <span>·</span>
                        <span>{m.context > 0 ? `${(m.context / 1000).toFixed(0)}k context` : 'auto context'}</span>
                      </div>
                    </div>
                    {isSelected && <Check className="w-4 h-4 text-blue-600 shrink-0 mt-0.5" />}
                  </button>
                );
              })}

              {/* Offline fallback button */}
              <div className="pt-2 mt-2 border-t border-neutral-100">
                <button
                  onClick={() => handleSelectModel('odon-simulated')}
                  className={`w-full text-left p-2 rounded-xl transition-all cursor-pointer flex items-center justify-between text-xs ${
                    selectedModel === 'odon-simulated'
                      ? 'bg-neutral-100 font-semibold text-neutral-900'
                      : 'hover:bg-neutral-50 text-neutral-500'
                  }`}
                >
                  <div className="flex items-center gap-1.5">
                    <span className="text-[10px] px-1.5 py-0.5 bg-neutral-200 rounded text-neutral-600 font-mono">Mock</span>
                    <span>Offline Simulated Mode</span>
                  </div>
                  {selectedModel === 'odon-simulated' && <Check className="w-3.5 h-3.5 text-neutral-700" />}
                </button>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          FLOATING TOOLS INVENTORY POPOVER
      ───────────────────────────────────────────────────────────── */}
      {isToolsMenuOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/20 backdrop-blur-2xs"
          onClick={() => setIsToolsMenuOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[440px] max-w-[90vw] max-h-[80vh] flex flex-col bg-white rounded-2xl border border-neutral-200 shadow-xl p-4 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100 shrink-0">
              <div className="flex items-center gap-2">
                <Globe className="w-4 h-4 text-blue-600" />
                <span className="font-bold text-sm text-neutral-900">
                  Registered Tools ({REGISTERED_TOOLS.length})
                </span>
              </div>
              <button
                onClick={() => setIsToolsMenuOpen(false)}
                className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <p className="text-[11px] text-neutral-500 my-2 shrink-0">
              Autonomous execution harness capabilities available to the agent runtime.
            </p>
            <div className="flex-1 overflow-y-auto space-y-1.5 pr-1">
              {REGISTERED_TOOLS.map((t) => (
                <div
                  key={t.name}
                  className="p-2 rounded-xl border border-neutral-100 bg-neutral-50/60 flex items-center justify-between text-xs"
                >
                  <div>
                    <div className="flex items-center gap-2 font-mono font-semibold text-neutral-900">
                      <span>{t.name}</span>
                      <span className="text-[9px] font-sans px-1.5 py-0.2 rounded bg-neutral-200/80 text-neutral-600">
                        {t.category}
                      </span>
                    </div>
                    <div className="text-[10px] text-neutral-500 font-sans mt-0.5">{t.desc}</div>
                  </div>
                  <span
                    className={`text-[9px] font-medium px-2 py-0.5 rounded-full ${t.tier === 'Tier 0'
                        ? 'bg-emerald-50 text-emerald-700 border border-emerald-200'
                        : t.tier === 'Tier 1'
                          ? 'bg-amber-50 text-amber-700 border border-amber-200'
                          : 'bg-red-50 text-red-700 border border-red-200'
                      }`}
                  >
                    {t.tier}
                  </span>
                </div>
              ))}
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          SEARCH SESSIONS & ARTIFACTS MODAL
      ───────────────────────────────────────────────────────────── */}
      {isSearchOpen && (
        <div
          className="fixed inset-0 z-50 flex items-start justify-center pt-20 bg-black/25 backdrop-blur-2xs"
          onClick={() => setIsSearchOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[480px] max-w-[90vw] bg-white rounded-2xl border border-neutral-200 shadow-2xl p-4 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center gap-2 pb-3 border-b border-neutral-100">
              <Search className="w-4 h-4 text-neutral-400" />
              <input
                type="text"
                value={searchQuery}
                onChange={(e) => setSearchQuery(e.target.value)}
                placeholder="Search conversations and artifacts..."
                className="flex-1 text-xs outline-none bg-transparent text-neutral-900 placeholder-neutral-400 font-sans"
                autoFocus
              />
              <button
                onClick={() => setIsSearchOpen(false)}
                className="text-neutral-400 hover:text-neutral-700 p-1 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="mt-3 max-h-60 overflow-y-auto space-y-1">
              {sessions.filter((s) => s.title.toLowerCase().includes(searchQuery.toLowerCase())).length > 0 ? (
                sessions
                  .filter((s) => s.title.toLowerCase().includes(searchQuery.toLowerCase()))
                  .map((s) => (
                    <button
                      key={s.id}
                      onClick={() => {
                        loadSessionChat(s.id, s.title);
                        setIsSearchOpen(false);
                      }}
                      className="w-full text-left p-2 rounded-xl hover:bg-neutral-50 transition-colors flex items-center justify-between text-xs cursor-pointer"
                    >
                      <span className="font-medium text-neutral-800 truncate">{s.title}</span>
                      <span className="text-[10px] text-neutral-400">
                        {new Date(s.updated_at).toLocaleDateString()}
                      </span>
                    </button>
                  ))
              ) : (
                <div className="text-center py-6 text-neutral-400 text-xs">
                  {searchQuery ? 'No matching sessions found.' : 'Type to search previous conversations...'}
                </div>
              )}
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          SYSTEM ACTIVITY / NOTIFICATIONS MODAL
      ───────────────────────────────────────────────────────────── */}
      {isBellOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/25 backdrop-blur-2xs"
          onClick={() => setIsBellOpen(false)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[420px] max-w-[90vw] bg-white rounded-2xl border border-neutral-200 shadow-2xl p-4 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center justify-between pb-2.5 border-b border-neutral-100">
              <div className="flex items-center gap-2">
                <Bell className="w-4 h-4 text-blue-600" />
                <span className="font-bold text-sm text-neutral-900">System Status & Activity</span>
              </div>
              <button
                onClick={() => setIsBellOpen(false)}
                className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="mt-3 space-y-2.5 text-xs">
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Runtime Daemon</span>
                <span className={`font-semibold ${daemonOnline ? 'text-emerald-600' : 'text-amber-600'}`}>
                  {daemonOnline ? 'Online (127.0.0.1:4096)' : 'Offline (Local Standby)'}
                </span>
              </div>
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Registered Tools</span>
                <span className="font-semibold text-neutral-900">18 active tools (All Ready)</span>
              </div>
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Telegram Bot Relay</span>
                <span className="font-semibold text-neutral-700">
                  {systemStatus?.relays.telegram ? 'Connected' : 'Standby (Token not set)'}
                </span>
              </div>
              <div className="p-2.5 rounded-xl bg-neutral-50 border border-neutral-100 flex items-center justify-between">
                <span className="text-neutral-600">Persistence Engine</span>
                <span className="font-semibold text-neutral-900">SQLite State (`odon_state.db`)</span>
              </div>
            </div>
          </div>
        </div>
      )}

      {/* ─────────────────────────────────────────────────────────────
          RELAY SETUP MODAL (TELEGRAM / WHATSAPP)
      ───────────────────────────────────────────────────────────── */}
      {relayModalOpen && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center bg-black/25 backdrop-blur-2xs"
          onClick={() => setRelayModalOpen(null)}
        >
          <div
            onClick={(e) => e.stopPropagation()}
            className="w-[450px] max-w-[90vw] bg-white rounded-2xl border border-neutral-200 shadow-2xl p-5 animate-in fade-in zoom-in-95 duration-100"
          >
            <div className="flex items-center justify-between pb-3 border-b border-neutral-100">
              <div className="flex items-center gap-2">
                <Radio className="w-4 h-4 text-blue-600" />
                <span className="font-bold text-sm text-neutral-900">
                  {relayModalOpen === 'telegram' ? 'Telegram Bot Relay' : 'WhatsApp Bridge Relay'}
                </span>
              </div>
              <button
                onClick={() => setRelayModalOpen(null)}
                className="p-1 text-neutral-400 hover:text-neutral-700 rounded-md cursor-pointer"
              >
                <X className="w-4 h-4" />
              </button>
            </div>
            <div className="mt-3.5 space-y-3 text-xs text-neutral-600 leading-relaxed">
              {relayModalOpen === 'telegram' ? (
                <>
                  <p>
                    ODON can be fully controlled directly from your personal or team Telegram chats. Commands sent to your bot execute autonomous agent turns and return rich markdown answers.
                  </p>
                  <div className="p-3 bg-neutral-50 rounded-xl border border-neutral-200/80 font-mono text-[11px] space-y-1">
                    <div className="text-neutral-500 font-sans text-[10px]">Start daemon with your bot token:</div>
                    <div className="text-neutral-800">$env:TELEGRAM_BOT_TOKEN="your_token"</div>
                    <div className="text-blue-600">odond serve --telegram-token "your_token"</div>
                  </div>
                  <div className="text-[11px] text-neutral-500">
                    Status:{' '}
                    <span className="font-semibold text-emerald-600">
                      {systemStatus?.relays.telegram ? 'Connected & Listening' : 'Waiting for Bot Token'}
                    </span>
                  </div>
                </>
              ) : (
                <>
                  <p>
                    WhatsApp relay connects ODON via a local WhatsApp bridge or cloud webhook. Messages sent to your WhatsApp number trigger background agent orchestration.
                  </p>
                  <div className="p-3 bg-neutral-50 rounded-xl border border-neutral-200/80 font-mono text-[11px] space-y-1">
                    <div className="text-neutral-500 font-sans text-[10px]">Webhook Listener Endpoint:</div>
                    <div className="text-neutral-800">http://127.0.0.1:4096/api/relays/whatsapp</div>
                  </div>
                  <div className="text-[11px] text-neutral-500">
                    Status: <span className="font-semibold text-amber-600">Standby (Ready for webhook pairing)</span>
                  </div>
                </>
              )}
            </div>
            <div className="mt-4 pt-3 border-t border-neutral-100 flex justify-end">
              <button
                onClick={() => setRelayModalOpen(null)}
                className="px-3.5 py-1.5 rounded-lg bg-neutral-900 hover:bg-neutral-800 text-white text-xs font-medium cursor-pointer"
              >
                Close
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}
