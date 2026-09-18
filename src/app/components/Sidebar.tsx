import { useEffect, useRef, useState, type DragEvent } from "react";
import {
  Archive,
  ChevronDown,
  ChevronRight,
  FolderInput,
  MoreHorizontal,
  Pin,
  PinOff,
  Plus,
  Search,
  Settings2,
  Pencil,
  Trash2,
  X,
} from "lucide-react";
import { PROVIDERS } from "../data/providers";
import type { Conversation, ConversationGroup } from "../types";

interface MenuState {
  convId: string;
  x: number;
  y: number;
}

interface Props {
  conversations: Conversation[];
  groups: ConversationGroup[];
  activeId: string;
  onSelect: (id: string) => void;
  onNewChat: () => void;
  onRename: (id: string, title: string) => void;
  onTogglePin: (id: string) => void;
  onMoveToGroup: (id: string, groupId: string | undefined) => void;
  onArchive: (id: string) => void;
  onDelete: (id: string) => void;
  onToggleGroup: (id: string) => void;
  onCreateGroup: () => void;
  onOpenConnections: () => void;
  userName?: string;
}

export function Sidebar({
  conversations,
  groups,
  activeId,
  onSelect,
  onNewChat,
  onRename,
  onTogglePin,
  onMoveToGroup,
  onArchive,
  onDelete,
  onToggleGroup,
  onCreateGroup,
  onOpenConnections,
  userName = "Workspace",
}: Props) {
  const [query, setQuery] = useState("");
  const [menu, setMenu] = useState<MenuState | null>(null);
  const [renaming, setRenaming] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const [dragId, setDragId] = useState<string | null>(null);
  const [dropGroup, setDropGroup] = useState<string | null>(null);
  const renameRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (renaming) renameRef.current?.select();
  }, [renaming]);

  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(null);
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", close);
    return () => {
      document.removeEventListener("mousedown", close);
      document.removeEventListener("keydown", close);
    };
  }, [menu]);

  const matches = (c: Conversation) =>
    !query || c.title.toLowerCase().includes(query.toLowerCase());

  const visible = conversations.filter(matches);
  const pinned = visible.filter((c) => c.pinned);
  const recents = visible.filter((c) => !c.pinned && !c.groupId);

  const commitRename = (id: string) => {
    const next = draft.trim();
    if (next) onRename(id, next);
    setRenaming(null);
  };

  const startRename = (c: Conversation) => {
    setDraft(c.title);
    setRenaming(c.id);
    setMenu(null);
  };

  const row = (c: Conversation) => {
    const active = c.id === activeId;
    return (
      <div
        key={c.id}
        className={[
          "sr-conv",
          active ? "sr-conv--active" : "",
          dragId === c.id ? "sr-conv--dragging" : "",
        ]
          .filter(Boolean)
          .join(" ")}
        draggable={renaming !== c.id}
        onDragStart={() => setDragId(c.id)}
        onDragEnd={() => {
          setDragId(null);
          setDropGroup(null);
        }}
      >
        <span className="sr-dot" style={{ width: 5, height: 5, background: PROVIDERS[c.lastProvider]?.color ?? "var(--sr-accent)" }} />

        {renaming === c.id ? (
          <input
            ref={renameRef}
            className="sr-conv__rename"
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => commitRename(c.id)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commitRename(c.id);
              if (e.key === "Escape") setRenaming(null);
            }}
            aria-label="Conversation title"
          />
        ) : (
          <>
            <button
              className="sr-conv__title"
              onClick={() => onSelect(c.id)}
              onDoubleClick={() => startRename(c)}
              title={c.title}
            >
              {c.title}
            </button>
            <span className="sr-conv__ago">{c.ago}</span>
            <button
              className="sr-conv__more"
              aria-label={`Actions for ${c.title}`}
              onClick={(e) => {
                e.stopPropagation();
                const r = (e.target as HTMLElement).getBoundingClientRect();
                setMenu({ convId: c.id, x: r.right + 6, y: r.top });
              }}
            >
              <MoreHorizontal size={12} />
            </button>
          </>
        )}
      </div>
    );
  };

  const menuConv = menu ? conversations.find((c) => c.id === menu.convId) : undefined;

  return (
    <>
      <div className="sr-sidebar__top">
        <button className="sr-newchat" onClick={onNewChat}>
          <Plus size={14} />
          New chat
          <span className="sr-spacer" />
          <span className="sr-kbd">⌘N</span>
        </button>

        <div className="sr-search">
          <Search size={13} color="var(--sr-text-3)" />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search conversations"
            aria-label="Search conversations"
          />
          {query ? (
            <button className="sr-icon-btn" style={{ width: 18, height: 18 }} aria-label="Clear search" onClick={() => setQuery("")}>
              <X size={11} />
            </button>
          ) : (
            <span className="sr-kbd">⌘K</span>
          )}
        </div>
      </div>

      <div className="sr-sidebar__list sr-scroll">
        {pinned.length > 0 && (
          <>
            <div className="sr-section">
              <Pin size={11} color="var(--sr-text-3)" />
              <span className="sr-label">Pinned</span>
            </div>
            {pinned.map((c) => row(c))}
          </>
        )}

        <div className="sr-section">
          <span className="sr-label">Groups</span>
          <span className="sr-spacer" />
          <button className="sr-icon-btn" style={{ width: 18, height: 18 }} aria-label="Create group" onClick={onCreateGroup}>
            <Plus size={12} />
          </button>
        </div>

        {groups.map((g) => {
          const members = visible.filter((c) => c.groupId === g.id && !c.pinned);
          return (
            <div key={g.id}>
              <button
                className={`sr-group-head${dropGroup === g.id ? " sr-group-head--drop" : ""}`}
                onClick={() => onToggleGroup(g.id)}
                aria-expanded={!g.collapsed}
                onDragOver={(e: DragEvent) => {
                  e.preventDefault();
                  setDropGroup(g.id);
                }}
                onDragLeave={() => setDropGroup((cur) => (cur === g.id ? null : cur))}
                onDrop={(e: DragEvent) => {
                  e.preventDefault();
                  if (dragId) onMoveToGroup(dragId, g.id);
                  setDragId(null);
                  setDropGroup(null);
                }}
              >
                {g.collapsed ? <ChevronRight size={11} color="var(--sr-text-2)" /> : <ChevronDown size={11} color="var(--sr-text-2)" />}
                <span className="sr-group-head__label">{g.label}</span>
                <span className="sr-group-head__count">{members.length}</span>
                {dropGroup === g.id && (
                  <>
                    <span className="sr-spacer" />
                    <span style={{ fontSize: 10, color: "var(--sr-accent)" }}>drop here</span>
                  </>
                )}
              </button>
              {!g.collapsed && members.length > 0 && (
                <div className="sr-group-body">{members.map((c) => row(c))}</div>
              )}
            </div>
          );
        })}

        <div className="sr-section">
          <span className="sr-label">Recents</span>
        </div>
        {recents.length === 0 ? (
          <p className="sr-empty">{query ? "No conversations match." : "Nothing here yet."}</p>
        ) : (
          recents.map((c) => row(c))
        )}
      </div>

      <div className="sr-sidebar__foot">
        <span className="sr-avatar">{userName.charAt(0).toUpperCase()}</span>
        <span style={{ fontSize: 12, color: "var(--sr-text-2)", flexGrow: 1 }}>{userName}</span>
        <button className="sr-icon-btn" style={{ width: 22, height: 22 }} aria-label="Providers and connections" onClick={onOpenConnections}>
          <Settings2 size={13} />
        </button>
      </div>

      {menu && menuConv && (
        <div
          className="sr-pop"
          style={{ position: "fixed", left: menu.x, top: menu.y, bottom: "auto", width: 196, padding: 5 }}
          role="menu"
          onMouseDown={(e) => e.stopPropagation()}
        >
          <button className="sr-menu-item" role="menuitem" onClick={() => startRename(menuConv)}>
            <Pencil size={13} />
            Rename
          </button>
          <button
            className="sr-menu-item"
            role="menuitem"
            onClick={() => {
              onTogglePin(menuConv.id);
              setMenu(null);
            }}
          >
            {menuConv.pinned ? <PinOff size={13} /> : <Pin size={13} />}
            {menuConv.pinned ? "Unpin" : "Pin"}
          </button>
          {groups.map((g) => (
            <button
              key={g.id}
              className="sr-menu-item"
              role="menuitem"
              onClick={() => {
                onMoveToGroup(menuConv.id, menuConv.groupId === g.id ? undefined : g.id);
                setMenu(null);
              }}
            >
              <FolderInput size={13} />
              {menuConv.groupId === g.id ? `Remove from ${g.label}` : `Move to ${g.label}`}
            </button>
          ))}
          <div className="sr-menu-sep" />
          <button
            className="sr-menu-item"
            role="menuitem"
            onClick={() => {
              onArchive(menuConv.id);
              setMenu(null);
            }}
          >
            <Archive size={13} />
            Archive
          </button>
          <button
            className="sr-menu-item sr-menu-item--danger"
            role="menuitem"
            onClick={() => {
              onDelete(menuConv.id);
              setMenu(null);
            }}
          >
            <Trash2 size={13} />
            Delete
          </button>
        </div>
      )}
    </>
  );
}
