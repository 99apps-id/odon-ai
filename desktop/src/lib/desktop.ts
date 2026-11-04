/**
 * Bridge to the Tauri desktop shell.
 *
 * The same bundle also runs in a plain browser against a manually
 * started daemon (`pnpm dev` + `odond serve`). There `__TAURI__` does
 * not exist, so every caller checks `isDesktopShell()` and falls back
 * to the HTTP API instead of assuming a native capability.
 */
type InvokeFn = (command: string, args?: Record<string, unknown>) => Promise<unknown>;

type TauriGlobal = { core?: { invoke?: InvokeFn } };

function shell(): TauriGlobal | undefined {
  return (window as unknown as { __TAURI__?: TauriGlobal }).__TAURI__;
}

export function isDesktopShell(): boolean {
  return typeof shell()?.core?.invoke === "function";
}

async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  const call = shell()?.core?.invoke;
  if (!call) {
    throw new Error("desktop shell is not available");
  }
  return (await call(command, args)) as T;
}

/**
 * Native OS folder chooser. Resolves to null when the dialog is
 * dismissed, and rejects when the shell has no picker.
 */
export async function pickFolder(): Promise<string | null> {
  return invoke<string | null>("pick_folder");
}

/**
 * Restarts the daemon the shell owns. Resolves with the shell's report
 * on success and rejects with its explanation when the daemon could not
 * be brought back (for example: a daemon started outside the app).
 */
export async function restartDaemon(): Promise<string> {
  return invoke<string>("restart_daemon");
}

/**
 * The loopback token the daemon generated for this session, read by the
 * shell from ~/.odon/api-token. Returns null when the shell is not
 * available or the daemon has not written the file yet.
 */
export async function daemonToken(): Promise<string | null> {
  try {
    const token = await invoke<string | null>("daemon_token");
    return typeof token === "string" && token.trim() ? token.trim() : null;
  } catch {
    return null;
  }
}

/**
 * Why the bundled daemon did not come up, as reported by the shell.
 * Returns null in a browser, or when the daemon started normally.
 */
export async function daemonBootFailure(): Promise<string | null> {
  try {
    const reason = await invoke<string | null>("daemon_boot_failure");
    return typeof reason === "string" && reason.trim() ? reason.trim() : null;
  } catch {
    return null;
  }
}

/**
 * Opens an http(s) URL in the operating system browser.
 *
 * The desktop webview cannot navigate to an OAuth page by itself, so the shell
 * hands the URL to the default browser where the loopback callback still
 * reaches the daemon. In a browser tab the normal `window.open` path is used.
 * Returns true when a browser was asked to open the page.
 */
export async function openExternal(url: string): Promise<boolean> {
  const target = url.trim();
  if (!target) return false;
  if (isDesktopShell()) {
    try {
      await invoke<null>("open_external", { url: target });
      return true;
    } catch {
      /* fall through to the browser path */
    }
  }
  try {
    const win = window.open(target, "_blank", "noopener,noreferrer");
    return win !== null;
  } catch {
    return false;
  }
}
