import { useState, useEffect, type KeyboardEvent } from "react";
import { ArrowLeft, ArrowRight, ExternalLink, Globe, Lock, Plus, RotateCw, X } from "lucide-react";

interface Props {
  url?: string;
  onClose: () => void;
}

function normalizeUrl(input: string): string {
  const trimmed = input.trim();
  if (!trimmed) return "http://localhost:5173";
  if (/^https?:\/\//i.test(trimmed)) return trimmed;
  return `http://${trimmed}`;
}

export function BrowserPanel({ url = "http://localhost:5173", onClose }: Props) {
  const [history, setHistory] = useState<string[]>([normalizeUrl(url)]);
  const [historyIndex, setHistoryIndex] = useState(0);
  const [inputUrl, setInputUrl] = useState(normalizeUrl(url));
  const [reloadKey, setReloadKey] = useState(0);

  const currentUrl = history[historyIndex] || normalizeUrl(url);

  useEffect(() => {
    const normalized = normalizeUrl(url);
    setHistory([normalized]);
    setHistoryIndex(0);
    setInputUrl(normalized);
  }, [url]);

  const navigateTo = (newUrl: string) => {
    const normalized = normalizeUrl(newUrl);
    const newHistory = history.slice(0, historyIndex + 1);
    newHistory.push(normalized);
    setHistory(newHistory);
    setHistoryIndex(newHistory.length - 1);
    setInputUrl(normalized);
  };

  const handleBack = () => {
    if (historyIndex > 0) {
      const nextIdx = historyIndex - 1;
      setHistoryIndex(nextIdx);
      setInputUrl(history[nextIdx]);
    }
  };

  const handleForward = () => {
    if (historyIndex < history.length - 1) {
      const nextIdx = historyIndex + 1;
      setHistoryIndex(nextIdx);
      setInputUrl(history[nextIdx]);
    }
  };

  const handleReload = () => {
    setReloadKey((k) => k + 1);
  };

  const handleKeyDown = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.key === "Enter") {
      navigateTo(inputUrl);
    }
  };

  const handleOpenExternal = () => {
    if (currentUrl) {
      window.open(currentUrl, "_blank", "noopener,noreferrer");
    }
  };

  return (
    <div className="sr-browser" style={{ width: "100%", height: "100%" }}>
      <div className="sr-browser__tabs">
        <span className="sr-browser__tab sr-browser__tab--on">
          <span className="sr-dot" style={{ width: 5, height: 5, background: "var(--sr-ok)" }} />
          Web Preview
        </span>
        <span className="sr-spacer" />
        <button
          className="sr-icon-btn"
          style={{ width: 22, height: 22 }}
          aria-label="New preview tab"
          onClick={() => navigateTo("http://localhost:5173")}
        >
          <Plus size={13} />
        </button>
      </div>

      <div className="sr-browser__bar">
        <button
          className="sr-icon-btn"
          style={{ width: 24, height: 24 }}
          aria-label="Back"
          disabled={historyIndex === 0}
          onClick={handleBack}
        >
          <ArrowLeft size={14} />
        </button>
        <button
          className="sr-icon-btn"
          style={{ width: 24, height: 24 }}
          aria-label="Forward"
          disabled={historyIndex >= history.length - 1}
          onClick={handleForward}
        >
          <ArrowRight size={14} />
        </button>
        <button
          className="sr-icon-btn"
          style={{ width: 24, height: 24 }}
          aria-label="Reload"
          onClick={handleReload}
        >
          <RotateCw size={14} />
        </button>
        <div className="sr-browser__url">
          <Lock size={11} color="var(--sr-ok)" />
          <input
            type="text"
            value={inputUrl}
            onChange={(e) => setInputUrl(e.target.value)}
            onKeyDown={handleKeyDown}
            style={{
              flex: 1,
              background: "transparent",
              border: "none",
              outline: "none",
              color: "inherit",
              fontFamily: "inherit",
              fontSize: "inherit",
            }}
            placeholder="http://localhost:5173"
            aria-label="Browser URL"
          />
        </div>
        <button
          className="sr-icon-btn"
          style={{ width: 24, height: 24 }}
          aria-label="Open in system browser"
          onClick={handleOpenExternal}
        >
          <ExternalLink size={13} />
        </button>
        <button
          className="sr-icon-btn"
          style={{ width: 24, height: 24 }}
          aria-label="Close browser panel"
          onClick={onClose}
        >
          <X size={14} />
        </button>
      </div>

      <div className="sr-browser__view" style={{ padding: 8, display: "flex", flex: 1, minHeight: 0 }}>
        <iframe
          key={`${currentUrl}-${reloadKey}`}
          src={currentUrl}
          title="Web Preview"
          sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
          style={{
            width: "100%",
            height: "100%",
            border: "1px solid var(--sr-border)",
            borderRadius: "var(--sr-radius-md)",
            background: "#ffffff",
          }}
        />
      </div>
    </div>
  );
}

export function BrowserEmpty() {
  return (
    <div className="sr-browser__empty">
      <Globe size={20} />
      <span>Nothing open. Paste a URL, or open a link from a response here.</span>
    </div>
  );
}
