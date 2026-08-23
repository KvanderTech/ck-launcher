interface MemorySettingsProps {
  memoryMb: number;
  maxMemoryMb: number;
  onChange(memoryMb: number): void;
}

export function MemorySettings({ memoryMb, maxMemoryMb, onChange }: MemorySettingsProps) {
  return (
    <section aria-labelledby="memory-settings-title">
      <h2 id="memory-settings-title">Оперативная память</h2>
      <output htmlFor="memory-slider">{memoryMb} МБ</output>
      <input
        aria-label="Оперативная память"
        id="memory-slider"
        max={maxMemoryMb}
        min={512}
        onChange={(event) => onChange(Number(event.currentTarget.value))}
        step={512}
        type="range"
        value={memoryMb}
      />
    </section>
  );
}
