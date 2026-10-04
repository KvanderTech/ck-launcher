import { KvanthIcon } from "./KvanthIcon";
import { useEffect, useRef, useState } from "react";

import type { AppApi } from "../app/tauri";
import type { BuildSummary, GameStartedEvent, ProgressEvent } from "../app/types";
import type { ContentInstallTask } from "../features/content/ContentPage";
import { GameActivity } from "./GameActivity";
import { WindowControls } from "./WindowControls";

export interface CompletedLauncherTask {
  id: string;
  title: string;
  detail: string;
  completedAt: number;
  iconUrl?: string;
}

interface LauncherTopbarProps {
  activeBuild?: BuildSummary;
  api: AppApi;
  canGoBack: boolean;
  canGoForward: boolean;
  cancelling: boolean;
  completedTasks: CompletedLauncherTask[];
  contentTask?: ContentInstallTask;
  onClearTasks(): void;
  onCancelProgress(): void;
  onGoBack(): void;
  onGoForward(): void;
  progress?: ProgressEvent;
  runningGame?: GameStartedEvent;
}

export function LauncherTopbar({ activeBuild, api, canGoBack, canGoForward, cancelling, completedTasks, contentTask, onCancelProgress, onClearTasks, onGoBack, onGoForward, progress, runningGame }: LauncherTopbarProps) {
  const [tasksOpen, setTasksOpen] = useState(false);
  const [bytesPerSecond, setBytesPerSecond] = useState(0);
  const speedSample = useRef<{ bytes: number; at: number; operationId: string } | undefined>(undefined);
  const taskManagerAnchor = useRef<HTMLDivElement>(null);
  const hasActiveTask = Boolean(contentTask || (progress && !["idle", "running", "failed"].includes(progress.stage)));
  const taskCount = (hasActiveTask ? 1 : 0) + completedTasks.length;

  useEffect(() => {
    if (!progress || progress.stage !== "downloading") {
      speedSample.current = undefined;
      setBytesPerSecond(0);
      return;
    }
    const now = performance.now();
    const previous = speedSample.current;
    if (previous && previous.operationId === progress.operationId) {
      const elapsed = (now - previous.at) / 1000;
      if (elapsed > .15) setBytesPerSecond(Math.max(0, (progress.completedBytes - previous.bytes) / elapsed));
    }
    speedSample.current = { bytes: progress.completedBytes, at: now, operationId: progress.operationId };
  }, [progress]);

  useEffect(() => {
    if (!tasksOpen) return;
    function closeOutside(event: PointerEvent) {
      if (!taskManagerAnchor.current?.contains(event.target as Node)) setTasksOpen(false);
    }
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setTasksOpen(false);
    }
    document.addEventListener("pointerdown", closeOutside);
    document.addEventListener("keydown", closeOnEscape);
    return () => {
      document.removeEventListener("pointerdown", closeOutside);
      document.removeEventListener("keydown", closeOnEscape);
    };
  }, [tasksOpen]);

  return (
    <>
      <div className="topbar-navigation" aria-label="Навигация">
        <button aria-label="Назад" disabled={!canGoBack} onClick={onGoBack} type="button"><KvanthIcon name="back" size={18} /></button>
        <button aria-label="Вперёд" disabled={!canGoForward} onClick={onGoForward} type="button"><KvanthIcon name="forward" size={18} /></button>
      </div>
      <div className="topbar-build" title={activeBuild?.name ?? "Сборка не выбрана"}>
        {activeBuild?.iconUrl ? <img alt="" src={activeBuild.iconUrl} /> : <span><KvanthIcon name="custom-pack" size={22} /></span>}
        <strong>{activeBuild?.name ?? "Выберите сборку"}</strong>
      </div>
      <div className="topbar-drag-region" data-tauri-drag-region />
      <div className="topbar-actions">
        <div className="task-manager-anchor" ref={taskManagerAnchor}>
          <div className="task-manager-controls">
            <button aria-expanded={tasksOpen} aria-label="Менеджер загрузок" className={hasActiveTask ? "task-manager-trigger is-active" : "task-manager-trigger"} onClick={() => setTasksOpen((value) => !value)} title="Менеджер загрузок" type="button">
              <KvanthIcon name="download" size={18} />
              {bytesPerSecond > 0 ? <span>{formatSpeed(bytesPerSecond)}</span> : null}
              {taskCount > 0 ? <b>{taskCount}</b> : null}
            </button>
            {progress && !["idle", "running", "failed"].includes(progress.stage) ? <button aria-label="Отменить текущую задачу" className="topbar-cancel-task" disabled={cancelling} onClick={onCancelProgress} title="Отменить загрузку" type="button"><KvanthIcon name="close" size={18} /></button> : null}
          </div>
          {tasksOpen ? <TaskManager activeBuild={activeBuild} completed={completedTasks} contentTask={contentTask} onClear={onClearTasks} progress={progress} /> : null}
        </div>
        {runningGame ? <GameActivity api={api} game={runningGame} iconUrl={activeBuild?.iconUrl} name={activeBuild?.name ?? "Minecraft"} /> : <div className="topbar-game-idle"><i />Нет запущенных игр</div>}
      </div>
      <WindowControls />
    </>
  );
}

