import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type MouseEvent as ReactMouseEvent,
  type ReactNode,
} from "react";
import { createPortal } from "react-dom";
import {
  Clipboard,
  Command,
  Copy,
  Flame,
  Globe,
  Scissors,
  Settings,
  Sparkles,
  TextSelect,
} from "lucide-react";
import { useAppStore } from "../store/appStore";

export interface ContextMenuItem {
  id?: string;
  label: string;
  icon?: ReactNode;
  shortcut?: string;
  disabled?: boolean;
  danger?: boolean;
  divider?: boolean;
  onSelect: () => void;
}

interface ContextMenuState {
  x: number;
  y: number;
  items: ContextMenuItem[];
}

interface ContextMenuContextValue {
  openContextMenu: (
    e: ReactMouseEvent | MouseEvent,
    items: ContextMenuItem[]
  ) => void;
  closeContextMenu: () => void;
}

const ContextMenuContext = createContext<ContextMenuContextValue | null>(null);

export function useContextMenu(): ContextMenuContextValue {
  const ctx = useContext(ContextMenuContext);
  if (!ctx) {
    throw new Error("useContextMenu must be used within ContextMenuProvider");
  }
  return ctx;
}

function isEditableTarget(target: EventTarget | null): HTMLElement | null {
  if (!(target instanceof HTMLElement)) return null;
  const el = target.closest<HTMLElement>(
    'input, textarea, [contenteditable="true"]'
  );
  if (!el) return null;
  if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
    if (el.disabled) return null;
  }
  return el;
}

function buildEditableItems(target: HTMLElement): ContextMenuItem[] {
  const isInputOrTextarea =
    target instanceof HTMLInputElement || target instanceof HTMLTextAreaElement;
  const isReadOnly =
    isInputOrTextarea && (target.readOnly || target.disabled);

  const hasSelection = isInputOrTextarea
    ? (target.selectionStart ?? 0) !== (target.selectionEnd ?? 0)
    : Boolean(window.getSelection()?.toString());

  const getSelectedText = (): string => {
    if (isInputOrTextarea) {
      const start = target.selectionStart ?? 0;
      const end = target.selectionEnd ?? 0;
      return target.value.slice(start, end);
    }
    return window.getSelection()?.toString() ?? "";
  };

  return [
    {
      id: "edit-cut",
      label: "Cut",
      icon: <Scissors className="w-3.5 h-3.5" />,
      shortcut: "Ctrl+X",
      disabled: isReadOnly || !hasSelection,
      onSelect: () => {
        target.focus();
        const selected = getSelectedText();
        if (selected && navigator.clipboard?.writeText) {
          void navigator.clipboard.writeText(selected);
        }
        document.execCommand("cut");
      },
    },
    {
      id: "edit-copy",
      label: "Copy",
      icon: <Copy className="w-3.5 h-3.5" />,
      shortcut: "Ctrl+C",
      disabled: !hasSelection,
      onSelect: () => {
        target.focus();
        const selected = getSelectedText();
        if (selected && navigator.clipboard?.writeText) {
          void navigator.clipboard.writeText(selected);
        } else {
          document.execCommand("copy");
        }
      },
    },
    {
      id: "edit-paste",
      label: "Paste",
      icon: <Clipboard className="w-3.5 h-3.5" />,
      shortcut: "Ctrl+V",
      disabled: isReadOnly,
      onSelect: () => {
        target.focus();
        if (navigator.clipboard?.readText) {
          void navigator.clipboard
            .readText()
            .then((text) => {
              if (!text) return;
              if (isInputOrTextarea) {
                const start = target.selectionStart ?? target.value.length;
                const end = target.selectionEnd ?? target.value.length;
                const next =
                  target.value.slice(0, start) + text + target.value.slice(end);
                const proto =
                  target instanceof HTMLTextAreaElement
                    ? HTMLTextAreaElement.prototype
                    : HTMLInputElement.prototype;
                const setter = Object.getOwnPropertyDescriptor(
                  proto,
                  "value"
                )?.set;
                if (setter) {
                  setter.call(target, next);
                } else {
                  target.value = next;
                }
                target.dispatchEvent(new Event("input", { bubbles: true }));
                const cursor = start + text.length;
                try {
                  target.setSelectionRange(cursor, cursor);
                } catch {
                  // Ignore InvalidStateError on input types that do not support selection ranges (e.g., number)
                }
              } else {
                document.execCommand("insertText", false, text);
              }
            })
            .catch(() => {
              document.execCommand("paste");
            });
        } else {
          document.execCommand("paste");
        }
      },
    },
    {
      id: "edit-select-all",
      label: "Select All",
      icon: <TextSelect className="w-3.5 h-3.5" />,
      shortcut: "Ctrl+A",
      divider: true,
      onSelect: () => {
        target.focus();
        if (isInputOrTextarea) {
          target.select();
        } else {
          document.execCommand("selectAll");
        }
      },
    },
  ];
}

