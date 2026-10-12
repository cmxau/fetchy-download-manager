import { useState } from "react";
import {
  AppWindow, ChevronDown, ChevronUp, FileArchive, FileText, Folder, FolderCheck, FolderDown, Globe, Settings2,
  Layers, LayoutDashboard, ListOrdered, Music, Video,
} from "lucide-react";
import type { Category, DownloadInfo, Filter, Queue } from "../types";

const cats: { key: Category; label: string; icon: React.ReactNode }[] = [
  { key: "program", label: "Applications", icon: <AppWindow size={16} /> },
  { key: "compressed", label: "Compressed", icon: <FileArchive size={16} /> },
  { key: "document", label: "Documents", icon: <FileText size={16} /> },
  { key: "music", label: "Music", icon: <Music size={16} /> },
  { key: "video", label: "Videos", icon: <Video size={16} /> },
];

interface Props {
  items: DownloadInfo[];
  filter: Filter;
  onFilter: (f: Filter) => void;
  queues: Queue[];
    projects: { id: string; name: string }[];
  onProjects: () => void;
  onQueues: () => void;
  onOverview: () => void;
  overviewOn: boolean;
}

export function Sidebar({ items, filter, onFilter, queues, projects, onProjects, onQueues, onOverview, overviewOn }: Props) {
  const [openProjects, setOpenProjects] = useState(true);
  const [openAll, setOpenAll] = useState(true);
  const [openQueues, setOpenQueues] = useState(true);
  const unfinished = items.filter((i) => i.status !== "complete").length;
  const finished = items.length - unfinished;
  const count = (c: Category) => items.filter((i) => i.category === c || (c === "program" && i.category === "apk")).length;

  return (
    <aside className="sidebar">
      <div className="tree card">
        <div className={overviewOn ? "tree-row top on" : "tree-row top"} onClick={onOverview}>
          <LayoutDashboard size={20} />
          <span>Overview</span>
        </div>
        <div className="tree-label">Categories</div>
        <div className={filter === "all" ? "tree-row top on" : "tree-row top"} onClick={() => onFilter("all")}>
          <Layers size={20} />
          <span>All Downloads</span>
          <button
            className="chev"
            aria-label="Toggle categories"
            onClick={(e) => {
              e.stopPropagation();
              setOpenAll(!openAll);
            }}
          >
            {openAll ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
          </button>
        </div>
        {openAll && (
          <div className="subs">
            {cats.map((c) => (
              <div key={c.key} className={filter === c.key ? "tree-row sub on" : "tree-row sub"} onClick={() => onFilter(c.key)}>
                {c.icon}
                <span>{c.label}</span>
                <em>{count(c.key) || ""}</em>
              </div>
            ))}
          </div>
        )}
        <div className={filter === "unfinished" ? "tree-row top on" : "tree-row top"} onClick={() => onFilter("unfinished")}>
          <FolderDown size={20} />
          <span>Unfinished</span>
          <em>{unfinished || ""}</em>
        </div>
        <div className={filter === "finished" ? "tree-row top on" : "tree-row top"} onClick={() => onFilter("finished")}>
          <FolderCheck size={20} />
          <span>Finished</span>
          <em>{finished || ""}</em>
        </div>
        <div className="tree-row top" onClick={() => setOpenProjects(!openProjects)} onDoubleClick={onProjects} title="Double-click to manage projects">
          <Globe size={20} />
          <span>Grabber Projects</span>
          <button className="chev" aria-label="Toggle projects">{openProjects ? <ChevronUp size={16} /> : <ChevronDown size={16} />}</button>
        </div>
        {openProjects && (
          <div className="subs">
            {projects.map((p) => {
              const key = `project:${p.name}` as Filter;
              const n = items.filter((i) => i.project === p.name).length;
              return (
                <div key={p.id} className={filter === key ? "tree-row sub on" : "tree-row sub"} onClick={() => onFilter(key)} title={p.name}>
                  <Folder size={16} />
                  <span>{p.name}</span>
                  <em>{n || ""}</em>
                </div>
              );
            })}
            <div className="tree-row sub" onClick={onProjects}>
              <Settings2 size={16} />
              <span>Manage projects…</span>
            </div>
          </div>
        )}
        <div className="tree-row top" onClick={() => setOpenQueues(!openQueues)} onDoubleClick={onQueues} title="Double-click to open the scheduler">
          <ListOrdered size={20} />
          <span>Queues</span>
          <button className="chev" aria-label="Toggle queues">{openQueues ? <ChevronUp size={16} /> : <ChevronDown size={16} />}</button>
        </div>
        {openQueues && queues.map((q) => {
          const key = `queue:${q.id}` as Filter;
          const n = items.filter((i) => i.queue === q.id && i.status !== "complete").length;
          return (
            <div key={q.id} className={filter === key ? "tree-row sub on" : "tree-row sub"} onClick={() => onFilter(key)}>
              <ListOrdered size={16} />
              <span>{q.name}</span>
              <em>{q.schedule?.enabled ? "⏲ " : ""}{n || ""}</em>
            </div>
          );
        })}
      </div>
    </aside>
  );
}
