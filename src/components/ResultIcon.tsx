import { memo, useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Activity,
  AppWindow,
  Bluetooth,
  Calculator,
  Cpu,
  Database,
  Download,
  File as FileIcon,
  Folder,
  FolderOpen,
  Gauge,
  HardDrive,
  Info,
  Laptop,
  LayoutGrid,
  Lock,
  LogOut,
  Monitor,
  Moon,
  Network,
  Power,
  PowerOff,
  RefreshCw,
  RotateCw,
  Scissors,
  ScrollText,
  Settings,
  SlidersHorizontal,
  Terminal,
  Trash2,
  Volume2,
  Wifi,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import type { Hit } from "../types";

const COMMAND_ICON: Record<string, LucideIcon> = {
  lock: Lock,
  sleep: Moon,
  hibernate: PowerOff,
  signout: LogOut,
  restart: RotateCw,
  shutdown: Power,
  trash: Trash2,
  recyclebin: Trash2,
  taskmgr: Activity,
  desktop: Monitor,
  snip: Scissors,
  flushdns: Network,
  reindex: RefreshCw,
  thispc: Laptop,
  downloads: Download,
  documents: FolderOpen,
  settings: Settings,
  wifi: Wifi,
  bluetooth: Bluetooth,
  display: Monitor,
  sound: Volume2,
  apps: LayoutGrid,
  startup: Power,
  storage: HardDrive,
  update: RefreshCw,
  control: SlidersHorizontal,
  devmgr: Cpu,
  diskmgmt: HardDrive,
  services: Wrench,
  eventvwr: ScrollText,
  resmon: Gauge,
  sysinfo: Info,
  regedit: Database,
  cmd: Terminal,
  powershell: Terminal,
  terminal: Terminal,
};

// Shell icons are fetched once per key: per file for exe/ico/lnk/..., per extension otherwise.
const UNIQUE_ICON_EXT = new Set(["exe", "ico", "lnk", "msi", "scr"]);
const ICON_CACHE_MAX = 300; // bounded: per-file icons must not grow forever
const pending = new Map<string, Promise<string | null>>();
const loaded = new Map<string, string | null>();

function remember(key: string, src: string | null) {
  if (loaded.size >= ICON_CACHE_MAX) {
    const oldest = loaded.keys().next().value!;
    loaded.delete(oldest);
    pending.delete(oldest);
  }
  loaded.set(key, src);
}

function iconKey({ kind, path }: Hit) {
  if (kind === "folder") return "folder";
  if (kind === "file") {
    const ext = path.split(".").pop()?.toLowerCase() ?? "";
    if (!UNIQUE_ICON_EXT.has(ext)) return `ext:${ext}`;
  }
  return path;
}

function loadIcon(key: string, path: string) {
  let p = pending.get(key);
  if (!p) {
    p = invoke<string | null>("icon", { path })
      .catch(() => null)
      .then((s) => (remember(key, s), s));
    pending.set(key, p);
  }
  return p;
}

const GLYPH = "size-6 shrink-0 p-0.5 text-muted";

export const ResultIcon = memo(function ResultIcon({ h }: { h: Hit }) {
  const key = iconKey(h);
  const native = h.kind !== "command" && h.kind !== "calc";
  const [src, setSrc] = useState(loaded.get(key));

  useEffect(() => {
    if (!native) return;
    let live = true;
    loadIcon(key, h.path).then((s) => live && setSrc(s));
    return () => {
      live = false;
    };
  }, [key, native, h.path]);

  if (h.kind === "command") {
    const Glyph = COMMAND_ICON[h.path] ?? Power;
    return <Glyph className={GLYPH} strokeWidth={1.75} />;
  }
  if (h.kind === "calc") return <Calculator className={GLYPH} strokeWidth={1.75} />;
  if (src) {
    return <img src={src} className="size-6 shrink-0 object-contain" draggable={false} alt="" />;
  }
  const Fallback = h.kind === "app" ? AppWindow : h.kind === "folder" ? Folder : FileIcon;
  return <Fallback className={GLYPH} strokeWidth={1.5} />;
});
