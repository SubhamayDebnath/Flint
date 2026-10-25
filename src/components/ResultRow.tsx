import { memo } from "react";
import type { Hit } from "../types";
import { ResultIcon } from "./ResultIcon";

function subtitle({ kind, path }: Hit) {
  if (kind === "file" || kind === "folder") return path.slice(0, path.lastIndexOf("\\")) || path;
  if (kind === "command") return "System command";
  if (kind === "calc") return path;
  return "";
}

interface Props {
  h: Hit;
  i: number;
  active: boolean;
  onHover: (i: number) => void;
  onPick: (h: Hit) => void;
}

export const ResultRow = memo(function ResultRow({ h, i, active, onHover, onPick }: Props) {
  const sub = subtitle(h);
  return (
    <li
      onMouseMove={() => onHover(i)}
      onClick={() => onPick(h)}
      className={`flex h-10 items-center gap-3 rounded-md px-3 ${active ? "bg-hover" : ""}`}
    >
      <ResultIcon h={h} />
      <span className="max-w-[55%] shrink-0 truncate text-[14px]">{h.name}</span>
      {sub && <span className="min-w-0 flex-1 truncate text-[12px] text-subtle">{sub}</span>}
    </li>
  );
});
