import React, { useEffect, useState } from 'react';
import { Download, History, Loader2, RotateCcw, X } from 'lucide-react';
import { api } from '../api';
import { Artifact } from '../types';

interface ArtifactVersionsModalProps {
  artifact: Artifact;
  onClose: () => void;
  onArtifactChanged: (artifact: Artifact) => void;
}

type ArtifactVersionSummary = {
  artifact_id: string;
  version: number;
  title: string;
  created_at: string;
};

export const ArtifactVersionsModal: React.FC<ArtifactVersionsModalProps> = ({
  artifact,
  onClose,
  onArtifactChanged,
}) => {
  const [versions, setVersions] = useState<ArtifactVersionSummary[]>([]);
  const [isLoading, setIsLoading] = useState(true);
  const [restoringVersion, setRestoringVersion] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);

  const loadVersions = React.useCallback(() => {
    setIsLoading(true);
    setError(null);
    api
      .listArtifactVersions(artifact.id)
      .then((items) => setVersions(Array.isArray(items) ? items : []))
      .catch((err: any) => setError(err.message || 'Failed to load versions.'))
      .finally(() => setIsLoading(false));
  }, [artifact.id]);

  useEffect(() => {
    loadVersions();
  }, [loadVersions]);

  const handleRestore = async (version: number) => {
    setRestoringVersion(version);
    setError(null);
    try {
      const updated = await api.restoreArtifactVersion(artifact.id, version);
      onArtifactChanged(updated);
      loadVersions();
    } catch (err: any) {
      setError(err.message || 'Failed to restore version.');
    } finally {
      setRestoringVersion(null);
    }
  };

  const handleExport = (format: 'json' | 'md' | 'html') => {
    const anchor = document.createElement('a');
    anchor.href = api.artifactExportUrl(artifact.id, format);
    anchor.rel = 'noopener';
    document.body.appendChild(anchor);
    anchor.click();
    document.body.removeChild(anchor);
  };

  return (
    <div className="fixed inset-0 z-[60] flex items-center justify-center p-4 bg-neutral-200/70 backdrop-blur-xs">
      <div className="w-full max-w-xl max-h-[85vh] flex flex-col bg-white rounded-3xl shadow-2xl border border-neutral-200">
        <div className="flex items-center justify-between px-5 py-4 border-b border-neutral-100">
          <div className="min-w-0">
            <div className="flex items-center gap-2">
              <History className="w-4 h-4 text-blue-600 shrink-0" />
              <h3 className="text-sm font-bold text-neutral-900 truncate">Artifact History</h3>
            </div>
            <p className="text-[11px] text-neutral-500 truncate mt-0.5">{artifact.title}</p>
          </div>
          <button
            onClick={onClose}
            className="p-1.5 text-neutral-400 hover:text-neutral-700 rounded-lg hover:bg-neutral-100"
          >
            <X className="w-4 h-4" />
          </button>
        </div>

        <div className="px-5 py-3 border-b border-neutral-100 flex flex-wrap items-center gap-2">
          <span className="text-[11px] font-semibold text-neutral-500 uppercase tracking-wider mr-auto">
            Export
          </span>
          {(['json', 'md', 'html'] as const).map((format) => (
            <button
              key={format}
              onClick={() => handleExport(format)}
              className="inline-flex items-center gap-1 px-2.5 py-1.5 rounded-lg border border-neutral-200 text-[11px] font-medium text-neutral-700 hover:bg-neutral-50"
            >
              <Download className="w-3 h-3" />
              {format.toUpperCase()}
            </button>
          ))}
        </div>

        <div className="flex-1 overflow-y-auto p-5 space-y-2">
          {error && (
            <div className="p-2.5 rounded-xl bg-clay-50 border border-clay-200 text-clay-700 text-xs">
              {error}
            </div>
          )}
          {isLoading ? (
            <div className="flex items-center justify-center py-8 text-neutral-500">
              <Loader2 className="w-4 h-4 animate-spin mr-2" />
              Loading versions...
            </div>
          ) : versions.length === 0 ? (
            <div className="py-8 text-center text-xs text-neutral-400">
              No versions recorded yet.
            </div>
          ) : (
            versions.map((version) => (
              <div
                key={version.version}
                className="flex items-center justify-between p-3 rounded-2xl border border-neutral-200 bg-neutral-50/60"
              >
                <div>
                  <div className="text-xs font-semibold text-neutral-900">
                    Version {version.version}
                  </div>
                  <div className="text-[10px] text-neutral-500 mt-0.5">
                    {new Date(version.created_at).toLocaleString()}
                  </div>
                </div>
                <button
                  onClick={() => handleRestore(version.version)}
                  disabled={restoringVersion !== null}
                  className="inline-flex items-center gap-1.5 px-3 py-1.5 rounded-lg bg-blue-600 hover:bg-blue-500 disabled:opacity-50 text-white text-[11px] font-semibold"
                >
                  {restoringVersion === version.version ? (
                    <Loader2 className="w-3 h-3 animate-spin" />
                  ) : (
                    <RotateCcw className="w-3 h-3" />
                  )}
                  Restore
                </button>
              </div>
            ))
          )}
        </div>
      </div>
    </div>
  );
};
