import { useEffect, useId, useRef, useState, type KeyboardEvent } from "react";

export interface AppSelectOption { value: string; label: string }

interface Props {
  value: string;
  options: AppSelectOption[];
  onChange(value: string): void;
  ariaLabel: string;
  className?: string;
  disabled?: boolean;
}

export function AppSelect({ value, options, onChange, ariaLabel, className = "", disabled = false }: Props) {
  const [open, setOpen] = useState(false);
  const [opensUp, setOpensUp] = useState(false);
  const root = useRef<HTMLDivElement>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const id = useId();
  const selectedIndex = Math.max(0, options.findIndex((option) => option.value === value));
  const selected = options[selectedIndex];

  useEffect(() => {
    if (!open) return;
    root.current?.querySelectorAll<HTMLButtonElement>(".app-select-option")[selectedIndex]?.focus();
    const outside = (event: PointerEvent) => {
      if (event.target instanceof Node && !root.current?.contains(event.target)) setOpen(false);
    };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open, selectedIndex]);

  function show() {
    if (disabled || options.length === 0) return;
    const bounds = root.current?.getBoundingClientRect();
    setOpensUp(Boolean(bounds && window.innerHeight - bounds.bottom < 230 && bounds.top > 230));
    setOpen(true);
  }

  function choose(option: AppSelectOption) {
    onChange(option.value);
    setOpen(false);
    trigger.current?.focus();
  }

  function handleKey(event: KeyboardEvent<HTMLDivElement>) {
    if (disabled || options.length === 0) return;
    if (event.key === "Escape" && open) {
      event.preventDefault(); setOpen(false); trigger.current?.focus(); return;
    }
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      if (!open) { show(); return; }
      const active = document.activeElement?.getAttribute("data-select-index");
      const index = active === null || active === undefined ? selectedIndex : Number(active);
      const next = (index + (event.key === "ArrowDown" ? 1 : -1) + options.length) % options.length;
      root.current?.querySelectorAll<HTMLButtonElement>(".app-select-option")[next]?.focus();
    }
    if (open && (event.key === "Home" || event.key === "End")) {
      event.preventDefault();
      root.current?.querySelectorAll<HTMLButtonElement>(".app-select-option")[event.key === "Home" ? 0 : options.length - 1]?.focus();
    }
  }

  return <div className={`app-select ${open ? "is-open" : ""} ${className}`} onKeyDown={handleKey} ref={root}>
    <button aria-expanded={open} aria-haspopup="listbox" aria-label={ariaLabel} aria-controls={open ? id : undefined} className="app-select-trigger" disabled={disabled} onClick={() => open ? setOpen(false) : show()} ref={trigger} type="button">
      <span>{selected?.label ?? "—"}</span><span aria-hidden="true" className="app-select-chevron" />
    </button>
    {open && <div aria-label={ariaLabel} className={`app-select-list ${opensUp ? "opens-up" : ""}`} id={id} role="listbox">
      {options.map((option, index) => <button aria-selected={option.value === value} className="app-select-option" data-select-index={index} key={`${option.value}-${index}`} onClick={() => choose(option)} role="option" tabIndex={-1} type="button">{option.label}</button>)}
    </div>}
  </div>;
}
