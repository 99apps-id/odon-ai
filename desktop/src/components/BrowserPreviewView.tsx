import React, { useState, useEffect, useRef } from 'react';
import {
  ArrowLeft,
  ArrowRight,
  RefreshCw,
  Lock,
  ExternalLink,
  Monitor,
  Smartphone,
  Tablet,
  Globe,
  FileCode,
  Copy,
  Check,
  Send,
  Eye,
  Terminal,
  ScanSearch,
  Info,
  Maximize2,
  FileText,
  Loader2,
  Radio,
  MousePointerClick,
} from 'lucide-react';
import { api } from '../api';
import { MarkdownView } from './MarkdownView';

export interface BrowserSeed {
  url?: string;
  html?: string;
  markdown?: string;
  /** Display name for an artifact-backed page (its title or file path). */
  title?: string;
  /** Live Chromium session the agent is driving: attach the live viewport to it. */
  sessionId?: string;
  /** Wait for the agent to register its session, then attach to it. */
  live?: boolean;
}

interface BrowserPreviewViewProps {
  initialUrl?: string;
  initialHtml?: string;
  seed?: BrowserSeed | null;
  onSendToAgent?: (text: string) => void;
}

export function BrowserPreviewView({
  initialUrl = '',
  initialHtml,
  seed,
  onSendToAgent,
}: BrowserPreviewViewProps) {
  const [url, setUrl] = useState<string>(initialUrl);
  const [inputUrl, setInputUrl] = useState<string>(initialUrl);
  const [htmlContent, setHtmlContent] = useState<string | null>(initialHtml || null);
  const [markdownContent, setMarkdownContent] = useState<string>('');
  const [seedTitle, setSeedTitle] = useState<string>('');
  const [mode, setMode] = useState<'iframe' | 'reader' | 'html' | 'markdown' | 'live'>('iframe');
  const [viewportMode, setViewportMode] = useState<'desktop' | 'tablet' | 'mobile'>('desktop');
  const [isLoading, setIsLoading] = useState<boolean>(false);
  const [history, setHistory] = useState<string[]>([initialUrl]);
  const [historyIndex, setHistoryIndex] = useState<number>(0);
  const [extractedText, setExtractedText] = useState<string>('');
  const [readerLoading, setReaderLoading] = useState<boolean>(false);
  const [copied, setCopied] = useState<boolean>(false);
  const [agentSent, setAgentSent] = useState<boolean>(false);
  // Live bridge: the panel mirrors the same Chromium session the agent drives.
  const [liveSessionId, setLiveSessionId] = useState<string | null>(null);
  const [livePng, setLivePng] = useState<string>('');
  const [liveTitle, setLiveTitle] = useState<string>('');
  const [liveUrl, setLiveUrl] = useState<string>('');
  const [liveError, setLiveError] = useState<string>('');
  const [liveBusy, setLiveBusy] = useState<boolean>(false);
  const liveTimerRef = useRef<number | undefined>(undefined);
  // A refresh takes one screenshot plus one page-state round-trip. Without this
  // guard a slow CDP response lets ticks pile up and the frames arrive out of order.
  const liveInFlightRef = useRef<boolean>(false);
  // An action that finished while a refresh was running still needs its frame.
  const liveRefreshQueuedRef = useRef<boolean>(false);

  const iframeRef = useRef<HTMLIFrameElement>(null);

  // Listen for global custom events to open URL, HTML, Markdown, or the live
  // session the agent is driving.
  useEffect(() => {
    const handleOpenBrowser = (e: Event) => {
      const customEvent = e as CustomEvent<BrowserSeed>;
      const detail = customEvent.detail;
      if (!detail) return;
      // A live session seed must not be proxied through /api/browser/read: the
      // agent's Chromium tab is the source of truth, and only it shows form
      // fills, clicks, and navigation the agent performed.
      if (detail.sessionId) {
        setLiveSessionId(detail.sessionId);
        if (detail.url) setLiveUrl(detail.url);
        setMode('live');
        return;
      }
      if (detail.live) {
        setMode('live');
        return;
      }
      if (detail.markdown) {
        setMarkdownContent(detail.markdown);
        setMode('markdown');
      }
      if (detail.html) {
        setHtmlContent(detail.html);
        setMode('html');
      }
      if (detail.url) {
        navigateTo(detail.url);
      }
    };

    window.addEventListener('odon:open-browser', handleOpenBrowser);
    return () => window.removeEventListener('odon:open-browser', handleOpenBrowser);
  }, [historyIndex, history]);

  // Apply the seed supplied by the parent. Without this the seed prop was dead
  // and a panel opened while unmounted (so it missed the custom event) fell back
  // to the daemon status JSON instead of the page the agent navigated to.
  useEffect(() => {
    if (!seed) return;
    if (seed.title) setSeedTitle(seed.title);
    if (seed.sessionId) {
      setLiveSessionId(seed.sessionId);
      if (seed.url) setLiveUrl(seed.url);
      setMode('live');
      return;
    }
    if (seed.live) {
      setMode('live');
      return;
    }
    if (seed.markdown) {
      setMarkdownContent(seed.markdown);
      setMode('markdown');
    }
    if (seed.html) {
      setHtmlContent(seed.html);
      setMode('html');
    }
    if (seed.url) {
      navigateTo(seed.url);
    }
  }, [seed]);

  const renderUrl = (formatted: string) => {
    setInputUrl(formatted);
    setUrl(formatted);
    setHtmlContent(null);
    setMode('iframe');
    setIsLoading(true);

    // Fetch and proxy the page. Direct iframes are blocked by X-Frame-Options,
    // CSP frame-ancestors, and unavailable localhost servers; the daemon proxy
    // makes typed URLs render reliably and still exposes text to the reader.
    fetchPagePreview(formatted);
  };

  const navigateTo = (newUrl: string) => {
    let formatted = newUrl.trim();
    // Only http(s) pages may be embedded. data:, javascript: and file: URLs
    // would turn the preview into a script/local-file execution surface.
    if (!/^https?:\/\//i.test(formatted)) {
      if (/^[a-z][a-z0-9+.-]*:/i.test(formatted)) {
        setExtractedText(`Blocked URL scheme: ${formatted}`);
        return;
      }
      formatted = `https://${formatted}`;
    }
    const updatedHistory = history.slice(0, historyIndex + 1);
    updatedHistory.push(formatted);
    setHistory(updatedHistory);
    setHistoryIndex(updatedHistory.length - 1);

    renderUrl(formatted);
  };

  const buildPreviewError = (targetUrl: string, message: string) =>
    `<!doctype html><html><head><meta charset="utf-8"><title>Preview unavailable</title></head><body style="margin:0;background:#fff;color:#111;font-family:system-ui"><div style="padding:24px"><h1 style="font-size:18px;margin:0 0 8px">Preview unavailable</h1><p style="font-size:13px;line-height:1.5;margin:0 0 12px">${message.replace(/[<>&]/g, '')}</p><code style="font-size:12px;color:#555">${targetUrl}</code></div></body></html>`;

  const fetchPagePreview = async (targetUrl: string) => {
    setReaderLoading(true);
    try {
      const data = await api.browserRead(targetUrl);
      const sections = [
        data.title ? `Title: ${data.title}` : '',
        `URL: ${data.url}`,
        `HTTP Status: ${data.status}`,
        '',
        data.text || '(No readable text found)',
      ];
      if (data.links?.length) {
        sections.push('', 'Links:', ...data.links.slice(0, 40));
      }
      setExtractedText(sections.filter(Boolean).join('\n'));
      setHtmlContent(
        data.html ||
          buildPreviewError(data.url, 'The server returned a response without renderable HTML.'),
      );
      setMode('iframe');
    } catch (e: any) {
      const message = e.message || 'Unknown error';
      setExtractedText(`Could not read ${targetUrl}\n\n${message}`);
      setHtmlContent(buildPreviewError(targetUrl, message));
      setMode('iframe');
    } finally {
      setReaderLoading(false);
      setIsLoading(false);
    }
  };

  const handleRefresh = () => {
    renderUrl(url);
  };

  const handleBack = () => {
    if (historyIndex > 0) {
      const prev = history[historyIndex - 1];
      setHistoryIndex(historyIndex - 1);
      renderUrl(prev);
    }
  };

  const handleForward = () => {
    if (historyIndex < history.length - 1) {
      const next = history[historyIndex + 1];
      setHistoryIndex(historyIndex + 1);
      renderUrl(next);
    }
  };

  const handleUrlSubmit = (e: React.FormEvent) => {
    e.preventDefault();
    if (mode === 'live' && liveSessionId) {
      const target = /^https?:/i.test(inputUrl) ? inputUrl : `https://${inputUrl}`;
      setLiveBusy(true);
      api
        .browserNavigate(liveSessionId, target)
        .then(() => refreshLive(liveSessionId))
        .catch((err: any) => setLiveError(err?.message || 'Navigation failed'))
        .finally(() => setLiveBusy(false));
      return;
    }
    navigateTo(inputUrl);
  };

  const handleSendToAgent = () => {
    const payload = `I'm viewing this web page in the browser panel: ${url}\n\nPage content / summary:\n\`\`\`text\n${extractedText || 'Web page loaded at ' + url}\n\`\`\`\nPlease analyze this page and explain what it does.`;
    
    if (onSendToAgent) {
      onSendToAgent(payload);
    } else {
      window.dispatchEvent(new CustomEvent('odon:send-to-agent', { detail: { text: payload } }));
    }

    setAgentSent(true);
    setTimeout(() => setAgentSent(false), 2500);
  };

  const handleCopyText = () => {
    navigator.clipboard.writeText(extractedText || url);
    setCopied(true);
    setTimeout(() => setCopied(false), 2000);
  };

  const refreshLive = async (id: string) => {
    if (liveInFlightRef.current) {
      liveRefreshQueuedRef.current = true;
      return;
    }
    liveInFlightRef.current = true;
    try {
      const png = await api.browserScreenshot(id);
      if (png) setLivePng(png);
      setLiveError('');
      const state = await api.browserPageState(id);
      if (state?.url) {
        setLiveUrl(state.url);
        setInputUrl(state.url);
      }
      setLiveTitle(state?.title || '');
    } catch (e: any) {
      setLiveError(e?.message || 'Live view unavailable');
    } finally {
      liveInFlightRef.current = false;
      if (liveRefreshQueuedRef.current) {
        liveRefreshQueuedRef.current = false;
        void refreshLive(id);
      }
    }
  };

  const ensureLive = async () => {
    setLiveBusy(true);
    try {
      const list = await api.browserSessions();
      let id = liveSessionId || list.sessions?.[0]?.session_id || null;
      if (!id) {
        const target = /^https?:/i.test(url) ? url : 'about:blank';
        const created = await api.browserOpenSession(target);
        id = created.session_id;
      }
      setLiveSessionId(id);
      await refreshLive(id);
    } catch (e: any) {
      setLiveError(e?.message || 'Cannot start a live browser session');
    } finally {
      setLiveBusy(false);
    }
  };

  // True while the agent is starting a session and the panel is waiting to
  // attach to it. Creating a browser of our own in that window would leave the
  // panel showing a second, unrelated page.
  const waitingForAgentSession = mode === 'live' && !liveSessionId && Boolean(seed?.live);

  useEffect(() => {
    if (mode !== 'live' || !waitingForAgentSession) return;
    let cancelled = false;
    const attach = async () => {
      try {
        const list = await api.browserSessions();
        const live = (list.sessions || []).find((s) => s.live !== false);
        if (!cancelled && live?.session_id) {
          setLiveError('');
          setLiveSessionId(live.session_id);
        }
      } catch {
        // The daemon may still be spawning Chromium; the next tick retries.
      }
    };
    void attach();
    const timer = window.setInterval(attach, 700);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [mode, waitingForAgentSession]);

  useEffect(() => {
    const handleRefresh = (e: Event) => {
      const detail = (e as CustomEvent<{ sessionId?: string }>).detail;
      const id = detail?.sessionId || liveSessionId;
      if (id) void refreshLive(id);
    };
    const handleDetach = (e: Event) => {
      const detail = (e as CustomEvent<{ sessionId?: string }>).detail;
      if (detail?.sessionId && liveSessionId && detail.sessionId !== liveSessionId) return;
      setLiveSessionId(null);
      setLivePng('');
      setLiveTitle('');
      setLiveError('');
    };
    window.addEventListener('odon:browser-refresh', handleRefresh);
    window.addEventListener('odon:browser-detach', handleDetach);
    return () => {
      window.removeEventListener('odon:browser-refresh', handleRefresh);
      window.removeEventListener('odon:browser-detach', handleDetach);
    };
  }, [liveSessionId]);

  useEffect(() => {
    if (liveTimerRef.current !== undefined) {
      window.clearInterval(liveTimerRef.current);
      liveTimerRef.current = undefined;
    }
    if (mode !== 'live') return;
    if (!liveSessionId) {
      if (!waitingForAgentSession) void ensureLive();
      return;
    }
    void refreshLive(liveSessionId);
    liveTimerRef.current = window.setInterval(() => {
      void refreshLive(liveSessionId);
    }, 700);
    return () => {
      if (liveTimerRef.current !== undefined) {
        window.clearInterval(liveTimerRef.current);
        liveTimerRef.current = undefined;
      }
    };
  }, [mode, liveSessionId, waitingForAgentSession]);

  const handleLiveClick = async (e: React.MouseEvent<HTMLImageElement>) => {
    if (!liveSessionId) return;
    const img = e.currentTarget;
    const rect = img.getBoundingClientRect();
    const natW = img.naturalWidth || 0;
    const natH = img.naturalHeight || 0;
    if (rect.width === 0 || rect.height === 0 || natW === 0 || natH === 0) return;
    // The frame is drawn with `object-contain`, so the bitmap is letterboxed
    // inside the element. Mapping against the element box alone sends the agent's
    // clicks to the wrong pixel whenever the aspect ratios differ.
    const scale = Math.min(rect.width / natW, rect.height / natH);
    const offsetX = rect.left + (rect.width - natW * scale) / 2;
    const offsetY = rect.top + (rect.height - natH * scale) / 2;
    const x = (e.clientX - offsetX) / scale;
    const y = (e.clientY - offsetY) / scale;
    if (x < 0 || y < 0 || x > natW || y > natH) return;
    try {
      await api.browserAct(liveSessionId, { action: 'click', x, y });
      await refreshLive(liveSessionId);
    } catch (err: any) {
      setLiveError(err?.message || 'Click failed');
    }
  };

  return (
    <div className="h-full flex flex-col bg-white rounded-2xl overflow-hidden border border-neutral-200/90 shadow-2xs font-sans">
      {/* ─────────────────────────────────────────────────────────────
          MINIMALIST BROWSER TOP BAR
      ───────────────────────────────────────────────────────────── */}
      <div className="bg-[#f9fafb] border-b border-neutral-200/80 px-3 py-2 flex items-center justify-between gap-2 shrink-0">
        {/* Navigation Arrows & Refresh */}
        <div className="flex items-center gap-0.5">
          <button
            onClick={handleBack}
            disabled={historyIndex <= 0}
            className="p-1 rounded-md text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200/60 transition-colors disabled:opacity-30 cursor-pointer"
            title="Back"
          >
            <ArrowLeft className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleForward}
            disabled={historyIndex >= history.length - 1}
            className="p-1 rounded-md text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200/60 transition-colors disabled:opacity-30 cursor-pointer"
            title="Forward"
          >
            <ArrowRight className="w-3.5 h-3.5" />
          </button>
          <button
            onClick={handleRefresh}
            className={`p-1 rounded-md text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200/60 transition-colors cursor-pointer ${
              isLoading ? 'animate-spin text-blue-600' : ''
            }`}
            title="Reload preview"
          >
            <RefreshCw className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* URL Address Bar */}
        <form onSubmit={handleUrlSubmit} className="flex-1 max-w-md bg-white border border-neutral-300/80 rounded-xl px-2.5 py-1 flex items-center gap-1.5 shadow-2xs focus-within:border-blue-500 focus-within:ring-1 focus-within:ring-blue-500">
          <Lock className="w-3 h-3 text-emerald-600 shrink-0" />
          <input
            type="text"
            value={inputUrl}
            onChange={(e) => setInputUrl(e.target.value)}
            placeholder="Type URL (e.g. http://localhost:3000)..."
            className="flex-1 text-xs font-mono text-neutral-800 outline-none bg-transparent"
          />
          <span className="text-[10px] font-mono text-neutral-500 px-1 py-0.2 rounded bg-neutral-100 shrink-0">
            {isLoading ? 'Loading...' : 'Ready'}
          </span>
        </form>

        {/* Viewport switch & Actions */}
        <div className="flex items-center gap-1">
          {/* Mode Switcher */}
          <div className="flex items-center bg-neutral-200/70 p-0.5 rounded-lg text-[11px] font-medium text-neutral-600">
            <button
              onClick={() => setMode('iframe')}
              className={`px-2 py-0.5 rounded-md transition-all cursor-pointer ${mode === 'iframe' ? 'bg-white text-blue-600 font-semibold shadow-2xs' : 'hover:text-neutral-900'}`}
              title="Interactive Web View"
            >
              <span className="flex items-center gap-1">
                <Globe className="w-3 h-3" />
                <span className="hidden sm:inline">Web</span>
              </span>
            </button>
            <button
              onClick={() => setMode('reader')}
              className={`px-2 py-0.5 rounded-md transition-all cursor-pointer ${mode === 'reader' ? 'bg-white text-blue-600 font-semibold shadow-2xs' : 'hover:text-neutral-900'}`}
              title="Agent Text Reader & Inspector"
            >
              <span className="flex items-center gap-1">
                {readerLoading ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <Eye className="w-3 h-3" />
                )}
                <span className="hidden sm:inline">Reader</span>
              </span>
            </button>
            <button
              onClick={() => setMode('live')}
              className={`px-2 py-0.5 rounded-md transition-all cursor-pointer ${mode === 'live' ? 'bg-white text-blue-600 font-semibold shadow-2xs' : 'hover:text-neutral-900'}`}
              title="Live CDP session shared with the agent"
            >
              <span className="flex items-center gap-1">
                <Radio className={`w-3 h-3 ${mode === 'live' ? 'animate-pulse' : ''}`} />
                <span className="hidden sm:inline">Live</span>
              </span>
            </button>
            {markdownContent && (
              <button
                onClick={() => setMode('markdown')}
                className={`px-2 py-0.5 rounded-md transition-all cursor-pointer ${mode === 'markdown' ? 'bg-white text-blue-600 font-semibold shadow-2xs' : 'hover:text-neutral-900'}`}
                title="Render Markdown / chart artifact"
              >
                <span className="flex items-center gap-1">
                  <FileText className="w-3 h-3" />
                  <span className="hidden sm:inline">Doc</span>
                </span>
              </button>
            )}
          </div>

          {/* Viewport Width */}
          <div className="flex items-center bg-neutral-200/70 p-0.5 rounded-lg">
            <button
              onClick={() => setViewportMode('desktop')}
              className={`p-1 rounded-md transition-all cursor-pointer ${viewportMode === 'desktop' ? 'bg-white text-neutral-900 shadow-2xs' : 'text-neutral-500 hover:text-neutral-800'}`}
              title="Desktop viewport (100%)"
            >
              <Monitor className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => setViewportMode('tablet')}
              className={`p-1 rounded-md transition-all cursor-pointer ${viewportMode === 'tablet' ? 'bg-white text-neutral-900 shadow-2xs' : 'text-neutral-500 hover:text-neutral-800'}`}
              title="Tablet viewport (768px)"
            >
              <Tablet className="w-3.5 h-3.5" />
            </button>
            <button
              onClick={() => setViewportMode('mobile')}
              className={`p-1 rounded-md transition-all cursor-pointer ${viewportMode === 'mobile' ? 'bg-white text-neutral-900 shadow-2xs' : 'text-neutral-500 hover:text-neutral-800'}`}
              title="Mobile viewport (375px)"
            >
              <Smartphone className="w-3.5 h-3.5" />
            </button>
          </div>

          {/* Artifact-backed page label */}
          {seedTitle && (mode === 'html' || mode === 'markdown') && (
            <span className="hidden md:flex items-center gap-1 px-2 py-0.5 rounded-md bg-neutral-200/70 text-[11px] font-medium text-neutral-600 max-w-[220px] truncate">
              <FileText className="w-3 h-3 shrink-0" />
              <span className="truncate">{seedTitle}</span>
            </span>
          )}

          {/* Popout to OS Browser */}
          <a
            href={url}
            target="_blank"
            rel="noreferrer"
            className="p-1.5 text-neutral-500 hover:text-neutral-900 hover:bg-neutral-200 rounded-lg transition-colors cursor-pointer"
            title="Open in default browser window"
          >
            <ExternalLink className="w-3.5 h-3.5" />
          </a>
        </div>
      </div>

      {/* Quick URL Preset Chips */}
      <div className="bg-[#fcfdfd] border-b border-neutral-100 px-3 py-1.5 flex items-center gap-2 overflow-x-auto text-[11px] shrink-0">
        <span className="text-neutral-400 font-medium">Quick links:</span>
        {[
          { label: 'Daemon Status', target: 'http://127.0.0.1:4096/api/status' },
          { label: 'Dev Server :3000', target: 'http://localhost:3000' },
          { label: 'Vite :5173', target: 'http://localhost:5173' },
          { label: 'Registered Tools', target: 'http://127.0.0.1:4096/api/tools' },
          { label: 'MCP Servers', target: 'http://127.0.0.1:4096/api/mcp/servers' },
        ].map((preset) => (
          <button
            key={preset.target}
            onClick={() => navigateTo(preset.target)}
            className="px-2 py-0.5 rounded-md bg-neutral-100 hover:bg-neutral-200/80 text-neutral-600 hover:text-neutral-900 transition-colors cursor-pointer shrink-0"
          >
            {preset.label}
          </button>
        ))}
      </div>

      {/* ─────────────────────────────────────────────────────────────
          CANVAS VIEWPORT AREA
      ───────────────────────────────────────────────────────────── */}
      <div className="flex-1 overflow-auto bg-neutral-100/60 p-3 flex justify-center items-stretch relative">
        {(mode === 'iframe' || mode === 'html') && (
          <div
            className={`bg-white rounded-xl shadow-xs border border-neutral-200/90 overflow-hidden flex flex-col transition-all duration-200 ${
              viewportMode === 'mobile'
                ? 'w-[375px]'
                : viewportMode === 'tablet'
                ? 'w-[768px]'
                : 'w-full'
            }`}
          >
            {!url && !htmlContent ? (
              <div className="flex-1 flex flex-col items-center justify-center gap-2 p-6 text-center">
                <p className="text-sm font-medium text-neutral-600">No page loaded</p>
                <p className="text-xs text-neutral-500">
                  Type a URL above, or let the agent navigate and this panel follows.
                </p>
              </div>
            ) : (
              <iframe
                ref={iframeRef}
                src={htmlContent ? undefined : url}
                srcDoc={htmlContent || undefined}
                onLoad={() => setIsLoading(false)}
                className="w-full h-full border-0 bg-white"
                sandbox="allow-scripts allow-forms"
                referrerPolicy="no-referrer"
                title="Browser Preview Viewport"
              />
            )}
          </div>
        )}

        {mode === 'live' && (
          <div
            className={`bg-white rounded-xl shadow-xs border border-neutral-200/90 overflow-hidden flex flex-col transition-all duration-200 ${
              viewportMode === 'mobile' ? 'w-[375px]' : viewportMode === 'tablet' ? 'w-[768px]' : 'w-full'
            }`}
          >
            <div className="flex items-center justify-between gap-2 px-2.5 py-1.5 border-b border-neutral-200/80 bg-[#f9fafb] text-[11px] shrink-0">
              <div className="flex items-center gap-1.5 min-w-0">
                <span
                  className={`w-1.5 h-1.5 rounded-full shrink-0 ${
                    liveError ? 'bg-clay-600' : liveSessionId ? 'bg-emerald-600' : 'bg-neutral-300'
                  }`}
                />
                <span className="font-mono text-neutral-600 truncate" title={liveUrl || url}>
                  {liveTitle || liveUrl || 'No live session yet'}
                </span>
              </div>
              <div className="flex items-center gap-1.5 shrink-0">
                <span className="font-mono text-[10px] text-neutral-400" title="Session shared with agent tools">
                  {liveSessionId || '—'}
                </span>
                <button
                  onClick={() => void ensureLive()}
                  disabled={liveBusy}
                  className="px-1.5 py-0.5 rounded bg-neutral-100 hover:bg-neutral-200 text-neutral-600 disabled:opacity-50 cursor-pointer"
                  title="Attach to the newest live session, or create one"
                >
                  {liveBusy ? 'Connecting…' : 'Attach'}
                </button>
              </div>
            </div>

            <div className="flex-1 relative bg-neutral-100/60 overflow-hidden">
              {livePng ? (
                <img
                  src={`data:image/png;base64,${livePng}`}
                  onClick={handleLiveClick}
                  alt="Live browser viewport"
                  className="w-full h-full object-contain cursor-crosshair select-none"
                  draggable={false}
                />
              ) : (
                <div className="absolute inset-0 flex flex-col items-center justify-center gap-2 text-neutral-500 text-xs p-6 text-center">
                  <Radio className="w-5 h-5 text-neutral-400" />
                  <span>
                    {liveError
                      ? liveError
                      : liveBusy
                        ? 'Starting a Chromium session…'
                        : waitingForAgentSession
                          ? 'Waiting for the agent to start its browser session…'
                          : 'No live session. Press Attach to open one the agent can drive too.'}
                  </span>
                </div>
              )}
              {liveError && livePng && (
                <div className="absolute bottom-2 left-2 right-2 px-2 py-1 rounded bg-clay-50 border border-clay-200 text-[11px] text-clay-800">
                  {liveError}
                </div>
              )}
            </div>

            <div className="px-2.5 py-1 border-t border-neutral-200/80 bg-white text-[10px] text-neutral-500 flex items-center gap-1.5 shrink-0">
              <MousePointerClick className="w-3 h-3" />
              <span>Click the page to drive it. The agent sees and clicks this same tab.</span>
            </div>
          </div>
        )}

        {mode === 'markdown' && (
          <div className="w-full max-w-3xl bg-white rounded-xl border border-neutral-200 p-6 shadow-xs overflow-y-auto flex flex-col">
            <div className="flex items-center justify-between pb-3 border-b border-neutral-100 mb-4">
              <div className="flex items-center gap-2">
                <FileText className="w-4 h-4 text-blue-600" />
                <h3 className="font-bold text-xs text-neutral-900">Markdown & Chart Preview</h3>
              </div>
              <button
                onClick={handleSendToAgent}
                className="px-3 py-1 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium transition-colors flex items-center gap-1 shadow-2xs cursor-pointer"
              >
                <Send className="w-3 h-3" />
                <span>{agentSent ? 'Sent to Agent!' : 'Send to Agent'}</span>
              </button>
            </div>
            <div className="flex-1 overflow-y-auto min-w-0 select-text">
              <MarkdownView content={markdownContent} />
            </div>
          </div>
        )}

        {mode === 'reader' && (
          <div className="w-full max-w-3xl bg-white rounded-xl border border-neutral-200 p-5 shadow-xs overflow-y-auto flex flex-col">
            <div className="flex items-center justify-between pb-3 border-b border-neutral-100 mb-3">
              <div className="flex items-center gap-2">
                <ScanSearch className="w-4 h-4 text-blue-600" />
                <h3 className="font-bold text-xs text-neutral-900">Agent Page Reader & DOM Inspector</h3>
              </div>
              <div className="flex items-center gap-2">
                <button
                  onClick={handleCopyText}
                  className="px-2.5 py-1 rounded-lg border border-neutral-200 hover:bg-neutral-200 text-neutral-700 text-xs font-medium transition-colors flex items-center gap-1 cursor-pointer"
                >
                  {copied ? <Check className="w-3 h-3 text-emerald-600" /> : <Copy className="w-3 h-3" />}
                  <span>{copied ? 'Copied' : 'Copy Text'}</span>
                </button>
                <button
                  onClick={handleSendToAgent}
                  className="px-3 py-1 rounded-lg bg-blue-600 hover:bg-blue-700 text-white text-xs font-medium transition-colors flex items-center gap-1 shadow-2xs cursor-pointer"
                >
                  <Send className="w-3 h-3" />
                  <span>{agentSent ? 'Sent to Agent!' : 'Send Page to Agent'}</span>
                </button>
              </div>
            </div>

            <div className="p-3 bg-neutral-50 rounded-xl border border-neutral-200/80 mb-3 text-xs font-mono space-y-1">
              <div><strong className="text-neutral-500 font-sans">Active URL:</strong> {url}</div>
              <div><strong className="text-neutral-500 font-sans">Mode:</strong> Headless Reading & Analysis</div>
              <div><strong className="text-neutral-500 font-sans">Last Read:</strong> {new Date().toLocaleTimeString()}</div>
            </div>

            <div className="flex-1 overflow-y-auto p-4 bg-neutral-900 text-neutral-100 rounded-xl font-mono text-xs leading-relaxed">
              {readerLoading ? (
                <div className="flex items-center gap-2 text-neutral-300">
                  <Loader2 className="w-4 h-4 animate-spin text-blue-400" />
                  <span>Reading page content…</span>
                </div>
              ) : (
                <pre className="whitespace-pre-wrap">{extractedText || `Inspecting web content at ${url}...\n\nUse 'Send Page to Agent' so ODON can read, extract data, and reason about this page.`}</pre>
              )}
            </div>
          </div>
        )}
      </div>

      {/* ─────────────────────────────────────────────────────────────
          BOTTOM STATUS STRIP
      ───────────────────────────────────────────────────────────── */}
      <div className="px-3.5 py-1.5 bg-white border-t border-neutral-100 flex items-center justify-between text-[11px] text-neutral-500 font-mono shrink-0">
        <div className="flex items-center gap-2 truncate">
          <span className="inline-block w-2 h-2 rounded-full bg-emerald-500" />
          <span className="truncate max-w-sm">{url}</span>
        </div>
        <div className="flex items-center gap-3 shrink-0">
          <button
            onClick={handleSendToAgent}
            className="text-blue-600 hover:underline font-medium font-sans flex items-center gap-1 cursor-pointer"
            title="Send current webpage context to ODON agent"
          >
            <Send className="w-3 h-3" />
            <span>{agentSent ? 'Sent!' : 'Send to Agent'}</span>
          </button>
          <span>{viewportMode.toUpperCase()}</span>
        </div>
      </div>
    </div>
  );
}
