export type BootFailureKind =
  | 'sidecar-missing'
  | 'spawn-failed'
  | 'timeout'
  | 'port-taken'
  | 'unknown';

export interface BootFailure {
  kind: BootFailureKind;
  title: string;
  detail: string;
  steps: string[];
  raw: string;
}

/**
 * Turns the shell's raw boot error into a cause the operator can act on.
 * The shell reports the failure verbatim (paths, OS errors); guessing a
 * generic message would hide the one line that says what to fix.
 */
export function classifyBootFailure(raw: string | null | undefined): BootFailure {
  const message = (raw ?? '').trim();
  const lower = message.toLowerCase();

  if (!message) {
    return {
      kind: 'unknown',
      title: 'Daemon did not answer',
      detail: 'The desktop shell did not report a reason, and nothing is listening on 127.0.0.1:4096.',
      steps: [
        'Start it manually: odond serve --bind 127.0.0.1:4096',
        'Open the log at %LOCALAPPDATA%\\odon\\odond.log for the failure line.',
      ],
      raw: message,
    };
  }

  if (lower.includes('sidecar not found')) {
    return {
      kind: 'sidecar-missing',
      title: 'The bundled daemon is missing',
      detail: 'odond.exe is not next to the app executable, so the shell has nothing to start.',
      steps: [
        'Reinstall ODON with the installer; the bundle ships odond.exe beside the app.',
        'If you run from source, build it: cargo build -p odond --release.',
      ],
      raw: message,
    };
  }

  if (lower.includes('failed to spawn')) {
    return {
      kind: 'spawn-failed',
      title: 'The daemon could not be started',
      detail: 'The executable exists but the operating system refused to run it.',
      steps: [
        'Check that antivirus or Windows Defender did not quarantine odond.exe.',
        'Run odond.exe serve once from a terminal to see the immediate error.',
      ],
      raw: message,
    };
  }

  if (lower.includes('did not answer within')) {
    return {
      kind: 'timeout',
      title: 'The daemon started but never answered',
      detail: 'The process launched, yet nothing began listening on 127.0.0.1:4096 in time.',
      steps: [
        'Read the last lines of %LOCALAPPDATA%\\odon\\odond.log; a bind failure is logged there.',
        'Free the port if another program holds 4096, then press Restart daemon.',
      ],
      raw: message,
    };
  }

  if (lower.includes('still answering') || lower.includes('not started by this app')) {
    return {
      kind: 'port-taken',
      title: 'Another daemon owns the port',
      detail: 'A daemon this window did not start is holding 127.0.0.1:4096, so the shell will not replace it.',
      steps: [
        'Use that daemon, or stop it yourself and start a fresh one.',
        'The shell never kills a process it did not start.',
      ],
      raw: message,
    };
  }

  return {
    kind: 'unknown',
    title: 'The daemon failed to start',
    detail: message,
    steps: ['Open %LOCALAPPDATA%\\odon\\odond.log for the full failure line.'],
    raw: message,
  };
}
