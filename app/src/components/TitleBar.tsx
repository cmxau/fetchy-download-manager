import { Moon, Search, Settings as Cog, Sun } from "lucide-react";
import { isTauri } from "../api";

interface Props {
  query: string;
  onQuery: (q: string) => void;
  searchRef: React.RefObject<HTMLInputElement | null>;
  dark: boolean;
  onToggleTheme: () => void;
  onSettings: () => void;
  settingsOpen: boolean;
}

async function win(op: "minimize" | "toggleMaximize" | "fullscreen" | "close") {
  if (!isTauri) return;
  if (op === "toggleMaximize") {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    void getCurrentWindow().toggleMaximize();
    return;
  }
  if (op === "fullscreen") {
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    const w = getCurrentWindow();
    void w.setFullscreen(!(await w.isFullscreen()));
    return;
  }
  // Minimize / close go through the app so the tray settings apply.
  const { invoke } = await import("@tauri-apps/api/core");
  void invoke("window_action", { op });
}

export function Logo({ size = 26 }: { size?: number }) {
  return (
    <svg width={size} height={size} viewBox="16 16 480 480" aria-hidden>
      <defs>
        <linearGradient id="lg" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="#22c55e" />
          <stop offset="1" stopColor="#3b82f6" />
        </linearGradient>
        <clipPath id="lgc"><rect x="16" y="16" width="480" height="480" rx="112" /></clipPath>
      </defs>
      <rect x="16" y="16" width="480" height="480" rx="112" fill="url(#lg)" />
      <g clipPath="url(#lgc)">
        <path d="M70 520 C70 400 130 330 215 320 L330 320 C300 400 300 470 320 520 Z" fill="#fff" />
        <path d="M118 392 C170 420 250 424 312 398 L318 436 C250 464 160 458 108 430 Z" fill="#0a0a0a" />
        <circle cx="222" cy="232" r="122" fill="#fff" />
        <rect x="290" y="226" width="130" height="92" rx="46" fill="#fff" />
        <path d="M152 128 C98 150 78 250 108 332 C146 350 186 306 192 232 C195 184 182 140 152 128 Z" fill="#0a0a0a" />
        <circle cx="282" cy="212" r="17" fill="#0a0a0a" />
        <ellipse cx="410" cy="250" rx="22" ry="16" fill="#0a0a0a" />
        <circle cx="352" cy="330" r="56" fill="#d9f99d" />
        <path d="M312 292 C336 316 336 346 312 372 M392 288 C368 314 368 346 392 372" fill="none" stroke="#fff" strokeWidth="9" strokeLinecap="round" />
      </g>
    </svg>
  );
}

export function TitleBar({ query, onQuery, searchRef, dark, onToggleTheme, onSettings, settingsOpen }: Props) {
  return (
    <header className="titlebar" data-tauri-drag-region onDoubleClick={() => win("toggleMaximize")}>
      <div className="brand" data-tauri-drag-region>
        <Logo />
        <span data-tauri-drag-region>Fetchy</span>
      </div>
      <label className="search" onDoubleClick={(e) => e.stopPropagation()}>
        <Search size={16} />
        <input ref={searchRef} value={query} onChange={(e) => onQuery(e.target.value)} placeholder="Search downloads" spellCheck={false} />
      </label>
      <div className="tb-actions" onDoubleClick={(e) => e.stopPropagation()}>
        <button className="tb-btn" onClick={onToggleTheme} aria-label={dark ? "Switch to light mode" : "Switch to dark mode"} title={dark ? "Light mode" : "Dark mode"}>
          {dark ? <Sun size={17} /> : <Moon size={17} />}
        </button>
        <button className={settingsOpen ? "tb-btn on" : "tb-btn"} onClick={onSettings} aria-label="Settings" title="Settings (Ctrl+,)">
          <Cog size={17} />
        </button>
      </div>
      <div className="win-ctl" onDoubleClick={(e) => e.stopPropagation()}>
        <button className="wc" aria-label="Minimize" onClick={() => win("minimize")}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 5.5h10" stroke="currentColor" strokeWidth="1" /></svg>
        </button>
        <button className="wc" aria-label="Maximize" onClick={(e) => win(e.altKey || document.documentElement.classList.contains("mac") ? "fullscreen" : "toggleMaximize")}>
          <svg width="10" height="10" viewBox="0 0 10 10"><rect x="0.5" y="0.5" width="9" height="9" fill="none" stroke="currentColor" strokeWidth="1" /></svg>
        </button>
        <button className="wc close" aria-label="Close" onClick={() => win("close")}>
          <svg width="10" height="10" viewBox="0 0 10 10"><path d="M0 0l10 10M10 0L0 10" stroke="currentColor" strokeWidth="1" /></svg>
        </button>
      </div>
    </header>
  );
}
