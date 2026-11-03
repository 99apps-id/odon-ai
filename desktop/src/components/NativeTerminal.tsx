import React, { useEffect, useRef, useState } from 'react';
import { Terminal } from '@xterm/xterm';
import { FitAddon } from '@xterm/addon-fit';
import '@xterm/xterm/css/xterm.css';
import { api } from '../api';
import { RotateCcw, Wifi, WifiOff, Terminal as TerminalIcon } from 'lucide-react';

export interface NativeTerminalProps {
  shell?: string;
  cwd?: string;
  sshProfileId?: string;
  title?: string;
  className?: string;
  onData?: (data: string) => void;
  initialCommand?: string;
}

export const NativeTerminal: React.FC<NativeTerminalProps> = ({
  shell,
  cwd,
  sshProfileId,
  title,
  className = '',
  onData,
  initialCommand,
}) => {
  const containerRef = useRef<HTMLDivElement>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const wsRef = useRef<WebSocket | null>(null);

  const [connected, setConnected] = useState<boolean>(false);
  const [connecting, setConnecting] = useState<boolean>(true);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);

  const connectTerminal = () => {
    if (!containerRef.current) return;

    setConnecting(true);
    setErrorMsg(null);

    // Clean up previous terminal instance if any
    if (wsRef.current) {
      wsRef.current.close();
      wsRef.current = null;
    }
    if (termRef.current) {
      termRef.current.dispose();
      termRef.current = null;
    }
    containerRef.current.innerHTML = '';

    // Initialize xterm.js
    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: 'block',
      fontFamily: 'Consolas, "Fira Code", Monaco, Menlo, "Courier New", monospace',
      fontSize: 13,
      lineHeight: 1.25,
      theme: {
        background: '#0d1117',
        foreground: '#e6edf3',
        cursor: '#58a6ff',
        cursorAccent: '#0d1117',
        selectionBackground: 'rgba(56, 139, 253, 0.35)',
        black: '#161b22',
        red: '#ff7b72',
        green: '#3fb950',
        yellow: '#d29922',
        blue: '#58a6ff',
        magenta: '#bc8cff',
        cyan: '#39c5cf',
        white: '#b1bac4',
        brightBlack: '#6e7681',
        brightRed: '#ffa198',
        brightGreen: '#56d364',
        brightYellow: '#e3b341',
        brightBlue: '#79c0ff',
        brightMagenta: '#d2a8ff',
        brightCyan: '#56d4dd',
        brightWhite: '#f0f6fc',
      },
      scrollback: 5000,
      convertEol: true,
    });

    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);

    term.open(containerRef.current);
    termRef.current = term;
    fitAddonRef.current = fitAddon;

    try {
      fitAddon.fit();
    } catch {
      // Ignore if not rendered yet
    }

    // Connect WebSocket to native PTY
    const cols = term.cols || 100;
    const rows = term.rows || 28;
    const wsUrl = api.getTerminalWsUrl({
      shell,
      cwd,
      ssh_profile_id: sshProfileId,
      cols,
      rows,
    });

    const ws = new WebSocket(wsUrl);
    ws.binaryType = 'arraybuffer';
    wsRef.current = ws;

    ws.onopen = () => {
      setConnected(true);
      setConnecting(false);
      term.focus();

      // Send initial resize
      ws.send(JSON.stringify({ type: 'resize', cols: term.cols, rows: term.rows }));

      if (initialCommand) {
        setTimeout(() => {
          if (ws.readyState === WebSocket.OPEN) {
            ws.send(initialCommand + '\r');
          }
        }, 300);
      }
    };

    ws.onmessage = (event) => {
      if (typeof event.data === 'string') {
        term.write(event.data);
        if (onData) onData(event.data);
      } else if (event.data instanceof ArrayBuffer) {
        const u8 = new Uint8Array(event.data);
        term.write(u8);
        if (onData) onData(new TextDecoder().decode(u8));
      }
    };

    ws.onclose = () => {
      setConnected(false);
      setConnecting(false);
      term.write('\r\n\x1b[33m[Session disconnected. Press Reconnect to restart.]\x1b[0m\r\n');
    };

    ws.onerror = () => {
      setConnected(false);
      setConnecting(false);
      setErrorMsg('Failed to connect to ODON native PTY server');
      term.write('\r\n\x1b[31m[WebSocket Connection Error: Check daemon at 127.0.0.1:4096]\x1b[0m\r\n');
    };

    // Forward user keystrokes from xterm to native PTY
    term.onData((data) => {
      if (ws.readyState === WebSocket.OPEN) {
        ws.send(data);
      }
    });
  };

  useEffect(() => {
    connectTerminal();

    // Resize Observer for auto-fitting xterm when window or panel changes
    let resizeTimer: any = null;
    const observer = new ResizeObserver(() => {
      clearTimeout(resizeTimer);
      resizeTimer = setTimeout(() => {
        if (fitAddonRef.current && termRef.current && wsRef.current?.readyState === WebSocket.OPEN) {
          try {
            fitAddonRef.current.fit();
            wsRef.current.send(
              JSON.stringify({
                type: 'resize',
                cols: termRef.current.cols,
                rows: termRef.current.rows,
              })
            );
          } catch {
            // Ignore
          }
        }
      }, 50);
    });

    if (containerRef.current) {
      observer.observe(containerRef.current);
    }

    return () => {
      observer.disconnect();
      if (wsRef.current) {
        wsRef.current.close();
      }
      if (termRef.current) {
        termRef.current.dispose();
      }
    };
  }, [shell, cwd, sshProfileId]);

  return (
    <div className={`flex flex-col h-full bg-[#0d1117] rounded-xl overflow-hidden border border-neutral-800 ${className}`}>
      {/* Top Status & Controls Bar */}
      <div className="flex items-center justify-between px-3.5 py-1.5 bg-[#161b22] border-b border-neutral-800 text-xs shrink-0 select-none">
        <div className="flex items-center gap-2">
          <TerminalIcon className="w-3.5 h-3.5 text-blue-400" />
          <span className="font-semibold text-neutral-300 font-mono text-[11px]">
            {title || (sshProfileId ? `SSH: ${sshProfileId}` : shell ? `${shell.toUpperCase()} Native PTY` : 'Shell Native PTY')}
          </span>
          <span className="text-[10px] text-neutral-500 font-mono">â€¢ PTY</span>
        </div>

        <div className="flex items-center gap-2.5">
          <div className="flex items-center gap-1.5 text-[11px] font-mono">
            {connecting ? (
              <span className="inline-flex items-center gap-1 text-amber-400">
                <span className="w-1.5 h-1.5 rounded-full bg-amber-400 animate-pulse" />
                <span>Connecting...</span>
              </span>
            ) : connected ? (
              <span className="inline-flex items-center gap-1 text-emerald-400">
                <Wifi className="w-3 h-3" />
                <span>Online</span>
              </span>
            ) : (
              <span className="inline-flex items-center gap-1 text-clay-400">
                <WifiOff className="w-3 h-3" />
                <span>Disconnected</span>
              </span>
            )}
          </div>

          <button
            onClick={connectTerminal}
            className="flex items-center gap-1 px-2 py-0.5 rounded text-[11px] font-medium text-neutral-300 hover:text-white bg-neutral-800 hover:bg-neutral-700 transition-colors cursor-pointer"
            title="Restart / Reconnect Terminal Session"
          >
            <RotateCcw className="w-3 h-3" />
            <span>Restart</span>
          </button>
        </div>
      </div>

      {/* Xterm.js Container */}
      <div
        ref={containerRef}
        className="flex-1 w-full h-full min-h-0 overflow-hidden p-2"
        style={{ backgroundColor: '#0d1117' }}
      />
    </div>
  );
};

