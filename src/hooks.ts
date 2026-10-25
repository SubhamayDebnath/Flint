import { useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { CATEGORIES, useStore } from "./store";
import type { Hit } from "./types";

/** Wait this long after the last keystroke before asking the backend. */
export const DEBOUNCE_MS = 120;
const CACHE_SIZE = 80;

/** Result cache shared by the search hook and the re-index handlers. */
export const searchCache = new Map<string, Hit[]>();

/**
 * Debounced, race-free search.
 * - cached queries are applied synchronously (instant backspace)
 * - everything else waits DEBOUNCE_MS, stale responses are dropped
 * - previous results stay on screen until new ones arrive (no flicker)
 */
export function useSearch() {
  const query = useStore((s) => s.query);
  const cat = useStore((s) => s.cat);
  const request = useRef(0);

  useEffect(() => {
    const id = ++request.current;
    const { setHits, clearHits } = useStore.getState();
    if (!query.trim()) {
      clearHits();
      return;
    }
    const key = `${cat}|${query}`;
    const cached = searchCache.get(key);
    if (cached) {
      setHits(cached, query);
      return;
    }
    const timer = window.setTimeout(() => {
      invoke<Hit[]>("search", { query, category: CATEGORIES[cat] })
        .then((r) => {
          if (id !== request.current) return;
          if (searchCache.size >= CACHE_SIZE) searchCache.delete(searchCache.keys().next().value!);
          searchCache.set(key, r);
          setHits(r, query);
        })
        .catch(() => {
          // backend busy or restarting: keep what is on screen, never crash the UI
        });
    }, DEBOUNCE_MS);
    return () => window.clearTimeout(timer);
  }, [query, cat]);

  // invalidate in-flight requests when the panel is reset
  useEffect(
    () =>
      useStore.subscribe((s, p) => {
        if (s.query === "" && p.query !== "") request.current++;
      }),
    [],
  );
}

/** Backend events: panel shown, index refreshed. */
export function useBackendEvents(onShow: () => void) {
  const cb = useRef(onShow);
  cb.current = onShow;

  useEffect(() => {
    let disposed = false;
    const off: Array<() => void> = [];
    const sub = (name: string, fn: () => void) =>
      listen(name, fn).then((u) => (disposed ? u() : off.push(u)));

    void sub("flint-show", () => cb.current());
    void sub("flint-index", () => searchCache.clear());
    return () => {
      disposed = true;
      off.forEach((u) => u());
    };
  }, []);
}
