import { useCallback, useEffect, useLayoutEffect, useMemo, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Search } from "lucide-react";
import { evalExpr } from "./calc";
import { ResultRow } from "./components/ResultRow";
import { useBackendEvents, useSearch, searchCache } from "./hooks";
import { CATEGORIES, useStore, type Category } from "./store";
import type { Hit } from "./types";

const CATEGORY_LABEL: Record<Category, string> = {
  all: "All",
  apps: "Apps",
  files: "Files",
  folders: "Folders",
};
const PREFIX: Record<string, Category> = { app: "apps", file: "files", folder: "folders" };
const LIMIT = 10;

const quiet = () => {};

export default function App() {
  const query = useStore((s) => s.query);
  const cat = useStore((s) => s.cat);
  const hits = useStore((s) => s.hits);
  const sel = useStore((s) => s.sel);
  const answered = useStore((s) => s.answered);
  const { setQuery, setSel, moveSel, cycleCat, reset } = useStore.getState();

  const inputRef = useRef<HTMLInputElement>(null);
  const panelRef = useRef<HTMLElement>(null);
  const lastHeight = useRef(0);

  useSearch();

  const calc = useMemo(() => evalExpr(query), [query]);
  const items = useMemo<Hit[]>(() => {
    const calcHit: Hit[] = calc
      ? [{ name: `= ${calc}`, path: "Press Enter to copy", kind: "calc" }]
      : [];
    return [...calcHit, ...hits].slice(0, LIMIT);
  }, [calc, hits]);

  const effective = useMemo<Category>(() => {
    const m = /^:(app|file|folder)(\s|$)/i.exec(query);
    return m ? PREFIX[m[1].toLowerCase()] : CATEGORIES[cat];
  }, [query, cat]);

  // the window is hidden on blur by the backend; the UI is cleaned up at the same time
  const onShow = useCallback(() => {
    reset();
    inputRef.current?.focus();
  }, [reset]);
  useBackendEvents(onShow);

  useEffect(() => {
    window.addEventListener("blur", reset);
    inputRef.current?.focus();
    return () => window.removeEventListener("blur", reset);
  }, [reset]);

  // native window height always matches the panel (IPC only when it changes)
  useLayoutEffect(() => {
    const el = panelRef.current!;
    let raf = 0;
    const ro = new ResizeObserver(() => {
      cancelAnimationFrame(raf);
      raf = requestAnimationFrame(() => {
        const height = Math.ceil(el.getBoundingClientRect().height);
        if (height === lastHeight.current) return;
        lastHeight.current = height;
        invoke("resize", { height }).catch(quiet);
      });
    });
    ro.observe(el);
    return () => {
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  }, []);

  const launch = useCallback(
    async (h?: Hit, reveal = false) => {
      if (!h) return;
      try {
        if (h.kind === "calc") {
          await navigator.clipboard.writeText(calc ?? "").catch(quiet);
          await invoke("hide");
          return;
        }
        await invoke("launch", { kind: h.kind, path: h.path, reveal });
      } catch {
        void invoke("hide").catch(quiet);
      }
    },
    [calc],
  );

  const pick = useCallback((h: Hit) => void launch(h), [launch]);

  const current = items[sel];
  const ghost =
    current &&
    current.kind !== "calc" &&
    query &&
    !query.startsWith(":") &&
    current.name.toLowerCase().startsWith(query.toLowerCase())
      ? current.name.slice(query.length)
      : "";

  const onKeyDown = (e: React.KeyboardEvent) => {
    // Ctrl+R: re-index apps and files
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "r") {
      e.preventDefault();
      searchCache.clear();
      void invoke("reindex").catch(quiet);
      return;
    }
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        moveSel(1, items.length);
        break;
      case "ArrowUp":
        e.preventDefault();
        moveSel(-1, items.length);
        break;
      case "Tab":
        e.preventDefault();
        cycleCat(e.shiftKey ? -1 : 1);
        break;
      case "Enter":
        e.preventDefault();
        void launch(current, e.shiftKey);
        break;
      case "Escape":
        e.preventDefault();
        void invoke("hide").catch(quiet);
        break;
      case "ArrowRight":
        if (ghost && inputRef.current?.selectionStart === query.length) {
          e.preventDefault();
          setQuery(query + ghost);
        }
        break;
    }
  };

  const showEmpty = !!query.trim() && items.length === 0 && answered === query;

  return (
    <main ref={panelRef} onKeyDown={onKeyDown} className="w-full overflow-hidden bg-background">
      <div className="flex h-12 items-center gap-3 px-4">
        <div className="relative flex-1">
          <input
            ref={inputRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search apps, files and commands…"
            spellCheck={false}
            autoComplete="off"
            maxLength={256}
            className="w-full border-none bg-transparent text-sm leading-6 outline-none placeholder:text-subtle"
          />
          {ghost && (
            <div className="pointer-events-none absolute inset-0 flex items-center text-sm leading-6 whitespace-pre">
              <span className="invisible">{query}</span>
              <span className="text-subtle">{ghost}</span>
            </div>
          )}
        </div>
        {effective !== "all" && (
          <span className="rounded bg-chip px-1.5 py-0.5 text-[11px] font-medium text-muted">
            {CATEGORY_LABEL[effective]}
          </span>
        )}
        <Search className="size-4.5 shrink-0 text-muted" strokeWidth={2} />
      </div>

      {(items.length > 0 || showEmpty) && (
        <ul className="p-1.5">
          {items.map((h, i) => (
            <ResultRow
              key={h.kind + h.path}
              h={h}
              i={i}
              active={i === sel}
              onHover={setSel}
              onPick={pick}
            />
          ))}
          {showEmpty && (
            <li className="flex h-10 items-center px-3 text-[13px] text-subtle">No results</li>
          )}
        </ul>
      )}
    </main>
  );
}
