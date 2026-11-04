import React from 'react';
import { CatLogo } from './CatLogo';
import { BootFailureNotice } from './BootFailureNotice';
import type { BootFailure } from '../lib/bootFailure';

interface BootSplashProps {
  /** Fade the overlay out; the caller unmounts it once the transition ends. */
  done: boolean;
  /** Changes the caption when the daemon never answers. */
  offline: boolean;
  /** Optional detail line, e.g. the daemon port or a failure hint. */
  detail?: string;
  /** Cause reported by the shell once boot is considered failed. */
  failure?: BootFailure | null;
  retrying?: boolean;
  retryError?: string | null;
  onRetry?: () => void;
}

/**
 * Boot overlay shown until the desktop UI can reach the local daemon.
 *
 * The surface is paper (sand-50) and the mark is ink, so the first frame the
 * operator sees is the same palette as the app behind it - no dark flash, no
 * gradient glow. Motion is limited to three cheap composited properties
 * (opacity, transform) and every animation is switched off under
 * `prefers-reduced-motion`.
 */
export function BootSplash({
  done,
  offline,
  detail,
  failure,
  retrying = false,
  retryError = null,
  onRetry,
}: BootSplashProps) {
  return (
    <div
      className={`odon-splash fixed inset-0 z-[200] flex flex-col items-center justify-center bg-sand-50 ${
        done ? 'is-done' : ''
      }`}
      role="status"
      aria-live="polite"
      aria-label={offline ? 'ODON daemon unavailable' : 'Connecting to ODON daemon'}
    >
      {/* Warm paper vignette: two very low-contrast radial washes, no gradient
          banding because both stops sit inside the sand ramp. */}
      <div className="odon-splash-wash pointer-events-none absolute inset-0" aria-hidden="true" />

      <div className="relative flex flex-col items-center">
        {/* Mark: the cat sits still inside a ring that turns, and a halo that
            breathes at half the ring's rate so the two never pulse in sync. */}
        <div className="odon-splash-mark relative flex items-center justify-center">
          <span className="odon-splash-ring absolute inset-0 rounded-full" aria-hidden="true" />
          <span className="odon-splash-halo absolute inset-0 rounded-full" aria-hidden="true" />
          <CatLogo className="relative w-[4.5rem] h-[4.5rem] text-neutral-900" strokeWidth={1.8} />
        </div>

        <h1 className="odon-splash-wordmark mt-7 select-none">ODON</h1>

        <p className="odon-splash-caption mt-3 text-[12px] tracking-[0.08em] uppercase">
          {offline ? 'Daemon not reachable' : 'Connecting to daemon'}
          <span className="odon-splash-dots" aria-hidden="true">
            <span>.</span>
            <span>.</span>
            <span>.</span>
          </span>
        </p>

        {/* Indeterminate hairline. A determinate bar would be a lie: the daemon
            does not report boot progress, only reachable / not reachable. */}
        <div className="odon-splash-track mt-5" aria-hidden="true">
          <span className={`odon-splash-bar ${offline ? 'is-offline' : ''}`} />
        </div>

        {detail && <p className="odon-splash-detail mt-4 max-w-[24rem] text-center">{detail}</p>}

        {failure && onRetry && (
          <BootFailureNotice
            failure={failure}
            retrying={retrying}
            onRetry={onRetry}
            retryError={retryError}
          />
        )}
      </div>
    </div>
  );
}