function TaskManager({ activeBuild, completed, contentTask, onClear, progress }: { activeBuild?: BuildSummary; completed: CompletedLauncherTask[]; contentTask?: ContentInstallTask; onClear(): void; progress?: ProgressEvent }) {
  const activeProgress = progress && !["idle", "running", "failed"].includes(progress.stage) ? progress : undefined;
  return <section aria-label="Менеджер загрузок" className="task-manager-popover">
    <header><strong>Задачи</strong><span>{activeProgress || contentTask ? "Выполняется" : "Всё готово"}</span></header>
    {activeProgress ? <TaskRow iconUrl={activeBuild?.iconUrl} progress={activeProgress.totalBytes ? activeProgress.completedBytes / activeProgress.totalBytes : undefined} subtitle={activeProgress.currentFile?.split(/[\\/]/).pop() ?? progressLabel(activeProgress.stage)} title={activeBuild?.name ?? "Подготовка Minecraft"} /> : null}
    {!activeProgress && contentTask && contentTask.step < 3 ? <TaskRow iconUrl={contentTask.project.icon_url} progress={contentTask.step / 3} subtitle={contentTask.error ?? contentTask.stage} title={contentTask.project.title} /> : null}
    {completed.length ? <div className="task-manager-section"><div><span>Завершено</span><button onClick={onClear} type="button">Очистить</button></div>{completed.map((task) => <TaskRow completed iconUrl={task.iconUrl} key={task.id} subtitle={`${relativeTime(task.completedAt)} · ${task.detail}`} title={task.title} />)}</div> : null}
    {!activeProgress && !contentTask && !completed.length ? <div className="task-manager-empty"><span><KvanthIcon name="confirm" size={32} /></span><strong>Активных задач нет</strong><p>Установки и загрузки появятся здесь.</p></div> : null}
  </section>;
}

function TaskRow({ completed = false, iconUrl, progress, subtitle, title }: { completed?: boolean; iconUrl?: string; progress?: number; subtitle: string; title: string }) {
  return <article className="task-manager-row">
    {iconUrl ? <img alt="" src={iconUrl} /> : <span className={completed ? "is-complete" : ""}><KvanthIcon name={completed ? "confirm" : "download"} size={22} /></span>}
    <div><strong title={title}>{title}</strong><small>{subtitle}</small>{progress !== undefined ? <i><b style={{ width: `${Math.max(3, Math.min(100, progress * 100))}%` }} /></i> : null}</div>
  </article>;
}

function formatSpeed(bytes: number) {
  if (bytes >= 1024 * 1024) return `${(bytes / 1024 / 1024).toFixed(1)} МБ/с`;
  return `${Math.max(1, Math.round(bytes / 1024))} КБ/с`;
}

function relativeTime(at: number) {
  const seconds = Math.max(0, Math.floor((Date.now() - at) / 1000));
  if (seconds < 60) return "только что";
  const minutes = Math.floor(seconds / 60);
  if (minutes < 60) return `${minutes} мин назад`;
  return `${Math.floor(minutes / 60)} ч назад`;
}

function progressLabel(stage: ProgressEvent["stage"]) {
  return ({ idle: "Ожидание", authenticating: "Проверка аккаунта", "resolving-metadata": "Получение метаданных", "resolving-java": "Подбор Java", checking: "Проверка файлов", downloading: "Загрузка", installing: "Установка", launching: "Запуск", running: "Запущено", failed: "Ошибка" })[stage];
}
