import React from 'react';
import ReactDOM from 'react-dom/client';
import App from './App.tsx';
import { ErrorBoundary } from './components/ErrorBoundary.tsx';
import { installCrashRecorder } from './lib/crashReport.ts';
import './lib/webglPolyfill.ts';
import './index.css';

// Faults thrown outside the render path (async rejections, stray errors) never
// reach ErrorBoundary. Recording them here means the window that launched from
// the Start menu still keeps a report on disk.
installCrashRecorder();

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <ErrorBoundary label="odon">
      <App />
    </ErrorBoundary>
  </React.StrictMode>,
);
