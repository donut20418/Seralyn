import { useEffect, useRef, useState } from "react";
import { Layers, MoreHorizontal, PanelLeft, PanelRight } from "lucide-react";
import type { ProviderId, ProviderInfo } from "../types";

interface Props {
  title: string;
  onRename: (title: string) => void;
  sidebarOpen: boolean;
  onToggleSidebar: () => void;
  browserOpen: boolean;
  onToggleBrowser: () => void;
  inspectorOpen: boolean;
  onToggleInspector: () => void;
  /** Present while a provider is answering. */
  running: { provider: ProviderId; label: string } | null;
  providers: Record<ProviderId, ProviderInfo>;
}

export function ChatHeader({
  title,
  onRename,
  sidebarOpen,
  onToggleSidebar,
  browserOpen,
  onToggleBrowser,
  inspectorOpen,
  onToggleInspector,
  running,
  providers,
}: Props) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);
  const ref = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (editing) {
      setDraft(title);
      window.setTimeout(() => ref.current?.select(), 0);
    }
  }, [editing, title]);

  const commit = () => {
    const next = draft.trim();
    if (next) onRename(next);
    setEditing(false);
  };

  return (
    <div className="sr-header">
      <button
        className="sr-icon-btn"
        aria-label={sidebarOpen ? "Collapse sidebar" : "Expand sidebar"}
        aria-pressed={sidebarOpen}
        onClick={onToggleSidebar}
      >
        <PanelLeft size={15} />
      </button>

      {editing ? (
        <input
          ref={ref}
          className="sr-header__title-input"
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onBlur={commit}
          onKeyDown={(e) => {
            if (e.key === "Enter") commit();
            if (e.key === "Escape") setEditing(false);
          }}
          aria-label="Conversation title"
        />
      ) : (
        <h1 className="sr-header__title" onDoubleClick={() => setEditing(true)} title={title}>
          {title}
        </h1>
      )}

      {running && (
        <span
          className="sr-running"
          style={{
            background: "rgba(255,255,255,0.05)",
            border: `1px solid ${providers[running.provider].color}33`,
            color: providers[running.provider].color,
          }}
        >
          <span className="sr-pulse" aria-hidden="true">
            <span />
            <span />
            <span />
          </span>
          {running.label}
        </span>
      )}

      <span className="sr-spacer" />

      <button
        className={`sr-chip${inspectorOpen ? " sr-chip--on" : ""}`}
        style={{ height: 26 }}
        onClick={onToggleInspector}
        aria-pressed={inspectorOpen}
      >
        <Layers size={13} />
        Inspector
      </button>
      <button
        className={`sr-icon-btn${browserOpen ? " sr-icon-btn--on" : " sr-icon-btn--bordered"}`}
        aria-label="Toggle browser panel"
        aria-pressed={browserOpen}
        onClick={onToggleBrowser}
      >
        <PanelRight size={14} />
      </button>
      <button className="sr-icon-btn" aria-label="More actions">
        <MoreHorizontal size={15} />
      </button>
    </div>
  );
}
