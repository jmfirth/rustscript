export function Logo({ className = '' }: { className?: string }) {
  return (
    <span className={`font-bold text-xl tracking-tight ${className}`}>
      <span className="text-[var(--color-rust)]">Rust</span>
      <span className="text-[var(--color-blue)]">Script</span>
    </span>
  );
}