export function ContextMenuProvider({ children }: { children: ReactNode }) {
  const { navigate, setCommandPaletteOpen } = useAppStore();
  const [menu, setMenu] = useState<ContextMenuState | null>(null);
  const [activeIndex, setActiveIndex] = useState<number>(0);
  const [position, setPosition] = useState<{ left: number; top: number }>({
    left: 0,
    top: 0,
  });
  const menuRef = useRef<HTMLDivElement | null>(null);

  const closeContextMenu = useCallback(() => {
    setMenu(null);
  }, []);

  const openContextMenu = useCallback(
    (e: ReactMouseEvent | MouseEvent, items: ContextMenuItem[]) => {
      e.preventDefault();
      e.stopPropagation();
      if (items.length === 0) return;
      const firstEnabled = items.findIndex((item) => !item.disabled);
      setActiveIndex(firstEnabled >= 0 ? firstEnabled : 0);
      setPosition({ left: e.clientX, top: e.clientY });
      setMenu({ x: e.clientX, y: e.clientY, items });
    },
    []
  );

  // Global document contextmenu listener
  useEffect(() => {
    const handleContextMenu = (e: MouseEvent) => {
      if (e.defaultPrevented) return;
      e.preventDefault();

      const editable = isEditableTarget(e.target);
      if (editable) {
        const items = buildEditableItems(editable);
        setActiveIndex(items.findIndex((item) => !item.disabled));
        setPosition({ left: e.clientX, top: e.clientY });
        setMenu({ x: e.clientX, y: e.clientY, items });
        return;
      }

      const quickItems: ContextMenuItem[] = [
        {
          id: "quick-command-palette",
          label: "Command Palette",
          icon: <Command className="w-3.5 h-3.5" />,
          shortcut: "Ctrl+K",
          onSelect: () => setCommandPaletteOpen(true),
        },
        {
          id: "quick-race",
          label: "Quick Race",
          icon: <Flame className="w-3.5 h-3.5" />,
          divider: true,
          onSelect: () => navigate("drive", "quick-race"),
        },
        {
          id: "quick-online",
          label: "Online Servers",
          icon: <Globe className="w-3.5 h-3.5" />,
          onSelect: () => navigate("online"),
        },
        {
          id: "quick-showroom",
          label: "3D Showroom",
          icon: <Sparkles className="w-3.5 h-3.5" />,
          onSelect: () => navigate("garage", "showroom"),
        },
        {
          id: "quick-settings",
          label: "App Settings",
          icon: <Settings className="w-3.5 h-3.5" />,
          divider: true,
          onSelect: () => navigate("setup", "app"),
        },
      ];

      setActiveIndex(0);
      setPosition({ left: e.clientX, top: e.clientY });
      setMenu({ x: e.clientX, y: e.clientY, items: quickItems });
    };

    document.addEventListener("contextmenu", handleContextMenu);
    return () => {
      document.removeEventListener("contextmenu", handleContextMenu);
    };
  }, [navigate, setCommandPaletteOpen]);

  // Viewport edge clamping
  useLayoutEffect(() => {
    if (!menu || !menuRef.current) return;
    const rect = menuRef.current.getBoundingClientRect();
    const pad = 8;
    let left = menu.x;
    let top = menu.y;

    if (left + rect.width > window.innerWidth - pad) {
      left = Math.max(pad, window.innerWidth - rect.width - pad);
    }
    if (top + rect.height > window.innerHeight - pad) {
      top = Math.max(pad, window.innerHeight - rect.height - pad);
    }

    setPosition({ left, top });
  }, [menu]);

  // Dismissal & keyboard navigation
  useEffect(() => {
    if (!menu) return;

    const handlePointerDown = (e: MouseEvent) => {
      if (menuRef.current && !menuRef.current.contains(e.target as Node)) {
        closeContextMenu();
      }
    };

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" || e.key === "Tab") {
        e.preventDefault();
        closeContextMenu();
        return;
      }

      const items = menu.items;
      if (items.length === 0) return;

      if (e.key === "ArrowDown") {
        e.preventDefault();
        let next = activeIndex;
        for (let i = 0; i < items.length; i++) {
          next = (next + 1) % items.length;
          if (!items[next].disabled) {
            setActiveIndex(next);
            break;
          }
        }
      } else if (e.key === "ArrowUp") {
        e.preventDefault();
        let prev = activeIndex;
        for (let i = 0; i < items.length; i++) {
          prev = (prev - 1 + items.length) % items.length;
          if (!items[prev].disabled) {
            setActiveIndex(prev);
            break;
          }
        }
      } else if (e.key === "Enter") {
        e.preventDefault();
        const current = items[activeIndex];
        if (current && !current.disabled) {
          closeContextMenu();
          current.onSelect();
        }
      }
    };

    const handleDismiss = () => closeContextMenu();

    window.addEventListener("mousedown", handlePointerDown);
    window.addEventListener("keydown", handleKeyDown);
    window.addEventListener("scroll", handleDismiss, true);
    window.addEventListener("blur", handleDismiss);
    window.addEventListener("resize", handleDismiss);

    return () => {
      window.removeEventListener("mousedown", handlePointerDown);
      window.removeEventListener("keydown", handleKeyDown);
      window.removeEventListener("scroll", handleDismiss, true);
      window.removeEventListener("blur", handleDismiss);
      window.removeEventListener("resize", handleDismiss);
    };
  }, [menu, activeIndex, closeContextMenu]);

  return (
    <ContextMenuContext.Provider value={{ openContextMenu, closeContextMenu }}>
      {children}
      {menu &&
        createPortal(
          <div
            ref={menuRef}
            role="menu"
            aria-label="Context menu"
            style={{ left: position.left, top: position.top }}
            className="fixed z-[120] min-w-[210px] max-w-[280px] p-1.5 bg-[#0B0F17]/95 backdrop-blur-xl border border-white/[0.1] rounded-xl shadow-[0_18px_48px_rgba(0,0,0,0.65)] text-sm select-none"
          >
            {menu.items.map((item, idx) => {
              const isActive = idx === activeIndex && !item.disabled;
              return (
                <div key={item.id ?? `${item.label}-${idx}`}>
                  {item.divider && (
                    <div className="my-1 border-t border-white/[0.08]" />
                  )}
                  <button
                    type="button"
                    role="menuitem"
                    disabled={item.disabled}
                    onMouseEnter={() => {
                      if (!item.disabled) setActiveIndex(idx);
                    }}
                    onClick={() => {
                      if (item.disabled) return;
                      closeContextMenu();
                      item.onSelect();
                    }}
                    className={[
                      "w-full flex items-center justify-between gap-3 px-2.5 py-1.5 rounded-lg text-left text-xs font-medium transition-colors",
                      item.disabled
                        ? "opacity-40 cursor-not-allowed text-[var(--text-muted)]"
                        : item.danger
                          ? isActive
                            ? "bg-red-500/20 text-red-300"
                            : "text-red-400 hover:bg-red-500/15"
                          : isActive
                            ? "bg-white/[0.1] text-white"
                            : "text-[var(--text-secondary)] hover:bg-white/[0.07] hover:text-white",
                    ].join(" ")}
                  >
                    <span className="flex items-center gap-2 min-w-0">
                      {item.icon && (
                        <span className="shrink-0 opacity-80">{item.icon}</span>
                      )}
                      <span className="truncate">{item.label}</span>
                    </span>
                    {item.shortcut && (
                      <span className="shrink-0 text-[10px] font-mono text-[var(--text-muted)]">
                        {item.shortcut}
                      </span>
                    )}
                  </button>
                </div>
              );
            })}
          </div>,
          document.body
        )}
    </ContextMenuContext.Provider>
  );
}
