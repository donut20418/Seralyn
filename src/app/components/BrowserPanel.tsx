import { ArrowLeft, ArrowRight, ExternalLink, Globe, Lock, Plus, RotateCw, X } from "lucide-react";

interface Props {
  url: string;
  onClose: () => void;
}

/**
 * Optional side panel: web pages, docs, localhost previews, URLs opened from a
 * response. It is never required for normal chat, and it is unrelated to a
 * provider's own web/search tooling.
 *
 * In the Tauri build the view below is replaced by a webview pointed at `url`.
 */
export function BrowserPanel({ url, onClose }: Props) {
  return (
    <div className="sr-browser" style={{ width: "100%" }}>
      <div className="sr-browser__tabs">
        <span className="sr-browser__tab sr-browser__tab--on">
          <span className="sr-dot" style={{ width: 5, height: 5, background: "var(--sr-ok)" }} />
          Rebuild log
        </span>
        <span className="sr-browser__tab">docs.rs / notify</span>
        <span className="sr-spacer" />
        <button className="sr-icon-btn" style={{ width: 22, height: 22 }} aria-label="New tab">
          <Plus size={13} />
        </button>
      </div>

      <div className="sr-browser__bar">
        <button className="sr-icon-btn" style={{ width: 24, height: 24 }} aria-label="Back">
          <ArrowLeft size={14} />
        </button>
        <button className="sr-icon-btn" style={{ width: 24, height: 24 }} aria-label="Forward" disabled>
          <ArrowRight size={14} />
        </button>
        <button className="sr-icon-btn" style={{ width: 24, height: 24 }} aria-label="Reload">
          <RotateCw size={14} />
        </button>
        <div className="sr-browser__url">
          <Lock size={11} color="var(--sr-ok)" />
          <span>{url}</span>
        </div>
        <button className="sr-icon-btn" style={{ width: 24, height: 24 }} aria-label="Open in system browser">
          <ExternalLink size={13} />
        </button>
        <button className="sr-icon-btn" style={{ width: 24, height: 24 }} aria-label="Close browser panel" onClick={onClose}>
          <X size={14} />
        </button>
      </div>

      <div className="sr-browser__view">
        <div className="sr-browser__frame sr-scroll">
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 4 }}>
            <span style={{ fontSize: 16, fontWeight: 600, letterSpacing: "-0.01em" }}>watchd</span>
            <span
              style={{
                display: "flex",
                alignItems: "center",
                height: 19,
                padding: "0 7px",
                borderRadius: 4,
                background: "rgba(92,201,141,0.12)",
                fontSize: 10.5,
                color: "var(--sr-ok)",
              }}
            >
              live
            </span>
          </div>
          <p style={{ margin: "0 0 16px", fontSize: 12, lineHeight: 1.7, color: "var(--sr-text-2)" }}>
            Rebuild log streaming from the dev server on port 5173.
          </p>

          <div className="sr-stats">
            <div className="sr-stat">
              <div className="sr-stat__k">Rebuilds</div>
              <div className="sr-stat__v">14</div>
            </div>
            <div className="sr-stat">
              <div className="sr-stat__k">Cancelled</div>
              <div className="sr-stat__v" style={{ color: "var(--sr-warn)" }}>
                3
              </div>
            </div>
            <div className="sr-stat">
              <div className="sr-stat__k">Median</div>
              <div className="sr-stat__v">0.94s</div>
            </div>
          </div>

          <div className="sr-log">
            <div>
              <span className="sr-log__t">14:22:04</span> <span style={{ color: "var(--sr-ok)" }}>ready</span> dev server in 214 ms
            </div>
            <div>
              <span className="sr-log__t">14:22:41</span> <span style={{ color: "var(--sr-text-3)" }}>event</span> Modify(Data) src/watcher.rs
            </div>
            <div>
              <span className="sr-log__t">14:22:41</span> <span style={{ color: "var(--sr-text-3)" }}>event</span> Modify(Data) src/watcher.rs
            </div>
            <div>
              <span className="sr-log__t">14:22:41</span> <span style={{ color: "var(--sr-warn)" }}>batch</span> 2 events → 1 rebuild
            </div>
            <div>
              <span className="sr-log__t">14:22:41</span> <span style={{ color: "var(--sr-warn)" }}>cancel</span> previous build at 14 ms
            </div>
            <div>
              <span className="sr-log__t">14:22:42</span> <span style={{ color: "var(--sr-ok)" }}>built</span> watchd in 0.94 s
            </div>
          </div>
        </div>
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
