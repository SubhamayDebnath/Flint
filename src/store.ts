import { create } from "zustand";
import type { Hit } from "./types";

export const CATEGORIES = ["all", "apps", "files", "folders"] as const;
export type Category = (typeof CATEGORIES)[number];

interface State {
  query: string;
  cat: number;
  hits: Hit[];
  sel: number;
  /** The query `hits` belongs to (prevents a "No results" flash while typing). */
  answered: string;
  setQuery: (q: string) => void;
  cycleCat: (delta: number) => void;
  setSel: (i: number) => void;
  moveSel: (delta: number, count: number) => void;
  setHits: (hits: Hit[], answered: string) => void;
  clearHits: () => void;
  reset: () => void;
}

const sameHits = (a: Hit[], b: Hit[]) =>
  a.length === b.length && a.every((h, i) => h.path === b[i].path && h.kind === b[i].kind);

export const useStore = create<State>()((set) => ({
  query: "",
  cat: 0,
  hits: [],
  sel: 0,
  answered: "",
  setQuery: (query) => set({ query }),
  cycleCat: (delta) =>
    set((s) => ({ cat: (s.cat + delta + CATEGORIES.length) % CATEGORIES.length })),
  setSel: (sel) => set((s) => (s.sel === sel ? s : { sel })),
  moveSel: (delta, count) => set((s) => (count ? { sel: (s.sel + delta + count) % count } : s)),
  // identical results keep the same array reference -> no re-render, no flicker
  setHits: (hits, answered) =>
    set((s) => ({ hits: sameHits(s.hits, hits) ? s.hits : hits, sel: 0, answered })),
  clearHits: () =>
    set((s) => (s.hits.length || s.answered ? { hits: [], answered: "", sel: 0 } : s)),
  reset: () => set({ query: "", cat: 0, hits: [], sel: 0, answered: "" }),
}));
