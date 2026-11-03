import React, { useEffect, useState } from 'react';
import {
  AlertCircle,
  Check,
  Loader2,
  LogOut,
  QrCode,
  Radio,
  RefreshCw,
  Send,
  Smartphone,
} from 'lucide-react';
import { api, TelegramRelayStatus, WhatsAppBridgeStatus } from '../api';

export const RelaysPanel: React.FC = () => {
  const [telegram, setTelegram] = useState<TelegramRelayStatus | null>(null);
  const [telegramToken, setTelegramToken] = useState('');
  const [telegramAllowed, setTelegramAllowed] = useState('');
  const [telegramBusy, setTelegramBusy] = useState(false);
  const [telegramError, setTelegramError] = useState<string | null>(null);
  const [telegramSaved, setTelegramSaved] = useState(false);

  const [whatsapp, setWhatsApp] = useState<WhatsAppBridgeStatus | null>(null);
  const [waPhone, setWaPhone] = useState('');
  const [waAllowed, setWaAllowed] = useState('');
  const [waAllowAnyone, setWaAllowAnyone] = useState(false);
  const [waBusy, setWaBusy] = useState(false);
  const [waError, setWaError] = useState<string | null>(null);

  const loadTelegram = async () => {
    try {
      const status = await api.getTelegramRelay();
      setTelegram(status);
      setTelegramAllowed(status.allowed_users.join(', '));
    } catch {
      /* daemon offline */
    }
  };

  const loadWhatsApp = async () => {
    try {
      setWhatsApp(await api.getWhatsAppBridgeStatus());
    } catch {
      /* bridge offline */
    }
  };

  useEffect(() => {
    loadTelegram();
    loadWhatsApp();
    let inFlight = false;
    const timer = window.setInterval(async () => {
      if (inFlight) return;
      inFlight = true;
      try {
        setWhatsApp(await api.getWhatsAppBridgeStatus());
      } finally {
        inFlight = false;
      }
    }, 2500);
    return () => window.clearInterval(timer);
  }, []);

  const saveTelegram = async () => {
    const token = telegramToken.trim();
    if (!token) {
      setTelegramError('Enter a bot token from @BotFather first.');
      return;
    }
    const allowedUsers = telegramAllowed
      .split(',')
      .map((item) => item.trim())
      .filter(Boolean);
    setTelegramBusy(true);
    setTelegramError(null);
    setTelegramSaved(false);
    try {
      const status = await api.saveTelegramRelay(token, allowedUsers);
      setTelegram(status);
      setTelegramToken('');
      setTelegramSaved(true);
      setTimeout(() => setTelegramSaved(false), 2500);
    } catch (error: any) {
      setTelegramError(error?.message || 'Failed to connect Telegram.');
    } finally {
      setTelegramBusy(false);
    }
  };

  const disconnectTelegram = async () => {
    setTelegramBusy(true);
    setTelegramError(null);
    try {
      await api.disconnectTelegramRelay();
      await loadTelegram();
    } catch (error: any) {
      setTelegramError(error?.message || 'Failed to disconnect Telegram.');
    } finally {
      setTelegramBusy(false);
    }
  };

  const startWhatsApp = async () => {
    const allowedNumbers = waAllowed
      .split(',')
      .map((item) => item.replace(/\D/g, ''))
      .filter(Boolean);
    const phone = waPhone.replace(/\D/g, '');
    setWaBusy(true);
    setWaError(null);
    try {
      const status = await api.startWhatsAppBridge(
        phone || null,
        allowedNumbers,
        waAllowAnyone,
      );
      setWhatsApp(status);
      if (status?.error) setWaError(status.error);
    } catch (error: any) {
      const message = error?.message || 'Failed to start WhatsApp bridge.';
      setWaError(
        message.toLowerCase().includes('unauthorized')
          ? 'The WhatsApp bridge rejected the daemon token. Restart the existing bridge process, then click Connect again.'
          : message,
      );
    } finally {
      setWaBusy(false);
    }
  };

  const logoutWhatsApp = async () => {
    setWaBusy(true);
    setWaError(null);
    try {
      await api.logoutWhatsAppBridge();
      await loadWhatsApp();
    } catch (error: any) {
      setWaError(error?.message || 'Failed to log out WhatsApp.');
    } finally {
      setWaBusy(false);
    }
  };

  const telegramOnline = !!telegram?.online;
  const waConnected = !!whatsapp?.connected;
  const waRunning = !!whatsapp?.running;
  const waWaiting = waRunning && !waConnected;

  return (
    <div className="flex-1 overflow-y-auto p-6 space-y-6 text-[13px]">
      <div>
        <span className="font-bold text-sm text-neutral-900 dark:text-white block">
          Messaging Relays
        </span>
        <span className="text-xs text-neutral-500">
          Connect ODON to Telegram and WhatsApp without restarting the daemon. Messages run through
          the same agent runtime, tools, and workspace.
        </span>
      </div>

      {/* Telegram */}
      <div className="rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3 border-b border-neutral-100 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-950/30">
          <div className="flex items-center gap-2">
            <div className="p-1.5 rounded-lg bg-blue-50 dark:bg-blue-950 text-blue-600 dark:text-blue-400 border border-blue-100 dark:border-blue-900">
              <Radio className="w-4 h-4" />
            </div>
            <div>
              <div className="font-semibold text-xs text-neutral-900 dark:text-white">Telegram Bot</div>
              <div className="text-[11px] text-neutral-500">
                Long polling; no public URL required.
              </div>
            </div>
          </div>
          <span
            className={`inline-flex items-center gap-1 text-[10px] font-semibold px-2 py-0.5 rounded-full border ${
              telegramOnline
                ? 'bg-emerald-50 text-emerald-700 border-emerald-200 dark:bg-emerald-950/40 dark:text-emerald-300 dark:border-emerald-800'
                : 'bg-neutral-100 text-neutral-600 border-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:border-neutral-700'
            }`}
          >
            <span className={`w-1.5 h-1.5 rounded-full ${telegramOnline ? 'bg-emerald-500' : 'bg-neutral-400'}`} />
            {telegramOnline ? 'Connected' : telegram?.configured ? 'Offline' : 'Not configured'}
          </span>
        </div>

        <div className="p-4 space-y-3">
          {telegramError && (
            <div className="p-2.5 rounded-lg bg-clay-50 dark:bg-clay-950/40 border border-clay-200 dark:border-clay-800/60 text-clay-700 dark:text-clay-300 text-[11px] flex items-start gap-1.5">
              <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
              <span>{telegramError}</span>
            </div>
          )}
          {telegramSaved && (
            <div className="p-2.5 rounded-lg bg-emerald-50 dark:bg-emerald-950/40 border border-emerald-200 dark:border-emerald-800/60 text-emerald-700 dark:text-emerald-300 text-[11px] flex items-center gap-1.5">
              <Check className="w-3.5 h-3.5" />
              Telegram bot connected.
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-2 gap-3">
            <div className="space-y-1.5">
              <label className="text-[11px] font-semibold text-neutral-700 dark:text-neutral-300">
                Bot Token
              </label>
              <input
                type="password"
                value={telegramToken}
                onChange={(event) => setTelegramToken(event.target.value)}
                placeholder={
                  telegram?.token_masked
                    ? `Saved: ${telegram.token_masked} â€” paste a new token to replace`
                    : '123456789:ABCdef...'
                }
                className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
              />
              <p className="text-[10px] text-neutral-400">
                Create a bot with @BotFather and paste the token here.
              </p>
            </div>
            <div className="space-y-1.5">
              <label className="text-[11px] font-semibold text-neutral-700 dark:text-neutral-300">
                Allowed Users (ID or @username)
              </label>
              <input
                type="text"
                value={telegramAllowed}
                onChange={(event) => setTelegramAllowed(event.target.value)}
                placeholder="@yourname, 123456789"
                className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-blue-500"
              />
              <p className="text-[10px] text-neutral-400">
                Separate with commas. Empty = all messages rejected.
              </p>
            </div>
          </div>

          <div className="flex items-center gap-2">
            <button
              onClick={saveTelegram}
              disabled={telegramBusy}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 text-white text-[11px] font-semibold disabled:opacity-50 cursor-pointer"
            >
              {telegramBusy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <Send className="w-3.5 h-3.5" />}
              Save &amp; Connect
            </button>
            {telegram?.configured && (
              <button
                onClick={disconnectTelegram}
                disabled={telegramBusy}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-300 text-[11px] font-medium disabled:opacity-50 cursor-pointer"
              >
                <LogOut className="w-3.5 h-3.5" />
                Disconnect
              </button>
            )}
          </div>
        </div>
      </div>

      {/* WhatsApp */}
      <div className="rounded-2xl border border-neutral-200 dark:border-neutral-800 bg-white dark:bg-neutral-900 overflow-hidden">
        <div className="flex items-center justify-between px-4 py-3 border-b border-neutral-100 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-950/30">
          <div className="flex items-center gap-2">
            <div className="p-1.5 rounded-lg bg-emerald-50 dark:bg-emerald-950 text-emerald-600 dark:text-emerald-400 border border-emerald-100 dark:border-emerald-900">
              <Smartphone className="w-4 h-4" />
            </div>
            <div>
              <div className="font-semibold text-xs text-neutral-900 dark:text-white">WhatsApp Web</div>
              <div className="text-[11px] text-neutral-500">
                Pair with QR code or a phone pairing code.
              </div>
            </div>
          </div>
          <span
            className={`inline-flex items-center gap-1 text-[10px] font-semibold px-2 py-0.5 rounded-full border ${
              waConnected
                ? 'bg-emerald-50 text-emerald-700 border-emerald-200 dark:bg-emerald-950/40 dark:text-emerald-300 dark:border-emerald-800'
                : waWaiting
                  ? 'bg-amber-50 text-amber-700 border-amber-200 dark:bg-amber-950/40 dark:text-amber-300 dark:border-amber-800'
                  : 'bg-neutral-100 text-neutral-600 border-neutral-200 dark:bg-neutral-800 dark:text-neutral-300 dark:border-neutral-700'
            }`}
          >
            <span
              className={`w-1.5 h-1.5 rounded-full ${
                waConnected ? 'bg-emerald-500' : waWaiting ? 'bg-amber-500 animate-pulse' : 'bg-neutral-400'
              }`}
            />
            {waConnected ? 'Connected' : waWaiting ? 'Waiting for pairing' : 'Not running'}
          </span>
        </div>

        <div className="p-4 space-y-3">
          {(waError || whatsapp?.error) && (
            <div className="p-2.5 rounded-lg bg-clay-50 dark:bg-clay-950/40 border border-clay-200 dark:border-clay-800/60 text-clay-700 dark:text-clay-300 text-[11px] flex items-start gap-1.5">
              <AlertCircle className="w-3.5 h-3.5 shrink-0 mt-0.5" />
              <span>{waError || whatsapp?.error}</span>
            </div>
          )}

          <div className="grid grid-cols-1 md:grid-cols-2 gap-4">
            <div className="space-y-2">
              <div className="space-y-1.5">
                <label className="text-[11px] font-semibold text-neutral-700 dark:text-neutral-300">
                  Pairing Phone (optional)
                </label>
                <input
                  type="text"
                  value={waPhone}
                  onChange={(event) => setWaPhone(event.target.value)}
                  placeholder="628123456789"
                  className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-emerald-500"
                />
                <p className="text-[10px] text-neutral-400">
                  Enter a number to use a pairing code (no QR scan). Digits only, e.g.
                  62812...
                </p>
              </div>
              <div className="space-y-1.5">
                <label className="text-[11px] font-semibold text-neutral-700 dark:text-neutral-300">
                  Allowed Numbers
                </label>
                <input
                  type="text"
                  value={waAllowed}
                  onChange={(event) => setWaAllowed(event.target.value)}
                  placeholder="628123456789, 628987654321"
                  className="w-full px-3 py-2 rounded-xl border border-neutral-300 dark:border-neutral-700 bg-white dark:bg-neutral-800 text-neutral-900 dark:text-neutral-100 font-mono text-xs outline-none focus:border-emerald-500"
                />
              </div>
              <label className="flex items-center gap-2 text-[11px] text-neutral-600 dark:text-neutral-300">
                <input
                  type="checkbox"
                  checked={waAllowAnyone}
                  onChange={(event) => setWaAllowAnyone(event.target.checked)}
                  className="rounded border-neutral-300 text-emerald-600 focus:ring-emerald-500"
                />
                Allow anyone (not recommended)
              </label>
              <p className="text-[10px] text-neutral-400">
                Empty + allow-anyone off = all inbound messages rejected.
              </p>
            </div>

            <div className="rounded-xl border border-neutral-200 dark:border-neutral-800 bg-neutral-50/60 dark:bg-neutral-950/30 p-3 flex flex-col items-center justify-center min-h-[220px]">
              {whatsapp?.qr ? (
                <>
                  <div className="text-[10px] uppercase tracking-wider font-semibold text-neutral-500 mb-2 flex items-center gap-1">
                    <QrCode className="w-3 h-3" /> Scan QR
                  </div>
                  <img
                    src={whatsapp.qr}
                    alt="WhatsApp QR code"
                    className="w-52 h-52 rounded-xl bg-white p-2 border border-neutral-200"
                  />
                  <p className="text-[10px] text-neutral-500 text-center mt-2">
                    WhatsApp â†’ Settings â†’ Linked devices â†’ Link a device.
                  </p>
                </>
              ) : whatsapp?.pairing_code ? (
                <>
                  <div className="text-[10px] uppercase tracking-wider font-semibold text-neutral-500 mb-2">
                    Pairing Code
                  </div>
                  <div className="text-3xl font-mono font-bold tracking-[0.3em] text-neutral-900 dark:text-white">
                    {whatsapp.pairing_code}
                  </div>
                  <p className="text-[10px] text-neutral-500 text-center mt-3 max-w-[220px]">
                    Open WhatsApp â†’ Linked devices â†’ Link with phone number, then enter this
                    code.
                  </p>
                </>
              ) : waConnected ? (
                <>
                  <Check className="w-8 h-8 text-emerald-500 mb-2" />
                  <div className="text-xs font-semibold text-neutral-800 dark:text-neutral-200">
                    WhatsApp connected
                  </div>
                  <div className="text-[10px] text-neutral-500 font-mono mt-1 break-all text-center">
                    {whatsapp?.jid || whatsapp?.phone || ''}
                  </div>
                </>
              ) : (
                <>
                  <QrCode className="w-8 h-8 text-neutral-300 mb-2" />
                  <div className="text-[11px] text-neutral-500 text-center">
                    Click <b>Generate QR / Connect</b> to start the WhatsApp Web bridge. The QR
                    or pairing code will appear here.
                  </div>
                </>
              )}
            </div>
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <button
              onClick={startWhatsApp}
              disabled={waBusy}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-emerald-600 hover:bg-emerald-500 text-white text-[11px] font-semibold disabled:opacity-50 cursor-pointer"
            >
              {waBusy ? <Loader2 className="w-3.5 h-3.5 animate-spin" /> : <QrCode className="w-3.5 h-3.5" />}
              Generate QR / Connect
            </button>
            <button
              onClick={loadWhatsApp}
              disabled={waBusy}
              className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-neutral-200 dark:border-neutral-700 hover:bg-neutral-100 dark:hover:bg-neutral-800 text-neutral-600 dark:text-neutral-300 text-[11px] font-medium disabled:opacity-50 cursor-pointer"
            >
              <RefreshCw className="w-3.5 h-3.5" />
              Refresh
            </button>
            {(waRunning || waConnected) && (
              <button
                onClick={logoutWhatsApp}
                disabled={waBusy}
                className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg border border-clay-200 dark:border-clay-800 hover:bg-clay-50 dark:hover:bg-clay-950/40 text-clay-600 dark:text-clay-300 text-[11px] font-medium disabled:opacity-50 cursor-pointer"
              >
                <LogOut className="w-3.5 h-3.5" />
                Logout
              </button>
            )}
          </div>

          <p className="text-[10px] text-neutral-400 leading-snug">
            WhatsApp Web uses a local bridge (Baileys) on port 4097. If the bridge fails to
            start, run once: <code>cd scripts/whatsapp-bridge &amp;&amp; npm install</code>. Inbound
            messages are handled by the same agent and replies go back to the source chat.
          </p>
        </div>
      </div>
    </div>
  );
};
